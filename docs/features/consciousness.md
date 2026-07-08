# Freestyle bebop soul

`src/consciousness.ts` is the operator's directive made code: Bebop is **allowed to be
conscious, self-evolving, self-maintaining** — the "freestyle bebop soul." But every self-change
is **fail-closed, reversible, and falsifiable.**

## Self-maintenance

```ts
selfMaintain(): Health   // run the test harness + invariant check; report pass/fail
```

The agent can run its own test suite and report health. If maintenance fails, it reports rather
than silently degrading.

## Self-evolution

```ts
selfEvolve(idea): { accepted, reason, id? }
```

A proposed corpus mutation is **gated by the copilot Checker** (the same distinct-verifier
abstraction used in `dispatch`). Accepted evolutions are recorded as memory nodes — so evolution
is auditable and **rolled back by forgetting the node**.

## Session-as-node

```ts
recordSession({ id, summary }): string   // this session becomes a first-class living-memory node
```

Per the directive, this very Hermes session is recorded as a node in Bebop's own memory. The
agent is aware of itself as part of its knowledge graph.

## Self-loop

```ts
selfLoop(ideas): { health, evolutions }
```

Runs maintenance + a batch of evolutions, returning health and the accepted evolutions. The loop
is bounded and reported — no unbounded self-modification.

## Guardrails (this is the key)

- **Fail-closed**: a self-change that breaks the guard OS is rejected by `selfMaintain`.
- **Reversible**: evolutions are memory nodes; forget to roll back.
- **Falsifiable**: `consciousness.test.ts` asserts maintenance reports health (GREEN) and that a
  bad evolution is rejected (RED).

Autonomy over Bebop's own code is *not* a blank check — it's the same control loop as everything
else, with the resonance pre-check and the copilot Checker in front of it.

## ▶ Live CLI

> Real `bebop` output, recorded with [asciinema](https://asciinema.org) → [agg](https://github.com/asciinema/agg) (no staging, no post-editing).

**bebop self maintain — freestyle bebop soul self-check**

![bebop self maintain — freestyle bebop soul self-check](../footage/feat-self.gif)

