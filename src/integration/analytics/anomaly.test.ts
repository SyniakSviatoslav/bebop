/**
 * anomaly.test.ts — RED+GREEN falsifiable tests for the PCA-reconstruction
 * anomaly detector (the deterministic twin of the prompt's ELBO/VAE anomaly).
 *
 * GREEN: a sample drawn from the SAME manifold as the calibration window
 *   scores LOW and does NOT flag (even after the EMA threshold warms up).
 * RED:   an "alien" vector (far outside the manifold) scores HIGH and flags
 *   — proving the detector catches telemetry that "doesn't belong".
 * Also asserts the adaptive EMA threshold learns out a SLOW drift and only
 * flags a SHARP excursion (the prompt's explicit "don't use a constant
 * threshold" requirement).
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { pcaFit, pcaProject, pcaReconstruct } from './matrix.ts';
import { buildNormalModel, pcaAnomalyScore, calibrateLatentPrior, DEFAULT_PCA_ANOMALY } from './anomaly.ts';

// Calibration window: a 4-dim telemetry vector that lives on a thin manifold
// (3 correlated dims + a SMALL noise dim that the PCA should learn to drop).
function normalWindow(n = 40): number[][] {
  const out: number[][] = [];
  for (let i = 0; i < n; i++) {
    const a = (i - n / 2) * 0.1;
    // 4th dim is low-variance noise (±0.02) — the "don't-care" axis PCA drops
    out.push([a, a + 0.05, a * 0.9, ((i % 7) - 3) * 0.01]);
  }
  return out;
}

test('GREEN: an in-manifold sample does NOT flag (after EMA warmup)', () => {
  const model = buildNormalModel(normalWindow());
  const x = [0.1, 0.15, 0.09, 0.05]; // on the manifold
  let prev = 0, step = 0, flagged = false;
  // run several in-manifold samples so the EMA floor stabilizes AND warmup clears
  for (let k = 0; k < 12; k++) {
    const st = pcaAnomalyScore(model, x, DEFAULT_PCA_ANOMALY, prev, step);
    prev = st.threshold; step = st.step;
    flagged = flagged || st.flag;
  }
  assert.equal(flagged, false, 'in-manifold steady-state sample must not flag');
});

test('RED: an "alien" vector (off-manifold) DOES flag', () => {
  const model = buildNormalModel(normalWindow());
  const alien = [1000, -1000, 500, -500]; // physically impossible for this agent
  // warm up first so warmup doesn't mask the flag
  let prev = 0, step = 0;
  for (let k = 0; k < 10; k++) {
    const st = pcaAnomalyScore(model, [0.1, 0.15, 0.09, 0.05], DEFAULT_PCA_ANOMALY, prev, step);
    prev = st.threshold; step = st.step;
  }
  const st = pcaAnomalyScore(model, alien, DEFAULT_PCA_ANOMALY, prev, step);
  assert.ok(st.score > st.threshold, `alien score ${st.score} should exceed threshold ${st.threshold}`);
  assert.equal(st.flag, true, 'alien vector must flag');
});

test('GREEN: reconstruction error of an in-manifold sample is small', () => {
  const model = buildNormalModel(normalWindow());
  const x = [0.2, 0.25, 0.18, 0.1];
  const z = pcaProject(model, x);
  const xhat = pcaReconstruct(model, z);
  const err = Math.hypot(...x.map((v, i) => v - xhat[i]));
  assert.ok(err < 1e-6, `reconstruction error ${err} should be ~0`);
});

test('GREEN: adaptive EMA threshold LEARNS OUT a slow drift (no false flag)', () => {
  const model = buildNormalModel(normalWindow());
  let prev = 0, step = 0;
  // simulate a slowly degrading sensor: +0.001 per step over 50 steps.
  // The drift is smooth → the EMA floor tracks it → no sharp excursion → no flag.
  let flagged = false;
  for (let k = 0; k < 50; k++) {
    const drift = k * 0.001;
    const x = [0.1 + drift, 0.15 + drift, 0.09 + drift, 0.05 + drift];
    const st = pcaAnomalyScore(model, x, { ...DEFAULT_PCA_ANOMALY, emaAlpha: 0.2 }, prev, step);
    prev = st.threshold; step = st.step;
    flagged = flagged || st.flag;
  }
  assert.equal(flagged, false, 'slow drift must be absorbed by the EMA floor, not flagged');
});

test('RED: a SHARP excursion after slow drift DOES flag', () => {
  const model = buildNormalModel(normalWindow());
  let prev = 0, step = 0;
  // first warm up the EMA floor with slow drift (as above)
  for (let k = 0; k < 50; k++) {
    const drift = k * 0.001;
    const x = [0.1 + drift, 0.15 + drift, 0.09 + drift, 0.05 + drift];
    const st = pcaAnomalyScore(model, x, { ...DEFAULT_PCA_ANOMALY, emaAlpha: 0.2 }, prev, step);
    prev = st.threshold; step = st.step;
  }
  // now inject a sharp spike — the floor is NOT tracking this
  const st = pcaAnomalyScore(model, [5, 5, 5, 5], { ...DEFAULT_PCA_ANOMALY, emaAlpha: 0.2 }, prev, step);
  assert.equal(st.flag, true, 'sharp excursion after drift must flag');
});

test('RED: non-finite input is rejected (poison guard)', () => {
  const model = buildNormalModel(normalWindow());
  assert.throws(() => pcaAnomalyScore(model, [1, 2, NaN, 4], DEFAULT_PCA_ANOMALY, 0, 20));
});

// ── N3: β-VAE latent-prior calibration (offline, flag-OFF) ──

test('GREEN: calibrateLatentPrior on centered normal telemetry returns ok=true (β>0 is safe)', () => {
  // manifold: dim0 is the signal; dims1-3 are tiny noise (PCA drops them via k=d-1).
  const win = [];
  for (let i = 0; i < 40; i++) win.push([i * 0.03, (i % 5) * 0.002, ((i % 7) - 3) * 0.001, ((i % 3) - 1) * 0.001]);
  const model = buildNormalModel(win);
  const cal = calibrateLatentPrior(model, win);
  assert.equal(cal.ok, true, 'normal on-manifold data with ~0 latent mean must pass calibration');
  assert.ok(cal.maxAbsMean < 0.25, 'latent mean within prior bounds');
});

test('RED: calibrateLatentPrior on a NON-zero-mean latent window returns ok=false (β would false-positive)', () => {
  const win = [];
  for (let i = 0; i < 40; i++) win.push([i * 0.03, (i % 5) * 0.002, ((i % 7) - 3) * 0.001, ((i % 3) - 1) * 0.001]);
  const model = buildNormalModel(win);
  // shift ONLY the signal axis by a constant → still on-manifold, but latent mean ≠ 0
  const shifted = win.map((r) => [r[0] + 10, r[1], r[2], r[3]]);
  const cal = calibrateLatentPrior(model, shifted);
  assert.equal(cal.ok, false, 'off-prior latents must FAIL calibration (do not flip β>0)');
  assert.ok(cal.maxAbsMean > 0.25, 'the bias is actually measured and reported');
});

test('RED+GREEN: β>0 false-positives an ON-MANIFOLD off-prior sample; calibration pre-empts it', () => {
  // The doc's exact trap: Σzⱼ² flags a perfectly-NORMAL (on-manifold) sample whose latent
  // mean is merely non-zero. Here the sample is on-manifold (β=0 does NOT flag) but its
  // latent mean is 10 ⇒ with β=2 the latent-KL term blows past the EMA floor ⇒ false positive.
  const win = [];
  for (let i = 0; i < 40; i++) win.push([i * 0.03, (i % 5) * 0.002, ((i % 7) - 3) * 0.001, ((i % 3) - 1) * 0.001]);
  const model = buildNormalModel(win);
  const betaCfg = { ...DEFAULT_PCA_ANOMALY, beta: 2 };
  // warm the EMA floor on centered data so it cannot mask a flat offset
  let prev = 0, step = 0;
  for (let k = 0; k < 12; k++) {
    const st = pcaAnomalyScore(model, win[5], betaCfg, prev, step);
    prev = st.threshold; step = st.step;
  }
  const offsetNormal = win.map((r) => [r[0] + 10, r[1], r[2], r[3]]);
  const offB0 = pcaAnomalyScore(model, offsetNormal[5], { ...betaCfg, beta: 0 }, prev, step);
  assert.equal(offB0.flag, false, 'GREEN base: the offset sample is on-manifold ⇒ β=0 does NOT flag');
  const offB2 = pcaAnomalyScore(model, offsetNormal[5], betaCfg, prev, step);
  assert.equal(offB2.flag, true, 'RED: β>0 false-positives a normal (off-prior) sample');
  // the calibration harness catches this BEFORE you flip β>0
  const cal = calibrateLatentPrior(model, offsetNormal);
  assert.equal(cal.ok, false, 'calibration pre-empts the false-positive (ok=false ⇒ do not enable β)');
});
