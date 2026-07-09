import test from 'node:test';
import assert from 'node:assert/strict';
import { rustBuild, rustSpectral, rustActive, rustVsaSimilarity, rustDispose, rustMemoryBytes, rustFieldCost, rustFieldRank, rustFieldArbiter } from './field-rust.ts';
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

// ── MEMORY DISCIPLINE (2026-07-09): leak-free graph lifecycle across a long-running agent ──
// RED: if build/propagate/dispose leaked, the wasm heap would grow monotonically across cycles.
// GREEN: after dispose the stored graph is dropped, so repeated cycles must NOT grow the heap.
test('rust memory is stable across build→propagate→dispose cycles (no leak)', async () => {
  const n = 200;
  const A = pathAdj(n);
  const u0 = new Float64Array(n);
  u0[0] = 1.0;

  await rustBuild(A);
  await rustSpectral(u0, 5.0, 1.0, 20);
  const baseline = await rustMemoryBytes(); // heap after first real workload

  // 100 rebuild + propagate + dispose cycles on the SAME graph size.
  for (let c = 0; c < 100; c++) {
    await rustBuild(A);
    await rustSpectral(u0, 5.0, 1.0, 20);
    await rustActive(u0, 10, { dt: 0.1, coeff: 1.0, eps: 1e-3 });
    await rustDispose();
  }

  const after = await rustMemoryBytes();
  // Heap is allowed to grow (wasm never shrinks a live buffer), but it must NOT keep growing per
  // cycle. A true leak would make `after` explode far beyond `baseline` + one page. We assert the
  // growth is bounded to at most a single 64KiB page over 100 cycles (i.e. effectively stable).
  const growth = after - baseline;
  assert.ok(growth <= 65536, `heap grew ${growth} bytes over 100 cycles (leak!)`);
});

// RED: disposing then propagating without a rebuild must NOT silently return stale data — the
// kernel must REFUSE (error code) because no graph is stored. This proves dispose actually freed
// the state rather than leaving a dangling graph.
test('rust dispose clears state — no stale graph lingers (RED: compute must refuse on empty)', async () => {
  const n = 30;
  const A = pathAdj(n);
  const u0 = new Float64Array(n);
  u0[0] = 1.0;
  await rustBuild(A);
  const before = await rustSpectral(u0, 5.0, 1.0, 20);
  const massBefore = Array.from(before).reduce((a, b) => a + b, 0);
  assert.ok(Math.abs(massBefore - 1.0) < 1e-2, `mass before dispose=${massBefore}`); // sanity

  await rustDispose();
  // After dispose STATE is empty (n=0). field_spectral must return rc=1 and rustSpectral must throw.
  // If dispose left a dangling graph, this would silently return a field → RED would be violated.
  await assert.rejects(
    () => rustSpectral(u0, 5.0, 1.0, 20),
    /error code 1/,
    'dispose must leave the kernel with no graph to propagate'
  );

  // re-build restores correctness (round-trip integrity)
  await rustBuild(A);
  const restored = await rustSpectral(u0, 5.0, 1.0, 20);
  const massR = Array.from(restored).reduce((a, b) => a + b, 0);
  assert.ok(Math.abs(massR - 1.0) < 1e-2, `mass after rebuild=${massR}`);
});

// ── PDDL ↔ FIELD BRIDGE (2026-07-09b): field-as-cost-function + final arbiter ──

test('rust field_cost conserves mass under uniform sensitivity (GREEN)', async () => {
  const n = 20;
  const A = pathAdj(n);
  await rustBuild(A);
  const seed = new Float64Array(n);
  seed[0] = 1.0; // impulse disruption at node 0
  const cost = await rustFieldCost(seed, { t: 20, deg: 40 });
  assert.ok(Math.abs(cost - 1.0) < 1e-2, `uniform-sensitivity cost=${cost} (expect ≈1)`);
});

test('rust field_rank mass equals field_cost (GREEN: rank is the per-node breakdown)', async () => {
  const n = 25;
  const A = pathAdj(n);
  await rustBuild(A);
  const seed = new Float64Array(n);
  seed[0] = 1.0;
  const cost = await rustFieldCost(seed, { t: 10, deg: 30 });
  const rank = await rustFieldRank(seed, { t: 10, deg: 30 });
  const rankMass = Array.from(rank).reduce((a, b) => a + b, 0);
  assert.ok(Math.abs(rankMass - cost) < 1e-9, `rank mass=${rankMass} vs cost=${cost}`);
});

test('rust field_cost rises with a sensitivity spike at the ripple frontier (GREEN)', async () => {
  const n = 40;
  const A = pathAdj(n);
  await rustBuild(A);
  const seed = new Float64Array(n);
  seed[0] = 1.0;
  const base = await rustFieldCost(seed, { t: 5, deg: 30 });
  const sens = new Float64Array(n).fill(1.0);
  sens[20] = 5.0; // weight where the disruption has spread by t=5
  const weighted = await rustFieldCost(seed, { t: 5, deg: 30, sensitivity: sens });
  assert.ok(weighted > base, `sensitivity spike must raise cost: base=${base} weighted=${weighted}`);
});

test('rust arbiter OVERRIDES when field impact vastly exceeds PDDL estimate (RED→GREEN: physics wins)', async () => {
  const n = 30;
  const A = pathAdj(n);
  await rustBuild(A);
  const seed = new Float64Array(n);
  seed[0] = 1.0; // a disruption PDDL thinks is cheap
  // PDDL underestimates (pddlCost tiny) while the field says it ripples far → OVERRIDE.
  const res = await rustFieldArbiter(seed, 0.01, { t: 30, deg: 40, mismatchRatio: 1.5 });
  assert.equal(res.verdict, 'override', `expected override, got ${res.verdict} (${res.reason})`);
  assert.ok(res.fieldCost > res.pddlCost, `field ${res.fieldCost} should beat pddl ${res.pddlCost}`);
});

test('rust arbiter PERMITS when field impact is within PDDL tolerance (GREEN: field concurs)', async () => {
  const n = 30;
  const A = pathAdj(n);
  await rustBuild(A);
  const seed = new Float64Array(n);
  seed[0] = 1.0;
  // PDDL estimate comfortably above the field cost → field is quiet → permit.
  const res = await rustFieldArbiter(seed, 5.0, { t: 2, deg: 24, mismatchRatio: 1.5 });
  assert.equal(res.verdict, 'permit', `expected permit, got ${res.verdict} (${res.reason})`);
});

test('rust arbiter WARNS when field exceeds PDDL but within the mismatch band (GREEN: grey zone)', async () => {
  const n = 30;
  const A = pathAdj(n);
  await rustBuild(A);
  const seed = new Float64Array(n);
  seed[0] = 1.0;
  // pddlCost chosen so fieldCost (≈1.0) lands between pddlCost and pddlCost*1.5.
  const res = await rustFieldArbiter(seed, 0.8, { t: 2, deg: 24, mismatchRatio: 1.5 });
  assert.equal(res.verdict, 'warn', `expected warn, got ${res.verdict} (${res.reason})`);
  assert.ok(res.fieldCost > res.pddlCost, 'field should exceed PDDL in the warn band');
});


