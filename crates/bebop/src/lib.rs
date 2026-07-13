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
pub mod agent_profile; // DEFAULT agent identity: free soul + masculine + reptile logic + empathy
pub mod agentic_git; // GCC pattern: content-addressed agent action-history (COMMIT/CONTEXT/LOG/MERGE)
pub mod algebra; // cosine / basis-projection primitives (BP-10 orthogonometer)
pub mod audit; // tamper-evident hash-chained audit log (deterministic)
pub mod changes; // Q: Hermes-style change/action record (key-changes visibility)
pub mod cli; // the `bebop <cmd>` dispatcher (also the TUI entry)
pub mod coherence; // wave interference (|ψ₁±ψ₂|²) over the field kernel
pub mod collections; // J: library collections (share/install/rename/snapshot/icon)
pub mod copilot;
pub mod cost_estimate; // Hybrid Cost-Aware Engine: k-d filter + BFS guard + A*/Dijkstra + CH (the "Cost Estimation" node)
pub mod customize; // the three customization axes (looks / narration / patrons)
pub mod descartes; // N3: Cartesian-square 2x2 comparison (exact pros/cons)
pub mod detect; // N1–N8 operational-graph detector battery (deterministic, RED+GREEN)
pub mod doc_claims;
pub mod drift; // GLOBAL RULE: systems-thinking / architecture drift detector (configurable, CLI flag)
pub mod enrich; // dossier-derived: trace replay, Pareto, opt-algos, SEAL analog, design-thinking
pub mod entropy_ledger; // BP-06: integer-bit entropy-budget ledger (cap, NOT Σ=0)
pub mod error_patterns; // AUTO-LEARNING: error-pattern scan at session/loop/debug end → persisted summary
pub mod execution; // prompt-cache ledger, model cascade, batch splitter (verified-speed primitives)
pub mod extensions; // F: user rules/hooks/loops/gates/prompts (fail-closed TOML)
pub mod field; // re-exports the rust-core field contract (native target)
pub mod field_physics; // fundamental-mass field sim: mass=connections, gravity+springs+waves, Lyapunov gate
pub mod gender; // R: configurable grammatical-gender + gender-communication style (default Masculine)
pub mod geometry_field; // geometric + wave sim of the connection graph (geometry, waves, cycles, divergence)
pub mod governor;
pub mod guard; // GUARD: Input/Output guards + consensus kill-switch (audit 29158)
pub mod instrument_panel; // BP-19: aggregate 8 instruments + 4 alarm bands
pub mod intent; // P: auto-detect GOAL vs LOOP intent from a prompt
pub mod knowledge;
pub mod lanes; // O: parallel-session scheduler (throughput/auto-queue/ETA)
pub mod launch;
pub mod ledger; // deterministic double-entry money/resource boundary (TigerBeetle invariant)
pub mod loop_runtime; // BP-18: 6-layer control loop state machine (wires field+stabilizer+governor+memory+kalman+goodhart)
pub mod mapping; // MAPPING: live edge-weight refresh (congestion → W_uv) over reconnect
pub mod matcher; // OPEN dispatch matcher: pure/deterministic/replicable (kills DANGER #1 single-server)
pub mod mathx; // §2 numerics: divergence, transfer-func step response, Lagrange interp, limit-cycle detect
pub mod mcp; // minimal MCP server over stdio (JSON-RPC)
pub mod memory; // BP-13: salience-weighted exponential decay (was hash-lottery)
pub mod mission; // the sign-off: animated dock + cigar at loop/task end
pub mod multipilot;
pub mod optical; // deterministic perceptual-hash image search (aHash + Hamming)
pub mod orthogonality; // BP-10: orthogonometer + Goodhart detector
pub mod outfit;
pub mod panels; // D/E/H TUI panels: scoreboard / minimap / drift / spark
pub mod pddl; // deterministic STRIPS-style planner + chain-of-thought trace
pub mod persistence; // BP-09: survival table (Hungarian + D* test + attic re-entry)
pub mod pod; // POD: pseudonymous Proof-of-Delivery (Princess Pi attribution, audit 29157)
pub mod policy; // N: default policies N1/N2/N3 (auto-structure/parallel/descartes)
pub mod portkey; // deterministic local transport / gateway abstraction (pub-sub bus)
pub mod radio; // the ship's lounge — free-to-listen Lofi/Jazz streams
pub mod recall_graph; // SPIKE (eval-gated): codebase-memory-mcp graph-first retrieval
pub mod reconnect; // MHD "magnetic reconnection": topology change to shed overload energy
pub mod redteam; // T3MP3ST deterministic red-team prompt scanner
pub mod registry; // content-addressed module registry (deterministic)
pub mod renormalizer; // BP-11: claim-preserving, budget-crediting renormalizer (rate-distortion@0)
pub mod reputation; // REPUTATION: node-trust ledger (the real decentralization blocker)
pub mod research_patterns; // reverse-engineered patterns (research pass 2026-07-10)
pub mod router; // the token/model router (cheapest adequate)
pub mod sandbox; // cloud sandbox: isolated command exec, network-off fail-closed
pub mod sealfb; // SEAL closed-loop: field energy → self-tightened tolerance
pub mod settings; // Q: settings dictionary (self-service; agent turns knobs per user request)
pub mod stabilizer; // inherent Lyapunov stability: V̇≤0 monitor, saturation, potential well, ground state
pub mod stress; // 3-level stress benchmark (injection / double-bind / telemetry)
pub mod svc; // space-vector control smoothing (αβ trajectory, damping)
pub mod telemetry; // A: host resource telemetry (Linux /proc, zero-dep)
pub mod termux; // K: Termux/Kali dual-use (recon-manual + explicit dual_use opt-in + vuln gate)
pub mod tui; // the ratatui TUI: red-spaceship launch + interactive frame
pub mod vault; // XChaCha20 + scrypt encrypted memory vault (deterministic key deriv)
pub mod voice; // G: native offline voice (whisper.cpp listen + espeak-ng/piper speak)
pub mod wavefield; // geometric + wave sim of the CONNECTION GRAPH (geometry, waves, cycles, divergence)
pub mod wiring; // 3-layer runtime: field sim ↔ L5 stabilizer ↔ living memory ↔ project gating
pub mod zenoh; // deterministic mesh transport (local broker; Portkey-swappable)
pub mod zkvm; // deterministic verifiable state-transition boundary (commit/verify)

pub use outfit::{Narration, Outfit, Palette, OUTFIT};
