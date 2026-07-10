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

pub mod active_inference; // deterministic FEP policy advisor (pymdp-grounded)
pub mod agentic_git; // GCC pattern: content-addressed agent action-history (COMMIT/CONTEXT/LOG/MERGE)
pub mod audit; // tamper-evident hash-chained audit log (deterministic)
pub mod cli; // the `bebop <cmd>` dispatcher (also the TUI entry)
pub mod coherence; // wave interference (|ψ₁±ψ₂|²) over the field kernel
pub mod copilot;
pub mod customize; // the three customization axes (looks / narration / patrons)
pub mod detect; // N1–N8 operational-graph detector battery (deterministic, RED+GREEN)
pub mod doc_claims;
pub mod enrich; // dossier-derived: trace replay, Pareto, opt-algos, SEAL analog, design-thinking
pub mod execution; // prompt-cache ledger, model cascade, batch splitter (verified-speed primitives)
pub mod field; // re-exports the rust-core field contract (native target)
pub mod governor;
pub mod knowledge;
pub mod launch;
pub mod ledger; // deterministic double-entry money/resource boundary (TigerBeetle invariant)
pub mod mcp; // minimal MCP server over stdio (JSON-RPC)
pub mod memory;
pub mod mission; // the sign-off: animated dock + cigar at loop/task end
pub mod multipilot;
pub mod optical; // deterministic perceptual-hash image search (aHash + Hamming)
pub mod outfit;
pub mod pddl; // deterministic STRIPS-style planner + chain-of-thought trace
pub mod portkey; // deterministic local transport / gateway abstraction (pub-sub bus)
pub mod radio; // the ship's lounge — free-to-listen Lofi/Jazz streams
pub mod recall_graph; // SPIKE (eval-gated): codebase-memory-mcp graph-first retrieval
pub mod reconnect; // MHD "magnetic reconnection": topology change to shed overload energy
pub mod redteam; // T3MP3ST deterministic red-team prompt scanner
pub mod registry; // content-addressed module registry (deterministic)
pub mod research_patterns; // reverse-engineered patterns (research pass 2026-07-10)
pub mod router; // the token/model router (cheapest adequate)
pub mod sealfb; // SEAL closed-loop: field energy → self-tightened tolerance
pub mod stabilizer; // inherent Lyapunov stability: V̇≤0 monitor, saturation, potential well, ground state
pub mod stress; // 3-level stress benchmark (injection / double-bind / telemetry)
pub mod svc; // space-vector control smoothing (αβ trajectory, damping)
pub mod tui; // the ratatui TUI: red-spaceship launch + interactive frame
pub mod vault;
pub mod zenoh; // deterministic mesh transport (local broker; Portkey-swappable)
pub mod zkvm; // deterministic verifiable state-transition boundary (commit/verify)

pub use outfit::{Narration, Outfit, Palette, OUTFIT};
