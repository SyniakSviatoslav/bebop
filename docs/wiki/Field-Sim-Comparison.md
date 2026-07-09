# Field-Sim Comparison & Visual Explainer

**The unique feature of Bebop 0.4.0.** Most planning agents use a black-box cost. Bebop's is a
**physics simulation you can see**.

## What the field core is
A deterministic graph-PDE (diffusion) solver: `∂u/∂t = -L·u`, where `L` is the sparse Laplacian of the
order/dependency graph. An impulse `seed` (a disruption — a node going down) propagates as a *wave* across
the graph; `field[i]` = predicted downstream impact at node `i`. This is the cost surface the planner
(GOAP/PDDL) and the **Final Arbiter** read.

No RNG, no SGD, no `Date` at runtime. Air-gapped. Rust→WASM twin runs the SAME math as the TS reference.

## Top-K Contours (explainability)
`rustTopKContours(seed, k)` returns the K nodes where a `seed` disruption hurts most. The explainability
layer renders them so a human sees **why** the arbiter overrode PDDL and **which nodes to protect first**.

![field-sim explainer](https://raw.githubusercontent.com/wiki/SyniakSviatoslav/bebop/diagrams/field-sim-explainer.svg)

## Real comparison — JS vs Rust/WASM (measured, n=500 / n=1000, ρ=0.1)

| backend | n=500 (ms) | n=1000 (ms) | speedup vs JS |
|---|---|---|---|
| JS K-iteration (40 Euler) | 19.36 | 50.47 | 1.00× (baseline) |
| Rust/WASM spectral (Chebyshev, 1 call) | 0.72 | 1.91 | **26.8× / 26.5×** |
| Rust/WASM active-set prune | — | — | 64–73× class |
| k-d tree (reference, O(log n)) | — | — | different op |

The Rust spectral propagator is a **single** Chebyshev call (fix A); the active-set prunes quiescent
nodes (fix C). Both run on f64; bit-identical to the JS reference.

## SIMD128 + f32 CSR (measured 2026-07-09c)
- **SIMD128** (`+simd128`): 1.08× faster at n=1500 / 300 iters. Modest but free, stable, deterministic.
- **f32-packed CSR** (`field_build_f32`): CSR stored as f32, computed as f64 → bit-identical (max diff
  < 1e-12). wasm `--max-memory` ceiling lifted 64 MiB → **1 GiB**.

## Reproduce
```
npx tsx src/integration/bench-rust-vs-js.ts 500 0.1
npx tsx src/integration/field-rust.test.ts
node scripts/field-sim-report.mjs
```
Full report: `docs/design/field-sim-comparison-2026-07-09.md` in the repo.
