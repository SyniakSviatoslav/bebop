import { test } from 'node:test';
import assert from 'node:assert/strict';
import { runHooks, preToolUse, type HookSpec } from './hooks.ts';

const allowRun = (_c: string, _i: string) => ({ code: 0, stdout: '' });
const denyRun = (_c: string, _i: string) => ({ code: 2, stdout: '' });
const denyJson = (_c: string, _i: string) => ({ code: 0, stdout: JSON.stringify({ permissionDecision: 'deny', permissionDecisionReason: 'nope' }) });
const blockJson = (_c: string, _i: string) => ({ code: 0, stdout: JSON.stringify({ decision: 'block', reason: 'stop' }) });

test('GREEN: no hooks → allow', () => {
  const d = runHooks([], { event: 'PreToolUse', tool: 'edit' });
  assert.equal(d.blocked, false);
});

test('RED: exit code 2 blocks the action', () => {
  const specs: HookSpec[] = [{ matcher: 'edit', command: 'false' }];
  const d = runHooks(specs, { event: 'PreToolUse', tool: 'edit' }, denyRun);
  assert.equal(d.blocked, true);
});

test('RED: permissionDecision:deny blocks', () => {
  const specs: HookSpec[] = [{ matcher: 'edit', command: 'echo deny' }];
  const d = runHooks(specs, { event: 'PreToolUse', tool: 'edit' }, denyJson);
  assert.equal(d.blocked, true);
  assert.equal(d.reason, 'nope');
});

test('RED: decision:block blocks (bebop-native)', () => {
  const specs: HookSpec[] = [{ matcher: '*', command: 'echo block' }];
  const d = runHooks(specs, { event: 'PreToolUse', tool: 'run' }, blockJson);
  assert.equal(d.blocked, true);
  assert.equal(d.reason, 'stop');
});

test('GREEN: allow hook passes through', () => {
  const specs: HookSpec[] = [{ matcher: 'edit', command: 'echo ok' }];
  const d = runHooks(specs, { event: 'PreToolUse', tool: 'edit' }, allowRun);
  assert.equal(d.blocked, false);
});

test('GREEN: matcher filters — hook for read does not affect edit', () => {
  const specs: HookSpec[] = [{ matcher: 'read', command: 'false' }];
  const d = runHooks(specs, { event: 'PreToolUse', tool: 'edit' }, denyRun);
  assert.equal(d.blocked, false, 'read-only hook must not block edit');
});

test('RED: crashing hook fails CLOSED (deny)', () => {
  const specs: HookSpec[] = [{ matcher: 'edit', command: 'x' }];
  const boom = () => { throw new Error('hook crashed'); };
  const d = runHooks(specs, { event: 'PreToolUse', tool: 'edit' }, boom);
  assert.equal(d.blocked, true);
  assert.equal(d.reason, 'hook error');
});

test('preToolUse helper wires PreToolUse event', () => {
  const specs: HookSpec[] = [{ matcher: 'edit', command: 'false', run: denyRun }];
  const d = preToolUse(specs, 'edit', {});
  assert.equal(d.blocked, true);
});
