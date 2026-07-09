// scripts/bench-field-vs-tree.mjs — runs the telemetry sweep and prints the report.
// Deterministic. FLAG-OFF measurement only (monotonic clock; no Date/RNG in math).
import { sweep } from '../src/integration/benchmark-field-vs-tree.ts';

// Telemetry matrix: probe many conditions (scale × density × mode × k).
const conds = [
  { n: 100, density: 0.1, mode: 'diffuse', k: 5, steps: 24 },
  { n: 100, density: 0.3, mode: 'diffuse', k: 5, steps: 24 },
  { n: 500, density: 0.1, mode: 'diffuse', k: 5, steps: 24 },
  { n: 500, density: 0.3, mode: 'wave', k: 5, steps: 24 },
  { n: 1000, density: 0.1, mode: 'diffuse', k: 5, steps: 24 },
  { n: 1000, density: 0.2, mode: 'wave', k: 5, steps: 24 },
  { n: 2500, density: 0.1, mode: 'diffuse', k: 5, steps: 24 },
  { n: 5000, density: 0.1, mode: 'diffuse', k: 5, steps: 24 },
];

const rows = sweep(conds);
const pad = (s, w) => String(s).padStart(w);

console.log('\n=== TELEMETRY: tensor+graph field/optical/VSA search vs traditional k-d tree (binary-tree) search ===');
console.log('Task: k-nearest-neighbor by structural tensor; PLUS change-impact prediction (field-only — k-d tree has no column).\n');
console.log(
  pad('n', 6), pad('dens', 6), pad('mode', 7),
  '|', pad('kdtBuild', 9), pad('kdtQ', 8), pad('kdtMem', 9), pad('kdtRec', 8),
  '|', pad('fldBuild', 9), pad('fldPred', 9), pad('fldMem', 9), pad('fldAff', 7),
  '|', pad('optQ', 8), pad('optRec', 8),
  '|', pad('vsaQ', 8), pad('vsaRec', 8),
);
for (const r of rows) {
  console.log(
    pad(r.cond.n, 6), pad(r.cond.density, 6), pad(r.cond.mode, 7),
    '|', pad(r.kdtree.buildMs.toFixed(3), 9), pad(r.kdtree.queryMs.toFixed(3), 8), pad(r.kdtree.memoryBytes, 9), pad(r.kdtree.recallAtK.toFixed(2), 8),
    '|', pad(r.field.buildMs.toFixed(3), 9), pad(r.field.predictMs.toFixed(3), 9), pad(r.field.memoryBytes, 9), pad(r.field.predictedAffected, 7),
    '|', pad(r.optical.queryMs.toFixed(3), 8), pad(r.optical.recallAtK.toFixed(2), 8),
    '|', pad(r.vsa.queryMs.toFixed(3), 8), pad(r.vsa.recallAtK.toFixed(2), 8),
  );
}
console.log('\nColumns: build/query/predict in ms · mem in bytes · rec = recall@k vs the field ground-truth impacted set.');
console.log('kdtRec = k-d tree recall on the GRAPH ripple (its blind spot); fldAff = nodes the field predicts affected.');
console.log('optRec/vsaRec = optical/VSA overlap with the field ground-truth (content-addressable recovery of the ripple).');
