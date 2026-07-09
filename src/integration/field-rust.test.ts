import test from 'node:test';
import assert from 'node:assert/strict';
import { rustBuild, rustSpectral, rustActive, rustVsaSimilarity } from './field-rust.ts';
import { laplacian } from './field-sim.ts';

// Build a path graph adjacency (the canonical test case for the Laplacian).
function pathAdj(n: number): number[][] {
  const A = Array.from({ length: n }, () => new Array(n).fill(0));
  for (let i = 0; i < n - 1; i++) { A[i][i + 1] = 1; A[i + 1][i] = 1; }
  return A;
}

test('rust spectral propagator converges to JS diffusion fixed point (GREEN)', async () => {
  const n = 20;
  const A = pathAdj(n);
  await rustBuild(A);

  // seed: impulse at node 0
  const u0 = new Float64Array(n);
  u0[0] = 1.0;

  // Rust ONE-SHOT spectral propagate for t=20 (the operator fix A: no K-loop)
  const rust = await rustSpectral(u0, 20.0, /*coeff*/ 1.0, /*deg*/ 40);

  // JS reference: many small explicit steps (the K-iteration baseline)
  const js = predictImpactLoop(A, u0, /*steps*/ 400, /*dt*/ 0.05); // 400*0.05 = 20

  const massR = Array.from(rust).reduce((a, b) => a + b, 0);
  const massJ = js.reduce((a, b) => a + b, 0);
  assert.ok(Math.abs(massR - 1.0) < 1e-2, `rust mass ${massR}`);      // Green: mass preserved
  assert.ok(Math.abs(massJ - 1.0) < 1e-2, `js mass ${massJ}`);

  // profile agreement (Chebyshev degree 40 should match explicit Euler closely)
  let maxDiff = 0;
  for (let i = 0; i < n; i++) maxDiff = Math.max(maxDiff, Math.abs(rust[i] - js[i]));
  assert.ok(maxDiff < 1e-2, `profile diff ${maxDiff}`);               // Green: same physical result
});

test('rust propagator is ONE call vs K iterations (the operator fix A)', async () => {
  const n = 50;
  const A = pathAdj(n);
  await rustBuild(A);
  const u0 = new Float64Array(n);
  u0[Math.floor(n / 2)] = 1.0;
  // a single spectral call replaces 200 explicit steps — verify it still spreads symmetrically
  const r = await rustSpectral(u0, 3.0, 1.0, 30);
  assert.ok(Math.abs(r[24] - r[26]) < 1e-3, 'spectral spread is symmetric about seed');
  assert.ok(r[25] > r[0], 'peak stays near seed (diffusion, not translation)');
});

test('rust active-set pruning collapses the active frontier (GREEN = less than full graph)', async () => {
  const n = 40;
  const A = pathAdj(n);
  await rustBuild(A);
  const u0 = new Float64Array(n);
  u0[0] = 1.0;
  const { activePermille } = await rustActive(u0, 30, { dt: 0.05, coeff: 1.0, eps: 1e-3 });
  // after 30 steps the ripple has left the far half of the path idle → pruning active < 1000
  assert.ok(activePermille < 950, `activePermille=${activePermille} (pruned ${1000 - activePermille}/1000)`);
});

test('rust spectral rejects deg<1 (RED / falsifiable error path)', async () => {
  const n = 20;
  const A = pathAdj(n);
  await rustBuild(A);
  const u0 = new Float64Array(n);
  u0[0] = 1.0;
  await assert.rejects(async () => {
    const r = await rustSpectral(u0, 1.0, 1.0, 0); // deg=0 → Rust returns 1 (error)
    if (r.length === 0) throw new Error('empty');    // deg<1 must not produce a field
  });
});

test('rust VSA similarity is exact for equal vectors, ~0 for orthogonal (falsifiable)', async () => {
  // bipolar ±1 hypervectors, deterministic pattern (no RNG) — the VSA representational basis
  const dim = 64;
  const a = new Float64Array(dim);
  const b = new Float64Array(dim);
  const c = new Float64Array(dim);
  for (let i = 0; i < dim; i++) {
    const s = i % 2 === 0 ? 1 : -1;       // a: +1,-1,+1,-1,...
    a[i] = s;
    b[i] = s;                              // b == a (identical)
    c[i] = i % 4 < 2 ? 1 : -1;            // c: orthogonal-ish pattern (different basis)
  }
  const simAA = await rustVsaSimilarity(a, b);
  const simAC = await rustVsaSimilarity(a, c);
  assert.ok(Math.abs(simAA - dim) < 1e-9, `self-sim=${simAA} (expect ${dim})`);   // GREEN
  assert.ok(Math.abs(simAC) < 1e-9, `orthogonal-sim=${simAC}`); // RED/tautology-free
});

// JS reference: K explicit Euler diffusion steps on the Laplacian (the baseline the operator flagged).
function predictImpactLoop(A: number[][], u0: Float64Array, steps: number, dt: number): number[] {
  const L = laplacian(A);
  const n = A.length;
  let u = Array.from(u0);
  for (let s = 0; s < steps; s++) {
    const nu = new Array(n).fill(0);
    for (let i = 0; i < n; i++) {
      let acc = 0;
      for (let j = 0; j < n; j++) acc += L[i][j] * u[j];
      nu[i] = u[i] - dt * acc; // explicit Euler heat eq (contractive)
    }
    u = nu;
  }
  return u;
}
