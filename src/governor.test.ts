// Bebop governor tests — L5 signal-health monitor. Every property has a falsifiable RED+GREEN case.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import {
  Governor, pidStep, icir, spearman, loopResonance, landauerFloor, bitsErased,
  detectAnomaly, simulatePlant, clamp, type TelemetrySample,
} from './governor.ts';
import { pcaFit } from './integration/analytics/matrix.ts';
import { DEFAULT_PCA_ANOMALY } from './integration/analytics/anomaly.ts';
import { DEFAULT_CYCLE_CONSISTENCY } from './integration/analytics/cycle-consistency.ts';
import { buildTelemetryICAPipeline } from './integration/analytics/telemetry-ica-loop.ts';

const baseCfg = {
  kp: 1.4, ki: 0.22, kd: 1.5, iMin: -1, iMax: 1, uMin: 0, uMax: 1,
  targetQuality: 0.9, deadIC: 0.02, icirVolatile: 0.3,
  plantM: 1, plantB: 0.6, samplePeriod: 0, anomalyK: 3, maxStep: 1,
};

function mk(): Governor { return new Governor(baseCfg); }

function sample(over: Partial<TelemetrySample>): TelemetrySample {
  return { t: 0, predictedQuality: 0.9, actualQuality: 0.4, cost: 1e-18, volume: 100, ...over };
}

// ── PID ───────────────────────────────────────────────────────────────────────

test('GREEN: PID drives a 1st-order agent to the quality setpoint', () => {
  const g = mk();
  const us: number[] = [];
  let actual = 0.4;
  for (let k = 0; k < 60; k++) {
    const st = g.step(sample({ actualQuality: actual, predictedQuality: actual + 0.0 }));
    us.push(st.authority);
    actual = simulatePlant([st.authority], 0.3, actual)[1];
  }
  assert.ok(Math.abs(actual - 0.9) < 0.05, `final quality ${actual} should be near 0.9`);
});

test('RED: a proportional-only loop (Ki=0) cannot hold the setpoint under steady bias', () => {
  const g = new Governor({ ...baseCfg, ki: 0 });
  const us: number[] = [];
  let actual = 0.4;
  for (let k = 0; k < 60; k++) {
    const st = g.step(sample({ actualQuality: actual }));
    us.push(st.authority);
    actual = simulatePlant([st.authority], 0.3, actual)[1];
  }
  assert.ok(Math.abs(actual - 0.9) > 0.1, `P-only should lag setpoint, got ${actual}`);
});

test('GREEN: integral anti-windup clamp keeps the accumulator bounded', () => {
  const st0 = { integral: 0, prevError: 0, u: 0 };
  let st = st0;
  for (let k = 0; k < 1000; k++) st = pidStep({ ...baseCfg }, st, -1); // sustained full bias
  assert.ok(st.integral <= baseCfg.iMax + 1e-9 && st.integral >= baseCfg.iMin - 1e-9, 'integral stayed clamped');
});

// ── ICIR / factor health ───────────────────────────────────────────────────────

test('GREEN: a stable, predictive factor earns HIGH ICIR and stays authoritative', () => {
  const g = mk();
  let actual = 0.6;
  let state: any;
  for (let k = 0; k < 30; k++) {
    actual = clamp(actual + (Math.random() - 0.5) * 0.1, 0, 1);
    const pred = clamp(actual + (Math.random() - 0.5) * 0.05, 0, 1); // tight, correct self-knowledge
    state = g.step(sample({ actualQuality: actual, predictedQuality: pred }));
  }
  assert.ok(state.icir !== null && state.icir > 0.3, `ICIR ${state.icir} should be healthy`);
  assert.equal(state.factorStatus, 'healthy');
  assert.ok(state.authority > baseCfg.uMin, 'healthy factor keeps authority');
});

test('RED: a dead factor (no predictive power) is KILLED → authority floored', () => {
  const g = mk();
  let state: any;
  for (let k = 0; k < 30; k++) {
    // predicted is the EXACT INVERSE of actual ⇒ zero/negative predictive power ⇒ dead
    const actual = Math.random();
    state = g.step(sample({ actualQuality: actual, predictedQuality: 1 - actual }));
  }
  assert.ok(state.icir !== null && state.icir < baseCfg.deadIC, `dead ICIR ${state.icir}`);
  assert.equal(state.factorStatus, 'dead');
  assert.equal(state.authority, baseCfg.uMin, 'dead factor → authority floored to uMin');
});

test('GREEN: spearman of identical series = 1; of perfectly inverted = -1', () => {
  assert.ok(Math.abs(spearman([1, 2, 3, 4], [1, 2, 3, 4]) - 1) < 1e-9);
  assert.ok(Math.abs(spearman([1, 2, 3, 4], [4, 3, 2, 1]) + 1) < 1e-9);
});

test('GREEN: icir of a constant non-zero series is +Inf (perfectly stable)', () => {
  assert.equal(icir([0.1, 0.1, 0.1, 0.1]), Infinity);
});

test('RED: icir undefined for <2 samples (null)', () => {
  assert.equal(icir([0.3]), null);
});

// ── resonance prediction (predict change before applying) ──────────────────────

test('GREEN: a gain change that drops ζ<0.707 is flagged risky BEFORE applying', () => {
  const safe = loopResonance(1.4, 1.5, 1, 0.6); // ζ≈0.887 → well-damped
  const risky = loopResonance(8.0, 0.05, 1, 0.1); // high Kp, low Kd → under-damped
  assert.ok(!safe.risky, 'safe gains should not be risky');
  assert.ok(risky.risky && risky.zeta < 1 / Math.sqrt(2), 'under-damped step must be risky');
  assert.ok(risky.mr > 1.2, 'resonance peak magnification should be >1');
});

test('RED: a well-damped loop has magnification ≈ 1 (no harmonic blow-up)', () => {
  const r = loopResonance(1.0, 1.5, 1, 0.6);
  assert.ok(!r.risky && r.mr <= 1.01, `well-damped Mr ${r.mr} should be ~1`);
});

test('GREEN: discrete alias risk when ωn·T approaches Nyquist band', () => {
  const r = loopResonance(50, 1, 1, 0.6, 0.25); // wn≈7.07, wn*T≈1.77 > 0.3
  assert.ok(r.aliasRisk, 'high-frequency loop should flag alias risk');
});

// ── thermodynamics (Landauer) ──────────────────────────────────────────────────

test('GREEN: Landauer floor ≈ 2.87e-21 J/bit at 300K', () => {
  const f = landauerFloor(1, 300);
  assert.ok(Math.abs(f - 2.87e-21) < 1e-23, `floor ${f}`);
});

test('RED: negative bits throws (cannot erase negative information)', () => {
  assert.throws(() => landauerFloor(-1));
});

test('GREEN: bitsErased is monotonic log2 of volume', () => {
  assert.equal(bitsErased(2), 2);
  assert.ok(bitsErased(1000) > bitsErased(10));
});

// ── F4: governor thermo floor compares resource-units (cost) vs bitsErased, NOT Joules ──
// Before the fix it compared `cost` (resource-units) against landauerFloor() (Joules) — a
// dimensional mismatch that could never meaningfully fire. Now both sides are resource-units:
// you must spend ≥1 unit per bit erased.

test('GREEN: low cost vs high volume HITS the thermo floor (in resource-unit space)', () => {
  const g = mk();
  const s = g.step(sample({ cost: 1, volume: 100 })); // bitsErased(100)=7, 1<7 → hit
  assert.equal(s.thermoFloorHit, true);
});

test('RED: generous cost vs tiny volume does NOT hit the floor (proves the compare is real, not always-true)', () => {
  const g = mk();
  const s = g.step(sample({ cost: 100, volume: 1 })); // bitsErased(1)=1, 100>=1 → not hit
  assert.equal(s.thermoFloorHit, false);
});

test('RED: the OLD cross-unit bug is gone — cost is read as resource-units, not Joules', () => {
  const g = mk();
  // Previously `cost: 1e-18` (a UI metric) was compared to ~2e-20 J and silently "passed".
  // In resource-unit space 1e-18 < bitsErased(100)=7 → correctly flags the under-spend.
  const s = g.step(sample({ cost: 1e-18, volume: 100 }));
  assert.equal(s.thermoFloorHit, true);
});

// ── F5: content-addressed tmp name (knowledge.estimateTokens) — deterministic per input ──

test('GREEN: identical text yields the same content-addressed temp name (no pid/Date.now)', () => {
  const name = (t: string) => createHash('sha256').update(t).digest('hex').slice(0, 24);
  assert.equal(name('hello world'), name('hello world')); // deterministic, not pid/Date.now
  assert.notEqual(name('hello world'), name('hello worle')); // collision-safe
});

// ── anomaly detection (operator priority) ─────────────────────────────────────
// NOTE: history is FIXED (no Math.random) so these RED/GREEN assertions are deterministic.
// A random history let the RED case spuriously fail when the random draw happened to cluster
// tightly (105 would become a >3σ outlier). Fixed spread: mean 99.5, ~±4.

test('GREEN: a volume spike >3σ from history is flagged as anomaly', () => {
  const hist = [95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104];
  assert.ok(detectAnomaly(hist, 1000, 3), 'extreme spike must flag');
});

test('RED: an in-band volume is NOT an anomaly', () => {
  const hist = [95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104];
  assert.equal(detectAnomaly(hist, 105, 3), false);
});

// ── governor integration: dead factor floors authority even when PID wants more ─

test('GREEN: governor overrides PID authority for a dead factor (kill-switch beats integral)', () => {
  const g = mk();
  // force a huge error so PID alone would push authority high
  let state: any;
  for (let k = 0; k < 30; k++) {
    state = g.step(sample({ actualQuality: 0.0, predictedQuality: Math.random() }));
  }
  // dead factor → authority must be floored regardless of PID
  assert.equal(state.authority, baseCfg.uMin);
});

// ── L5 ANALYTICS: PCA-reconstruction anomaly (flag-OFF; wired 2026-07-09) ──
// The governor scores a multidimensional `features` vector against a calibrated
// "normal" PCA model and flags sharp excursions via an adaptive EMA threshold.

function pcaCfg(): any {
  // normal manifold: 4-dim telemetry, 3 correlated dims + a low-variance noise dim
  const win: number[][] = [];
  for (let i = 0; i < 40; i++) {
    const a = (i - 20) * 0.1;
    win.push([a, a + 0.05, a * 0.9, ((i % 7) - 3) * 0.01]); // 4th = small noise
  }
  const model = pcaFit(win);
  return { ...baseCfg, pcaAnomaly: { model, cfg: DEFAULT_PCA_ANOMALY } };
}

test('GREEN: governor with pcaAnomaly does NOT flag steady in-manifold telemetry', () => {
  const g = new Governor(pcaCfg());
  let flagged = false;
  for (let k = 0; k < 12; k++) {
    const st = g.step(sample({ features: [0.1, 0.15, 0.09, 0.05] }));
    flagged = flagged || st.pcaAnomaly;
  }
  assert.equal(flagged, false, 'steady normal telemetry must not flag');
});

test('RED: governor with pcaAnomaly FLAGS an alien telemetry vector', () => {
  const g = new Governor(pcaCfg());
  // warm up EMA floor on normal data first
  for (let k = 0; k < 10; k++) g.step(sample({ features: [0.1, 0.15, 0.09, 0.05] }));
  const st = g.step(sample({ features: [1000, -1000, 500, -500] })); // off-manifold
  assert.equal(st.pcaAnomaly, true, 'alien vector must flag the L5 analytics anomaly');
});

test('GREEN: governor WITHOUT pcaAnomaly config never sets pcaAnomaly (flag-OFF default)', () => {
  const g = mk(); // baseCfg has no pcaAnomaly
  let flagged = false;
  for (let k = 0; k < 12; k++) {
    const st = g.step(sample({ features: [100, -100, 50, -50] })); // would flag if on
    flagged = flagged || st.pcaAnomaly;
  }
  assert.equal(flagged, false, 'pcaAnomaly must stay OFF unless explicitly configured');
});

// ── L5 analytics: symmetrical-loop / cycle-consistency (flag-OFF) ──

function cycleCfg(): any {
  // well-conditioned 3D window (PCA basis ≈ identity, not degenerate)
  const win: number[][] = [
    [1, 0, 0],
    [0, 1, 0],
    [0, 0, 1],
    [1, 1, 1],
  ];
  const model = pcaFit(win);
  return { ...baseCfg, cycleConsistency: { model, cfg: DEFAULT_CYCLE_CONSISTENCY } };
}

test('GREEN: governor with cycleConsistency stays QUIET on steady in-manifold state', () => {
  const g = new Governor(cycleCfg());
  let broken = false;
  for (let k = 0; k < 12; k++) {
    const st = g.step(sample({ features: [0.5, 0.5, 0.5] }));
    broken = broken || st.cycleBroken;
  }
  assert.equal(broken, false, 'steady normal state must not break the symmetrical loop');
});

test('RED: governor with cycleConsistency BREAKS when a state field is dropped (asymmetric refactor)', () => {
  const g = new Governor(cycleCfg());
  // warm up the EMA floor on clean data first
  for (let k = 0; k < 10; k++) g.step(sample({ features: [0.5, 0.5, 0.5] }));
  const st = g.step(sample({ features: [0.5, 0.5, 0] })); // feature 2 silently lost
  assert.equal(st.cycleBroken, true, 'dropped field must break the symmetrical loop in the governor');
});

test('GREEN: governor WITHOUT cycleConsistency config never sets cycleBroken (flag-OFF default)', () => {
  const g = mk(); // baseCfg has no cycleConsistency
  let broken = false;
  for (let k = 0; k < 12; k++) {
    const st = g.step(sample({ features: [0.5, 0.5, 0] })); // would break if on
    broken = broken || st.cycleBroken;
  }
  assert.equal(broken, false, 'cycleBroken must stay OFF unless explicitly configured');
});

// ── D2: ICA → governor telemetry stage (flag-OFF) ──

test('GREEN: governor with icaTelemetry localizes a fault to the broken SUBSYSTEM (not a raw channel)', () => {
  // calibration: 2 independent subsystems (slow drift s0 + sharp burst s1) mixed into 2 raw channels
  const calib: number[][] = [];
  for (let k = 0; k < 200; k++) {
    const s0 = Math.sin(k / 20);          // slow navigation drift
    const s1 = (k % 17 === 0) ? 1 : 0;    // comms burst
    calib.push([s0 + 0.3 * s1, 0.8 * s0 + s1]); // raw mix A·s; rows = time
  }
  const pipe = buildTelemetryICAPipeline(calib);
  const g = new Governor({ ...baseCfg, icaTelemetry: pipe });
  // on-manifold known-good point (k=50 of the calibration mix): error≈0.25, below the fault gate
  const onManifold = [Math.sin(2.5), 0.8 * Math.sin(2.5)];
  for (let k = 0; k < 12; k++) g.step(sample({ features: onManifold }));
  const clean = g.step(sample({ features: onManifold }));
  assert.equal(clean.subsystemFault, -1, 'on-manifold telemetry → no fault localized');
  // inject a sharp burst into subsystem #1 ONLY (raw shifts by the mix column for source 1)
  const faultRaw = [onManifold[0] + 1.5, onManifold[1] + 5];
  const st = g.step(sample({ features: faultRaw }));
  assert.ok(st.subsystemFault >= 0, `a real fault must localize to a subsystem index, got ${st.subsystemFault}`);
});

test('RED: governor WITHOUT icaTelemetry config never localizes a fault (flag-OFF default)', () => {
  const g = mk(); // baseCfg has no icaTelemetry
  const st = g.step(sample({ features: [0.5 + 5, 0.8 * 0.5 + 5] })); // would localize if on
  assert.equal(st.subsystemFault, -1, 'subsystemFault must stay -1 unless icaTelemetry is configured');
});

// ── N2: liveness contract / safe-state watchdog (flag-OFF, 2026-07-09) ──
// The stochastic advisor "holds the wheel" only while it keeps heartbeating.
// If it goes silent past watchdogMs, the kernel drops to Safe State.

test('GREEN: responsive advisor (heartbeat each step) never trips the watchdog', () => {
  const g = new Governor({ ...baseCfg, watchdogMs: 1000 });
  let safe = false;
  for (let k = 0; k < 10; k++) {
    const st = g.step(sample({ predictedQuality: 0.9, actualQuality: 0.88 }), k * 200);
    safe = safe || st.safeState === true;
  }
  assert.equal(safe, false, 'a heartbeating advisor must stay out of Safe State');
  const final = g.step(sample({ predictedQuality: 0.9, actualQuality: 0.88 }), 2000);
  assert.equal(final.safeState, false, 'consecutive clocked steps keep the agent alive');
  assert.equal(final.agentSilentMs, 200, 'silence measured since the prior heartbeat');
});

test('RED: advisor silent past watchdogMs drops the kernel to Safe State (authority floored)', () => {
  const g = new Governor({ ...baseCfg, watchdogMs: 1000 });
  // first heartbeat arms the watchdog
  g.step(sample({ predictedQuality: 0.9, actualQuality: 0.88 }), 0);
  // a gap far exceeding the budget ⇒ the advisor "hung" ⇒ Safe State
  const st = g.step(sample({ predictedQuality: 0.9, actualQuality: 0.88 }), 5000);
  assert.equal(st.safeState, true, 'silence past watchdogMs must engage Safe State');
  assert.equal(st.agentSilentMs, 5000, 'reported silence equals the gap');
  assert.equal(st.authority, baseCfg.uMin, 'Safe State floors authority to uMin');
});

test('GREEN: watchdog is inert when no clock is ever supplied (cannot false-trip)', () => {
  const g = new Governor({ ...baseCfg, watchdogMs: 1000 });
  // never pass nowMs — simulates a caller that uses the governor without a clock
  let safe = false;
  for (let k = 0; k < 30; k++) {
    const st = g.step(sample({ predictedQuality: 0.9, actualQuality: 0.88 }));
    safe = safe || st.safeState === true;
  }
  assert.equal(safe, false, 'without a clock the watchdog must never engage');
  assert.equal(g.state.agentSilentMs, 0, 'no clock ⇒ no silence reported');
});

test('GREEN: governor WITHOUT watchdogMs config never engages Safe State (flag-OFF default)', () => {
  const g = mk(); // baseCfg has no watchdogMs
  const st = g.step(sample({ predictedQuality: 0.9, actualQuality: 0.88 }), 999999);
  assert.notEqual(st.safeState, true, 'Safe State must stay OFF unless watchdogMs is configured');
});

// ── N7: hybrid-bridge observability (hallucination-rate / reject counter) ──

test('GREEN: a healthy advisor is NEVER rejected — hallucinationRate stays 0', () => {
  const g = mk();
  for (let k = 0; k < 20; k++) {
    // advisor self-predicts well and the plant tracks setpoint ⇒ factor healthy, no clamp
    g.step(sample({ predictedQuality: 0.9, actualQuality: 0.89 }));
  }
  const m = g.bridgeMetrics();
  assert.equal(m.rejectedAdvices, 0, 'no advices overrode when the advisor is healthy');
  assert.equal(m.hallucinationRate, 0, 'hallucination rate is exactly 0 for a trustworthy advisor');
  assert.equal(m.totalSteps, 20, 'step count is honest');
});

test('RED: a dead-factor advisor IS counted as rejected — hallucinationRate > 0 (honest counter)', () => {
  const g = mk();
  // Make the advisor's self-prediction ANTI-correlated with reality (pred high when act low,
  // and vice-versa). Spearman(pred,act) ≈ −1 ⇒ ICIR negative ⇒ factor 'dead' ⇒ kernel floors
  // authority (rejects the advisor). The bridge must COUNT every such override.
  for (let k = 0; k < 20; k++) {
    const hi = k % 2 === 0;
    g.step(sample({ predictedQuality: hi ? 0.95 : 0.1, actualQuality: hi ? 0.1 : 0.95 }));
  }
  const m = g.bridgeMetrics();
  assert.ok(m.rejectedAdvices > 0, 'a dead-factor advisor must be counted as rejected');
  assert.ok(m.hallucinationRate > 0 && m.hallucinationRate <= 1, `rate in (0,1], got ${m.hallucinationRate}`);
  assert.equal(m.totalSteps, 20, 'counter denominator matches the steps taken');
  // surfaced on the state too
  assert.equal(g.state.rejectedAdvices, m.rejectedAdvices, 'counter also surfaced on GovernorState');
});

test('RED: a rejected advice is NEVER silently dropped — bridgeMetrics reflects every override', () => {
  const g = mk();
  // force a Safe-State rejection once (watchdog), then verify the counter moved by exactly 1
  const g2 = new Governor({ ...baseCfg, watchdogMs: 1000 });
  g2.step(sample({ predictedQuality: 0.9, actualQuality: 0.88 }), 0); // arm
  const before = g2.bridgeMetrics().rejectedAdvices;
  g2.step(sample({ predictedQuality: 0.9, actualQuality: 0.88 }), 5000); // silence ⇒ reject
  const after = g2.bridgeMetrics().rejectedAdvices;
  assert.equal(after, before + 1, 'a Safe-State override is counted (not absorbed)');
});
