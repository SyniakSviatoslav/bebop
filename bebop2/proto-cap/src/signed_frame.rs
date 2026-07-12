//! Signed frame — a frame carrying its own capability + signature(s).
//!
//! A [`SignedFrame`] binds a capability (authorizing an action on a resource by a
//! key, until a nonce/expiry) to the frame payload via a signature over
//! `(capability_canonical_bytes || payload)`. Because the signature covers the
//! capability, the frame cannot be replayed on a different payload/scope/nonce.
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
//!   `binding_signing_domain()`, NOT `signing_domain()` alone. `signing_domain()`
//!   is kept public because WS-2 (TLV codec) may still use it as the inner
//!   domain; the binding slot is layered on top, so this merges cleanly.
//!
//! # Signing — REAL, not faked
//! The **classical leg** is signed with `bebop2-core::sign` Ed25519 (RFC 8032,
//! from scratch, zero-dep). This is a genuine signature: `verify` returns `false`
//! on tamper, and the round-trip test asserts that.
//!
//! The **post-quantum leg** is ML-DSA-65 in `bebop2-core::pq_dsa`. It is NOT yet
//! wired here because that module exposes its keys/signature as private structs
//! with no `pack`/`unpack` byte API yet (see the `TODO-PQ` marker in `sign_pq` /
//! `verify_pq`). Until then the hybrid gate still requires the classical leg to
//! verify, and the PQ todo is surfaced explicitly — we do NOT invent a fake PQ
//! signature. This is the honest "TODO with exact call shape" the protocol review
//! gate requires.
//!
//! CI GUARD: NO-COURIER-SCORING — a frame binds action+resource+key only. No
//! score, no trust accumulation, no reputation ledger.

use serde::{Deserialize, Serialize};

use crate::capability::Capability;
use crate::error::{CapError, CapResult};
use crate::hybrid_gate::HybridGate;

/// A frame that carries its own signed capability. Neutral transport payload —
/// the `payload` bytes are opaque to authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedFrame {
    /// The authorization statement (no score fields).
    pub capability: Capability,
    /// Opaque, carrier-neutral payload (the route/ledger/delivery intent bytes).
    pub payload: Vec<u8>,
    /// Ed25519 signature (64 bytes) over `signing_domain()`. Stored as `Vec<u8>`
    /// because serde's derive only auto-implements arrays up to length 32; the
    /// byte length is fixed at 64 by `bebop2_core::sign`.
    pub classical_sig: Option<Vec<u8>>,
    /// TODO-PQ: 32-byte-encoded ML-DSA-65 signature over `signing_domain()`.
    /// `None` until the PQ pack/unpack API lands. Not faked.
    pub pq_sig: Option<Vec<u8>>,
    /// Channel-binding slot (F7): SHA3-256 of the handshake transcript.
    /// `None` => zero-filled 32-byte binding (legacy/insecure; frame is NOT
    /// bound to a channel and could be replayed cross-channel). `Some(hash)` =>
    /// the Ed25519 signature commits to `hash`, so the frame can only verify on
    /// the channel that produced the same transcript hash.
    pub channel_binding: Option<[u8; 32]>,
}

impl SignedFrame {
    /// Build an unsigned frame (signatures filled by [`sign`]).
    pub fn new(capability: Capability, payload: Vec<u8>) -> Self {
        SignedFrame {
            capability,
            payload,
            classical_sig: None,
            pq_sig: None,
            channel_binding: None,
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

    /// The exact bytes a signature commits to: `capability_canonical || payload`.
    /// Any change to the capability (scope/nonce/expiry/subject) or the payload
    /// invalidates the signature.
    ///
    /// NOTE: WS-2 (TLV codec) may replace `capability.canonical_bytes()` with a
    /// TLV codec and extend this. This method is kept stable (returns `Vec<u8>`)
    /// so WS-2 can build on top of it; the channel-binding slot is layered in
    /// [`binding_signing_domain`], which WS-2 does NOT need to touch.
    pub fn signing_domain(&self) -> CapResult<Vec<u8>> {
        let mut buf = self.capability.canonical_bytes()?;
        buf.extend_from_slice(&self.payload);
        Ok(buf)
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
    /// This produces a REAL Ed25519 signature; tampering fails verification.
    pub fn sign_classical(&mut self, seed: &[u8; 32]) -> CapResult<()> {
        let msg = self.binding_signing_domain()?;
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
        let msg = self.binding_signing_domain()?;
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
        assert!(frame.verify_classical().is_ok(), "same-channel binding must verify");
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
        let mut captured = SignedFrame::new(cap.clone(), b"replay-me".to_vec()).with_binding(binding_b);
        captured.sign_classical(&seed).unwrap();
        assert!(captured.verify_classical().is_ok(), "sanity: signed on A verifies on A");

        // Attacker replays it on channel B', claiming binding=B' (≠ B).
        let binding_b_prime = [0x22u8; 32]; // channel B' transcript hash
        let replayed = SignedFrame {
            capability: cap.clone(),
            payload: b"replay-me".to_vec(),
            classical_sig: captured.classical_sig.clone(), // OLD signature (covers binding_b)
            pq_sig: None,
            channel_binding: Some(binding_b_prime),        // attacker swaps binding field
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
