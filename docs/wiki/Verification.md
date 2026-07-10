# Verification

Every claim in Bebop is **falsifiable** (Verified-by-Math): an assertion that goes RED on bad input,
shipped alongside the GREEN.

## Counts (1.0.0, 2026-07-09 — native Rust, no TypeScript runtime)
- **Rust kernel (`bebop-core` / `rust-core`):** 16 tests (`cargo test -p bebop-core`), wasm32 build clean.
- **Rust agent (`bebop`):** 63 tests (`cargo test -p bebop`), 0 fail.
- **Total:** **79 Rust tests** green — this is the number README/AGENTS claim, and it is real.
- **Doc-gate:** `node scripts/verify-doc-claims.mjs` → all doc claims backed by live proof.
- **Falsifiable-proof:** `node scripts/guardrail-falsifiable-proof.mjs` → 95/95 `#[test]` fn bodies have a non-tautological assertion (RED case exists).
- **Lint/format:** `cargo fmt --check` + `cargo clippy` gate the native path.

## Principles
- **Constant Doubt:** no verification, no statement.
- **Better less than sorry:** never state what isn't fact-checked.
- **Ground truth over proxy:** deterministic math truth may delete rotten processes.
- **Red-line globs** (auth / money / RLS / migrations / bulk-edit) need a human gate before change.

## Honest status (no silent losses, no silent fakes)
Deleting the TypeScript layer retired ~30 analytic behaviors that were TS-only. Every one of
them is now a real, deterministic, falsifiable Rust module with RED+GREEN tests — none faked:

- **N1–N8** anomaly / cycle / liveness detector battery → `crates/bebop/src/detect.rs` (10 tests)
- **T3MP3ST redteam** heuristic prompt-storm scanner → `crates/bebop/src/redteam.rs` (4 tests; `bebop scan`)
- **Portkey gateway** in-process pub/sub bus → `crates/bebop/src/portkey.rs` (3 tests)
- **PDDL `logicalCot`** STRIPS planner + CoT trace → `crates/bebop/src/pddl.rs` (3 tests; `bebop plan`)
- **module registry** content-addressed (SHA-256) → `crates/bebop/src/registry.rs` (4 tests)
- **audit** tamper-evident hash-chained log → `crates/bebop/src/audit.rs` (3 tests; `bebop audit`)
- **optical search** aHash + Hamming perceptual match → `crates/bebop/src/optical.rs` (3 tests)
- **ML-KEM/ML-DSA post-quantum identity** → hybrid vault `crates/bebop/src/vault.rs` (7 tests)

The two L5 research slots from Research.md are also wired as honest native prototypes:
- **Zenoh mesh** → `crates/bebop/src/zenoh.rs` (local broker, Portkey-swappable iface; 3 tests)
- **zkVM money boundary** → `crates/bebop/src/zkvm.rs` (deterministic commit/verify state seal; 3 tests; `bebop boundary`)

All are covered by `doc_claims` + the falsifiable-proof guardrail. What remains
genuinely forward-looking (not claimed done): a real Zenoh/network transport
(replace the local broker), a real zk circuit (replace the hash-seal with an
actual proof system), and TigerBeetle ledger integration — these are research
slots, explicitly not yet in the native core.
