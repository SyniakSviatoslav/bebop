/**
 * arch-mine.test.ts — RED+GREEN falsifiable tests for the reverse-engineering
 * / architecture-mining harness (v2: namespaced edges, md wikilinks, gap
 * detectors, full-latent drift).
 *
 * GREEN: tightly-coupled same-namespace modules co-cluster; markdown wikilinks
 *        build edges; isolated + cycle detectors fire correctly.
 * RED:   cross-namespace imports do NOT merge into one node (the v1 bug);
 *        identical graphs have ~0 drift; non-trivial real scans produce a graph.
 */

import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  buildAdjacency,
  couplingClusters,
  architectureDrift,
  isolatedNodes,
  findCycle,
  extractImports,
  extractWikilinks,
  mineGraph,
  pointsOfFailure,
} from './arch-mine.ts';
import { scanProjects, reverseEngineeringLoop } from './loop.ts';

test('GREEN: extractImports + extractWikilinks parse both edge kinds', () => {
  const src = `import { a } from './a.ts';\nconst x = await import('./d.ts');`;
  assert.ok(extractImports(src).includes('./a.ts'));
  assert.ok(extractImports(src).includes('./d.ts'));
  assert.deepEqual(extractWikilinks('see [[note-one]] and [[note-two|label]]'), ['note-one', 'note-two']);
});

test('GREEN: same-namespace coupled modules co-cluster', () => {
  const mods = [
    { id: 'p:a', source: `import { b } from './b.ts';` },
    { id: 'p:b', source: `import { a } from './a.ts';` },
    { id: 'p:c', source: `// alone` },
  ];
  const adj = buildAdjacency(mods);
  assert.equal(adj.A[0][1], 1, 'a→b directed edge');
  assert.equal(adj.A[1][0], 1, 'b→a directed edge (mutual)');
  assert.equal(adj.A[0][2], 0, 'c isolated');
  const cl = couplingClusters(adj);
  assert.ok(cl.some((c) => c.members.includes('p:a') && c.members.includes('p:b')), 'a,b co-cluster');
  assert.equal(cl.some((c) => c.members.includes('p:c')), false, 'c never in a cluster');
});

test('RED: cross-namespace imports do NOT merge into one node (v1 false-merge bug)', () => {
  // dowiz vendors bebop: './bebop/src/kernel.ts' must NOT collapse into bebop:kernel
  const mods = [
    { id: 'bebop:kernel', source: `export const x = 1;` },
    { id: 'dowiz:bebop/src/kernel', source: `import { x } from './kernel.ts';` }, // relative, same prefix
    { id: 'dowiz:app', source: `import { x } from 'bebop/src/kernel.ts';` }, // bare, resolves to dowiz:bebop/src/kernel only
  ];
  const adj = buildAdjacency(mods);
  // dowiz:app → dowiz:bebop/src/kernel (same prefix, basename match); never → bebop:kernel
  assert.equal(adj.A[2][1], 1, 'dowiz:app binds to dowiz kernel (one-way)');
  assert.equal(adj.A[2][0], 0, 'dowiz:app does NOT bind to bebop:kernel (no false merge)');
  assert.equal(adj.A[0][1], 0, 'bebop:kernel stays separate');
  const cl = couplingClusters(adj);
  const merged = cl.some((c) => c.members.includes('bebop:kernel') && c.members.includes('dowiz:bebop/src/kernel'));
  assert.equal(merged, false, 'vendored + upstream kernel must NOT co-cluster');
});

test('GREEN: markdown wikilinks build same-namespace edges', () => {
  const mods = [
    { id: 'lm:a', source: `links to [[b]] and [[c]]`, isMarkdown: true },
    { id: 'lm:b', source: `[[a]]`, isMarkdown: true },
    { id: 'lm:c', source: `standalone`, isMarkdown: true },
  ];
  const adj = buildAdjacency(mods);
  assert.equal(adj.A[0][1], 1, 'a→b wikilink (directed)');
  assert.equal(adj.A[1][0], 1, 'b→a wikilink (mutual)');
  assert.equal(adj.A[0][2], 1, 'a→c wikilink (one-way)');
  const iso = isolatedNodes(adj);
  assert.equal(iso.length, 0, 'all three are linked (no orphans)');
});

test('GREEN: isolatedNodes finds true orphans', () => {
  const mods = [
    { id: 'p:a', source: `import { b } from './b.ts';` },
    { id: 'p:b', source: `import { a } from './a.ts';` },
    { id: 'p:orphan', source: `export const z = 9;` },
  ];
  const iso = isolatedNodes(buildAdjacency(mods));
  assert.ok(iso.includes('p:orphan'), 'orphan detected');
  assert.equal(iso.includes('p:a'), false, 'coupled node not isolated');
});

test('GREEN: findCycle detects a real (>=3) circular reference, ignores benign test<->impl back-edge', () => {
  const real = buildAdjacency([
    { id: 'p:a', source: `import { b } from './b.ts';` },
    { id: 'p:b', source: `import { c } from './c.ts';` },
    { id: 'p:c', source: `import { a } from './a.ts';` }, // a→b→c→a
  ]);
  const cyc = findCycle(real);
  assert.ok(cyc && cyc.length >= 3, `real cycle found: ${JSON.stringify(cyc)}`);
  // benign: test imports impl (mutual 2-edge) must NOT be a cycle
  const benign = buildAdjacency([
    { id: 'p:impl', source: `export const x = 1;` },
    { id: 'p:impl.test', source: `import { x } from './impl.ts';` },
  ]);
  assert.equal(findCycle(benign), null, 'test<->impl is not a circular dependency');
});

test('GREEN+RED: architectureDrift ~0 for identical graphs, >0 when edges added', () => {
  const base = buildAdjacency([
    { id: 'p:a', source: `import { b } from './b.ts';` },
    { id: 'p:b', source: `import { a } from './a.ts';` },
    { id: 'p:c', source: `// alone` },
  ]);
  const same = architectureDrift(base, base);
  assert.ok(same.shift < 1e-9, `identical ⇒ ~0 drift, got ${same.shift}`);
  const grown = buildAdjacency([
    { id: 'p:a', source: `import { b } from './b.ts'; import { c } from './c.ts';` },
    { id: 'p:b', source: `import { a } from './a.ts';` },
    { id: 'p:c', source: `import { a } from './a.ts';` },
  ]);
  const d = architectureDrift(base, grown);
  assert.ok(d.shift > 0, `adding edges ⇒ shift>0, got ${d.shift}`);
});

test('GREEN: real scan of bebop analytics + integration yields a non-trivial graph with no false merges', () => {
  const res = scanProjects([
    { path: '/root/bebop-repo/src/integration/analytics', prefix: 'bebop-analytics' },
    { path: '/root/bebop-repo/src/integration', prefix: 'bebop-int' },
  ]);
  assert.ok(res.scanned > 10, `scanned ${res.scanned}`);
  assert.ok(res.nodes.length > 10, `nodes ${res.nodes.length}`);
  assert.ok(res.clusters.length >= 1, 'at least one cluster');
  // namespaced guards: no node id should contain two different repo prefixes glued
  const bad = res.nodes.filter((n) => /bebop:[^:]*dowiz/.test(n) || /dowiz:[^:]*bebop/.test(n));
  assert.equal(bad.length, 0, 'no false cross-repo namespace merges');
});

test('GREEN: reverseEngineeringLoop runs end-to-end with gap detectors', () => {
  const res = reverseEngineeringLoop({ roots: [{ path: '/root/bebop-repo/src/integration', prefix: 'bebop-int' }] });
  assert.ok(res.scanned > 5);
  assert.ok(Array.isArray(res.isolated), 'isolated computed');
  assert.ok(res.cycle === null || Array.isArray(res.cycle), 'cycle computed');
});

test('GREEN+RED: cap is DYNAMIC — under the EVD ceiling every node is kept (no fabricated orphans); explicit cap still truncates', () => {
  // 12 modules, a chain p:0→p:1→…→p:11 (all reachable, none orphaned by structure)
  const mods = Array.from({ length: 12 }, (_, i) => ({
    id: `p:m${i}`,
    source: i < 11 ? `import { x } from './m${i + 1}.ts';` : `export const x = 1;`,
  }));
  // no explicit cap ⇒ dynamic cap = min(12, HARD_EVD_CEILING) ⇒ keep ALL 12
  const dyn = buildAdjacency(mods);
  assert.equal(dyn.nodes.length, 12, 'dynamic cap keeps all 12 nodes (fn=12 < ceiling)');
  // forcing a tiny cap MUST truncate (RED side: proves the truncation path is real,
  // and that the old static default would have been wrong for small graphs)
  const forced = buildAdjacency(mods, { cap: 4 });
  assert.ok(forced.nodes.length <= 4, 'explicit small cap truncates to ≤4');
  assert.ok(forced.nodes.length < 12, 'explicit cap removes nodes (dynamic would not)');
});

// ── D6: aggregate mineGraph report (flag-OFF archMine pass consumes this) ──

test('GREEN: mineGraph aggregates cycle + orphans + clusters in one deterministic report', () => {
  const rep = mineGraph([
    { id: 'p:a', source: "import x from './b.ts';" },
    { id: 'p:b', source: "import y from './c.ts';" },
    { id: 'p:c', source: "import z from './a.ts';" }, // 3-node cycle
    { id: 'p:orphan', source: '// alone' },
  ]);
  assert.equal(rep.moduleCount, 4, 'counts all modules');
  assert.ok(rep.cycle && rep.cycle.length >= 3, `detects the cycle, got ${JSON.stringify(rep.cycle)}`);
  assert.ok(rep.isolated.includes('p:orphan'), 'flags the orphan');
});

test('RED: mineGraph on an acyclic, fully-connected set reports no cycle and no orphans', () => {
  const rep = mineGraph([
    { id: 'p:a', source: "import x from './b.ts';" },
    { id: 'p:b', source: "import y from './c.ts';" },
    { id: 'p:c', source: '// leaf' },
  ]);
  assert.equal(rep.cycle, null, 'acyclic ⇒ no cycle');
  assert.equal(rep.isolated.length, 0, 'connected ⇒ no orphans');
});

// ── N4: causal counterfactual surface (pointsOfFailure) ──

test('GREEN: pointsOfFailure reports the blast-radius of a known dependency', () => {
  const adj = buildAdjacency([
    { id: 'p:core', source: "import { x } from './util.ts';" },
    { id: 'p:util', source: 'export const x = 1;' },
    { id: 'p:app', source: "import { x } from './core.ts';" },
  ]);
  const pof = pointsOfFailure(adj, 'p:core');
  assert.ok(pof, 'focus exists');
  assert.deepEqual(pof!.downstream.sort(), ['p:app'], 'app depends on core (would break if core changes)');
  assert.deepEqual(pof!.upstream, ['p:util'], 'core depends on util (core supply risk)');
  assert.equal(pof!.isOrphan, false, 'core is not orphaned');
});

test('GREEN: pointsOfFailure flags a cycle-participant and an orphan', () => {
  const adj = buildAdjacency([
    { id: 'p:a', source: "import { b } from './b.ts';" },
    { id: 'p:b', source: "import { c } from './c.ts';" },
    { id: 'p:c', source: "import { a } from './a.ts';" }, // a→b→c→a (real ≥3 cycle)
    { id: 'p:lonely', source: 'export const z = 1;' },
  ]);
  const inCycle = pointsOfFailure(adj, 'p:a');
  assert.ok(inCycle!.inCycle && inCycle!.inCycle.length >= 3, 'a is in a real cycle');
  const orphan = pointsOfFailure(adj, 'p:lonely');
  assert.equal(orphan!.isOrphan, true, 'lonely has no dependents/suppliers ⇒ safe to change');
});

test('RED: pointsOfFailure over a BROKEN edge is NOT silently absorbed (returns real blast-radius, not null)', () => {
  // focus exists but the "downstream" edge was intentionally dropped from the graph.
  // The function must still report based on what the graph CONTAINS (honest), and never
  // pretend a dependency is safe by returning a vacuous result.
  const adj = buildAdjacency([
    { id: 'p:core', source: 'export const x = 1;' }, // core now imports NOTHING
    { id: 'p:app', source: "import { x } from './core.ts';" }, // app still depends on core
  ]);
  const pof = pointsOfFailure(adj, 'p:core');
  assert.ok(pof, 'focus resolved even though it imports nothing');
  assert.deepEqual(pof!.downstream, ['p:app'], 'app→core edge is still surfaced (not absorbed)');
  assert.deepEqual(pof!.upstream, [], 'core supplies nothing after the edge was dropped (honest)');
});
