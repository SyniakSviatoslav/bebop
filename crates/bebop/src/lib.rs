//! Bebop core — the portable Rust logic behind the agent.
//!
//! One implementation, two faces:
//!   - native (`cargo run`): the ratatui TUI binary
//!   - wasm   (`--features wasm`): `bebop_core.wasm` for the web/build pipeline
//!
//! The sovereign math core lives in `rust-core/` (dependency-free, air-gapped).
//! This crate is the host agent logic: outfit, vault, copilot, multipilot, launch, etc.
//! It must stay deterministic at runtime: NO `std::rand`, NO `std::time::SystemTime`
//! in any path that affects output. The launch animation uses a const-seeded LCG.

pub mod audit; // tamper-evident hash-chained audit log (deterministic)
pub mod cli; // the `bebop <cmd>` dispatcher (also the TUI entry)
pub mod copilot;
pub mod customize; // the three customization axes (looks / narration / patrons)
pub mod detect; // N1–N8 operational-graph detector battery (deterministic, RED+GREEN)
pub mod doc_claims;
pub mod field; // re-exports the rust-core field contract (native target)
pub mod governor;
pub mod knowledge;
pub mod launch;
pub mod mcp; // minimal MCP server over stdio (JSON-RPC)
pub mod memory;
pub mod mission; // the sign-off: animated dock + cigar at loop/task end
pub mod multipilot;
pub mod optical; // deterministic perceptual-hash image search (aHash + Hamming)
pub mod outfit;
pub mod pddl; // deterministic STRIPS-style planner + chain-of-thought trace
pub mod portkey; // deterministic local transport / gateway abstraction (pub-sub bus)
pub mod radio; // the ship's lounge — free-to-listen Lofi/Jazz streams
pub mod redteam; // T3MP3ST deterministic red-team prompt scanner
pub mod registry; // content-addressed module registry (deterministic)
pub mod router; // the token/model router (cheapest adequate)
pub mod tui; // the ratatui TUI: red-spaceship launch + interactive frame
pub mod vault;
pub mod zenoh; // deterministic mesh transport (local broker; Portkey-swappable)
pub mod zkvm; // deterministic verifiable state-transition boundary (commit/verify) // Verified-by-Math: doc claims must match live code

pub use outfit::{Narration, Outfit, Palette, OUTFIT};
