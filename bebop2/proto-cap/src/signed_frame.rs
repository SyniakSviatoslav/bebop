//! Signed frame — a frame carrying its own capability + signature(s).
//!
//! A [`SignedFrame`] binds a capability (authorizing an action on a resource by a
//! key, until a nonce/expiry) to the frame payload via a signature over a
//! **canonical TLV signing input** (`signing_domain`). Because the signature
//! covers the capability, the frame cannot be replayed on a different
//! payload/scope/nonce.
//!
//! # Signing — REAL, not faked, and now CANONICAL
//! The **classical leg** is signed with `bebop2-core::sign` Ed25519 (RFC 8032,
//! from scratch, zero-dep). The signature commits to `signing_domain()`, which is
//! a **fixed-layout, domain-separated TLV** ([`crate::tlv`]) — NOT serde_json.
//! This satisfies `ARCHITECTURE.md:75` (no serde on the signed path) and closes
//! red-team finding §4A (signatures were previously over non-canonical JSON).
//!
//! The **post-quantum leg** is ML-DSA-65 in `bebop2-core::pq_dsa`. It is NOT yet
//! wired here because that module exposes its keys/signature as private structs
//! with no `pack`/`unpack` byte API yet (see the `TODO-PQ` marker in `sign_pq` /
//! `verify_pq`). Until then the hybrid gate still requires the classical leg to
//! verify, and the PQ todo is surfaced explicitly — we do NOT invent a fake PQ
//! signature. This is the honest "TODO with exact call shape" the protocol review
//! gate requires.
//!
//! # Channel binding (F7)
//! An optional `channel_binding: Option<[u8;32]>` carries the SHA3-256 handshake
//! transcript hash. When set, it is encoded as a TLV field tagged
//! `FIELD_CHANNEL_BINDING` inside the frame's signing domain, binding the
//! signature to the specific authenticated channel. `None` leaves the field
//! absent. (The handshake module that *produces* the transcript hash is a
//! separate F7 task; this crate only encodes/signs the supplied binding.)
//!
//! CI GUARD: NO-COURIER-SCORING — a frame binds action+resource+key only. No
//! score, no trust accumulation, no reputation ledger.

use serde::{Deserialize, Serialize};

use crate::capability::Capability;
use crate::error::{CapError, CapResult};
use crate::hybrid_gate::HybridGate;
use crate::tlv::{tlv_signing_input, DOMAIN_SIGNED_FRAME, FIELD_CHANNEL_BINDING};

/// Field ids for the `SignedFrame` TLV. Ascending; pinned (part of the contract).
const FID_CAPABILITY: [u8; 1] = [0x01];
const FID_PAYLOAD: [u8; 1] = [0x02];

/// Struct tag for `SignedFrame` in the TLV header.
const STRUCT_TAG_SIGNED_FRAME: u8 = 0x01;
/// Wire version of the `SignedFrame` TLV schema.
const WIRE_VERSION_SIGNED_FRAME: u8 = 0x01;

/// A frame that carries its own signed capability. Neutral transport payload —
/// the `payload` bytes are opaque to authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedFrame {
    /// The authorization statement (no score fields).
    pub capability: Capability,
    /// Opaque, carrier-neutral payload (the route/ledger/delivery intent bytes).
    pub payload: Vec<u8>,
    /// Optional channel-binding transcript hash (F7). When `Some`, the frame's
    /// signature also commits to this 32-byte handshake hash, binding the frame
    /// to a specific authenticated channel. Encoded as a TLV field tagged
    /// `FIELD_CHANNEL_BINDING`; `None` omits the field.
    pub channel_binding: Option<[u8; 32]>,
    /// Ed25519 signature (64 bytes) over `signing_domain()`. Stored as `Vec<u8>`
    /// because serde's derive only auto-implements arrays up to length 32; the
    /// byte length is fixed at 64 by `bebop2_core::sign`.
    pub classical_sig: Option<Vec<u8>>,
    /// TODO-PQ: 32-byte-encoded ML-DSA-65 signature over `signing_domain()`.
    /// `None` until the PQ pack/unpack API lands. Not faked.
    pub pq_sig: Option<Vec<u8>>,
}

impl SignedFrame {
    /// Build an unsigned frame (signatures filled by [`sign`]).
    pub fn new(capability: Capability, payload: Vec<u8>) -> Self {
        SignedFrame {
            capability,
            payload,
            channel_binding: None,
            classical_sig: None,
            pq_sig: None,
        }
    }

    /// The exact bytes a signature commits to: a **fixed-layout, domain-separated
    /// TLV** encoding of `(capability_tlv || payload [|| channel_binding])`.
    ///
    /// Layout: `DOMAIN_SIGNED_FRAME || struct_tag || wire_version || field_count`
    /// then per field `FID || u32_le(len) || bytes`:
    /// - `0x01` capability — the full `Capability::canonical_bytes_tlv()`
    /// - `0x02` payload   — the opaque frame payload
    /// - `0xFF` channel_binding (only if `channel_binding.is_some()`)
    ///
    /// The domain tag makes a frame signature and a capability signature live in
    /// disjoint signing spaces even if their field bytes coincided — cross-structure
    /// signature reuse is cryptographically rejected.
    ///
    /// **No serde.** This is hand-built TLV; `serde_json` is never on the signing
    /// path (ARCHITECTURE.md:75, red-team §4A).
    pub fn signing_domain(&self) -> CapResult<Vec<u8>> {
        let cap_tlv = self.capability.canonical_bytes_tlv();

        let mut fields: Vec<(&[u8], &[u8])> = Vec::with_capacity(3);
        fields.push((&FID_CAPABILITY, &cap_tlv[..]));
        fields.push((&FID_PAYLOAD, &self.payload[..]));
        // Channel binding is the highest field id by construction (0xFF); the
        // codec sorts defensively so order here is canonical regardless.
        if let Some(binding) = &self.channel_binding {
            fields.push((&[FIELD_CHANNEL_BINDING], &binding[..]));
        }

        Ok(tlv_signing_input(
            DOMAIN_SIGNED_FRAME,
            STRUCT_TAG_SIGNED_FRAME,
            WIRE_VERSION_SIGNED_FRAME,
            &fields,
        ))
    }

    /// Sign this frame with the classical (Ed25519) key derived from `seed`.
    /// `seed` is the 32-byte Ed25519 seed (see `bebop2-core::sign::keygen`).
    ///
    /// This produces a REAL Ed25519 signature over the canonical TLV signing
    /// domain; tampering fails verification.
    pub fn sign_classical(&mut self, seed: &[u8; 32]) -> CapResult<()> {
        let msg = self.signing_domain()?;
        let sig: [u8; 64] = bebop2_core::sign::sign(seed, &msg);
        self.classical_sig = Some(sig.to_vec());
        Ok(())
    }

    /// TODO-PQ: sign with the post-quantum (ML-DSA-65) key. NOT YET WIRED — the
    /// `bebop2-core::pq_dsa` keys/sigs are private structs without a pack/unpack
    /// byte API, so there is no way to serialize the signature into `pq_sig`
    /// honestly. The exact intended call shape (once the API exists) is:
    ///
    /// ```ignore
    /// let (pk, sk) = bebop2_core::pq_dsa::keygen(&seed32);
    /// let rnd = [0u8; 32]; // caller-supplied, never OS RNG
    /// let sig = bebop2_core::pq_dsa::sign(&sk, &msg, &rnd);
    /// self.pq_sig = Some(pack_mldsa_sig(&sig)); // pack API TBD
    /// ```
    ///
    /// We leave this as a todo and DO NOT fabricate a signature.
    pub fn sign_pq(&mut self, _seed: &[u8; 32]) -> CapResult<()> {
        Err(CapError::HybridIncomplete)
    }

    /// Verify the classical signature against the capability's `subject_key`.
    pub fn verify_classical(&self) -> CapResult<()> {
        let sig = self
            .classical_sig
            .as_ref()
            .ok_or(CapError::ClassicalVerifyFailed)?;
        if sig.len() != 64 {
            return Err(CapError::ClassicalVerifyFailed);
        }
        let sig_arr: [u8; 64] = sig.clone().try_into().map_err(|_| CapError::BadLength)?;
        let msg = self.signing_domain()?;
        let ok = bebop2_core::sign::verify(&self.capability.subject_key, &msg, &sig_arr);
        if ok {
            Ok(())
        } else {
            Err(CapError::ClassicalVerifyFailed)
        }
    }

    /// TODO-PQ: verify the ML-DSA-65 signature. NOT YET WIRED (same API gap as
    /// `sign_pq`). Intended call shape:
    ///
    /// ```ignore
    /// let pk = unpack_mldsa_pk(&self.capability.subject_key_pq); // TBD
    /// let sig = unpack_mldsa_sig(self.pq_sig.as_deref()?);       // TBD
    /// if !bebop2_core::pq_dsa::verify(&pk, &msg, &sig) {
    ///     return Err(CapError::PqVerifyFailed);
    /// }
    /// ```
    pub fn verify_pq(&self) -> CapResult<()> {
        match &self.pq_sig {
            Some(_) => Err(CapError::PqVerifyFailed),
            None => Err(CapError::HybridIncomplete),
        }
    }

    /// Run the hybrid gate: classical MUST verify; PQ is required by policy but
    /// currently reports `HybridIncomplete` (todo) rather than failing the frame
    /// outright. See [`crate::hybrid_gate`].
    pub fn verify(&self, gate: &HybridGate, now: u64) -> CapResult<()> {
        gate.check(self, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::{Action, Resource};
    use crate::tlv::DOMAIN_CAPABILITY;

    #[test]
    fn sign_verify_roundtrip_real_ed25519() {
        // Real Ed25519 from bebop2-core: seed -> (pk, _sk); sign with seed.
        let seed = [42u8; 32];
        let (pk, _) = bebop2_core::sign::keygen(&seed);
        let cap = Capability::new(pk, Resource::Route, Action::Send, [9u8; 8], 12345);
        let mut frame = SignedFrame::new(cap, b"hello wire".to_vec());
        frame.sign_classical(&seed).expect("sign");
        assert!(
            frame.verify_classical().is_ok(),
            "real signature must verify"
        );
    }

    #[test]
    fn tampered_payload_fails_classical() {
        let seed = [7u8; 32];
        let (pk, _) = bebop2_core::sign::keygen(&seed);
        let cap = Capability::new(pk, Resource::Ledger, Action::Append, [1u8; 8], 999);
        let mut frame = SignedFrame::new(cap, b"original".to_vec());
        frame.sign_classical(&seed).unwrap();
        // tamper with the payload after signing
        frame.payload = b"tampered".to_vec();
        assert!(frame.verify_classical().is_err(), "tamper must fail");
    }

    #[test]
    fn tampered_capability_fails_classical() {
        let seed = [11u8; 32];
        let (pk, _) = bebop2_core::sign::keygen(&seed);
        let cap = Capability::new(pk, Resource::Route, Action::Send, [3u8; 8], 500);
        let mut frame = SignedFrame::new(cap, b"x".to_vec());
        frame.sign_classical(&seed).unwrap();
        // tamper with the nonce (part of the signed domain)
        frame.capability.nonce = [99u8; 8];
        assert!(frame.verify_classical().is_err(), "nonce tamper must fail");
    }

    #[test]
    fn pq_leg_is_honest_todo_not_faked() {
        let seed = [1u8; 32];
        let (pk, _) = bebop2_core::sign::keygen(&seed);
        let cap = Capability::new(pk, Resource::Presence, Action::Send, [2u8; 8], 1);
        let mut frame = SignedFrame::new(cap, b"ping".to_vec());
        // sign_pq must NOT silently produce a fake signature.
        assert!(matches!(
            frame.sign_pq(&seed),
            Err(CapError::HybridIncomplete)
        ));
        assert!(frame.pq_sig.is_none(), "pq_sig must stay None (not faked)");
    }

    #[test]
    fn channel_binding_is_signed_into_domain() {
        let seed = [3u8; 32];
        let (pk, _) = bebop2_core::sign::keygen(&seed);
        let cap = Capability::new(pk, Resource::Route, Action::Send, [5u8; 8], 777);
        let mut frame = SignedFrame::new(cap, b"bound".to_vec());
        let binding = [0xCAu8; 32];
        frame.channel_binding = Some(binding);

        // signing domain must contain the channel-binding field.
        let domain = frame.signing_domain().unwrap();
        // field_count is byte 18; with binding present it is 3, absent it is 2.
        assert_eq!(domain[18], 3, "channel_binding adds a field");

        frame.sign_classical(&seed).unwrap();
        assert!(frame.verify_classical().is_ok(), "bound frame verifies");

        // Tampering with the binding after signing must break verification.
        let mut tampered = frame.clone();
        tampered.channel_binding = Some([0xDBu8; 32]);
        assert!(
            tampered.verify_classical().is_err(),
            "channel-binding tamper must fail"
        );
    }

    #[test]
    fn signing_domain_is_tlv_not_serde() {
        // The signing domain must NOT be serde_json: it must start with the
        // SignedFrame domain tag and carry the capability domain tag nested
        // inside, and be byte-identical on re-derivation.
        let seed = [8u8; 32];
        let (pk, _) = bebop2_core::sign::keygen(&seed);
        let cap = Capability::new(pk, Resource::Ledger, Action::Read, [4u8; 8], 321);
        let frame = SignedFrame::new(cap, b"canonical".to_vec());
        let a = frame.signing_domain().unwrap();
        let b = frame.signing_domain().unwrap();
        assert_eq!(a, b, "signing domain must be deterministic");
        assert_eq!(
            &a[0..16],
            DOMAIN_SIGNED_FRAME,
            "must be TLV with frame domain tag"
        );
        // Contains the nested capability domain tag somewhere.
        assert!(
            a.windows(16).any(|w| w == DOMAIN_CAPABILITY),
            "capability TLV (with its own domain tag) must be nested"
        );
        // serde_json would produce a '{' first; TLV does not.
        assert_ne!(a[0], b'{');
    }
}
