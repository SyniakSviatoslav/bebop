//! iroh transport — QUIC + DHT hole-punching node-to-node carrier.
//!
//! This REPLACES the legacy `crates/bebop/src/zenoh.rs` process-local pub/sub
//! stub. The zenoh stub proved routing/dispatch logic deterministically in
//! process; this line is where that same contract is realised over a real
//! swarmed, NAT-traversal transport.
//!
//! # Status of the iroh carrier — DEFERRED (offline-build policy)
//! The REAL iroh/QUIC implementation is **not wired in this change** and is
//! gated behind a feature that is intentionally NOT resolvable offline. The
//! `iroh` crate is heavy and, in this workspace, conflicts with the
//! `ed25519-dalek` pin in `crates/bebop` (iroh 1.0.0 requires
//! `=3.0.0-rc.0`; `bebop` pins `^3` → 3.0.0). To keep the SOVEREIGN CORE
//! building OFFLINE with zero network deps, the `iroh` dependency is NOT a
//! default/required dependency and is absent from `Cargo.toml`.
//!
//! The blueprint (MESH-09) mandates a real iroh QUIC carrier; the
//! store-and-forward BPv7 overlay in [`crate::bpv7`] is the part that is
//! implemented and tested today (offline, exactly-once RED test). The iroh
//! carrier itself is deferred to the post-G11-GREEN tier where the
//! `ed25519-dalek` conflict is resolved in a network-enabled build.
//!
//! innovate: iroh QUIC carrier is the deferred upgrade. Trigger: resolve the
//! dalek version conflict (unify `crates/bebop` + iroh on one `ed25519-dalek`
//! line) in a network-enabled build, then add `iroh = { version = "1", optional
//! = true }`, flip the feature to `iroh = ["dep:iroh"]`, and restore the
//! `#[cfg(feature = "iroh")] mod real_impl` (its source is preserved in git
//! history at this commit's parent). Until then `IrohTransport` is a
//! compile-clean stub and the BPv7 overlay carries the mesh store-forward
//! semantics over any `Transport` (incl. the live `wss_transport`).
//!
//! CI GUARD: NO-COURIER-SCORING — transport neutrality: moves frames only. No
//! reputation, no scoring, no trust ranking.

#![allow(dead_code)]

use bebop_proto_cap::SignedFrame;

use crate::error::{WireError, WireResult};
use crate::Transport;

/// iroh endpoint descriptor (TODO: real iroh `NodeId` / ticket once wired).
#[derive(Debug, Clone)]
pub enum IrohEndpoint {
    /// A node ticket / URL to dial as a client.
    Ticket(String),
    /// A bind address for an iroh node accepting connections.
    Bind(String),
}

/// ALPN protocol tag for the bebop2 wire carrier (shared by iroh + wss framing).
pub const ALPN_BEBOP2_WIRE: &[u8] = b"bebop2/wire/1";

/// Placeholder iroh transport. Carries no stream yet; `connect`/`accept`/`send`/
/// `recv` are intentionally unimplemented (return `NotConnected`). The type
/// exists so the `Transport` contract is satisfied structurally and the module
/// compiles offline without the `iroh` dependency. The BPv7 overlay
/// ([`crate::bpv7`]) provides the store-and-forward semantics over this same
/// trait, exercised today by the offline RED test against an in-memory
/// `Transport`.
pub struct IrohTransport {
    _endpoint: IrohEndpoint,
}

impl IrohTransport {
    /// Construct a placeholder (no connection). Real wiring deferred (see
    /// module docs `innovate:` marker).
    pub fn new(endpoint: IrohEndpoint) -> Self {
        IrohTransport {
            _endpoint: endpoint,
        }
    }
}

impl Transport for IrohTransport {
    type Endpoint = IrohEndpoint;

    async fn connect(_endpoint: &Self::Endpoint) -> WireResult<Self> {
        // Deferred: real iroh QUIC dial-by-pubkey under feature `iroh`
        // (see module `innovate:` marker). Offline build stays green.
        Err(WireError::NotConnected)
    }

    async fn accept(_endpoint: &Self::Endpoint) -> WireResult<Self> {
        Err(WireError::NotConnected)
    }

    async fn send(&mut self, _frame: SignedFrame) -> WireResult<()> {
        Err(WireError::NotConnected)
    }

    async fn recv(&mut self) -> WireResult<SignedFrame> {
        Err(WireError::NotConnected)
    }
}
