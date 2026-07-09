# ADR-003 — Neuro-Symbolic Gate (L5 hallucination firewall)

- **Status:** ACCEPTED (2026-07-09)
- **Supersedes:** — (new)
- **Deciders:** bebop core
- **Context (the dump's ask):** The 2026 research dump proposed a "Governor / Controller-Observer"
  split so an LLM agent (stochastic, hallucination-prone) can never drive the deterministic
  kernel directly. It named Neuro-Symbolic Logic (a symbolic arbiter over an NN advisor) as the
  cleanest way to guarantee safety. This ADR records that the bebop governor **already enforces
  that contract deterministically** and pins the rules so it cannot regress.

## 1. Decision

Adopt a **Neuro-Symbolic Gate** as the L5 control boundary:

- The **advisor** (LLM / GNN / any stochastic policy) may PROPOSE an authority `u` and a
  `predictedQuality`. It is a *consultant*, never a driver.
- The **kernel** (the `Governor` class, Rust-equivalent deterministic) is the only actor that
  writes `authority` to the actuator plane. Every advisor proposal is translated into a
  deterministic function call and clamped against hard invariants.
- A **symbolic arbiter** (pure functions: `clamp`, factor kill, resonance cap, safe-state floor,
  poison guard, cycle/PCA breach gate) sits BETWEEN advisor and actuator. It is not "AI" — it is
  typed code that mathematically cannot emit an out-of-contract command.

This is **already implemented** (not aspirational). See `src/governor.ts`:
- `pidStep` computes the advisor's requested `u`; the kernel then applies `clamp(authority, uMin, uMax)`
  and factor/resonance/safe-state overrides. The advisor's raw `u` is surfaced as `pidU` for audit
  but the *granted* authority is what is returned.
- N7 (`bridgeMetrics().hallucinationRate`) measures how often the advisor is overridden — the
  empirical proof the gate is doing its job.

## 2. Why (alternatives rejected)

| Option | Verdict | Reason |
|---|---|---|
| Let the LLM emit raw actuator commands | REJECTED | Directly drives "nuclear logic"; one hallucination = physical/contract failure. |
| Runtime RLHF / PPO to make the LLM "safer" | REJECTED | Violates sovereign-core (no SGD / no RNG at runtime; air-gapped). Training is offline-only. |
| Pure symbolic, no advisor | REJECTED | Loses the generalization/stochastic-search leverage the advisor gives on strategy. |
| Neuro-Symbolic Gate (advisor proposes, kernel decides, symbolic arbiter clamps) | **ACCEPTED** | Keeps advisor leverage, mathematically bounds what reaches the actuator, needs no training loop. |

## 3. Invariants the gate guarantees (test-backed)

1. **Authority can never leave [uMin, uMax].** `clamp` is the final word. (GREEN in `governor.test.ts`.)
2. **A dead factor (ICIR < kill) floors authority to uMin.** The advisor is overruled, not trusted. (N7 RED test.)
3. **A silent advisor (no watchdog heartbeat) drops the kernel to Safe State.** `authority = uMin`, `safeState = true`. (N2 RED test.)
4. **Non-finite / poisoned telemetry is rejected before the gate, never clamped into a command.** (RED-TEAM poison guard.)
5. **A symmetry breach (cycle-consistency) or PCA anomaly only informs — it never silently mutates state.** The kernel logs; the advisor is still bound by (1)-(4). (N1/N3.)

## 4. Interface contract (the "Decoupling" rule)

- The advisor is pluggable: you may swap the LLM for a GNN or a fixed heuristic **without touching
  `governor.ts`**. The only coupling is the `TelemetrySample` shape and the returned `GovernorState`.
- `GovernorConfig` is the formal interface spec. Any new advisor must satisfy it; the kernel does
  not import the advisor.

## 5. Consequences / follow-ups

- N7 observability is the operator dashboard surface for "is the system degrading before it fails?"
- If a future build wants a GNN advisor (N6), it slots in as a *new advisor implementation* behind
  the same `GovernorConfig` — zero kernel change.
- The gate does NOT need Logic Tensor Networks at runtime: the symbolic arbiter is plain typed code,
  which is stronger (no gradient, no approximation) than an LTN soft-constraint.

---
*Verified by:* `src/governor.test.ts` (36 tests, incl. N2/N7 RED+GREEN) + `npm run verify` (≥441 TS tests).
*Gate claim:* checked by `scripts/verify-doc-claims.mjs` check O.
