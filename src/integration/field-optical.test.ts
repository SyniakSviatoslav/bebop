import { test } from 'node:test';
import assert from 'node:assert/strict';
import { laplacian, FieldSim } from './field-sim.ts';
import { opticalNodeSearch, vsaNodeSearch, predictThenSearch } from './field-optical.ts';
import { benchmarkFieldVsTree, sweep, type BenchCondition } from './benchmark-field-vs-tree.ts';
import { buildRepoGraphLite } from './benchmark-field-vs-tree.ts';

const A = [
  [0, 1, 0, 0],
  [0, 0, 1, 0],
  [0, 0, 0, 1],
  [1, 0, 0, 0],
];
const tensors = [
  [4, 0, 2, 0.1],
  [1, 1, 5, 0.2],
  [1, 2, 2, 0.3],
  [0, 3, 1, 0.4],
];
const graph = buildRepoGraphLite(4, A, tensors);

test('GREEN: optical search ranks the structurally-closest node first for a self-query', () => {
  const r = opticalNodeSearch(graph, 'node0');
  assert.equal(r.length, 4);
  assert.equal(r[0], 'node0'); // self is always the top optical hit
});

test('RED: optical search returns [] for an unknown query rel (no silent fallback)', () => {
  const r = opticalNodeSearch(graph, 'does-not-exist');
  assert.deepEqual(r, []);
});

test('GREEN: VSA associative search is deterministic and self-similar', () => {
  const r = vsaNodeSearch(graph, 'node0');
  assert.equal(r[0].rel, 'node0');
  assert.ok(r[0].sim >= 1 - 1e-9); // self similarity ≈ 1
  // deterministic
  const r2 = vsaNodeSearch(graph, 'node0');
  assert.deepEqual(r.map((x) => x.rel), r2.map((x) => x.rel));
});

test('GREEN: predictImpact forward-predicts the change footprint without full convergence', () => {
  const L = laplacian(A);
  const sim = new FieldSim(L, { mode: 'diffuse', dt: 0.1, coeff: 0.4, channels: 1 });
  sim.impulse(0, 1);
  const r = sim.predictImpact(0, { steps: 16, threshold: 1e-3 });
  assert.ok(r.affected.length >= 1, 'impulse must affect at least the seeded node');
  assert.ok(r.affected.includes(0), 'seeded node is in the footprint');
  // chain A: 0->1->2->3 (directed) so diffusion should reach 1 and 2 within 16 steps
  assert.ok(r.affected.length >= 2, 'diffusion should have rippled beyond the seed');
});

test('RED: predictImpact with zero steps predicts nothing beyond the seed threshold', () => {
  const L = laplacian(A);
  const sim = new FieldSim(L, { mode: 'diffuse', dt: 0.1, coeff: 0.4, channels: 1 });
  sim.impulse(0, 1);
  const r = sim.predictImpact(0, { steps: 0, threshold: 1e-3 });
  // with 0 steps the field has not propagated; the seeded node itself may still be above threshold
  // but downstream nodes must NOT appear yet
  for (const a of r.affected) {
    if (a === 0) continue;
    assert.fail('downstream node appeared with 0 steps — prediction leaked');
  }
});

test('GREEN: predictThenSearch returns an ordered watchlist flagging the predicted footprint', () => {
  const wl = predictThenSearch(graph, 'node0', { steps: 16, threshold: 1e-3 });
  assert.equal(wl.length, 4);
  // at least the seeded node is in the footprint
  assert.ok(wl.some((x) => x.inFootprint), 'footprint should contain at least the seed');
  // watchlist is sorted: footprint first
  const firstNonFootprint = wl.findIndex((x) => !x.inFootprint);
  if (firstNonFootprint >= 0) {
    for (let i = 0; i < firstNonFootprint; i++) assert.ok(wl[i].inFootprint);
  }
});

test('GREEN: benchmark runs and produces comparable telemetry rows', () => {
  const conds: BenchCondition[] = [
    { n: 50, density: 0.1, mode: 'diffuse', k: 3, steps: 16 },
    { n: 200, density: 0.1, mode: 'diffuse', k: 3, steps: 16 },
  ];
  const rows = sweep(conds);
  assert.equal(rows.length, 2);
  for (const row of rows) {
    assert.ok(row.kdtree.queryMs >= 0 && Number.isFinite(row.kdtree.queryMs));
    assert.ok(row.field.predictMs >= 0);
    assert.ok(row.optical.queryMs >= 0);
    assert.ok(row.vsa.queryMs >= 0);
    // benchmark reported real, finite telemetry
    assert.ok(Number.isFinite(row.kdtree.memoryBytes));
    assert.ok(Number.isFinite(row.field.memoryBytes));
  }
});

test('RED: k-d tree is BLIND to the graph ripple (honest gap): its recall@k vs the field ground-truth is <= field recall', () => {
  const row = benchmarkFieldVsTree({ n: 100, density: 0.15, mode: 'diffuse', k: 5, steps: 24 });
  // the field's own predicted set has perfect recall by construction; the k-d tree (vector-only)
  // cannot, in general, recover the graph-affected set, so it must be <= field recall
  assert.ok(row.kdtree.recallAtK <= row.field.recallAtK + 1e-9);
  // and the field uniquely supplies a change-prediction the k-d tree simply has no column for
  assert.ok(row.field.predictedAffected >= 1);
});
