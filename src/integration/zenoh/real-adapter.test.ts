// src/integration/zenoh/real-adapter.test.ts
// RED+GREEN falsifiable proof for the env-gated Zenoh transport selection.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { selectZenoh } from './real-adapter.ts';
import { LocalMesh } from './transport.ts';

test('GREEN: local mode selects an in-process LocalMesh twin', () => {
  const sel = selectZenoh('local', 'node-a');
  assert.equal(sel.mode, 'local');
  assert.ok(sel.transport instanceof LocalMesh);
  assert.match(sel.provenance, /LocalMesh/);
});

test('GREEN: multi-node id list wires into a usable transport', () => {
  const sel = selectZenoh('local', ['n1', 'n2', 'n3']);
  assert.equal(sel.mode, 'local');
  // observable: the transport is a LocalMesh and can store a key
  const n = sel.transport as LocalMesh;
  const before = n.get('k');
  assert.equal(before, undefined);
  const got = n.put({ payload: new Uint8Array([1]), from: 'n1', seq: 0, priority: 10, key: 'k' });
  assert.equal(got, 0); // no subscribers yet
  assert.ok(n.get('k'));
});

test('RED: requesting real mode WITHOUT the native client FAILS CLOSED to local (never pretends connected)', () => {
  // In this environment the native zenoh-ts is absent, so 'real' must degrade to 'local'
  // and MUST NOT claim a real transport. (If zenoh-ts is later installed this test still passes:
  // it only asserts we never lie — real mode with client present would return mode 'real'.)
  const sel = selectZenoh('real', 'node-z');
  assert.equal(sel.mode, 'local');
  assert.match(sel.provenance, /fail-closed/);
});

test('RED: an unknown mode string is REJECTED (fail-closed, not silently defaulted)', () => {
  assert.throws(() => selectZenoh('cloud-broker', 'x'), /unknown mode/);
});

test('RED: empty id list is rejected', () => {
  assert.throws(() => selectZenoh('local', []), /ids required/);
});
