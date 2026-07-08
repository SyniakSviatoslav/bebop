import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { loadSettings, EMPTY_SETTINGS } from './settings.ts';

function tmpFile(name: string, content: string): string {
  const dir = path.join(os.tmpdir(), `bebop-set-${Math.random().toString(36).slice(2)}`);
  fs.mkdirSync(dir, { recursive: true });
  const f = path.join(dir, name);
  fs.writeFileSync(f, content);
  return f;
}

test('EMPTY_SETTINGS has sane defaults', () => {
  assert.equal(EMPTY_SETTINGS.model, undefined);
  assert.deepEqual(EMPTY_SETTINGS.permissions.allow, []);
  assert.deepEqual(EMPTY_SETTINGS.hooks, {});
});

test('GREEN: loads project bebop.json permissions + model', () => {
  const settings = loadSettings({
    cwd: '/nonexistent-cwd',
    userFile: '/no/user/file.json',
    projectFile: '/no/project/bebop.json',
  });
  // both missing → empty
  assert.equal(settings.model, undefined);
  assert.deepEqual(settings.permissions.deny, []);
});

test('GREEN: merges user + project; project overrides model', () => {
  const user = JSON.stringify({ model: 'user-model', permissions: { deny: ['**/secret/**'] } });
  const project = JSON.stringify({ model: 'proj-model', permissions: { allow: ['tools/**'] } });
  const userFile = tmpFile('user.json', user);
  const projFile = tmpFile('proj.json', project);
  const settings = loadSettings({ cwd: path.dirname(projFile), userFile, projectFile: projFile });
  assert.equal(settings.model, 'proj-model'); // project wins
  assert.deepEqual(settings.permissions.deny, ['**/secret/**']);
  assert.deepEqual(settings.permissions.allow, ['tools/**']);
});

test('GREEN: invalid JSON is ignored (safe fallback)', () => {
  const projFile = tmpFile('proj.json', '{ not json');
  const settings = loadSettings({ cwd: path.dirname(projFile), userFile: '/x', projectFile: projFile });
  assert.equal(settings.model, undefined);
});
