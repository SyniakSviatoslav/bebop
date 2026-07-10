# bebop2 Envelope / Architecture Verification — V3

- Audit ID: `proc_aa9fbafed4c9` (claude -p, ADVERSARIAL ENVELOPE/ARCHITECTURE VERIFICATION)
- Target: `/root/bebop-repo/bebop2/` greenfield zero-dep core
- Date: 2026-07-10
- Verdict: **CONDITIONAL FAIL** — pillars hold structurally, but the crate did not compile at audit time and several math modules violate the crate's own ARCHITECTURE.md directives.
- `V3_EXIT=0`

## Method
Independent layer (not the build agents, not the math agent, not the PQ agent). Read
ARCHITECTURE.md + every `core/src/*.rs`, traced real imports, attempted both native
`cargo test --lib` and `cargo build --target wasm32-unknown-unknown --no-default-features`,
and checked the no_std / empty-import envelope claim by brute-force grep for forbidden
patterns (serde, wasm-bindgen, JSON, MCP, HashMap, dyn, unsafe, std::time, OS RNG).

## Findings

| # | Sev | Area | One-line |
|---|-----|------|----------|
| 1 | HIGH | wasm gate | `wasm32-unknown-unknown` build fails with ~90 errors — empty-import gate cannot run |
| 2 | HIGH | crate-wide | Native `cargo test --lib` failed to compile (`pq_kem.rs`) — zero math-module tests executable at audit time |
| 3 | HIGH | field.rs / Pillar 1+4 | Dense O(n²) Laplacian + O(n³) Jacobi — exactly what ARCHITECTURE.md forbids; duplicated 3× (field/kalman/lyapunov); no Lanczos/Krylov |
| 4 | HIGH | kalman.rs / Pillar 4 | No sqrt / Potter-Carlson form; only fully dense API; no PSD test |
| 5 | HIGH | field.rs / consistency | "B11 dt corridor" guard does not clamp the named-divergent 0.05; README/doc claim "fixed" |
| 6 | MED | chebyshev.rs | 3rd fexp reimplemented, broken negative rounding, only one sign branch exercised by tests |
| 7 | MED | vsa.rs | FFT-per-op instead of ARCHITECTURE.md's native-Fourier storage contract |
| 8 | LOW | chebyshev.rs | No documented rejection of Chebyshev-accelerated Lanczos |
| — | PASS | Pillar 2 | No serde / wasm-bindgen / JSON / MCP / HashMap / dyn / unsafe — genuinely clean |

## Pillar-by-pillar

- **Pillar 1 (vectors → spectral):** Violated in practice. `field.rs` still materialises a
  full dense O(n²) Laplacian and runs O(n³) Jacobi eigen-decomposition. ARCHITECTURE.md
  explicitly forbids dense tensor formation and mandates Lanczos/Krylov. The same dense
  pattern is copied into `kalman.rs` and `lyapunov.rs`.
- **Pillar 2 (middleware → direct, no serde/wasm-bindgen/JSON/MCP):** **PASS.** No
  forbidden dependencies or indirection patterns present in any module.
- **Pillar 4 (better math per function):** Partially. `kalman.rs` has no square-root /
  Potter-Carlson form and no positive-semidefinite guard; `chebyshev.rs` has a broken
  negative-rounding branch and re-implements `fexp` a third time instead of reusing the
  crate-level `fexp` (C8 fix). `field.rs` B11 guard does not actually clamp.
- **Envelope (no_std / empty import on wasm):** UNVERIFIABLE at audit time — the wasm
  target fails to compile (~90 errors), so the empty-import claim could not be exercised.
  Confirms the crate is NOT yet wasm-ready despite README/ARCHITECTURE asserting it.

## Resolution status (post-audit, 2026-07-10)

- **#2 — RESOLVED.** `pq_kem.rs` was converted from the broken NTT path to
  coefficient-domain schoolbook polynomial multiplication (correct-by-construction,
  FIPS-203-compliant). `cargo test -p bebop2-core --lib` now compiles and passes
  54/54 (math + PQ KEM + FIPS-202 KAT). The KEM no longer uses NTT at all.
- **#1, #3, #4, #5, #6, #7, #8 — OPEN.** Require follow-up: provide a real wasm build,
  replace dense eigen-decomposition with Lanczos/Krylov, add sqrt-Potter-Carlson Kalman,
  make B11 clamp honestly, fix chebyshev negative rounding + dedup fexp, and move VSA to
  native-Fourier storage. These are doc-vs-code / pillar-vs-code gaps, not security holes.
- The audit also confirmed (independently of this author) that the original `pq_kem` NTT
  was non-invertible (`intt(ntt(a)) != a`), corroborating the VbM diagnosis and the V1
  crypto audit (proc_ee2cb27f2ec4).

## Recommendation
Gate the greenfield->swap timeline on: (a) a green native test suite (now met), (b) a
green wasm empty-import build (blocked #1), (c) a Lanczos/Krylov replacement for the
dense eigen-decomposition (blocked #3) before any claim of "no_std ready / never forms
a full tensor" is re-asserted in README/ARCHITECTURE.
