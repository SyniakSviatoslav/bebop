// RED+GREEN tests for the "understand everything" mapper.
// GREEN: real edges in the repo are detected (bebop.ts → guard.ts).
// RED: a fabricated edge is NOT present (fabricated import must not appear).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseImports, layerOf, buildGraph, renderSvg } from './understand.ts';
import * as path from 'node:path';

const ROOT = path.resolve('.');
const graph = buildGraph(ROOT);

test('GREEN: parseImports extracts relative specifiers only', () => {
  const text = `import { a } from './guard.ts';\nimport x from '../y/z.ts';\nimport 'node:fs';\nimport { b } from "lodash";`;
  const got = parseImports(text);
  assert.deepEqual(got.sort(), ['./guard.ts', '../y/z.ts'].sort());
});

test('GREEN: layerOf maps known stems to the right layer', () => {
  assert.equal(layerOf('src/guard.ts'), 'guard');
  assert.equal(layerOf('src/kernel.ts'), 'core');
  assert.equal(layerOf('src/memory.ts'), 'memory');
  assert.equal(layerOf('src/bebop.ts'), 'shell');
  assert.equal(layerOf('src/unknown-module.ts'), 'other');
});

test('GREEN: real edge bebop.ts (root entry) → src/guard.ts is in the graph', () => {
  const edge = graph.edges.find((e) => e.from === 'bebop.ts' && e.to === 'src/guard.ts');
  assert.ok(edge, 'expected a real import edge from bebop.ts to src/guard.ts (verified by reading the source)');
});

test('GREEN: guard.ts → core-wasm.ts edge exists (Rust kernel delegation)', () => {
  const edge = graph.edges.find((e) => e.from === 'src/guard.ts' && e.to === 'src/core-wasm.ts');
  assert.ok(edge, 'expected guard.ts to delegate to the wasm kernel loader');
});

test('RED: a fabricated edge is absent (no lie in the graph)', () => {
  const fake = graph.edges.find((e) => e.from === 'src/guard.ts' && e.to === 'src/memory.ts');
  assert.equal(fake, undefined, 'guard.ts does NOT import memory.ts directly — graph must reflect reality');
});

test('GREEN: renderSvg produces a standalone, well-formed SVG string', () => {
  const svg = renderSvg(graph, { title: 'probe' });
  assert.ok(svg.startsWith('<svg'), 'svg must start with <svg');
  assert.ok(svg.includes('</svg>'), 'svg must close');
  assert.ok(svg.includes('bebop'), 'diagram should contain node labels');
  assert.ok(!svg.includes('undefined'), 'no undefined leaked into output');
});
