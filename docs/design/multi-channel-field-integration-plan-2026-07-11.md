# Multi-Channel Field Surface — Integration Plan (2026-07-11)

> Design doc for wiring the new multi-parameter field math into bebop/bebop2
> WITHOUT breaking existing RED+GREEN KATs. Companion to the three builder
> batches (deleg_e5f07c15 B2a–B2c, deleg_b108bef9 B3a/B3b).
> File:line anchors are per the 2026-07-11 fable audit (deleg_91222529);
> re-verify against current source before editing.

## 0. Goal

Today the cost/field surface is a SINGLE continuous graph-Laplacian PDE
(`field.rs` `from_edges` CSR L=D−A, `propagate_spectral`). The research asks
for:
- (a) **continuous** update (online/incremental L on edge change) — not
      full rebuild per batch;
- (b) **multi-parameter**: k co-located scalar channels
      `u: G → ℝ^k` (cost, demand-density, trust-decay, latency,
      security-posture) as a multichannel field;
- (c) **integration** into `field_gate`, `propagate_spectral`,
      `field_kalman`, `stabilizer::stabilize_step`, `multipilot` fan-out;
- (d) **honest math**: only the REAL operators get coded (Fick diffusion;
      symplectic wave; spectral λ₂). Poetry (Emden/redshift/vorticity as
      physics sims) stays in `docs/poetry/` as reference, not code.

## 1. The correct multi-channel object

Each channel `c ∈ [0,k)` solves its own diffusion (Fick — REAL):

```
∂_t u_c = -L u_c                      (heat/diffusion, dissipative)
```

per-channel, uncoupled for v1. Coupling (e.g. trust-decay feeding
latency) is a later extension via a coupling matrix `C` (NOT yet built).

- **Online update**: when edge `(i,j)` weight changes by `w`, apply a
  rank-1 Sherman–Morrison update to `L`:
  `L += w · (e_i - e_j)(e_i - e_j)^T`. Recompute `jacobi_eigen` lazily
  (only if a consumer needs eigenmodes). This preserves the existing
  single-channel `propagate_spectral` path (it just reads the updated `L`).
- **Hyperbolic channel (wave)**: if any channel is wave-like, it MUST use
  symplectic `velocity-Verlet` (see `wave_transport.rs`, B2c). Explicit
  Euler injects energy → fails the energy-conservation KAT. Do NOT mix
  Euler into the diffusion channels.

## 2. Falsifiable KATs (every one goes RED when wrong — Verified-by-Math)

- **Mass conservation**: over N steps with no source, `Σ_i u_c[i]` constant
  within 1e-9 per channel. (RED if diffusion leaks mass.)
- **Davis–Kahan perturbation**: after `add_edge_incremental`, recompute
  λ₂ of new L; assert `|λ₂_new − λ₂_old| ≤ C·‖ΔL‖` with `‖ΔL‖ = 2w`
  (rank-1). (RED if eigenvalue bound violated → update wrong.)
- **Symplectic energy**: wave channel `E = Σ(½v² + ½uᵀLu)` constant
  within ε over N Verlet steps. (RED under explicit Euler — proves
  integrator matters.)
- **field_gate veto preserved**: `redline_task_is_vetoed` must still return
  true for red-line tasks after multi-channel wiring. (RED if regressed.)
- **Single-channel KATs stay GREEN**: `spectrum_has_zero_mode_for_connected`,
  `kalman_p_matches_dense_oracle` (1e-9), `kalman_red_breaks`,
  `unstable_system_has_positive_margin`, `nyquist_stable_vs_unstable`,
  `recall_excludes_noise_floor` — none may regress.

## 3. Integration points (file:line, per fable audit)

| Consumer | Anchor | How the channels plug in |
|---|---|---|
| `propagate_spectral` | `chebyshev.rs:111` | Loop the existing spectral step over each channel `c`; reuse one `L` eigen-decomp. Keep scalar overload for backward-compat. |
| `field_kalman` | `field.rs:174` | Run `SpectralKalman` (`kalman.rs:130`) per channel; `field_kalman` becomes a fan-out over k channels. |
| `stabilize_step` | `stabilizer.rs:42` | Stabilizer reads the `trust-decay` channel (not a scalar) → `lyapunov_derivative` (`stabilizer.rs:114`) unchanged API, fed k-th channel. |
| `multipilot` fan-out | `multipilot.rs` | Pilot brief enriched with channel vector `(cost, demand, trust, latency, sec)` per node; existing dispatch logic untouched. |
| `field_gate` | `field.rs` (gate fn) | UNCHANGED — still vetoes red-line tasks pre-dispatch. |
| `lambda_max` / λ₂ | `chebyshev.rs:63` | New `lambda_2` (B2a) feeds a `fragility_alert` consumed by `multipilot` to avoid near-split sub-graphs. |
| `node_harmonic_field` | `geometry_field.rs:238` | Spherical-harmonic channel (angular coverage) optional 3rd axis; documented, not required for v1. |

Backward-compat rule: the existing scalar `u: G→ℝ` API is kept as
`k=1` special case. Old KATs call `k=1`; they must stay green. No public
signature break.

## 4. Honest verdict — what gets coded vs parked

- ✅ CODE: Fick diffusion per channel (load-balancing / demand-spreading,
  REAL); spectral λ₂ fragility (REAL); spherical-harmonic angular channel
  (REAL); Cauchy–Schwarz bound gate on embeddings (REAL); redshift→
  freshness weight (REAL as staleness).
- ⚠️ CODE-WITH-CAVEAT: wave channel ONLY via symplectic Verlet (explicit
  Euler forbidden by KAT).
- 🚩 PARK (docs/poetry/ reference only, NOT physics code): Emden
  "demand black holes"; vorticity "courier loops" (use graph cycle-basis);
  redshift "trust coefficient" (rename→freshness); contour-integral
  "network stability"; Noether/Fock/Catalan as implemented physics.
- ❌ REJECT: fractional-derivative identity (fabricated).

## 5. Guardrails (operational RED-lines)

- Continuous update must be bounded: cap channel count `k ≤ 16`, node
  count from graph; no unbounded `Vec` growth. Enforce at
  `mcp.rs::call_tool` arg cap (L1 security batch) + cgroups `memory.max`.
- Incremental `L` update is O(1) rank-1; full `jacobi_eigen` only on
  demand (lazy), so streaming stays cheap.
- Determinism: no RNG/time in the hot path (clippy wasm32 disallowed-methods
  gate). All channels pure functions of `(L, u, dt)`.

## 6. Sequencing

1. B2b lands `multichannel.rs` (diffusion + low-rank update) → GREEN.
2. B2c lands `wave_transport.rs` (Verlet) → GREEN.
3. B2a lands λ₂ + CS-bound + freshness → GREEN.
4. INTEGRATION PR (separate, post-review): wire channels into
   `propagate_spectral`/`field_kalman`/`stabilize_step` as `k=1` default,
   then expose `k>1`. Re-run ALL single-channel KATs → must stay 0 failed.
5. Reviewer agent re-runs the merged suite; only then merge to
   `feat/wire-native-core`.
