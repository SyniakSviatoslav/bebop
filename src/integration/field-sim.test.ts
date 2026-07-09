// field-sim.test.ts — coupled graph-Laplacian field evolution (RED+GREEN).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { FieldSim, laplacian, blockLaplacian } from './field-sim.ts';

test('GREEN: laplacian of a path 1-2-3 has correct zero-row-sum (proper Laplacian)', () => {
  // A = 1-2-3 chain
  const A = [
    [0, 1, 0],
    [1, 0, 1],
    [0, 1, 0],
  ];
  const L = laplacian(A);
  for (const row of L) assert.equal(row.reduce((a, b) => a + b, 0), 0, 'each Laplacian row sums to 0');
  assert.deepEqual(L[1], [-1, 2, -1]);
});

test('GREEN: diffuse step decays an impulse (energy falls — memory fade)', () => {
  const A = [[0, 1, 0], [1, 0, 1], [0, 1, 0]];
  const L = laplacian(A);
  const sim = new FieldSim(L, { mode: 'diffuse', dt: 0.1, coeff: 0.5, channels: 1 });
  sim.impulse(1, 1); // seed the middle node
  const e0 = sim.energy();
  for (let i = 0; i < 20; i++) sim.step();
  const e1 = sim.energy();
  assert.ok(e1 < e0, `diffusion must decay energy: ${e0} -> ${e1}`);
});

test('GREEN: wave step conserves energy (oscillates, no decay) — the physical reconsider cycle', () => {
  const A = [[0, 1], [1, 0]];
  const L = laplacian(A);
  const sim = new FieldSim(L, { mode: 'wave', dt: 0.05, coeff: 0.2, channels: 1 });
  sim.impulse(0, 1);
  const e0 = sim.energy();
  sim.run(50);
  const e1 = sim.energy();
  assert.ok(Math.abs(e1 - e0) / e0 < 0.05, `wave should conserve energy: ${e0} -> ${e1}`);
});

test('RED: energy of an un-seeded field stays zero (no phantom signal)', () => {
  const L = laplacian([[0, 1], [1, 0]]);
  const sim = new FieldSim(L, { mode: 'diffuse' });
  sim.run(10);
  assert.equal(sim.energy(), 0);
});

test('GREEN: multi-channel tensor field carries C independent channels', () => {
  const L = laplacian([[0, 1], [1, 0]]);
  const sim = new FieldSim(L, { mode: 'diffuse', channels: 3, dt: 0.1, coeff: 0.3 });
  sim.impulse(0, 1, 0); // only channel 0 seeded
  sim.run(5);
  assert.ok(sim.u[0].some((x) => x > 0), 'channel 0 active');
  assert.ok(sim.u[1].every((x) => x === 0), 'channel 1 untouched (independent until coupling)');
});

test('GREEN: blockLaplacian couples two layers with inter-layer edges', () => {
  const A1 = [[0, 1], [1, 0]];
  const A2 = [[0, 1], [1, 0]];
  const C = [[[0, 0], [0, 0]], [[1, 0], [0, 1]]]; // layer0-node0 ↔ layer1-node0, node1↔node1
  const L = blockLaplacian([A1, A2], [C]);
  assert.equal(L.length, 4);
  // inter-layer coupling appears as off-diagonal -1 blocks
  assert.equal(L[0][2], -1);
  assert.equal(L[2][0], -1);
});
