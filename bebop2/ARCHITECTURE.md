# bebop2 — vector → tensor → wave (first-principles representation)

> Operator directive (2026-07-10): "wherever there are vectors, think about whether it is
> possible and how to replace [them] with tensors and waves." Below is the resolved design.
> It is NOT prose — it is the API contract the agents implement against.

## The physical distinction (don't store abstractions, store the irreducible object)

A `Vec<f64>` in code is almost always one of three physically-distinct things. Confusing
them is why software burns 1000× the silicon:

1. **State point** (drone pose, embedding) — genuinely a vector = coordinates in a basis.
   BUT the *natural* basis is usually spectral (eigenmodes), not canonical. Store the
   **coefficients in the eigenbasis**, not the sample list.
2. **Linear operator / tensor** (matrix, Hessian, Jacobian, covariance `P`) — a dense
   O(n²) tensor is a CRIME against the AGC envelope. Almost every operator we use is
   *local* (Laplacian, diffusion, convolution). Its eigenmodes are **waves**. Store the
   operator as its **spectrum** (O(n) eigenvalues + a few modes), never the dense tensor.
3. **Field** (spatial/temporal signal) — a sampled field IS a wave decomposition. Store
   **spectral coefficients** (Fourier/Chebyshev), not grid samples. Physics lives in
   frequency domain: heat `∂u/∂t = -λu`, waves oscillate at eigenvalue `ω`.

## Unifying primitive set (the "machine code")

Replace every dense buffer with these spectral primitives. The "tensor" becomes a spectrum;
the "vector" becomes spectral coefficients; both ARE waves.

| Old (dense) | New (spectral / wave) | Module |
|--------------|----------------------|--------|
| `Vec<f64>` state | `project(state, basis)` → coeffs; `reconstruct(coeffs, basis)` | `algebra` |
| dense matrix operator | `spectral(op)` → (eigenvalues, modes); modes = waves | `field` |
| tensor contraction | pointwise in spectrum: `propagate(spectrum, t)` = `exp(-λt)` / `exp(iωt)` | `chebyshev`, `fft` |
| VSA hypervector (dense) | `bind(a,b)` = circular convolution = **pointwise multiply in Fourier** = wave interference | `vsa` |
| Kalman covariance `P` (dense matrix) | `P` as spectrum / low-rank factors; integrate the resolvent — never form the full tensor | `kalman` |
| cost / distance tensor | resolvent of weighted adjacency = spectral; Bellman fixed-point via few spectral iterations | `field` |
| free-energy / precision (dense) | Laplacian of generative model = spectral; beliefs diffuse | `active` |

## Why this survives the AGC envelope
- O(n) storage instead of O(n²) for every operator → fits 2K core RAM per primitive.
- Operations are **pointwise multiplies** + small matrix-vector products — exactly what a
  2.048 MHz machine does. No dense matmul, no allocator thrash at hot path.
- Deterministic, no RNG/clock/network → empty wasm import section (core-RE-loop v2 gate).

## Falsifiability (every primitive ships RED+GREEN)
- `propagate` on a known Laplacian == analytic heat kernel to 1e-9.
- `bind`/`unbind` round-trip: `‖unbind(bind(a,b), a) - b‖ ≈ 0` (symmetry gap).
- spectral Kalman `P` == brute-force dense `P` to 1e-9 on a reference system.
- C8: `fexp` symmetric reduction correct for x<0 (ALREADY fixed in `lib.rs`).
- crypto: FIPS 203/204 + RFC KAT vectors pass bit-exact (committed in `kat/`).

## Carry-forward bug patterns (from fable + audit — do NOT repeat)
- **C8** fixed in `lib.rs::fexp` (negative-arg range reduction). Don't regress.
- **B4** route used LIFO `Vec` → use BinaryHeap + admissible heuristic (i→dst).
- **B8** vault keystream reuse → `rng.rs` must be per-nonce, never reused.
- **B11** hardcoded `dt` → stable corridor `dt=0.02`.
- **C2** stabilizer gate checked raw value → saturate FIRST, then gate.
- **Fable meta-fallacy**: verify PROPERTIES (empty import, named-test greps, bit-exact
  execution), never LABELS (grep symbol presence ≠ correct function).

## Middleware / proxies / transpilers → direct communication (operator directive 2026-07-10)

> "wherever there is middleware, proxies or transpilers — think and analyze whether it is
> possible to replace [them] with direct communication."

Same first-principles move as vector→wave, one layer up the stack. A middleware/proxy/transpiler
is a *relay + translation* hop between a producer and a consumer that could, in principle, talk
directly. The test: **does the hop carry real physics, or only accidental indirection?**

| Layer in old `crates/bebop` | What it is | Fundamental or accidental? | bebop2 replacement |
|------|------|------|------|
| **wasm-bindgen** (wasm target) | rust-core ↔ JS/TS translator | **Accidental** on native path. Dead weight; only needed for web build. | Native CLI calls core via raw `cdylib` C-ABI over linear memory — like AGC read core-rope directly. No bindgen shim in hot path. |
| **MCP stdio server** | proxy: agent ↔ engine over JSON-RPC | **Accidental in-process.** Exists only so external tools reach the engine. | In-tree: direct `decide()` calls. MCP stays OPTIONAL external boundary (flag-OFF), never on the execution path. |
| **`loop.ts` GUARD GATE / `guard.ts`** | middleware intercepting every command | **Mislabeled.** Not a proxy — it's a *verifier* (as-above-so-below checker). | Keep the check as a direct `apply_command_checked()` predicate, not a separate process. |
| **dual-track / advisor→kernel** | proxy: stochastic proposes, kernel decides | **Fundamental (safety).** The indirection IS the air-gap (propose-don't-execute). | Keep — it's a trust boundary, not middleware. Direct predicate call, not a transport. |
| **serde / serde_json / toml** | transpilers: struct ↔ bytes | **Accidental for kernel.** Pull 4 crates + alloc into deterministic core. | Hand-written **fixed-layout** (de)serializer in `core` — O(n) linear scan, no reflection, no alloc at hot path. Envelope = content-addressed fixed bytes (like core-rope). |
| **ratatui / crossterm** | TUI middleware over terminal | **Accidental for core; keep at edge.** Agent logic must not depend on a TUI lib. | Core emits structured state; a *thin* in-tree terminal writer (no ratatui dep) renders it. Leaf, not spine. |

### Unifying principle
> Every hop that *relays or translates without changing the physics* is accidental → delete it.
> Every hop that *verifies or enforces a boundary* (guard gate, advisor→kernel air-gap) is
> fundamental → keep, but as a **direct predicate call, not a process/transport**.

AGC precedent: no middleware between the IMU and the guidance computer. The IMU wrote directly
into fixed memory; the Executive read it directly. The only indirection that survived was LVDC's
TMR voting — and that's a *verifier* (3× compute, majority vote), not a proxy.

### Concrete bebop2 rules (enforced by reloop v2 + agent briefs)
1. Native CLI → core: **direct `cdylib` C-ABI over linear memory.** No wasm-bindgen, no JSON, no MCP in hot path.
2. Serialization: **fixed-layout direct codec** in `core`, no serde.
3. Guard / dual-track: **inline predicates** (`apply_command_checked`, `dual_track_gate`), not servers.
4. MCP / web-bindgen: **flag-OFF external-only boundary**, never on deterministic execution path.
5. TUI: **leaf renderer**, in-tree, no ratatui/crossterm dependency in the core crate.

### Falsifiability
- `bebop2-core` wasm artifact: **empty import section** (no proxy/transport reachable) AND **zero
  `extern` calls to any transport** — verified by reloop v2.
- Benchmark: in-tree `decide()` call latency vs old MCP-stdio round-trip must show the middleware
  was pure overhead (RED+GREEN: removing it speeds up, not breaks).

## Latency / overhead → zero-accidental-cost (operator directive 2026-07-10)

> "Everywhere, seek shortcuts for maximum latency reduction and any unnecessary overhead."

AGC envelope taken to its end: not just delete accidental indirection, but delete ALL overhead
that doesn't carry physics. The AGC ran ~40k instr/s on 2.048 MHz with NO allocator,
NO OS scheduler (fixed-priority Executive loop, not a general kernel), NO dynamic dispatch.
It hit its budget by *having nothing to cut*. bebop2 aims for the same.

### Overhead audit → cut list
| Source | Cut? | bebop2 move |
|--------|-------|--------------|
| `alloc` at hot path (Box/Vec/HashMap in `decide`/`fold`) | **CUT** | Pre-allocated fixed scratch / arena in `core`. Hot path = `no_std` + static buffers, zero `alloc`. |
| serde reflection (derive, dyn dispatch) | **CUT** | Fixed-layout direct codec. No `derive`, no `Any`, no vtable. |
| `f64` where `f32` suffices | **CUT where physics allows** | `field_*` already f32 CSR. Heat/diffusion f32; only crypto/high-precision keeps f64. Half RAM, half bus. |
| dynamic dispatch (trait objects) | **CUT** | Monomorphize. `decide` = one concrete fn, not `Box<dyn Engine>`. |
| MCP/JSON round-trip | **CUT from hot path** | Direct `decide()` call. |
| wasm-bindgen shim | **CUT** | Raw C-ABI linear memory. |
| unused generality (generic N-dim, configurable backends) | **CUT** | Bake the ONE physics. Like AGC: one hand-tuned routine, not a "configurable framework". |
| `std::time`/logging in hot path | **CUT** | No timestamps at kernel. Logging is a leaf, off the path. |
| HashMap for small fixed sets | **CUT → indexed** | CSR/graph arrays (already in `field_build`). Lookup = index, not hash. |
| RNG in crypto keygen | **KEEP (unavoidable)** but QUARANTINED to `rng.rs` keygen only; kernel is provably RNG-free. |

### The latency contract (enforced by reloop v2 + agent briefs)
1. **Zero `alloc` on `decide`/`fold`/`replay` path** — fixed scratch, like core-rope.
2. **Monomorphized, no vtable** — engine is one function, not a trait object.
3. **f32 by default; f64 only where spectral math demands it** (crypto, range-reduction).
4. **Indexed, not hashed** — CSR/graph arrays, not HashMaps.
5. **No serialization on the path** — structs ARE their wire format (fixed layout).
6. **RNG quarantined** to `rng.rs` keygen only; kernel provably RNG-free (empty-import gate).

### HONEST boundary (physicality)
Algorithmic work is NOT overhead. ML-KEM-768 inherently does NTT + ring-MatrixMul over
Z_q; you cannot "shortcut" the math without breaking the algorithm. The shortcut rule applies
to IMPLEMENTATION overhead (alloc, dispatch, serialization, hashing), NOT to algorithmic
cost. Agents must NOT "optimize" PQ crypto into insecurity — the KAT gate forbids it.

### Falsifiability
- Benchmark: `decide()` wall-time per reference command with a HARD ceiling (wasm-instr budget).
- RED+GREEN regression: a `#[cfg(test)]` runs the hot path under a no-alloc allocator and
  PANICS on any `alloc::alloc` / vtable / HashMap call → proves the contract.
- reloop v2 asserts: wasm imports NOTHING (no I/O latency) + bounded `.text` size (icache/decode proxy).

## Better math per function (operator directive 2026-07-10)

> "On ANY function or method — research and analyze whether a mathematically superior solution
> is possible." This is the META-pillar: it governs the other three (vectors→waves,
> middleware→direct, latency→zero-overhead). For every primitive, ask: *is this the
> mathematically optimal formulation for its physics, or a convenient one?* If a
> Krylov/Chebyshev/Lanczos/square-root/spectral-native formulation does the same
> physics in fewer ops or with better numerics — USE IT.

The three categories this lives in, and the upgrade questions each module MUST answer
before it is declared done:

### Cat 1 — Post-quantum security (pq_kem / pq_dsa / aead / kdf / sign)
- The SCHEME (ML-KEM-768 / ML-DSA-65) is a FIPS STANDARD. "Better math" ≠
  re-deriving the algorithm (that leaves the audited/interop envelope). The standard
  IS near-optimal for NTT/Module-LWE. **Optimize ONLY within what FIPS permits:**
  - **Small sampler** (FIPS 203 §"small" centered-binomial): one rejection round,
    fewer RNG bytes, constant-time — SUPERIOR to naive two-uniform-product. TAKE IT.
  - **Fused hash-to-field**: one SHAKE tree call, not per-coefficient hashing.
  - **NTT**: for Kyber n=256, NTT is near-optimal; do NOT "upgrade" to
    Karatsuba/Toom (they win only at small degrees). Document WHY NTT is the optimum.
- Verdict: scheme = optimum (provable lattice reduction); apply the *approved* micro-optimizations.

### Cat 2 — Optimization & latency (field / fft / chebyshev / kalman / lyapunov)
- **`fft.rs` is often the WRONG math.** For graph spectra you don't FFT a dense
  matrix — you use **Lanczos / Arnoldi (Krylov subspace)**: O(n·k), NEVER forms
  the dense Laplacian. This IS the tensors→waves directive made concrete.
- **`chebyshev.rs` (spectral propagator) = already optimal** polynomial approx for
  `exp(-λt)`. Upgrade further: **Chebyshev-accelerated Lanczos** — propagate in
  the Krylov basis, fewer modes, same accuracy.
- **`field.rs` heat kernel `exp(-L t)·x`**: the OPTIMAL form is the
  **matrix-function-vector product via Lanczos+Chebyshev** — approximate the ACTION
  without diagonalizing. Never form λ explicitly if you can avoid it.
- **`kalman.rs`**: upgrade dense-P / spectral-P to **square-root covariance filtering
  (Potter/Carlson)** — numerically stable, P cannot lose PSD (a real failure mode).
  Information form when observations are dense.

### Cat 3 — Tensors & waves (vsa / algebra / field)
- **`vsa.rs` bind = circular convolution = pointwise Fourier multiply.** The SUPERIOR
  math: **store hypervectors natively in the Fourier/spectral basis** — bind = multiply,
  no forward FFT per op. Collapses the transform overhead entirely (tying Cat 3 → Cat 2).
- **`algebra.rs` sinc**: `sin(πx)/(πx)` has the x=0 division issue — guard the
  limit, OR use the **Lanczos sigma (σ) factor** for anti-aliasing (Gibbs control).
- **Deepest Cat-3 upgrade**: a vector = projection on a basis; the OPTIMAL basis for a
  *dynamic* system is its **Karhunen-Loève (PCA) = eigenbasis of its covariance** —
  which IS the spectral decomposition. "Better math for vectors" = **project onto the
  system's OWN eigenbasis (KLE), not a fixed Fourier basis.** Unifies Cat 3 with Cat 2:
  the waves ARE the optimal basis.

### The meta-rule (enforced at integration, not just build)
> Standard (FIPS) = optimum for INTEROP/AUDIT, not necessarily for MATH. Optimize
> within what the standard permits. For non-standard math (spectral kernels, VSA,
> Kalman), the Krylov/Chebyshev/square-root/spectral-native form is the bar — a
> module that uses naive dense/Taylor/per-op-FFT MUST justify WHY, or be rewritten.

### Falsifiability (integration gate — applied when agents return)
- Each module's brief report MUST name the mathematical formulation chosen AND the
  rejected alternative + WHY (e.g. "Lanczos over FFT: O(n·k) vs O(n²log n);
  dense Laplacian never formed"). A module with no such justification = RED, send back.
- Numerical: Lanczos `exp(-L t)·x` == dense-eigen decomposition to 1e-9 on a
  reference graph (RED+GREEN: perturb the iteration count → divergence detected).
- Square-root Kalman P stays symmetric-PSD where naive P goes non-PSD (test asserts
  eigenvalues ≥ 0 across a stress trajectory).

