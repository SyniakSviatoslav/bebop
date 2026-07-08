import { test } from 'node:test';
import assert from 'node:assert/strict';
import { checkRedLine, checkScope, RED_LINE_GLOBS, DEFAULT_SCOPE_GLOBS } from './guard.ts';

test('GREEN: secret/credential globs are red-lines (.env / secret / secrets)', () => {
  for (const p of ['.env', '.env.local', 'config/.env', 'secret/key.txt', 'secrets/vault.json', 'auth/token']) {
    assert.equal(checkRedLine(p).ok, false, `${p} must be a red-line`);
  }
});

test('GREEN: non-red-line paths pass', () => {
  for (const p of ['tools/bebop/loop.ts', 'docs/README.md', 'src/mcp.ts']) {
    assert.equal(checkRedLine(p).ok, true, `${p} must be allowed`);
  }
});

test('GREEN: extraGlobs strengthen the red-line set (user deny)', () => {
  assert.equal(checkRedLine('src/experimental.ts').ok, true, 'not a red-line by default');
  assert.equal(checkRedLine('src/experimental.ts', ['**/experimental.ts']).ok, false, 'user deny glob must apply');
});

test('GREEN: checkScope honors custom scope', () => {
  assert.equal(checkScope('tools/bebop/x.ts', DEFAULT_SCOPE_GLOBS, '/repo').ok, true);
  assert.equal(checkScope('random/y.ts', ['tools/**'], '/repo').ok, false, 'outside custom scope');
});
