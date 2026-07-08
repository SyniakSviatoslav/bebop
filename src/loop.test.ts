import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { runLoop, subagent, type BebopConfig } from './loop.ts';
import type { HookSpec } from './hooks.ts';

function tmp(): string {
  const dir = path.join(os.tmpdir(), `bebop-loop-test-${Math.random().toString(36).slice(2)}`);
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

// A stub LLM that issues an edit then done.
function editThenDone(file: string): BebopConfig['llm'] {
  let n = 0;
  return () => {
    if (n++ === 0) return { content: '', tool_calls: [{ name: 'edit', args: { path: file, content: 'mutated' } }] };
    return { content: 'done', tool_calls: [{ name: 'done', args: {} }] };
  };
}

test('GREEN: edit allowed within scope (no plan, no hooks)', async () => {
  const dir = tmp();
  const file = path.join(dir, 'safe.txt');
  fs.writeFileSync(file, 'orig');
  const res = await runLoop({ cwd: dir, taskClass: 'doer', llm: editThenDone(file), scope: ['**'] });
  assert.equal(res.denied, 0);
  assert.equal(res.mutations, 1);
  assert.equal(fs.readFileSync(file, 'utf8'), 'mutated');
});

test('RED: plan mode denies edit (read-only)', async () => {
  const dir = tmp();
  const file = path.join(dir, 'safe.txt');
  fs.writeFileSync(file, 'orig');
  const res = await runLoop({ cwd: dir, taskClass: 'doer', llm: editThenDone(file), scope: ['**'], planMode: true });
  assert.equal(res.denied, 1, 'edit must be denied in plan mode');
  assert.equal(res.mutations, 0);
  assert.equal(fs.readFileSync(file, 'utf8'), 'orig', 'file unchanged in plan mode');
});

test('RED: PreToolUse hook can deny the edit', async () => {
  const dir = tmp();
  const file = path.join(dir, 'safe.txt');
  fs.writeFileSync(file, 'orig');
  const denyEdit: HookSpec[] = [{ matcher: 'edit', command: 'true', run: () => ({ code: 2, stdout: '' }) }];
  const res = await runLoop({ cwd: dir, taskClass: 'doer', llm: editThenDone(file), scope: ['**'], hooks: denyEdit });
  assert.equal(res.denied, 1, 'hook must deny');
  assert.equal(res.mutations, 0);
});

test('GREEN: PreToolUse allow hook passes edit through', async () => {
  const dir = tmp();
  const file = path.join(dir, 'safe.txt');
  fs.writeFileSync(file, 'orig');
  const allowEdit: HookSpec[] = [{ matcher: 'edit', command: 'true', run: () => ({ code: 0, stdout: '' }) }];
  const res = await runLoop({ cwd: dir, taskClass: 'doer', llm: editThenDone(file), scope: ['**'], hooks: allowEdit });
  assert.equal(res.denied, 0);
  assert.equal(res.mutations, 1);
});

test('subagent runs read-only and returns a summary without mutating', async () => {
  const dir = tmp();
  const file = path.join(dir, 'safe.txt');
  fs.writeFileSync(file, 'orig');
  const r = await subagent('review the safe file', { cwd: dir });
  assert.ok(r.summary.includes('[subagent]'), 'summary carries the delegated marker');
  assert.equal(fs.readFileSync(file, 'utf8'), 'orig', 'subagent never mutates');
});
