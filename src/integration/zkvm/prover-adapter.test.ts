// src/integration/zkvm/prover-adapter.test.ts
// RED+GREEN falsifiable proof for the env-gated zkVM proving selection.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { prove } from './prover-adapter.ts';
import { decide } from './decide.ts';

const STATE = new TextEncoder().encode('state-a');
const CMD = new TextEncoder().encode('cmd-hash');
const CTX = new TextEncoder().encode('ctx');

test('GREEN: digest mode returns the canonical digest (tamper-evident), no seal', () => {
  const r = prove(STATE, CMD, CTX, 1, 'digest');
  assert.equal(r.mode, 'digest');
  assert.equal(r.kind, 'digest');
  assert.equal(r.seal, undefined);
  // digest matches the raw decide() — the kernel journal binds to this
  assert.deepEqual(r.digest, decide(STATE, CMD, CTX, 1));
  assert.match(r.provenance, /tamper-evident/);
});

test('RED: prove mode WITHOUT a prover FAILS CLOSED to digest — NO seal fabricated', () => {
  // Native zkVM proving is unavailable here, so 'prove' must degrade to a tamper-evident digest and
  // MUST NOT invent a seal (no false receipt). This is the honest-gap guardrail.
  const r = prove(STATE, CMD, CTX, 2, 'prove');
  assert.equal(r.kind, 'digest');
  assert.equal(r.seal, undefined);
  assert.match(r.provenance, /fail-closed|no seal fabricated/);
  // the digest is still correct
  assert.deepEqual(r.digest, decide(STATE, CMD, CTX, 2));
});

test('RED: an unknown prover mode is REJECTED (fail-closed)', () => {
  assert.throws(() => prove(STATE, CMD, CTX, 3, 'bonsai-cloud'), /unknown mode/);
});

test('RED: digest is deterministic + tamper-evident across modes (same input → same digest)', () => {
  const a = prove(STATE, CMD, CTX, 7, 'digest').digestHex;
  const b = prove(STATE, CMD, CTX, 7, 'digest').digestHex;
  assert.equal(a, b);
  // different counter → different digest (tamper-evident binding)
  const c = prove(STATE, CMD, CTX, 8, 'digest').digestHex;
  assert.notEqual(a, c);
});
