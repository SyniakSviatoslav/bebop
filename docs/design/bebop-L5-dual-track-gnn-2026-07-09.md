# L5 Dual-Track GNN Hybrid — Design & Seam (N6)

- **Date:** 2026-07-09
- **Status:** Seam implemented (FLAG-OFF) + design locked. NOT training anything at runtime.
- **Depends on:** N4 (causal counterfactual), N5 (Neuro-Symbolic Gate ADR-003).

## 1. The hybrid, in one sentence

Keep a **deterministic graph** (Truth Layer: facts, dependencies, allowed routes) and a
**stochastic advisor** (Operational Layer: LLM / GNN intuition). The advisor PROPOSES; the graph
GATES. This is the dump's "Constraint-Based Gatekeeper" made concrete and testable.

## 2. Why not train a GNN at runtime (and not even offline-here)

- Sovereign-core forbids SGD/RNG/Date in runtime; the system is air-gapped. Any GNN *training*
  must happen offline in a separate toolchain and be exported as weights — out of scope for this
  repo's runtime.
- The **seam** we ship here is the *interface contract* + the *gate*. A future GNN advisor slots in
  behind `GnnAdvisor` with **zero change to the kernel** (this satisfies ADR-003 §4 decoupling).

## 3. The seam (`src/integration/analytics/dual-track.ts`)

```ts
interface GnnAdvisor { propose(focus: string): { target: string; confidence: number } | null; }
interface TruthGraph { nodes: string[]; A: number[][]; } // A[i][j]>0 ⇔ edge i→j
function dualTrackGate(graph, advisor, focus, opts): DualTrackVerdict
```

`dualTrackGate` returns `honored: false` (and a precise `reason`) when the advisor:
- gives no advice (`no-advice`),
- names an unknown focus node (`unknown-focus`),
- proposes a target with **no edge** in the Truth Layer (`no-such-edge` — the hallucination case),
- or falls below the confidence floor (`low-confidence`).

Only a proposal matching a real graph edge is `honored`. The counterfactual blast-radius (N4
`pointsOfFailure`) can be surfaced on every honored proposal for ops triage.

## 4. RED+GREEN proof (deterministic, no training)

`src/integration/analytics/dual-track.test.ts`:
- GREEN: advisor proposes `core→util` (a real edge) ⇒ `honored`.
- RED: advisor proposes a non-existent `ghost` edge ⇒ `rejected` (reason `no-such-edge`).
- RED: advisor invents an unknown focus ⇒ `rejected` (`unknown-focus`).
- RED: low-confidence hunch ⇒ `rejected` (`low-confidence`).
- GREEN: silent advisor ⇒ safe `no-advice` no-op.
- GREEN: N4 counterfactual wired into the honored verdict.

## 5. Rejected alternatives (C-class, deferred with reason)

| Dump suggestion | Verdict | Reason |
|---|---|---|
| Train a GNN on Dowiz graphs at runtime | REJECTED | sovereign-core: no SGD/RNG/Date in runtime; air-gapped. |
| Use PyG/DGL/Logic Tensor Networks in this repo | DEFERRED | belongs in an offline training toolchain + exported weights; the *runtime* only needs the gate. |
| Replace the graph with pure tensors | REJECTED | loses the exact "is this edge real?" fact-check that makes the gate honest. Hybrid > pure-tensor. |

## 6. Next

When a GNN advisor is built offline and exported, implement `GnnAdvisor` over its weights and pass
it to `dualTrackGate`. No kernel change. The gate's RED cases already prove it cannot be fooled by
a hallucinated edge.

---
*Verified by:* `dual-track.test.ts` + `npm run verify`.
*Gate claim:* checked by `scripts/verify-doc-claims.mjs` check P.
