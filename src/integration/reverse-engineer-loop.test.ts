// reverse-engineer-loop.test.ts — brain-inside-brain RE (multipilot overlay over the RE map).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { reverseEngineerLoop } from './reverse-engineer-loop.ts';

function makeRepo(): string {
  const dir = mkdtempSync(path.join(tmpdir(), 'rel-'));
  writeFileSync(path.join(dir, 'a.ts'), "import x from './b.ts';");
  writeFileSync(path.join(dir, 'b.ts'), 'export const k = 1;');
  return dir;
}

test('GREEN: reverseEngineerLoop converges (all 3 axes approve) for a coupled target', async () => {
  const dir = makeRepo();
  const r = await reverseEngineerLoop(dir, 'b');
  assert.equal(r.multipilot.overlay, 'converged');
  assert.equal(r.multipilot.promote, true);
  assert.ok(r.map.found);
  rmSync(dir, { recursive: true, force: true });
});

test('RED: reverseEngineerLoop diverges (surfaced, not averaged) when map is missing', async () => {
  const dir = makeRepo();
  const r = await reverseEngineerLoop(dir, 'ghost');
  assert.equal(r.map.found, false);
  assert.equal(r.multipilot.overlay, 'divergent'); // truth axis rejects the missing target
  rmSync(dir, { recursive: true, force: true });
});
