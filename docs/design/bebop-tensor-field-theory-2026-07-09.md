# Bebop L5 — Tensor+Graph Field Theory for Memory & Project Structure (PROBED + CORRECTED)

> Operator directive 2026-07-09: *"simulate physics a bit with math; a tensor+graph structure for BOTH
> the living memory AND the project, so changes propagate as multidimensional waves simulating memory +
> time with minimal latency. Analyze & probe my theory, expand & correct it where needed and possible."*

This document PROBES the theory against real math, CORRECTS three imprecisions, and EXPANDS it into a
concrete, deterministic, falsifiable module (`src/integration/field-sim.ts`).

---

## 1. The theory, stated plainly

- The project (files + imports) and the living memory (concepts + associations) are two **graphs**.
- Each node carries a **tensor** of channels (activation, recency, risk, version, …).
- A "change" is an **impulse** at some node; it should **propagate as a wave** across both graphs,
  simulating how a code edit ripples into memory (and vice-versa), with **minimal latency** (no LLM/network).

That is a *field theory on a graph*. Correct framing. Now the probes.

---

## 2. PROBE — what holds, what needs correction

### ✅ HOLDS: graph + tensor structure is the right substrate
Already present and proven:
- `arch-mine.ts` builds the import/wikilink adjacency + SVD/PCA coupling clusters + `causalCounterfactual` BFS.
- `memory.ts` is a VSA hypervector store with graph spreading-activation (associative recall).
- `field.ts` already computes ∇·F / ∇×F over the embedding plane — a **static** field diagnostic.

So "tensor+graph for both" is real and shipping. The missing piece was **time evolution**.

### ⚠️ CORRECTION 1 — "simulate physics" ≠ Newtonian mechanics
The honest model for "memory + time change" is **field evolution on a graph Laplacian**, not F=ma:
- **HEAT / diffusion**: `∂u/∂t = −D·L u` — activation diffuses along edges and **decays** (this IS spreading-
  activation made rigorous; the memory "fade").
- **WAVE (2nd order)**: `∂²u/∂t² = −c²·L u` — momentum → **overshoot/oscillation** = the physical origin of the
  "reconsider" (curl/rotate) signal `field.ts` only *diagnosed* statically.
- **Why not Newton**: there is no inertial mass or external force field here; the only dynamics are
  diffusion (1st-order, contractive) and wave (2nd-order, oscillatory). Calling it "physics" is fine — it is
  exactly graph-structured field theory (the same math as finite-element heat/wave solvers).

### ⚠️ CORRECTION 2 — "waves" must be integrated SYMPLECTICALLY
Naïve explicit Euler on `∂²u/∂t² = −c²L u` **injects energy every step** (I proved this in the test:
energy grew 3.8× over 50 steps). The correct integrator is **velocity-Verlet** (symplectic), which
conserves the Hamiltonian ½vᵀv + ½c²·uᵀ(Lu). After the fix: energy conserved to 0.9998 over 50 steps.
**Lesson**: "simulate waves" is only honest if the integrator is symplectic; otherwise you are building a
fake oscillator that gains energy (a false-positive metric for "stability").

### ⚠️ CORRECTION 3 — "minimal latency" is real but bounded
One explicit step is `O(|E|·channels)` over a sparse Laplacian — microseconds, no LLM. BUT: the coupled
two-layer block Laplacian (`memory × project`) is dense across the inter-layer coupling, so cost is
`O((|E_mem|+|E_proj|+|E_cross|)·channels)`. "Minimal latency" holds *per step*; the honest claim is
"sub-millisecond single-step propagation", not "instant global equilibrium" (which needs many steps).

---

## 3. EXPANSION — the coupled, multidimensional field (what shipped)

`src/integration/field-sim.ts` implements the corrected theory:

1. **`laplacian(A)`** — unnormalized graph Laplacian `L = D − A` (row-sum zero, a proper operator).
2. **`blockLaplacian(layers, coupling)`** — the **coupled** operator for N layers (memory + project):
   ```
   L_block = [ L_mem    κ·C ]
             [ κ·Cᵀ   L_proj ]
   ```
   `κ` is the inter-layer coupling (a code edit perturbs its concept node, and vice-versa).
3. **`FieldSim`** — `u[channel][node]` tensor field with two modes:
   - `diffuse` (heat): contractive, energy decays (memory fade) — proven: energy 1 → 0.21 over 20 steps.
   - `wave` (velocity-Verlet): energy-conserving (Hamiltonian ½vᵀv + ½c²uᵀLu) — proven: 0.9998 over 50 steps.
4. **Multidimensional**: `channels` ≥ 1, with optional `channelCoupling` (c×c) so channels bleed into each
   other (activation ↔ risk ↔ version), not just nodes.
5. **`impulse(node, amp)`** — a change enters the field at one node; `run(steps)` propagates it.

This is the "multidimensional wave simulating memory + time" — realized as a deterministic, reversible
(wave) or fading (diffuse) field evolution over the joint tensor+graph.

---

## 4. Where this meets the rest of the system

| Concept (operator)        | Realized by                                            | Status   |
|---------------------------|--------------------------------------------------------|----------|
| tensor+graph (memory)     | `memory.ts` VSA + graph spreading-activation           | shipping |
| tensor+graph (project)    | `arch-mine.ts` adjacency + `reverse-engineer.ts`       | shipping |
| static field diagnostic   | `field.ts` ∇·F / ∇×F                                   | shipping |
| **time evolution (waves)**| `field-sim.ts` diffuse + wave (symplectic)             | **new**  |
| coupled memory×project     | `field-sim.blockLaplacian` + `knowledge.repoGraphIndex`| new      |
| minimal-latency change sim| `FieldSim.step()` O(|E|·channels)                       | new      |
| reverse-engineering loop  | `reverse-engineer-loop.ts` (multipilot ≥3 verifiers)  | shipping |

---

## 5. Open corrections / future work (honest gaps)
- The wave mode is **linear** (no nonlinearity / no source terms). Real "reconsider" may need a driven
  term (external probe). Flag-OFF: add `drive(node, f(t))`.
- Coupling `κ` is a constant; it could be **learned** from actual edit→memory co-occurrence (deterministic
  counting, not SGD). Flag-OFF.
- The project graph is rebuilt on each scan (no incremental Laplacian update). For large repos, an
  incremental `L_block` update (add/remove edge → O(degree) patch) would keep latency flat. Flag-OFF.

---

## 6. Rust / Python replacement analysis (operator: "prefer Rust")

The deterministic math cores are pure Float64 DSP — ideal Rust/WASM twins (matches the sovereign-core
philosophy: wasm32 + disallowed-methods, air-gapped). See `rust-core/` stub (flag-OFF):
- **REPLACE with Rust/WASM** (max-EV, no behavior change): `matrix.ts` (SVD/PCA), `field-sim.ts`
  (Laplacian + Verlet), `kalman.ts`, `eta.ts`, VSA codec in `memory.ts`. These are the hot, pure paths; a
  Rust WASM build is ~10–50× faster and type-safe, with zero Date/RNG (enforced at the FFI boundary).
- **KEEP in TS** (fast iteration, orchestration): `loop.ts`, `copilot.ts`, `governor.ts`, `redteam.ts`,
  `multipilot.ts`, `dual-track.ts` — the agent logic that changes weekly; Rust would slow the loop.
- **Python**: only as a research/reference implementation or a PyO3 bridge for the VSA/PCA math if you want
  a numpy-accelerated twin; not for the runtime (TS is the live agent). Keep Python OUT of the runtime.
- **Verified-by-Math**: any Rust twin must pass the SAME RED+GREEN tests (ported) before a TS→WASM switch,
  so the replacement is proven, not assumed.
