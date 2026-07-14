//! bebop-proto-wire — **W** line of the bebop2 protocol (wire transport).
//!
//! # Scope
//! This crate is the *wire transport* library line. It replaces the legacy
//! `crates/bebop/src/zenoh.rs` process-local pub/sub stub with real transport
//! designs built on:
//!
//! - **WSS** (WebSocket Secure) — the edge/browser fallback transport, used where
//!   a raw QUIC endpoint is not reachable. Implemented in [`wss_transport`] using
//!   `tokio-tungstenite` (pure-Rust, std-friendly).
//! - **iroh** — QUIC-based, NAT-traversal node-to-node transport. The
//!   [`iroh_transport`] module is a TODO pending the (heavy) iroh crate wiring;
//!   it shares the same [`Transport`] contract so it is a drop-in carrier.
//!
//! Both carriers speak the same [`envelope`] frame contract and the same
//! [`framing`] layer, so they are interchangeable behind the [`Transport`] trait.
//! Every frame carried is a signed [`bebop_proto_cap::SignedFrame`] — the
//! transport moves signed frames and verifies them; it does NOT grade the mover.
//!
//! ─────────────────────────────────────────────────────────────────────────────
//! ╔══════════════════════════════════════════════════════════════════════════╗
//! ║ CI GUARD — NO-COURIER-SCORING (operator-final hard fork, 2026-07-11)      ║
//! ║ This crate MUST NOT implement any courier/agent reputation, rating,       ║
//! ║ scoring, trust-ranking, or scoring-derived cost-surface logic. The bebop   ║
//! ║ `reputation.rs` courier-scoring ledger is DROPPED (DRIFT R2). Any PR that   ║
//! ║ adds scoring will be rejected by the doc-claim gate. Transport is neutral  ║
//! ║ plumbing: it moves signed frames, it never grades the mover.              ║
//! ╚══════════════════════════════════════════════════════════════════════════╝
//! ─────────────────────────────────────────────────────────────────────────────

pub mod bpv7;
pub mod envelope;
pub mod error;
pub mod framing;
pub mod handshake;
pub mod iroh_transport;
/// MESH-07 — pull anti-entropy + Merkle digest of the event-log.
pub mod sync_pull;
pub mod transport_policy;
pub mod wss_transport;

pub use error::{WireError, WireResult};

/// Shared transport contract. Both [`iroh_transport`] and [`wss_transport`]
/// implement this so the rest of the stack is carrier-agnostic.
///
/// A `Transport` carries [`SignedFrame`]s: the caller signs on `send`, and the
/// transport verifies on `recv` (via the [`bebop_proto_cap`] hybrid gate). The
/// transport never inspects, scores, or ranks the mover — it is neutral plumbing.
///
/// Object-safe and allocation-light: `send`/`recv` take/return owned frames (no
/// streaming borrow obligations on the caller).
pub trait Transport {
    /// Identity of the remote endpoint (carrier-specific; e.g. a WS peer addr or
    /// an iroh node id). Carries no score.
    type Endpoint;

    /// Establish a client connection to `endpoint` (e.g. a `wss://` URL).
    fn connect(
        endpoint: &Self::Endpoint,
    ) -> impl core::future::Future<Output = WireResult<Self>> + Send
    where
        Self: Sized;

    /// Accept one inbound connection (e.g. upgrade a server WS). Returns the
    /// connected transport for that peer.
    fn accept(
        endpoint: &Self::Endpoint,
    ) -> impl core::future::Future<Output = WireResult<Self>> + Send
    where
        Self: Sized;

    /// Send a signed frame to the peer. The frame is encoded with [`framing`] and
    /// carried as a WebSocket binary message. Fails closed if the carrier drops.
    fn send(
        &mut self,
        frame: bebop_proto_cap::SignedFrame,
    ) -> impl core::future::Future<Output = WireResult<()>> + Send;

    /// Receive one signed frame from the peer. The frame's capability is verified
    /// through the hybrid gate before being returned; a frame that fails
    /// verification is rejected with [`WireError::CapabilityVerify`].
    fn recv(
        &mut self,
    ) -> impl core::future::Future<Output = WireResult<bebop_proto_cap::SignedFrame>> + Send;
}

/// Convenience: sign an in-memory [`SignedFrame`] with the classical (Ed25519)
/// key derived from `seed`, ready to hand to [`Transport::send`].
///
/// This is a thin helper so callers do not reach into `bebop_proto_cap` directly.
/// Honors the no-fake-signature rule: it uses the REAL Ed25519 from
/// `bebop2-core`; the PQ leg is a marked TODO inside `SignedFrame::sign_pq`.
pub fn sign_frame(frame: &mut bebop_proto_cap::SignedFrame, seed: &[u8; 32]) -> WireResult<()> {
    frame.sign_classical(seed)?;
    Ok(())
}

/// Convenience: bind a frame to the channel, then sign it, in one call.
///
/// This is the carrier's send path: after the TLS handshake completes, the
/// carrier computes `channel_binding_hash(handshake_transcript)` and passes it
/// here so the resulting Ed25519 signature commits to the channel. A frame
/// produced this way cannot be replayed on a different channel (F7).
///
/// `transcript` MUST be the authenticated handshake bytes (see
/// [`crate::handshake::channel_binding_hash`]).
pub fn sign_frame_bound(
    frame: &mut bebop_proto_cap::SignedFrame,
    seed: &[u8; 32],
    handshake_transcript: &[u8],
) -> WireResult<()> {
    let binding = crate::handshake::channel_binding_hash(handshake_transcript);
    *frame = frame.clone().with_binding(binding);
    // Real classical signature committing to the channel binding.
    frame.sign_classical(seed)?;
    // PQ-IN-FORCE: the hybrid gate (RequireBoth) also requires a valid ML-DSA-65
    // signature over the bound frame. C6: derive the ML-DSA seed from the master under
    // a distinct domain-separation label (NOT the raw `seed`, which also derives the
    // Ed25519 key above) so the two legs of the hybrid identity are independent. The
    // capability's `subject_key_pq` MUST be minted from this same `derive_pq_seed`.
    let pq_seed = bebop2_core::pq_dsa::derive_pq_seed(seed);
    let (_pq_pk, pq_sk) = bebop2_core::pq_dsa::keygen(&pq_seed);
    frame
        .sign_pq(&pq_sk.bytes.clone().try_into().unwrap(), &[0u8; 32])
        .map_err(|e| crate::error::WireError::Carrier(format!("pq sign: {e:?}")))?;
    Ok(())
}
