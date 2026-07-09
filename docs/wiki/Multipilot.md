# Multipilot

**"Copilot is now a multipilot."** — Bebop 0.4.0.

`src/copilot.ts::runMultiPilot` fans a single task out to **N specialist pilots** (distinct backends,
so no single failure mode dominates), then a **distinct synthesizer** merges their outputs. The Rust
field arbiter (`rustFieldArbiter`) can **veto** the combined plan if the physics says it's unsafe.

## Invariants (falsifiable)
1. **Distinctness** — every pilot AND the synthesizer is a DISTINCT backend. If the roster can't supply
   N+1 distinct *available* backends, it falls back to single-copilot rather than fake parallelism.
2. **Field veto** — if `rustFieldArbiter` returns OVERRIDE for the merged plan, `runMultiPilot` returns
   `ok:false` (the plan is blocked, not force-run).
3. **Determinism** — no RNG; the synthesizer is a deterministic stub by default.

## Usage
```
bebop multipilot "wire the field core into the planner"
```

## RED+GREEN proof
`src/copilot.test.ts`:
- fans to N distinct pilots (GREEN) / single-pilot fallback when roster can't (GREEN),
- field OVERRIDE blocks the plan (RED→GREEN),
- default synthesizer merges distinct outputs.
