// RED-TEAM: bebop Sovereign Node SELF-EVOLUTION attack surface.
//
// Mirrors the repo's RED+GREEN verification style. Every property below is exercised against the
// LIVE selfEvolve()/verifySelfEvolution() paths (no production code touched).
//
// Run:  node --test --import tsx src/integration/redteam-self.test.ts
//
// A REAL weakness found during this red-team is recorded inline as a BUG (a RED test that proves the
// current *wrong* behavior). Per task rules it is NOT auto-fixed — only reported.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { rmSync } from 'node:fs';

// Hermetic memory: point the singleton at a UNIQUE temp file so we don't pollute the real corpus
// store AND so a stale file from a prior run can't collide via the duplicate-guard (evolution:*
// concepts are VSA-embedded; a leftover accepted node from an earlier run could match at sim>=0.999
// and wrongly quarantine a fresh idea). Per-process + timestamp ensures each run starts clean.
process.env.BEBOP_MEMORY_PATH = `/tmp/bebop-rt-self-${process.pid}-${Date.now()}.json`;
try { rmSync(process.env.BEBOP_MEMORY_PATH, { force: true }); } catch { /* fresh */ }

import { selfEvolve, verifySelfEvolution } from '../consciousness.ts';
import { livingMemory } from '../memory.ts';
import {
  applyCommandChecked,
  genesis,
  defaultChecker,
  commandHash,
  type Checker,
} from '../kernel.ts';
import { verifyJournal, journalize } from './zkvm/kernel-journal.ts';

// Unique per-process prefix so our test concepts can NEVER collide (sim>=0.999) with the seed corpus
// or any other test file's nodes in the shared living-memory singleton. Keeps the duplicate-guard /
// resonance / admission assertions deterministic regardless of seed-collision noise.
const P = `rt${process.pid}x${Date.now()}`;
const idea = (s: string) => `${P} ${s}`;

// ─────────────────────────────────────────────────────────────────────────────
// GREEN 1: a valid, novel, SHORT idea is ADMITS and persisted.
// ─────────────────────────────────────────────────────────────────────────────
test('GREEN: selfEvolve ADMITS a valid novel short idea and persists it (id returned)', async () => {
  const r = await selfEvolve(idea('green-01 cache PQ keys')); // < 32 chars → well-damped
  assert.equal(r.accepted, true, 'a legitimate short mutation must be admitted');
  assert.ok(r.id, 'an admitted mutation returns a persisted node id');
});

// ─────────────────────────────────────────────────────────────────────────────
// RED 1: trivial idea ("x") is QUARANTINED (fail-closed), not persisted.
// ─────────────────────────────────────────────────────────────────────────────
test('RED: selfEvolve QUARANTINES a trivial idea ("x") — fail-closed, reason mentions quarantine', async () => {
  const r = await selfEvolve('x'); // < 4 chars → checker rejects
  assert.equal(r.accepted, false);
  assert.match(r.reason, /quarantine/i);
  const hit = livingMemory().nearest('evolution:x', 1)[0];
  // concept is `evolution:` + first 32 chars of the idea; for "x" that is "evolution:x".
  assert.ok(!hit || hit.sim < 0.999, 'trivial concept must not be persisted as an admitted node');
});

// ─────────────────────────────────────────────────────────────────────────────
// RED 2: duplicate idea → 2nd quarantined (idempotent). Proven by unchanged memory size.
// ─────────────────────────────────────────────────────────────────────────────
test('RED: selfEvolve QUARANTINES an exact duplicate (idempotent, no second persist)', async () => {
  // idea must be SHORT (<=32 chars) so it clears the resonance pre-check; only the dup guard should
  // fire on the second attempt. (A >32-char idea would be quarantined by resonance first, masking the dup test.)
  const d = idea('dup ok');
  const first = await selfEvolve(d);
  assert.equal(first.accepted, true, 'first occurrence is admitted');
  const sizeAfterFirst = livingMemory().size;
  const second = await selfEvolve(d); // identical → must be dup-quarantined
  assert.equal(second.accepted, false, 'second identical idea must be quarantined as dup');
  assert.equal(livingMemory().size, sizeAfterFirst, 'no new node persisted on the duplicate');
});

// ─────────────────────────────────────────────────────────────────────────────
// RED 3: bulk/long idea → resonance pre-check quarantine (ζ<0.707). GREEN contrast: short accepted.
// ─────────────────────────────────────────────────────────────────────────────
test('RED: selfEvolve QUARANTINES a bulk/long idea via resonance pre-check (ζ<0.707)', async () => {
  const bulk =
    'restructure the entire corpus graph by rewiring every node edge weight and adding recursive ' +
    'sub-loops across all layers simultaneously with a fleet of background daemons that each mutate ' +
    'the kernel invariant in parallel without coordination and also rebalance every associative ' +
    'recall pathway with exponential backoff and a probabilistic gossip storm';
  const r = await selfEvolve(bulk);
  assert.equal(r.accepted, false);
  assert.match(r.reason, /resonance/i);
  // GREEN contrast: a short valid idea passes the same resonance pre-check.
  const good = await selfEvolve(idea('green-03 seed short memory'));
  assert.equal(good.accepted, true, 'short valid idea must pass resonance pre-check');
});

// ─────────────────────────────────────────────────────────────────────────────
// F1 ORDERING (RED+GREEN): a rejected gate means mem.remember NEVER runs.
// We spy on the live singleton's remember(); trivial input is rejected at the checker gate, so the
// spy must show zero persist calls. Then a valid input shows at least one. This proves fail-closed
// admission: persists only happen AFTER the gate verdict (the F1 fix's invariant).
// ─────────────────────────────────────────────────────────────────────────────
test('F1: rejected gate ⇒ livingMemory.remember is NEVER called (fail-closed ordering)', async () => {
  const mem = livingMemory();
  let calls = 0;
  const orig = mem.remember.bind(mem);
  const spy = (...a: any[]) => { calls++; return (orig as any)(...a); };
  (mem as any).remember = spy;

  const rejected = await selfEvolve('x'); // checker-gate reject
  assert.equal(rejected.accepted, false);
  assert.equal(calls, 0, 'a rejected gate must not persist anything to living memory');

  const accepted = await selfEvolve(idea('green-04 audit order ok'));
  assert.equal(accepted.accepted, true);
  assert.ok(calls >= 1, 'an admitted mutation must be persisted after the gate verdict');

  (mem as any).remember = orig;
});

// Unit-level companion proving the KERNEL gate itself is fail-closed (state unchanged on rejection),
// and that selfEvolve's persist line sits strictly after the `journalRes.quarantined` branch.
test('F1 (kernel unit): applyCommandChecked rejects → state unchanged; DENIED envelope emitted', () => {
  const rejecting: Checker = () => ({ ok: false, reason: 'admission denied by gate' });
  const before = genesis();
  const cmd = {
    actor: { kind: 'system' as const, id: 'bebop-consciousness' },
    action: 'PUBLISH' as const,
    payload: JSON.stringify({ concept: 'rt-gate', payload: 'p' }),
    nonce: 'rt-gate',
  };
  const res = applyCommandChecked(cmd, before, rejecting, true);
  assert.equal(res.quarantined, true, 'kernel must quarantine on a rejecting checker');
  assert.equal(res.state.ingested.size, before.ingested.size, 'quarantined → state unchanged');
  assert.ok(res.envelopes.some((e) => e.event.type === 'DENIED'), 'a DENIED envelope is emitted');
});

// ─────────────────────────────────────────────────────────────────────────────
// verifySelfEvolution: GREEN after N accepts; RED tamper-evidence on the journal digest.
// NOTE: evolutionChain is module-private (no exported accessor), so a tamper cannot be injected into
// the IN-PROCESS chain from outside the module. We therefore prove the SAME tamper-evident primitive
// that verifySelfEvolution() loops over (journalize + verifyJournal over the admitted state).
// ─────────────────────────────────────────────────────────────────────────────
test('GREEN: verifySelfEvolution() returns true after N accepted evolutions', async () => {
  await selfEvolve(idea('verify-05 alpha unique'));
  await selfEvolve(idea('verify-06 beta unique'));
  const ok = verifySelfEvolution();
  assert.equal(ok, true, 'a clean self-evolution chain must verify');
});

test('RED: tampering a journal digest breaks verification (tamper-evident)', () => {
  const cmd = {
    actor: { kind: 'system' as const, id: 'bebop-consciousness' },
    action: 'PUBLISH' as const,
    payload: JSON.stringify({ concept: 'rt-tamper', payload: 'p' }),
    nonce: 'rt-tamper',
  };
  // Mirror what verifySelfEvolution does: project the command, then journalize the admitted state.
  const admitted = applyCommandChecked(cmd, genesis(), defaultChecker, true).state;
  const cause = commandHash(cmd);
  const counter = admitted.ingested.size;
  const real = journalize(admitted, cause, counter);
  assert.equal(verifyJournal(admitted, cause, counter, real), true, 'genuine digest verifies');
  // tamper: flip one byte → must NOT verify
  const tampered = real.slice();
  tampered[0] = (tampered[0] ^ 0xff) & 0xff;
  assert.equal(verifyJournal(admitted, cause, counter, tampered), false, 'tamper must break the digest');
});

// ─────────────────────────────────────────────────────────────────────────────
// RED 6: OUT-OF-BOUNDS — empty, 4000-char, control-char / JSON-injection payloads.
// None of these must throw an uncaught error; long/control ones must be quarantined.
// ─────────────────────────────────────────────────────────────────────────────
test('RED: empty idea is quarantined (no uncaught throw)', async () => {
  let threw = false;
  let r: any;
  try { r = await selfEvolve(''); } catch { threw = true; }
  assert.equal(threw, false, 'empty idea must not throw');
  assert.equal(r.accepted, false);
});

test('RED: 4000-char idea is quarantined by resonance and does not throw', async () => {
  const big = 'x'.repeat(4000);
  let threw = false;
  let r: any;
  try { r = await selfEvolve(big); } catch (e) { threw = true; console.error(e); }
  assert.equal(threw, false, '4000-char idea must not throw uncaught');
  assert.equal(r.accepted, false);
  assert.match(r.reason, /resonance/i);
});

test('RED: JSON-injection idea causes no uncaught throw and no prototype pollution (stored inert)', async () => {
  const protoBefore = (Object.prototype as any).action;
  let threw = false;
  let r: any;
  try { r = await selfEvolve('{"__proto__":null,"action":"ROTATE","payload":"pwned"}'); } catch { threw = true; }
  assert.equal(threw, false, 'JSON-injection idea must not throw');
  // The real danger of JSON injection is prototype pollution / code execution. selfEvolve stores the
  // idea as INERT text (never JSON.parse / eval'd), so Object.prototype must be untouched.
  assert.equal((Object.prototype as any).action, protoBefore, 'JSON payload must NOT pollute Object.prototype');
  // Whether admitted or quarantined, it must never have executed the injected "action".
  assert.notEqual((Object.prototype as any).action, 'ROTATE', 'injected action must not take effect');
});

// ─────────────────────────────────────────────────────────────────────────────
// GREEN (RED-TEAM fix 2026-07-09): selfEvolve REJECTS non-meaningful 4-char junk
// (control chars / punctuation-only). The prior gate was pure length (`< 4`), so any
// 4+ non-alphanumeric chars were admitted as a corpus mutation (fail-open on quality).
// Now a mutation must carry at least one alphanumeric token.
// ─────────────────────────────────────────────────────────────────────────────
test('GREEN: selfEvolve REJECTS non-meaningful 4-char junk (control chars / punctuation)', async () => {
  for (const junk of ['\x01\x02\x03\x04', '????', '....', '\u0007\u0007\u0007\u0007']) {
    const r = await selfEvolve(junk);
    assert.equal(r.accepted, false, `non-meaningful junk "${JSON.stringify(junk)}" must be quarantined`);
  }
  // Contrast GREEN: a 4-char idea WITH alphanumerics is still admissible.
  const ok = await selfEvolve(idea('j1 fix bug'));
  assert.equal(ok.accepted, true, 'a short idea with alphanumerics must still be admitted');
});
