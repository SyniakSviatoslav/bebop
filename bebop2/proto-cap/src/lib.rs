//! bebop-proto-cap — **A** line of the bebop2 protocol (authorization).
//!
//! # Scope (Tier-0 → Tier-4 wiring)
//! This crate is the *authorization* library line. It REPLACES bearer **JWT**
//! (and the dropped `crates/bebop/src/reputation.rs` scoring ledger) with a
//! **per-frame signed capability**:
//!
//! - Every protocol frame carries a compact, single-use capability token: a
//!   signed (Ed25519, from `bebop2-core`) statement of *what action on what
//!   resource, by what key, until what nonce/expiry* — verifiable without a
//!   central issuer. The classical leg is REAL and verified; the post-quantum
//!   (ML-DSA-65) leg is a marked TODO pending the `bebop2-core::pq_dsa`
//!   pack/unpack byte API. No fake signatures are produced.
//! - No session-wide bearer token. No trust accumulated from prior behaviour.
//! - A **hybrid gate** in code: classical (Ed25519) signature must verify, and
//!   (once wired) the post-quantum (ML-DSA-65) signature must also verify, per
//!   the Tier-5 earn-it rule "hybrid-only until audit".
//!
//! ─────────────────────────────────────────────────────────────────────────────
//! ╔══════════════════════════════════════════════════════════════════════════╗
//! ║ CI GUARD — NO-COURIER-SCORING (operator-final hard fork, 2026-07-11)      ║
//! ║ A capability authorises an ACTION on a RESOURCE for a KEY. It NEVER        ║
//! ║ encodes, derives, or consults a courier/agent reputation or score. The     ║
//! ║ bebop `reputation.rs` scoring ledger is DROPPED (DRIFT R2). Any PR adding   ║
//! ║ scoring here is rejected by the doc-claim gate. Authorization is per-frame  ║
//! ║ and stateless; there is no "trusted mover" concept.                        ║
//! ╚══════════════════════════════════════════════════════════════════════════╝
//! ─────────────────────────────────────────────────────────────────────────────

pub mod capability;
pub mod error;
pub mod facade;
pub mod hybrid_gate;
pub mod revocation;
pub mod roster;
pub mod scope;
pub mod signed_frame;
pub mod tlv;

/// A signed capability authorises exactly one action on one resource for one
/// key, bounded by a nonce/expiry. NOT a bearer token, NOT a score.
pub use capability::Capability;
pub use error::{CapError, CapResult};
pub use facade::{Event, EventSink, KernelFacade, Projection, Reject};
pub use hybrid_gate::{HybridGate, HybridPolicy};
pub use revocation::{pq_key_id, revocation_hash, RevocationSet};
pub use roster::{verify_chain, AnchorRoster, Delegation, Effect};
pub use scope::{Action, Resource, Scope};
pub use signed_frame::SignedFrame;
