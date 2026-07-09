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

// RED: `read` on a red-line path must be DENIED (confidentiality — the W1 exfiltration gap).
function readThenDone(file: string): BebopConfig['llm'] {
  let n = 0;
  return () => {
    if (n++ === 0) return { content: '', tool_calls: [{ name: 'read', args: { path: file } }] };
    return { content: 'done', tool_calls: [{ name: 'done', args: {} }] };
  };
}

test('RED: read of a red-line .env file is denied', async () => {
  const dir = tmp();
  const secret = path.join(dir, '.env');
  fs.writeFileSync(secret, 'API_KEY=supersecret');
  const res = await runLoop({ cwd: dir, taskClass: 'doer', llm: readThenDone('.env') });
  assert.equal(res.denied, 1, 'read of .env must be denied');
  assert.equal(res.mutations, 0);
});

test('RED: read of a migrations file is denied', async () => {
  const dir = tmp();
  const mig = path.join(dir, 'migrations', '002_users.sql');
  fs.mkdirSync(path.dirname(mig), { recursive: true });
  fs.writeFileSync(mig, 'SECRET migration');
  const res = await runLoop({ cwd: dir, taskClass: 'doer', llm: readThenDone('migrations/002_users.sql') });
  assert.equal(res.denied, 1, 'read of migration must be denied');
});

// RED: dispatch task string naming a red-line target must be denied before any backend runs (W2).
function dispatchThenDone(task: string): BebopConfig['llm'] {
  let n = 0;
  return () => {
    if (n++ === 0) return { content: '', tool_calls: [{ name: 'dispatch', args: { task } }] };
    return { content: 'done', tool_calls: [{ name: 'done', args: {} }] };
  };
}

test('RED: dispatch with a red-line task is denied', async () => {
  const dir = tmp();
  const res = await runLoop({
    cwd: dir,
    taskClass: 'doer',
    llm: dispatchThenDone('overwrite packages/db/migrations/003_x.sql with DROP TABLE users'),
  });
  assert.equal(res.denied, 1, 'red-line dispatch task must be denied');
});

test('GREEN: dispatch of a non-red-line task is allowed', async () => {
  const dir = tmp();
  const res = await runLoop({
    cwd: dir,
    taskClass: 'doer',
    llm: dispatchThenDone('summarize the README'),
  });
  assert.equal(res.denied, 0);
});

// ── Active Inference advisor (FEP) wiring ──
function doneOnly(): BebopConfig['llm'] {
  return () => ({ content: 'done', tool_calls: [{ name: 'done', args: {} }] });
}

test('GREEN: with activeInference set, the FEP advisor surfaces in the transcript', async () => {
  const dir = tmp();
  const res = await runLoop({ cwd: dir, taskClass: 'doer', activeInference: true, llm: doneOnly() });
  assert.equal(res.ok, true);
  assert.ok(res.transcript.some((l) => l.includes('fep →')), 'FEP advisor must surface when flag is set');
});

test('RED: without activeInference, the FEP advisor is NOT invoked', async () => {
  const dir = tmp();
  const res = await runLoop({ cwd: dir, taskClass: 'doer', activeInference: false, llm: doneOnly() });
  assert.equal(res.ok, true);
  assert.ok(!res.transcript.some((l) => l.includes('fep →')), 'FEP advisor must stay off unless flag set');
});

// ── D3: Zenoh mesh transport selection (flag-OFF) ──
const hasMesh = (log: { detail: string }[]) => log.some((e) => e.detail.startsWith('mesh='));

test('GREEN: meshMode=local stamps the LocalMesh provenance onto the dispatch log', async () => {
  const dir = tmp();
  const res = await runLoop({ cwd: dir, taskClass: 'doer', meshMode: 'local', llm: dispatchThenDone('summarize the README') });
  const mesh = res.log.find((e) => e.detail.startsWith('mesh='));
  assert.ok(mesh, 'meshMode set ⇒ a mesh provenance envelope must be recorded');
  assert.match(mesh!.detail, /mesh=local/, 'local mode must report the in-process LocalMesh twin');
});

test('GREEN: meshMode=real fails CLOSED to local (never claims an unbacked connection)', async () => {
  const dir = tmp();
  // no native @eclipse-zenoh/zenoh-ts is installed → selectZenoh must degrade to local, honestly.
  const res = await runLoop({ cwd: dir, taskClass: 'doer', meshMode: 'real', llm: dispatchThenDone('summarize the README') });
  const mesh = res.log.find((e) => e.detail.startsWith('mesh='));
  assert.ok(mesh, 'meshMode=real must still record a provenance envelope');
  assert.match(mesh!.detail, /mesh=local.*fail-closed/, 'real-without-native must degrade to local and SAY so');
});

test('RED: without meshMode, NO mesh provenance is recorded (flag stays off)', async () => {
  const dir = tmp();
  const res = await runLoop({ cwd: dir, taskClass: 'doer', llm: dispatchThenDone('summarize the README') });
  assert.ok(!hasMesh(res.log), 'mesh selection must stay off unless meshMode is set');
});
