// modules.test.ts — module registry: versioning, relation graph, bounded change-log (RED+GREEN).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ModuleRegistry } from './modules.ts';

const SRC = [
  { id: 'repo:a', source: "import x from './b.ts';", isMarkdown: false },
  { id: 'repo:b', source: "import y from './c.ts';", isMarkdown: false },
  { id: 'repo:c', source: 'export const y = 1;', isMarkdown: false },
  { id: 'repo:orphan', source: 'export const z = 2;', isMarkdown: false },
];
const RELS = ['a', 'b', 'c', 'orphan'];

test('GREEN: relation graph records directional edges (who depends on whom)', () => {
  const reg = new ModuleRegistry();
  reg.load(SRC, RELS);
  const a = reg.get('a')!;
  // a imports b → a.dependsOn = [b]; nobody imports a → a.dependedOnBy = []
  assert.deepEqual(a.dependsOn, ['repo:b'], 'a imports b → a.dependsOn = [b]');
  assert.deepEqual(a.dependedOnBy, [], 'nobody imports a → a.dependedOnBy = []');
  const c = reg.get('c')!;
  // b imports c → c.dependedOnBy = [b]
  assert.deepEqual(c.dependedOnBy, ['repo:b']);
});

test('GREEN: blast radius is the transitive downstream closure (importers of X)', () => {
  const reg = new ModuleRegistry();
  reg.load(SRC, RELS);
  // changing c breaks b (imports c) and a (imports b, transitively depends on c)
  const radius = reg.blastRadius('c').sort();
  assert.deepEqual(radius, ['repo:a', 'repo:b']);
  // changing a breaks nobody (nothing imports a)
  assert.deepEqual(reg.blastRadius('a'), []);
});

test('RED: an orphan has empty blast radius + no dependents', () => {
  const reg = new ModuleRegistry();
  reg.load(SRC, RELS);
  assert.deepEqual(reg.blastRadius('orphan'), []);
  assert.deepEqual(reg.dependents('orphan'), []);
});

test('GREEN: recordChange bumps version + keeps a BOUNDED change-log (ring buffer)', () => {
  const reg = new ModuleRegistry();
  reg.load(SRC, RELS);
  const m = reg.get('a')!;
  const v0 = m.version;
  for (let t = 1; t <= 20; t++) reg.recordChange('a', `import x from './b.ts'; // tick ${t}`, t, 'edit');
  const m2 = reg.get('a')!;
  assert.notEqual(m2.version, v0, 'version bumped on content change');
  assert.ok(m2.changes.length <= 16, `change-log is bounded (cap 16), got ${m2.changes.length}`);
  assert.equal(m2.changes[0].tick, 5, 'oldest kept tick is 5 (20-16+1)');
});

test('RED: recordChange is idempotent on identical content (no version bump, no log)', () => {
  const reg = new ModuleRegistry();
  reg.load(SRC, RELS);
  const before = reg.get('a')!.version;
  const r = reg.recordChange('a', "import x from './b.ts';", 1, 'same');
  assert.equal(r!.version, before, 'no bump when content unchanged');
  assert.equal(r!.changes.length, 0);
});
