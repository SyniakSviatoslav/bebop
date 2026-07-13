//! Signed frame — a frame carrying its own capability + signature(s).
//!
//! A [`SignedFrame`] binds a capability (authorizing an action on a resource by a
//! key, until a nonce/expiry) to the frame payload via a signature over a
//! **canonical TLV signing input** (`signing_domain`). Because the signature
//! covers the capability, the frame cannot be replayed on a different
//! payload/scope/nonce.
//!
//! # Channel binding (F7) — defeats cross-channel replay
//! An optional `channel_binding: Option<[u8;32]>` slot binds the frame to the
//! transport channel it was signed on. When `Some(hash)`, the hash is the
//! SHA3-256 over the handshake transcript (ClientHello..ServerFinished, or
//! whatever the carrier records). It is appended to the signing domain, so the
//! Ed25519 signature commits to the channel. A frame captured on channel A
//! **cannot** be replayed on channel B: B' != B, and the signature no longer
//! verifies.
//!
//! - `None` => the binding slot is filled with 32 zero bytes. This is the
//!   **legacy / insecure** mode for frames that predate channel binding. It is
//!   explicitly flagged: a zero binding is accepted by any channel, so the
//!   cross-channel replay defense is *not* in effect. Implementations MUST set
//!   a real binding on every fresh channel.
//! - `binding_signing_domain()` = `signing_domain()` ++ binding slot. The actual
//!   classical signature (`sign_classical` / `verify_classical`) covers
//!   `binding_signing_domain()`, NOT `signing_domain()` alone.
//!
//! # Signing — REAL, not faked
//! The **classical leg** is signed with `bebop2-core::sign` Ed25519 (RFC 8032,
//! from scratch, zero-dep). The signature commits to `signing_domain()`, which is
//! a **fixed-layout, domain-separated TLV** ([`crate::tlv`]) — NOT serde_json.
//! This satisfies `ARCHITECTURE.md:75` (no serde on the signed path) and closes
//! red-team finding §4A (signatures were previously over non-canonical JSON).
//!
//! The **post-quantum leg** is ML-DSA-65 in `bebop2-core::pq_dsa` (FIPS 204,
//! ACVP-verified, from scratch, zero-dep). `sign_pq` / `verify_pq` are now
//! WIRED: `sign_pq` computes a real ML-DSA-65 signature over `binding_signing_domain()`
//! and stores the 3309-byte sig in `pq_sig`; `verify_pq` verifies it against the
//! capability's `subject_key_pq` (ML-DSA-65 public key, 1952 bytes). The hybrid
//! gate enforces the PQ leg under `HybridPolicy::RequireBoth`; `ClassicalUntilPqAudit`
//! is the transitional policy that still accepts classical-only but reports the
//! PQ leg as `HybridIncomplete` rather than faking it. No fake PQ signature is
//! ever produced.
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

use bebop2_core::pq_dsa;

use crate::capability::Capability;
use crate::error::{CapError, CapResult};
use crate::hybrid_gate::HybridGate;
use crate::revocation::RevocationSet;
use crate::roster::{AnchorRoster, Delegation};
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
    /// Ed25519 signature (64 bytes) over `binding_signing_domain()`. Stored as
    /// `Vec<u8>` because serde's derive only auto-implements arrays up to length
    /// 32; the byte length is fixed at 64 by `bebop2_core::sign`.
    pub classical_sig: Option<Vec<u8>>,
    /// TODO-PQ: 32-byte-encoded ML-DSA-65 signature over `signing_domain()`.
    /// `None` until the PQ pack/unpack API lands. Not faked.
    pub pq_sig: Option<Vec<u8>>,
    /// The UCAN-subset delegation chain rooting this frame's authority in an
    /// enrolled trust anchor. Carried on the wire (serde) for the verifier's
    /// `AnchorRoster` to check — it is NOT part of the signed domain (the chain
    /// is self-validating via its own per-link signatures). A self-signed frame
    /// (no real anchor-rooted chain) is rejected by `HybridGate::check`.
    pub delegation_chain: Vec<Delegation>,
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
            delegation_chain: Vec::new(),
        }
    }

    /// Builder: attach a channel-binding hash to a frame before signing.
    ///
    /// The binding MUST be set *before* `sign_classical` is called, otherwise the
    /// signature will not cover it. This is the ergonomic path used by carriers
    /// after the handshake completes (see `bebop_proto_wire::handshake`).
    pub fn with_binding(mut self, hash: [u8; 32]) -> Self {
        self.channel_binding = Some(hash);
        self
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

    /// The bytes the *actual* signature commits to: `signing_domain()` followed
    /// by the 32-byte channel-binding slot.
    ///
    /// - `channel_binding = Some(h)` => append `h` (32 bytes). The frame is bound
    ///   to the channel that produced `h`.
    /// - `channel_binding = None` => append 32 zero bytes. Legacy/insecure: the
    ///   signature does not bind to any specific channel, so a captured frame can
    ///   be replayed cross-channel. Explicitly flagged in the module docs.
    pub fn binding_signing_domain(&self) -> CapResult<Vec<u8>> {
        let mut buf = self.signing_domain()?;
        let binding = self.channel_binding.unwrap_or([0u8; 32]);
        buf.extend_from_slice(&binding);
        Ok(buf)
    }

    /// Sign this frame with the classical (Ed25519) key derived from `seed`.
    /// `seed` is the 32-byte Ed25519 seed (see `bebop2-core::sign::keygen`).
    ///
    /// This produces a REAL Ed25519 signature over the canonical TLV signing
    /// domain; tampering fails verification.
    pub fn sign_classical(&mut self, seed: &[u8; 32]) -> CapResult<()> {
        let msg = self.binding_signing_domain()?;
        let sig: [u8; 64] = bebop2_core::sign::sign(seed, &msg);
        self.classical_sig = Some(sig.to_vec());
        Ok(())
    }

    /// Sign the post-quantum (ML-DSA-65) leg over `binding_signing_domain()`
    /// using the real, ACVP-verified `bebop2-core::pq_dsa` implementation. The
    /// resulting signature (3309 bytes) is stored in `pq_sig`. `sk_pq` is the
    /// ML-DSA-65 secret key (4032 bytes); `rnd` is the caller-supplied 32-byte
    /// randomness (FIPS deterministic mode when zero). Never fakes a signature.
    pub fn sign_pq(&mut self, sk_pq: &[u8; 4032], rnd: &[u8; 32]) -> CapResult<()> {
        let msg = self.binding_signing_domain()?;
        // `pq_dsa::MlDsa65Sk` wraps a `Vec<u8>`; rebuild it from the raw sk bytes.
        let sk = pq_dsa::MlDsa65Sk {
            bytes: sk_pq.to_vec(),
        };
        let sig = pq_dsa::sign(&sk, &msg, rnd);
        self.pq_sig = Some(sig.bytes);
        Ok(())
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
        let msg = self.binding_signing_domain()?;
        let ok = bebop2_core::sign::verify(&self.capability.subject_key, &msg, &sig_arr);
        if ok {
            Ok(())
        } else {
            Err(CapError::ClassicalVerifyFailed)
        }
    }

    /// Verify the post-quantum (ML-DSA-65) leg against the capability's
    /// `subject_key_pq`. Returns `HybridIncomplete` if the capability carries no
    /// PQ public key, `PqVerifyFailed` if the signature is absent or invalid.
    pub fn verify_pq(&self) -> CapResult<()> {
        let pk_pq = self
            .capability
            .subject_key_pq
            .as_ref()
            .ok_or(CapError::HybridIncomplete)?;
        let sig = self.pq_sig.as_ref().ok_or(CapError::PqVerifyFailed)?;
        let msg = self.binding_signing_domain()?;
        let pk = pq_dsa::MlDsa65Pk {
            bytes: pk_pq.to_vec(),
        };
        let sig = pq_dsa::MlDsa65Sig { bytes: sig.clone() };
        if pq_dsa::verify(&pk, &msg, &sig) {
            Ok(())
        } else {
            Err(CapError::PqVerifyFailed)
        }
    }

    /// Run the hybrid gate: classical MUST verify; PQ is required by policy but
    /// currently reports `HybridIncomplete` (todo) rather than failing the frame
    /// outright. See [`crate::hybrid_gate`]. The anchor-rooted `delegation_chain`
    /// is passed through so the gate enforces the root-of-trust live.
    ///
    /// `revocations` is the UCAN-style invalidation set (MESH-11); a revoked
    /// capability/key is rejected even with valid signatures. Most callers pass
    /// an empty set (`RevocationSet::new()`) when they have no revocations yet.
    pub fn verify(
        &self,
        gate: &HybridGate,
        roster: &AnchorRoster,
        revocations: &RevocationSet,
        now: u64,
    ) -> CapResult<()> {
        gate.check(self, roster, &self.delegation_chain, revocations, now)
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
    fn pq_leg_real_mldsa_sign_verify_roundtrip() {
        // Real ML-DSA-65 (FIPS 204, ACVP-verified) PQ leg: sign the binding
        // signing domain, then verify it. No fake signature.
        let seed = [1u8; 32];
        let (pk, _) = bebop2_core::sign::keygen(&seed);
        // Provision a real ML-DSA-65 keypair for the subject.
        let pq_seed = [7u8; 32];
        let (pq_pk, pq_sk) = bebop2_core::pq_dsa::keygen(&pq_seed);
        let cap = Capability::new_hybrid(
            pk,
            pq_pk.bytes.clone(),
            Resource::Presence,
            Action::Send,
            [2u8; 8],
            1,
        );
        let mut frame = SignedFrame::new(cap, b"ping".to_vec());
        frame.sign_classical(&seed).unwrap();
        // Sign the PQ leg with the real secret key.
        let rnd = [0u8; 32];
        frame
            .sign_pq(&pq_sk.bytes.clone().try_into().unwrap(), &rnd)
            .unwrap();
        assert!(frame.pq_sig.is_some(), "real ML-DSA-65 sig must be stored");
        // verify_pq over the same domain must succeed.
        assert!(frame.verify_pq().is_ok(), "real PQ signature must verify");
        // A tampered domain must break PQ verification.
        let mut tampered = frame.clone();
        tampered.payload = b"pong".to_vec();
        assert!(
            tampered.verify_pq().is_err(),
            "PQ sig must fail after tamper"
        );
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

    // ── Channel binding (F7): cross-channel replay defense ────────────────────

    /// Happy path: bind to channel hash B, sign, verify on the SAME channel.
    #[test]
    fn bound_frame_verifies_on_same_channel() {
        let seed = [21u8; 32];
        let (pk, _) = bebop2_core::sign::keygen(&seed);
        let cap = Capability::new(pk, Resource::Route, Action::Send, [4u8; 8], 777);
        let binding = [0xabu8; 32]; // hash of channel A's handshake transcript
        let mut frame = SignedFrame::new(cap, b"bound-payload".to_vec()).with_binding(binding);
        frame.sign_classical(&seed).unwrap();
        // Same binding => signature still covers it => verifies.
        assert!(
            frame.verify_classical().is_ok(),
            "same-channel binding must verify"
        );
    }

    /// RED→GREEN: a frame signed with binding=B, then verified on a DIFFERENT
    /// channel binding B' (!= B), MUST FAIL. This is the core replay defense:
    /// the signature now commits to the binding slot, so swapping channels
    /// breaks verification.
    #[test]
    fn bound_frame_fails_on_different_channel() {
        let seed = [33u8; 32];
        let (pk, _) = bebop2_core::sign::keygen(&seed);
        let cap = Capability::new(pk, Resource::Route, Action::Send, [5u8; 8], 888);

        // Attacker captures a frame legitimately signed on channel A (binding=B).
        let binding_b = [0x11u8; 32]; // channel A transcript hash
        let mut captured =
            SignedFrame::new(cap.clone(), b"replay-me".to_vec()).with_binding(binding_b);
        captured.sign_classical(&seed).unwrap();
        assert!(
            captured.verify_classical().is_ok(),
            "sanity: signed on A verifies on A"
        );

        // Attacker replays it on channel B', claiming binding=B' (≠ B).
        let binding_b_prime = [0x22u8; 32]; // channel B' transcript hash
        let replayed = SignedFrame {
            capability: cap.clone(),
            payload: b"replay-me".to_vec(),
            classical_sig: captured.classical_sig.clone(), // OLD signature (covers binding_b)
            pq_sig: None,
            channel_binding: Some(binding_b_prime), // attacker swaps binding field
            delegation_chain: vec![],
        };
        // Signature covers binding_b, not binding_b_prime => MUST FAIL.
        assert!(
            replayed.verify_classical().is_err(),
            "cross-channel replay (binding swap) must FAIL"
        );
    }

    /// Negative: an attacker who flips the `channel_binding` field on a VALID
    /// frame and re-verifies cannot make it pass — the sig covers the slot.
    #[test]
    fn tampering_binding_field_fails() {
        let seed = [44u8; 32];
        let (pk, _) = bebop2_core::sign::keygen(&seed);
        let cap = Capability::new(pk, Resource::Ledger, Action::Append, [6u8; 8], 999);
        let mut frame = SignedFrame::new(cap, b"binding-test".to_vec()).with_binding([0x99u8; 32]);
        frame.sign_classical(&seed).unwrap();
        assert!(frame.verify_classical().is_ok());

        // Swap the binding field to a different hash, keep the old sig.
        frame.channel_binding = Some([0x77u8; 32]);
        assert!(
            frame.verify_classical().is_err(),
            "tampering the binding field must break the signature"
        );
    }

    /// Legacy compatibility: `None` binding => zero-filled slot. A frame signed
    /// with no binding (legacy) is channel-agnostic: it verifies only under the
    /// zero-slot interpretation, and does NOT become bound to a new channel when
    /// a receiver fabricates a binding. This is the INSECURE path and is
    /// explicitly flagged — fresh channels MUST set a real binding; receivers
    /// MUST reject `None` once binding is enforced.
    #[test]
    fn legacy_none_binding_is_zero_filled_and_channel_agnostic() {
        let seed = [55u8; 32];
        let (pk, _) = bebop2_core::sign::keygen(&seed);
        let cap = Capability::new(pk, Resource::Presence, Action::Send, [7u8; 8], 1);
        let mut frame = SignedFrame::new(cap, b"legacy".to_vec()); // channel_binding = None
        frame.sign_classical(&seed).unwrap();
        // Sanity: the binding slot in the signed domain is 32 zero bytes.
        let dom = frame.binding_signing_domain().unwrap();
        assert_eq!(dom[dom.len() - 32..], [0u8; 32]);
        // Legacy frame verifies when the receiver also treats binding as None.
        assert!(frame.verify_classical().is_ok());

        // A receiver that requires a REAL binding (Some(hash)) will REJECT a
        // legacy frame: the sig covered the zero slot, not their channel hash.
        // This proves the legacy frame is unbound — it is NOT silently accepted
        // as bound to the new channel. The insecure part is that a receiver who
        // STILL accepts `None` would let it replay cross-channel.
        let mut as_bound = frame.clone();
        as_bound.channel_binding = Some([0xccu8; 32]); // receiver's channel hash
        assert!(
            as_bound.verify_classical().is_err(),
            "legacy frame must NOT verify as bound to a new channel"
        );
    }
}
