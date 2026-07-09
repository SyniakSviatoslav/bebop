// Bebop governor — L5 signal-health monitor (operator directive 2026-07-08).
//
// Unifies FOUR math foundations into meta-control over ANY autonomous process (agent/model/loop):
//   1. PID        — discrete position form + integral anti-windup; drives resource AUTHORITY.
//   2. ICIR       — quant factor-health: IC_t = rank-corr(predicted, actual); ICIR = mean/std.
//                   an agent's "factor" is its self-knowledge (does it predict its own quality?).
//   3. RESONANCE  — predict closed-loop ζ/ω_r of a PROPOSED gain BEFORE applying it; refuse
//                   changes that push the loop under-damped (harmonic blow-up across layers).
//   4. THERMO     — Landauer floor k·T·ln2 per bit; a hard floor on cost (can't think for free).
//
// Every property is verified by a falsifiable RED+GREEN test (Verified-by-Math bar).

export interface TelemetrySample {
  t: number;
  predictedQuality: number; // agent's self-predicted quality ∈ [0,1]
  actualQuality: number; // observed quality ∈ [0,1]
  cost: number; // resource units consumed this step
  volume: number; // throughput (tokens/actions) this step
  /** Optional multidimensional telemetry vector for the PCA-analytics anomaly
   *  path (flag-OFF). When present AND cfg.pcaAnomaly is set, the governor
   *  scores this vector for "weirdness". Ignored otherwise. */
  features?: number[];
}

export type FactorStatus = 'unknown' | 'healthy' | 'volatile' | 'dead';

export interface GovernorState {
  authority: number; // ∈ [uMin,uMax], recommended resource latitude
  pidU: number; // raw PID output before clamps
  icir: number | null;
  factorStatus: FactorStatus;
  resonanceRisky: boolean;
  anomaly: boolean;
  pcaAnomaly: boolean; // L5 analytics (flag-OFF); true on adaptive-EMA excursion of the features vector
  cycleBroken: boolean; // L5 analytics (flag-OFF); symmetry-loop breach (F(G(x))≠x)
  /** D2 ICA→governor: index of the localized broken SUBSYSTEM (−1 when none / not configured). */
  subsystemFault: number;
  thermoFloorHit: boolean;
  error: number;
  poisoned?: boolean; // true when a non-finite sample was rejected (RED-TEAM fix 2026-07-09)
  /** N2 liveness contract (flag-OFF): true when the agent has been silent longer than
   *  `watchdogMs` ⇒ kernel dropped to Safe State (authority floored, no advisory honored). */
  safeState?: boolean;
  /** N2: ms since the last advisory (heartbeat age). 0 until the first step with a clock. */
  agentSilentMs?: number;
  /** N7 hybrid-bridge observability (flag-OFF): running count of advisor advices the
   *  kernel REJECTED (overrode the requested authority). Surfaced for the "is the
   *  system degrading 10 min before it fails?" metric. */
  rejectedAdvices?: number;
  /** N7: hallucination rate = rejectedAdvices / totalSteps ∈ [0,1]. */
  hallucinationRate?: number;
}

// ── pure math primitives ──────────────────────────────────────────────────────

export function clamp(x: number, lo: number, hi: number): number {
  return x < lo ? lo : x > hi ? hi : x;
}

function rank(xs: number[]): number[] {
  const idx = xs.map((v, i) => [v, i] as [number, number]).sort((a, b) => a[0] - b[0]);
  const r = new Array(xs.length).fill(0);
  let i = 0;
  while (i < idx.length) {
    let j = i;
    while (j + 1 < idx.length && idx[j + 1][0] === idx[i][0]) j++;
    const avg = (i + j) / 2 + 1; // 1-based average rank (ties)
    for (let k = i; k <= j; k++) r[idx[k][1]] = avg;
    i = j + 1;
  }
  return r;
}

export function pearson(a: number[], b: number[]): number {
  const n = a.length;
  const ma = a.reduce((s, x) => s + x, 0) / n;
  const mb = b.reduce((s, x) => s + x, 0) / n;
  let num = 0, da = 0, db = 0;
  for (let i = 0; i < n; i++) { const pa = a[i] - ma, pb = b[i] - mb; num += pa * pb; da += pa * pa; db += pb * pb; }
  const den = Math.sqrt(da * db);
  return den === 0 ? 0 : num / den;
}

/** Spearman rank correlation — the IC of a quant factor. */
export function spearman(a: number[], b: number[]): number {
  if (a.length !== b.length || a.length < 2) return 0;
  // both series constant ⇒ perfectly comonotonic (a perfectly self-knowing agent) ⇒ IC = 1
  const av = a[0]; const bv = b[0];
  if (a.every((x) => x === av) && b.every((x) => x === bv)) return 1;
  return pearson(rank(a), rank(b));
}

export interface PIDConfig {
  kp: number; ki: number; kd: number;
  iMin: number; iMax: number; uMin: number; uMax: number; maxStep?: number;
}

export interface PIDState { integral: number; prevError: number; u: number; }

export function pidStep(cfg: PIDConfig, st: PIDState, error: number): PIDState {
  // integral with anti-windup clamp — bounds the accumulator so a sustained error can't explode
  const rawI = st.integral + cfg.ki * error;
  const integral = clamp(rawI, cfg.iMin, cfg.iMax);
  const d = cfg.kd * (error - st.prevError);
  const u = clamp(cfg.kp * error + integral + d, cfg.uMin, cfg.uMax);
  return { u, integral, prevError: error };
}

/** ICIR = mean(IC) / std(IC) over a window. null when undefined. */
export function icir(icSeries: number[]): number | null {
  if (icSeries.length < 2) return null;
  const m = icSeries.reduce((s, x) => s + x, 0) / icSeries.length;
  const v = icSeries.reduce((s, x) => s + (x - m) ** 2, 0) / icSeries.length;
  const sd = Math.sqrt(v);
  if (sd === 0) return m > 0 ? Infinity : 0;
  return m / sd;
}

// ── resonance prediction (predict the change before applying it) ──────────────

const SQRT1_2 = 1 / Math.sqrt(2);

export interface Resonance {
  wn: number; zeta: number; wr: number; mr: number; risky: boolean; aliasRisk: boolean;
}

/** Closed-loop resonance of a 2nd-order agent-plant under gains (Kp,Kd), inertia M, damping B. */
export function loopResonance(kp: number, kd: number, M: number, B: number, samplePeriod = 0): Resonance {
  const wn = Math.sqrt(Math.max(0, kp / M));
  const zeta = (B + kd) / (2 * Math.sqrt(Math.max(1e-9, kp * M)));
  const risky = zeta < SQRT1_2;
  const wr = wn * Math.sqrt(Math.max(0, 1 - 2 * zeta * zeta));
  const mr = risky ? 1 / (2 * zeta * Math.sqrt(Math.max(1e-9, 1 - zeta * zeta))) : 1;
  // discrete alias: if the natural frequency approaches the Nyquist band, harmonics fold in
  const aliasRisk = samplePeriod > 0 && wn * samplePeriod > 0.3;
  return { wn, zeta, wr, mr, risky: risky || aliasRisk, aliasRisk };
}

// ── thermodynamics of computation (Landauer) ──────────────────────────────────

const K_B = 1.380649e-23;
const LN2 = Math.LN2;

/** Minimum energy to erase `bits` bits at temperature T (Kelvin). @300K ≈ 2.87e-21 J/bit. */
export function landauerFloor(bits: number, T = 300): number {
  if (bits < 0) throw new Error('thermo: negative bits');
  return bits * K_B * T * LN2;
}

/** Bits erased by a decision of given volume (log2 of distinct states touched). */
export function bitsErased(volume: number): number {
  return Math.max(1, Math.ceil(Math.log2(volume + 2)));
}

// ── anomaly detection (operator priority: flag telemetry breaching estimated bounds) ──

import { pcaAnomalyScore, type PcaAnomalyState } from './integration/analytics/anomaly.ts';
import { cycleConsistencyGate, type CycleConsistencyGateState } from './integration/analytics/cycle-consistency.ts';
import type { TelemetryICAPipeline } from './integration/analytics/telemetry-ica-loop.ts';
import { scoreTelemetrySample } from './integration/analytics/telemetry-ica-loop.ts';

export function detectAnomaly(history: number[], x: number, k = 3): boolean {
  if (history.length < 2) return false;
  const m = history.reduce((s, v) => s + v, 0) / history.length;
  const sd = Math.sqrt(history.reduce((s, v) => s + (v - m) ** 2, 0) / history.length);
  if (sd === 0) return false; // zero-variance history: no basis to call a breach; stay quiet
  return Math.abs(x - m) > k * sd;
}

// ── the Governor: ties it all together over one agent/loop ─────────────────────

export interface GovernorConfig extends PIDConfig {
  targetQuality: number; // setpoint ∈ [0,1]
  deadIC?: number; // ICIR below this → factor 'dead' → authority floored (legacy name)
  icirKill?: number; // ICIR below this → factor 'dead' → authority floored
  icirVolatile: number; // ICIR below this (but ≥ kill) → 'volatile'
  plantM: number; plantB: number; samplePeriod?: number;
  anomalyK?: number;
  volHistoryLen?: number;
  /** L5 ANALYTICS (2026-07-09, flag-OFF): PCA-reconstruction anomaly over a
   *  multidimensional telemetry vector. When set, the governor scores the
   *  incoming feature vector against a calibrated "normal" PCA model and
   *  raises `pcaAnomaly` on an adaptive-EMA excursion. Off unless this is
   *  provided. Deterministic — no RNG/training; PCA === linear autoencoder. */
  pcaAnomaly?: {
    model: import('./integration/analytics/anomaly.ts').PCA;
    cfg: import('./integration/analytics/anomaly.ts').PcaAnomalyConfig;
  };
  /** L5 ANALYTICS (2026-07-09, flag-OFF): symmetrical-loop (cycle-consistency)
   *  invariant over a state snapshot. When set, the governor round-trips the
   *  `features` vector through PCA (Decompose→Reconstruct) and raises
   *  `cycleBroken` on a symmetry breach (F(G(x))≠x). Off unless provided.
   *  Deterministic. Pairs with pcaAnomaly but has a different job: anomaly=
   *  "is input weird?", cycle= "is the round-trip lossless?". */
  cycleConsistency?: {
    model: import('./integration/analytics/cycle-consistency.ts').PCA;
    cfg: import('./integration/analytics/cycle-consistency.ts').CycleConsistencyConfig;
  };
  /** D2 ICA→governor telemetry stage (flag-OFF, 2026-07-09). When supplied, the governor runs each
   *  incoming `features` vector through the fitted ICA+cycle-consistency pipeline and surfaces the
   *  localized SUBSYSTEM fault (index into the separated sources, not a raw channel). The pipeline
   *  is calibrated offline from known-good telemetry via `buildTelemetryICAPipeline`; the live
   *  connector (feeding Dowiz telemetry rows into `features`) is operator-wired in apps/api.
   *  `icaFaultError` = symmetry-gap threshold above which a fault is surfaced (the locator always
   *  reports a candidate `breakAt`, so we gate on the actual reconstruction error, not its presence). */
  icaTelemetry?: TelemetryICAPipeline;
  icaFaultError?: number;
  /** N2 liveness contract (2026-07-09, flag-OFF). If the stochastic agent (control-plane
   *  advisor) goes silent for longer than `watchdogMs`, the kernel drops to SAFE STATE:
   *  authority floored to uMin and `safeState=true`. This is the "Heartbeat / Watchdog"
   *  from the Sandbox-Paradox research — a probabilistic advisor that hangs mid-thought
   *  must not hold the wheel. Off unless supplied. When supplied, `step()` must be called
   *  with the current monotonic clock ms (a 4th arg) OR the caller must feed `t` itself;
   *  if no time is ever supplied the watchdog is inert (cannot false-trip on a missing clock). */
  watchdogMs?: number;
}

export class Governor {
  cfg: GovernorConfig;
  pid: PIDState = { integral: 0, prevError: 0, u: 0 };
  private predAct: Array<[number, number]> = []; // trailing (pred,act) pairs for IC
  private icSeries: number[] = [];
  private volHistory: number[] = [];
  private _meanIC = 0;
  anomaly = false;
  resonanceRisky = false;
  thermoFloorHit = false;
  private last!: GovernorState;
  // L5 analytics (flag-OFF): EMA-floored PCA-reconstruction anomaly state
  pcaAnomaly = false;
  private pcaState: PcaAnomalyState | null = null;
  // L5 analytics (flag-OFF): symmetrical-loop (cycle-consistency) state
  cycleBroken = false;
  private cycleState: CycleConsistencyGateState | null = null;
  // D2 ICA→governor: localized subsystem fault index
  subsystemFault = -1;
  // N2 liveness contract (flag-OFF): Safe State flag + last advisory clock
  safeState = false;
  private lastAdvisoryMs: number | null = null;
  // N7 hybrid-bridge observability (flag-OFF): counts a rejected advisor advice
  // and smooths analytics latency. Inert unless you read bridgeMetrics() — no
  // behavior change to the governor's control surface.
  private totalSteps = 0;
  private rejectedAdvices = 0;
  private analyticsLatencyEma = 0;

  constructor(cfg: GovernorConfig) {
    this.cfg = cfg;
    this.last = { authority: (cfg.uMin + cfg.uMax) / 2, pidU: 0, icir: null, factorStatus: 'unknown', resonanceRisky: false, anomaly: false, thermoFloorHit: false, error: 0, pcaAnomaly: false, cycleBroken: false, subsystemFault: -1, safeState: false, agentSilentMs: 0 };
  }

  get authority(): number { return this.last.authority; }
  get state(): GovernorState { return this.last; }

  private pushFactor(pred: number, act: number): number | null {
    this.predAct.push([pred, act]);
    const W = 8;
    if (this.predAct.length > W) this.predAct.shift();
    if (this.predAct.length < 4) return null;
    const ic = spearman(this.predAct.map((p) => p[0]), this.predAct.map((p) => p[1]));
    this.icSeries.push(ic);
    const L = 16;
    if (this.icSeries.length > L) this.icSeries.shift();
    this._meanIC = this.icSeries.reduce((s, x) => s + x, 0) / this.icSeries.length;
    return icir(this.icSeries);
  }

  factorStatus(icirV: number | null): FactorStatus {
    if (icirV === null) return 'unknown'; // insufficient telemetry — neither trust nor kill
    const kill = this.cfg.deadIC ?? this.cfg.icirKill ?? 0.05; // ICIR below this ⇒ 'dead'
    if (icirV < kill) return 'dead'; // proven zero predictive power → kill-switch
    if (icirV < this.cfg.icirVolatile) return 'volatile';
    return 'healthy';
  }

  step(s: TelemetrySample, nowMs?: number): GovernorState {
    // N2 liveness contract (flag-OFF): if the agent has been silent longer than
    // watchdogMs, drop to SAFE STATE. We only arm the watchdog once a clock has
    // ever been supplied (lastAdvisoryMs !== null) so a caller that never passes
    // time cannot false-trip on a missing clock. `nowMs` should be a monotonic
    // clock (performance.now()); it is intentionally NOT Date.now() to keep the
    // unit-test surface deterministic.
    let agentSilentMs = 0;
    if (nowMs !== undefined && nowMs !== null && Number.isFinite(nowMs)) {
      // each step with a valid clock IS a heartbeat; measure gap vs the prior
      // advisory, then advance the heartbeat to "now" so a responsive agent
      // never accumulates silence. Only arms after the first clocked step.
      if (this.lastAdvisoryMs === null) this.lastAdvisoryMs = nowMs;
      agentSilentMs = Math.max(0, nowMs - this.lastAdvisoryMs);
      this.lastAdvisoryMs = nowMs;
    }
    const watchdogArmed = this.cfg.watchdogMs !== undefined && this.lastAdvisoryMs !== null;
    const silentTooLong = watchdogArmed && agentSilentMs > (this.cfg.watchdogMs ?? 0);
    // FAILOUT/Poison guard (RED-TEAM finding 2026-07-09): a non-finite sample (NaN/Infinity from a
    // degraded upstream) must NOT corrupt the integral accumulator — once NaN enters `this.pid`,
    // every future `authority` is NaN (silent poison that the L5 authority gate trusts). On bad input
    // we floor authority to uMin and return a safe state WITHOUT integrating the bad sample.
    if (!Number.isFinite(s.predictedQuality) || !Number.isFinite(s.actualQuality) || !Number.isFinite(s.cost) || !Number.isFinite(s.volume)) {
      return (this.last = {
        authority: this.cfg.uMin,
        pidU: 0,
        icir: this._meanIC || null,
        factorStatus: 'dead',
        resonanceRisky: this.resonanceRisky,
        anomaly: true,
        thermoFloorHit: false,
        error: NaN,
        poisoned: true,
        subsystemFault: -1,
        safeState: silentTooLong,
        agentSilentMs: watchdogArmed ? agentSilentMs : 0,
      } as GovernorState);
    }
    const error = this.cfg.targetQuality - s.actualQuality;
    const c = this.cfg;
    const { u, integral } = pidStep(c, this.pid, error);
    this.pid = { integral, prevError: error, u: u };

    const icirV = this.pushFactor(s.predictedQuality, s.actualQuality);
    const status = this.factorStatus(icirV);

    // dead factor → kill-switch: floor authority (reduce exposure), no integral growth
    let authority = u;
    if (status === 'dead') authority = c.uMin;
    if (status === 'volatile') authority = Math.min(authority, (c.uMin + c.uMax) / 2);

    // N2 liveness contract (flag-OFF): if the agent has been silent past watchdogMs,
    // drop to SAFE STATE — floor authority regardless of what the advisor computed.
    // A responsive agent resets the heartbeat every clocked step, so this only fires
    // when the gap between two advisories exceeds the budget (the advisor "hung").
    const safeState = silentTooLong;
    if (safeState) authority = c.uMin;

    // resonance: estimate closed-loop ζ of the PROPOSED step; if risky, cap the change magnitude
    const res = loopResonance(c.kp, c.kd, c.plantM, c.plantB, c.samplePeriod ?? 0);
    this.resonanceRisky = res.risky;
    const maxStep = c.maxStep ?? (c.uMax - c.uMin);
    if (res.risky) authority = clamp(authority, this.cfg.uMin, this.cfg.uMin + maxStep * 0.2);

    // anomaly on volume channel
    this.volHistory.push(s.volume);
    const VH = c.volHistoryLen ?? 32;
    if (this.volHistory.length > VH) this.volHistory.shift();
    this.anomaly = detectAnomaly(this.volHistory.slice(0, -1), s.volume, c.anomalyK ?? 3);

    // thermodynamics: cost is in RESOURCE-UNITS; bitsErased is a lower bound on the units that
    // MUST be spent to touch that volume. Cross-unit compare against Joules (landauerFloor) was a
    // dimensional mismatch (F4 in the determinism handoff) — thinking isn't free, so you must spend
    // ≥1 unit per bit erased. Stay in resource-unit space.
    const floor = bitsErased(s.volume);
    this.thermoFloorHit = s.cost < floor;

    // L5 ANALYTICS (flag-OFF): PCA-reconstruction anomaly. Only active when the
    // caller BOTH (a) supplied a `features` vector AND (b) configured `pcaAnomaly`.
    // The adaptive EMA threshold learns out slow drift (battery/weather) and flags
    // only SHARP excursions — the deterministic twin of the prompt's ELBO anomaly.
    const tAnalytics0 = performance.now();
    if (this.cfg.pcaAnomaly && s.features && s.features.length === this.cfg.pcaAnomaly.model.mean.length) {
      const prev = this.pcaState ? this.pcaState.threshold : 0;
      const prevStep = this.pcaState ? this.pcaState.step : 0;
      const st = pcaAnomalyScore(this.cfg.pcaAnomaly.model, s.features, this.cfg.pcaAnomaly.cfg, prev, prevStep);
      this.pcaState = st;
      this.pcaAnomaly = st.flag;
    }

    // L5 ANALYTICS (flag-OFF): symmetrical-loop / cycle-consistency. Active only
    // when the caller BOTH (a) supplied `features` AND (b) configured
    // `cycleConsistency`. Round-trips the feature vector (Decompose→Reconstruct);
    // a symmetry breach means a module dropped/corrupted a field. Adaptive EMA
    // floor learns slow drift, flags only sharp asymmetries. Off by default.
    if (this.cfg.cycleConsistency && s.features && s.features.length === this.cfg.cycleConsistency.model.mean.length) {
      const prev = this.cycleState ? this.cycleState.threshold : 0;
      const prevStep = this.cycleState ? this.cycleState.step : 0;
      const st = cycleConsistencyGate(this.cfg.cycleConsistency.model, s.features, this.cfg.cycleConsistency.cfg, prev, prevStep);
      this.cycleState = st;
      this.cycleBroken = st.broken;
    }

    // D2 ICA→GOVERNOR (flag-OFF): run the raw telemetry `features` vector through the fitted
    // ICA+cycle-consistency pipeline and localize the BROKEN SUBSYSTEM (source index after
    // unmixing), not a raw channel. Off unless cfg.icaTelemetry is supplied. The pipeline is
    // calibrated offline from known-good Dowiz telemetry (buildTelemetryICAPipeline); the live
    // feed is operator-wired in apps/api. Surface breakAt>=0 (a localized candidate) — the gate's
    // `broken` flag is a stricter confirm; we expose the candidate so ops can triage every hit.
    if (this.cfg.icaTelemetry && s.features && s.features.length === this.cfg.icaTelemetry.ica.mean.length) {
      const r = scoreTelemetrySample(this.cfg.icaTelemetry, s.features);
      const thr = this.cfg.icaFaultError ?? 1.0; // gate on real symmetry-gap, not the always-present candidate
      if (r.breakAt >= 0 && r.error > thr) this.subsystemFault = r.breakAt;
    }
    // N7: smooth the analytics pass latency (EMA). Honest telemetry, never gated on.
    const analyticsMs = performance.now() - tAnalytics0;
    this.analyticsLatencyEma = 0.1 * analyticsMs + 0.9 * this.analyticsLatencyEma;

    // N7 hybrid-bridge observability: a "rejected advice" = the kernel overrode the
    // advisor's requested authority `u` (safe-state floor, dead-factor kill, resonance cap,
    // or any clamp). This is the honest "hallucination rate" the dump's architect test asks for.
    this.totalSteps += 1;
    const rejected = authority < u - 1e-12; // advisor asked for MORE than the kernel granted
    if (rejected) this.rejectedAdvices += 1;
    const hallucinationRate = this.totalSteps > 0 ? this.rejectedAdvices / this.totalSteps : 0;

    return (this.last = { authority: clamp(authority, c.uMin, c.uMax), pidU: u, icir: icirV, factorStatus: status, resonanceRisky: this.resonanceRisky, anomaly: this.anomaly, thermoFloorHit: this.thermoFloorHit, error, pcaAnomaly: this.pcaAnomaly, cycleBroken: this.cycleBroken, subsystemFault: this.subsystemFault, safeState, agentSilentMs: watchdogArmed ? agentSilentMs : 0, rejectedAdvices: this.rejectedAdvices, hallucinationRate });
  }

  /**
   * N7 hybrid-bridge observability (flag-OFF): the "is the system degrading 10 min
   * before it fails?" surface. Returns the running reject/hallucination counters and
   * the smoothed analytics latency. Pure read — no effect on control. When no step()
   * has run yet the rate is 0 (no division by zero).
   */
  bridgeMetrics(): { totalSteps: number; rejectedAdvices: number; hallucinationRate: number; analyticsLatencyMs: number } {
    return {
      totalSteps: this.totalSteps,
      rejectedAdvices: this.rejectedAdvices,
      hallucinationRate: this.totalSteps > 0 ? this.rejectedAdvices / this.totalSteps : 0,
      analyticsLatencyMs: this.analyticsLatencyEma,
    };
  }
}

// ── closed-loop plant sim (deterministic proof: governor drives a 1st-order agent to setpoint) ──

/** y_{k+1} = y_k + a·(authority_k − y_k). Returns final error after `steps`. */
export function simulatePlant(authoritySeries: number[], a = 0.3, y0 = 0): number[] {
  const ys: number[] = [y0];
  for (const u of authoritySeries) ys.push(ys[ys.length - 1] + a * (u - ys[ys.length - 1]));
  return ys;
}
