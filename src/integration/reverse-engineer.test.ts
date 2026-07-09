// reverse-engineer.test.ts — tensor+graph repo RE harness (RED+GREEN).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { buildRepoGraph, repoTensorSearch, reverseEngineer } from './reverse-engineer.ts';

function makeRepo(): string {
  const dir = mkdtempSync(path.join(tmpdir(), 're-'));
  writeFileSync(path.join(dir, 'a.ts'), "import x from './b.ts'; import y from './c.ts';");
  writeFileSync(path.join(dir, 'b.ts'), "import z from './c.ts';");
  writeFileSync(path.join(dir, 'c.ts'), 'export const k = 1;');
  writeFileSync(path.join(dir, 'orphan.ts'), 'export const o = 2;');
  return dir;
}

test('GREEN: buildRepoGraph builds adjacency + per-node tensors deterministically', () => {
  const dir = makeRepo();
  const g = buildRepoGraph(dir);
  assert.ok(g.nodes.length >= 4);
  assert.equal(g.tensors.length, g.nodes.length);
  rmSync(dir, { recursive: true, force: true });
});

test('GREEN: repoTensorSearch joins graph proximity + tensor similarity (overlay)', () => {
  const dir = makeRepo();
  const g = buildRepoGraph(dir);
  const hits = repoTensorSearch(g, 'b');
  assert.ok(hits.length > 0);
  assert.ok(hits[0].rel === 'b' || hits.some((h) => h.rel === 'b'));
  assert.ok(hits.every((h) => h.graphDist >= 0 && h.tensorSim >= -1 && h.tensorSim <= 1));
  assert.ok(hits.every((h, i) => i === 0 || hits[i - 1].score >= h.score)); // sorted desc
  rmSync(dir, { recursive: true, force: true });
});

test('RED: repoTensorSearch on a term with NO match returns [] (no hallucinated hit)', () => {
  const dir = makeRepo();
  const g = buildRepoGraph(dir);
  const hits = repoTensorSearch(g, 'zzz-nonexistent-xyz');
  assert.equal(hits.length, 0);
  rmSync(dir, { recursive: true, force: true });
});

test('GREEN: reverseEngineer returns downstream blast-radius + orphan flag in one shot', () => {
  const dir = makeRepo();
  const g = buildRepoGraph(dir);
  const c = reverseEngineer(g, 'c'); // c is imported by a and b
  assert.equal(c.found, true);
  assert.ok(c.downstream.includes('repo:a') || c.downstream.includes('repo:b'));
  assert.equal(c.isOrphan, false);
  const orphan = reverseEngineer(g, 'orphan');
  assert.equal(orphan.isOrphan, true);
  rmSync(dir, { recursive: true, force: true });
});

test('RED: reverseEngineer on unknown target reports not-found (no fabricated map)', () => {
  const dir = makeRepo();
  const g = buildRepoGraph(dir);
  const m = reverseEngineer(g, 'does-not-exist');
  assert.equal(m.found, false);
  assert.equal(m.downstream.length, 0);
  rmSync(dir, { recursive: true, force: true });
});
