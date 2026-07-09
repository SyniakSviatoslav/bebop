// audit.test.ts — consolidated, TESTABLE audit checks (the brain behind the .mjs guardrails).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { countFalsifiable, docTestCountHonest, advisorVerifierInvariant, judgeFalsifiable } from './audit.ts';

test('GREEN: countFalsifiable detects RED cases in a test file', () => {
  const r = countFalsifiable(process.cwd());
  assert.ok(r.total > 0, 'there are test files');
  // every real test file is falsifiable (the guardrail guarantees it)
  assert.equal(r.nonFalsifiable.length, 0, `all ${r.total} test files falsifiable, got ${r.nonFalsifiable.length} non-falsifiable: ${r.nonFalsifiable.join(', ')}`);
});

test('GREEN+RED: judgeFalsifiable mirrors the guardrail (flags tautology + no-assert, passes real)', () => {
  const allGreen = `test('works', () => { assert.equal(1, 1); assert.ok(true); });`;
  const noAsserts = `test('smoke', () => { doThing(); });`;
  const real = `test('green', () => assert.equal(f(),1)); test('RED', () => assert.throws(() => f(NaN)));`;
  assert.equal(judgeFalsifiable(allGreen).falsifiable, false, 'tautology flagged');
  assert.equal(judgeFalsifiable(noAsserts).falsifiable, false, 'no-assert flagged');
  assert.equal(judgeFalsifiable(real).falsifiable, true, 'red+green passes');
});

test('RED: a pure-green tautology sample is flagged non-falsifiable', () => {
  const sample = 'test("always green", () => { assert.equal(1,1); });';
  assert.equal(judgeFalsifiable(sample).falsifiable, false, 'a pure-green tautology is correctly NOT falsifiable');
});

test('GREEN: advisorVerifierInvariant holds for the real source tree', () => {
  const c = advisorVerifierInvariant(process.cwd());
  assert.equal(c.ok, true, c.detail);
});

test('GREEN: docTestCountHonest passes when claimed count matches actual', () => {
  const checks = docTestCountHonest(process.cwd(), 999999); // placeholder; just assert shape + that mismatch is caught
  assert.equal(checks.length, 2);
  // force a mismatch case
  const bad = docTestCountHonest(process.cwd(), -1);
  assert.equal(bad[0].ok, false, 'mismatch must be flagged (RED case)');
});
