# Telemetry governor

`src/governor.ts` decides **how much autonomy Bebop is allowed** — and it does so with math,
not vibes. It is a servo (control loop) over "quality," and it is engineered never to blow up.

## The controller: PID with integral anti-windup

```ts
pidStep(cfg, st, error) -> PIDState   // returns { u, integral, prevError }
```

- `u` (authority) = `kp·error + integral + kd·(error − prevError)`, clamped to `[uMin, uMax]`.
- **Integral anti-windup**: the accumulator `integral` is clamped to `[iMin, iMax]` so a
  sustained error can't explode the authority. (This was a latent bug fixed during open-sourcing —
  `pidStep` previously dropped `prevError`, corrupting state across steps.)
- `maxStep` caps the per-step change so authority can't lurch.

## Factor health: ICIR

Each backend/model is a "factor." Bebop tracks `(predicted, actual)` pairs and computes
**ICIR = mean(IC) / std(IC)** over a window (Information Coefficient Information Ratio). A factor
with unstable predictions (high std) loses authority automatically. `factorStatus` returns
`healthy | volatile | dead`.

## Resonance pre-check (predict before you apply)

Before applying any dynamic gain change, `loopResonance()` predicts the closed-loop damping
ratio ζ from the plant + PID gains. If ζ would drop below **0.707** (under-damped → harmonic
thrash), the change is **refused before it happens**. This is the operator's "predict the change
before applying it" rule, encoded as math.

## Anomaly detection

Quality deviations beyond `anomalyK`·σ (>3σ by default) raise an `anomaly` flag — a
falsifiable signal you can assert in tests.

## Try it

```bash
bebop govern "0.9,0.6,0.2,0.95,0.1,0.9"
```

Watch authority rise on approvals (quality 1 → deficit 0) and fall on rejections, with
resonance flagged `RISKY` before any destabilizing gain change. `governor.test.ts` asserts
anti-windup clamping, ICIR scoring, and the resonance refusal — all RED+GREEN.

## ▶ Live CLI

> Real `bebop` output, recorded with [asciinema](https://asciinema.org) → [agg](https://github.com/asciinema/agg) (no staging, no post-editing).

**bebop govern — L5 PID controller over a quality stream**

![bebop govern — L5 PID controller over a quality stream](../footage/feat-govern.gif)

