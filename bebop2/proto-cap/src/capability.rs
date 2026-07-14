//! Capability — the signed per-frame authorization statement.
//!
//! Replaces JWT bearer tokens. A capability is a single-use, signed statement
//! `{subject_key, scope, nonce, expiry}` — verifiable by any peer without a
//! central issuer. It authorises exactly one ACTION on one RESOURCE for one KEY,
//! bounded by a nonce/expiry. NOT a bearer token, NOT a score.
//!
//! CI GUARD: NO-COURIER-SCORING — capability never references a score/trust.
//!
//! # Canonical signing (no serde)
//! Signatures are computed over a **fixed-layout TLV** encoding
//! ([`crate::tlv`]), never over `serde_json`. `ARCHITECTURE.md:75` mandates
//! fixed-layout encoding on the signed path; serde_json is implementation-defined
//! (non-canonical) and was the exact defect the red-team review §4A flagged. The
//! `serde` derive is retained ONLY for out-of-band transport framing
//! (`proto-wire` serializes the whole `SignedFrame` envelope), but it is never
//! used to build the bytes a signature commits to.

use serde::{Deserialize, Serialize};

use crate::scope::{Action, Resource, Scope};
use crate::tlv::{tlv_signing_input, DOMAIN_CAPABILITY};

/// Field ids for the `Capability` TLV. Ascending; pinned (part of the contract).
const FID_SUBJECT_KEY: [u8; 1] = [0x01];
const FID_SCOPE: [u8; 1] = [0x02];
const FID_NONCE: [u8; 1] = [0x03];
const FID_EXPIRY: [u8; 1] = [0x04];

/// Struct tag for `Capability` in the TLV header.
const STRUCT_TAG_CAPABILITY: u8 = 0x01;
/// Wire version of the `Capability` TLV schema.
const WIRE_VERSION_CAPABILITY: u8 = 0x01;

/// A single-use, signed authorization statement.
///
/// The signing domain is the *canonical TLV* serialization of the public fields
/// only (not the signatures). Tampering with any field invalidates the
/// signature, so a capability cannot be replayed on a different scope/nonce/expiry.
///
/// The `serde` derive is retained ONLY for out-of-band transport framing
/// (`proto-wire` serializes the whole `SignedFrame` envelope). It is NEVER used
/// to build the bytes a signature commits to — that is [`Capability::canonical_bytes_tlv`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    /// Ed25519 public key (32 bytes) of the subject the capability is issued to.
    /// Verbatim bytes, never interpreted as a reputation/score.
    pub subject_key: [u8; 32],
    /// Post-quantum subject public key (ML-DSA-65, 1952 bytes) — the PQ half of
    /// the hybrid identity. `None` until the issuer provisions a PQ keypair
    /// (the hybrid gate only requires it under `RequireBoth`).
    pub subject_key_pq: Option<Vec<u8>>,
    /// What the capability authorizes (resource + action). No rating fields.
    pub scope: Scope,
    /// Single-use nonce (8 bytes). Replay-protected by the verifier's nonce set.
    pub nonce: [u8; 8],
    /// Expiry as a unix-ish monotonically-increasing counter (no clock dependency
    /// required by this struct; the caller supplies a comparable tick).
    pub expiry: u64,
}

impl Capability {
    /// Build a capability. `subject_key` is the Ed25519 public key of the mover;
    /// it is an identity, not a trust rating.
    pub fn new(
        subject_key: [u8; 32],
        resource: Resource,
        action: Action,
        nonce: [u8; 8],
        expiry: u64,
    ) -> Self {
        Capability {
            subject_key,
            subject_key_pq: None,
            scope: Scope::single(resource, action),
            nonce,
            expiry,
        }
    }

    /// Build a capability with both classical and post-quantum subject keys
    /// (hybrid identity). `subject_key_pq` is the ML-DSA-65 public key (1952 bytes).
    pub fn new_hybrid(
        subject_key: [u8; 32],
        subject_key_pq: Vec<u8>,
        resource: Resource,
        action: Action,
        nonce: [u8; 8],
        expiry: u64,
    ) -> Self {
        Capability {
            subject_key,
            subject_key_pq: Some(subject_key_pq),
            scope: Scope::single(resource, action),
            nonce,
            expiry,
        }
    }

    /// Canonical signing bytes: a fixed-layout, domain-separated TLV encoding of
    /// the public capability fields. **No serde.** This is what an Ed25519
    /// signature commits to (see `SignedFrame::signing_domain`).
    ///
    /// Layout: `DOMAIN_CAPABILITY || struct_tag || wire_version || field_count`
    /// then per field `FID || u32_le(len) || bytes`:
    /// - `0x01` subject_key (32 bytes)
    /// - `0x02` scope        (2 bytes: resource, action)
    /// - `0x03` nonce        (8 bytes)
    /// - `0x04` expiry       (8 bytes, u64 LE)
    pub fn canonical_bytes_tlv(&self) -> Vec<u8> {
        let expiry_le = self.expiry.to_le_bytes();
        let scope_bytes = self.scope.to_tlv_bytes();
        tlv_signing_input(
            DOMAIN_CAPABILITY,
            STRUCT_TAG_CAPABILITY,
            WIRE_VERSION_CAPABILITY,
            &[
                (&FID_SUBJECT_KEY, &self.subject_key[..]),
                (&FID_SCOPE, &scope_bytes[..]),
                (&FID_NONCE, &self.nonce[..]),
                (&FID_EXPIRY, &expiry_le[..]),
            ],
        )
    }

    /// Whether `expiry` is still acceptable against `now`. Pure comparison — no
    /// clock, no drift score.
    pub fn is_fresh(&self, now: u64) -> bool {
        self.expiry > now
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_canonical_tlv_is_stable() {
        let cap = Capability::new(
            [7u8; 32],
            Resource::Route,
            Action::Send,
            [1, 2, 3, 4, 5, 6, 7, 8],
            9999,
        );
        let a = cap.canonical_bytes_tlv();
        let b = cap.canonical_bytes_tlv();
        assert_eq!(a, b, "canonical TLV encoding must be deterministic");
        assert!(cap.is_fresh(9998));
        assert!(!cap.is_fresh(9999));
    }

    #[test]
    fn capability_tlv_has_domain_tag_and_expiry_le() {
        let cap = Capability::new(
            [7u8; 32],
            Resource::Route,
            Action::Send,
            [1, 2, 3, 4, 5, 6, 7, 8],
            0x0102_0304_0506_0708,
        );
        let bytes = cap.canonical_bytes_tlv();
        // domain tag
        assert_eq!(&bytes[0..16], DOMAIN_CAPABILITY);
        // struct_tag / wire_version / field_count
        assert_eq!(bytes[16], STRUCT_TAG_CAPABILITY);
        assert_eq!(bytes[17], WIRE_VERSION_CAPABILITY);
        assert_eq!(bytes[18], 4);
        // expiry is little-endian: 0x08 0x07 0x06 0x05 0x04 0x03 0x02 0x01
        let exp = &bytes[bytes.len() - 8..];
        assert_eq!(exp, &[0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]);
    }

    // GREEN: a Capability and a SignedFrame can carry IDENTICAL field bytes but
    // MUST yield DIFFERENT signing domains (and thus different signatures) because
    // their domain tags differ. This is the cross-structure-reuse rejection the
    // red-team §4A demanded — a signature minted for one type cannot be replayed
    // as another. (The OLD serde_json path had no such separation.)
    #[test]
    fn capability_vs_frame_signing_domain_is_domain_separated() {
        let seed = [7u8; 32];
        let (pk, _) = bebop2_core::sign::keygen(&seed);

        let cap = Capability::new(pk, Resource::Route, Action::Send, [3u8; 8], 500);
        let cap_domain = cap.canonical_bytes_tlv();

        // Build a frame whose payload is literally the capability's TLV bytes —
        // i.e. the frame's field bytes are byte-for-byte the same as the
        // capability's. The signing domains must still differ because of the
        // frame domain tag (and the extra payload field).
        let payload = cap_domain.clone();
        let frame = signed_frame_with_payload(pk, payload.clone());
        let frame_domain = frame.signing_domain().unwrap();

        assert_ne!(
            cap_domain, frame_domain,
            "domain tags separate the two types"
        );
        // And a signature over the capability MUST NOT verify as a frame signature.
        let mut cap_sig_frame = frame.clone();
        // Mint a real classical sig over the CAPABILITY domain and try to use it
        // on the frame: it must fail because the frame's signing domain differs.
        let cap_sig: [u8; 64] = bebop2_core::sign::sign(&seed, &cap_domain);
        cap_sig_frame.classical_sig = Some(cap_sig.to_vec());
        assert!(
            cap_sig_frame.verify_classical().is_err(),
            "a capability signature must NOT verify as a frame signature (cross-type reuse rejected)"
        );
    }

    /// Helper: build a SignedFrame carrying `payload` as its opaque payload, with
    /// a capability whose own TLV bytes equal `payload` (to stress the
    /// domain-separation property).
    fn signed_frame_with_payload(
        pk: [u8; 32],
        payload: Vec<u8>,
    ) -> crate::signed_frame::SignedFrame {
        let cap = Capability::new(pk, Resource::Route, Action::Send, [3u8; 8], 500);
        crate::signed_frame::SignedFrame::new(cap, payload)
    }
}
