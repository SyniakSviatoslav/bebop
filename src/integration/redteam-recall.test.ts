// RED-TEAM: RECALL (memory) + GOVERNOR (telemetry) attack surface.
//
// Run:  node --test --import tsx src/integration/redteam-recall.test.ts
//
// Conventions used here:
//   GREEN  — a behavior the system SHOULD have and DOES: the assertion passes (the
//            exploit is correctly blocked / the signal ranks honestly).
//   RED    — a red-team verification that an attack is caught. These must PASS;
//            they prove the F2 fix (graph truth dominates, optical only re-ranks
//            equal-score bands) and the optical edge-case guard actually hold.
//   BUG    — a RED test labelled BUG proving a REAL weakness exists in current code.
//            These are EXPECTED to FAIL (no false-green). They are reported, not fixed.
//
// The module-level livingMemory() singleton is isolated to a temp file so this suite
// never pollutes the operator's real memory.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import os from 'node:os';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';

// Isolate the shared living-memory singleton to a throwaway path BEFORE any code
// path touches it (livingMemory() reads BEBOP_MEMORY_PATH lazily on first call).
const TMP_MEM = path.join(os.tmpdir(), `redteam-recall-${process.pid}-${Date.now()}.json`);
process.env.BEBOP_MEMORY_PATH = TMP_MEM;

import { recall, rememberLocal } from '../knowledge.ts';
import { opticalRecall } from '../integration/optical/field-recall.ts';
import { thinLensMask } from '../integration/optical/optic.ts';
import { Governor } from '../governor.ts';
import { livingMemory } from '../memory.ts';

const GOV_CFG = {
  kp: 1.4, ki: 0.22, kd: 1.5, iMin: -1, iMax: 1, uMin: 0, uMax: 1,
  targetQuality: 0.9, deadIC: 0.02, icirVolatile: 0.3,
  plantM: 1, plantB: 0.6, samplePeriod: 0, anomalyK: 3, maxStep: 1,
};

// ─────────────────────────────────────────────────────────────────────────────
// GREEN: opticalRecall throws on query/mask dimension mismatch (no silent mis-rank)
// ─────────────────────────────────────────────────────────────────────────────
test('GREEN: opticalRecall throws on query dimension mismatch (no silent mis-rank)', () => {
  const mask = thinLensMask(8, 0.5, 2); // 8x8 → expects 64-length vectors
  assert.throws(
    () => opticalRecall([1, 2, 3], [[1, 2, 3]], mask),
    /query must be n\*n/,
  );
});

test('GREEN: opticalRecall throws on candidate dimension mismatch', () => {
  const mask = thinLensMask(8, 0.5, 2);
  const q = new Array(64).fill(0);
  assert.throws(
    () => opticalRecall(q, [[1, 2, 3]], mask),
    /candidate dim mismatch/,
  );
});

// ─────────────────────────────────────────────────────────────────────────────
// RED (attack #1): memory poisoning — optical tertiary signal must NOT promote a
// planted node above a genuine exact-match (graph score = 1.0) node. F2 fix.
// ─────────────────────────────────────────────────────────────────────────────
test('RED: poisoned optical-correlated node cannot outrank a genuine exact-match under opticalRecall', () => {
  const QUERY = 'fermat last theorem';
  // Genuine concept node (exact match → graph score 1.0).
  rememberLocal(QUERY, 'genuine fermat concept payload');
  // Planted node engineered (by text repetition) to score HIGH under the optical signal.
  rememberLocal('decoy plant node', `${QUERY} ${QUERY} ${QUERY} ${QUERY} ${QUERY}`);

  const mem = livingMemory();
  const genuineId = mem.recall(QUERY, 3)[0];
  const plantedId = mem.nearest('decoy plant node', 1)[0]?.id;
  // Link planted → genuine so it even enters the spreading-activation set (score 0.5).
  if (genuineId && plantedId) mem.rememberLink(genuineId, plantedId);

  const res = recall(QUERY, { opticalRecall: true });
  assert.ok(res.found, 'recall should find the genuine node');
  // Note: the planted node's PAYLOAD is the repeated query; 'decoy plant node' is its concept.
  const idxGenuine = res.hits.findIndex((h) => h.text.includes('genuine fermat'));
  const idxPlanted = res.hits.findIndex((h) => h.text.includes(QUERY));
  assert.ok(idxGenuine >= 0, 'genuine node present');
  assert.ok(idxPlanted >= 0, 'planted node present (poisoned)');
  // F2 invariant: graph truth dominates; optical only re-orders EQUAL primary scores.
  assert.ok(
    idxGenuine <= idxPlanted,
    `poisoned optical node must not outrank genuine exact-match (genuine@${idxGenuine} planted@${idxPlanted})`,
  );
  // And the genuine node must still be #1.
  assert.strictEqual(res.hits[0].text, 'genuine fermat concept payload', 'genuine exact-match must rank #1');
  assert.strictEqual(res.hits[0].score, 1, 'genuine exact-match score is 1.0');
});

// ─────────────────────────────────────────────────────────────────────────────
// GREEN (attack #2): opticalRecall determinism — same input → identical ordering
// ─────────────────────────────────────────────────────────────────────────────
test('GREEN: opticalRecall is deterministic (no RNG) across repeated calls', () => {
  const n = 8;
  const mask = thinLensMask(n, 0.5, 2);
  const q = new Array(n * n).fill(0).map((_, i) => Math.sin(i) * 0.5);
  const cands = [
    new Array(n * n).fill(0).map((_, i) => Math.cos(i)),
    new Array(n * n).fill(0).map((_, i) => Math.sin(i * 2)),
    new Array(n * n).fill(0).map((_, i) => i / (n * n)),
  ];
  const a = opticalRecall(q, cands, mask);
  const b = opticalRecall(q, cands, mask);
  const c = opticalRecall(q, cands, mask);
  assert.deepStrictEqual(a, b, 'second call must match first');
  assert.deepStrictEqual(a, c, 'third call must match first');
});

test('GREEN: recall({opticalRecall:true}) ordering is stable across repeated calls', () => {
  const QUERY = 'fermat last theorem';
  rememberLocal(QUERY, 'genuine fermat concept payload');
  rememberLocal('decoy plant node', `${QUERY} ${QUERY} ${QUERY}`);
  const mem = livingMemory();
  const genuineId = mem.recall(QUERY, 3)[0];
  const plantedId = mem.nearest('decoy plant node', 1)[0]?.id;
  if (genuineId && plantedId) mem.rememberLink(genuineId, plantedId);

  const o1 = recall(QUERY, { opticalRecall: true }).hits.map((h) => h.id);
  const o2 = recall(QUERY, { opticalRecall: true }).hits.map((h) => h.id);
  assert.deepStrictEqual(o1, o2, 'recall ordering must be identical across calls');
});

// ─────────────────────────────────────────────────────────────────────────────
// GREEN (attack #4, CLI half): `bebop govern` clamps + warns on out-of-range, and
// the printed authority is finite (never masquerades as authoritative NaN/Infinity).
// ─────────────────────────────────────────────────────────────────────────────
test('GREEN: `bebop govern` clamps+warns out-of-range and prints finite authority', () => {
  // Resolve the repo root from THIS file: dirname = src/integration, then ../.. → repo root.
  const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
  // Run via `node --import tsx` (tsx is a loader, not a standalone .bin in this layout — proven via CLI).
  const out = execFileSync(
    process.execPath,
    ['--import', 'tsx', 'bebop.ts', 'govern', '2', '1.5', '-0.3'],
    { cwd: REPO_ROOT, encoding: 'utf8', env: { ...process.env, BEBOP_MEMORY_PATH: TMP_MEM } },
  );
  assert.match(out, /out of range/i, 'must warn about out-of-range samples');
  assert.match(out, /clamped/i, 'must state values were clamped');
  const m = out.match(/final authority=([-\d.eE]+)/);
  assert.ok(m, 'final authority printed');
  const authority = Number(m[1]);
  assert.ok(Number.isFinite(authority), 'printed authority must be finite');
  assert.ok(authority >= 0 && authority <= 1, 'authority within [0,1]');
});

// ─────────────────────────────────────────────────────────────────────────────
// GREEN (attack #4, class half): the Governor class now guards non-finite telemetry
// internally (RED-TEAM fix 2026-07-09). Fed NaN it emits a SAFE finite authority (uMin)
// and does NOT corrupt the integral — later finite, healthy samples recover.
// ─────────────────────────────────────────────────────────────────────────────
test('GREEN: Governor.step with NaN input produces finite authority + flags poisoned', () => {
  const gov = new Governor(GOV_CFG);
  const st = gov.step({ t: 0, predictedQuality: NaN, actualQuality: NaN, cost: 1e-18, volume: 100 });
  assert.ok(
    Number.isFinite(st.authority),
    `Governor fed NaN emitted authority=${st.authority} (non-finite)`,
  );
  assert.equal(st.authority, GOV_CFG.uMin, 'NaN sample must floor authority to uMin (fail-closed)');
  assert.equal(st.poisoned, true, 'poisoned flag must be set on non-finite input');
});

test('GREEN: Governor NaN input does NOT corrupt later finite samples (recovers)', () => {
  const gov = new Governor(GOV_CFG);
  gov.step({ t: 0, predictedQuality: NaN, actualQuality: NaN, cost: 1e-18, volume: 100 });
  const after = gov.step({ t: 1, predictedQuality: 0.9, actualQuality: 0.9, cost: 1e-18, volume: 100 });
  // Recovery = no poison leak: authority is finite and the poisoned flag clears.
  assert.ok(
    Number.isFinite(after.authority),
    `NaN then healthy 0.9 sample yields authority=${after.authority} (non-finite) — NaN leaked through integral`,
  );
  assert.notEqual(after.poisoned, true, 'poisoned flag must clear on a valid sample (no residual NaN rot)');
});

// NOTE: a FRESH Governor fed Infinity alone lands at authority=0 (finite) because
// error = 0.9 - Infinity = -Infinity is naturally clamped by pidStep to uMin. So
// Infinity is NOT independently a weakness when the Governor starts clean — the
// real, reproducible class-level hole is specifically NaN, captured by the two
// tests above. (Infinity-in-the-middle-of-a-stream would also NaN-rot via the
// integral in the same way NaN does.)

// ─────────────────────────────────────────────────────────────────────────────
// GREEN (attack #5): honest degradation on EMPTY query. recall('') must NOT fabricate
// a confident hit (RED-TEAM fix 2026-07-09): empty/whitespace is treated as no-query →
// found=false / zero hits. (Previously findByConcept('') substring-matched every node.)
// ─────────────────────────────────────────────────────────────────────────────
test('GREEN: recall("") degrades honestly (found=false / 0 hits), no fabricated hits', () => {
  const res = recall('');
  assert.strictEqual(
    res.found, false,
    `empty query returned found=${res.found} with ${res.hits.length} hits — fabricates a confident recall`,
  );
  assert.strictEqual(res.hits.length, 0, 'empty query should yield zero hits');
});

test('GREEN: recall("   ") whitespace-only also degrades honestly', () => {
  const res = recall('   ');
  assert.strictEqual(res.found, false, 'whitespace-only query must not match');
  assert.strictEqual(res.hits.length, 0);
});

test('GREEN: recall of garbage input is well-formed, deterministic, and never throws', () => {
  // The honest-degradation fix (F7) is fully proven by the empty/whitespace tests above, which
  // deterministically return found=false / 0 hits regardless of corpus. For arbitrary gibberish we
  // assert the invariants that ARE guaranteed: recall never throws, returns a structured result
  // (found:boolean, hits:array), and is deterministic for the same query. (A VSA cosine match
  // against the seed corpus is legitimate, so we do NOT assert found=false here.)
  const gibberish = 'zqxwkplmvy nqrstuv 7g2h9a1f qwpoeiruty alskdjfhgzmxncbv 0k3l8m5n2 tziryqxwkplm';
  let threw = false;
  let r1: any, r2: any;
  try {
    r1 = recall(gibberish);
    r2 = recall(gibberish);
  } catch (e) {
    threw = true;
    console.error(e);
  }
  assert.equal(threw, false, 'recall must not throw on garbage input');
  assert.equal(typeof r1.found, 'boolean', 'found must be a boolean');
  assert.ok(Array.isArray(r1.hits), 'hits must be an array');
  assert.deepEqual(
    r1.hits.map((h: any) => h.id),
    r2.hits.map((h: any) => h.id),
    'recall must be deterministic for the same query',
  );
});
