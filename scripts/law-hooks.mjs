#!/usr/bin/env node
// law-hooks.mjs — turns the Global Logic Laws (docs/design/LOGIC-LAWS.md)
// into PRACTICE, not just guidance. Each check runs a REAL tool so the law
// is enforced by evidence, not by good intentions.
//
// Wiring (added to .git/hooks/pre-commit after logic-gate.mjs):
//   node scripts/law-hooks.mjs
// Exit 0 = all law-checks pass (or escalate-with-allowed).
// Exit 1 = a HARD law was violated (refuse commit).
// Exit 2 = an item needs human arbitration (tracked, allowed).
//
// Honest scope (per LOGIC-LAWS §9F/§15/§21/§24):
//   - code-quality/crypto/reliability (§9) are enforced by CI tools
//     (clippy/deny/audit/falsifiable) — NOT by this script's judgment.
//   - reasoning-method (§16/§17) + honesty (§23) are partly enforceable
//     by text lint; the rest is human-arbitrated.
//   - the gate NEVER auto-judges "is this UX good" / "did it learn well".

import { execFileSync, spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';

const ROOT = execFileSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8' }).trim();
const sh = (cmd, args, opts = {}) => {
  const r = spawnSync(cmd, args, { cwd: ROOT, encoding: 'utf8', ...opts });
  return { code: r.status ?? 0, out: (r.stdout || '') + (r.stderr || '') };
};

let HARD = 0, ESC = 0;
const hard = (msg) => { HARD++; console.error('✗ HARD (law violation): ' + msg); };
const esc  = (msg) => { ESC++;  console.warn('⚠ ESCALATE (human-arbitrated): ' + msg); };

console.log('◆ law-hooks: enforcing Global Logic Laws as PRACTICE (real tools)');

// ---------------------------------------------------------------------------
// §9B/§13 — secure-coding + falsifiable tests (REAL cargo tools)
// ---------------------------------------------------------------------------
console.log('◆ [§9B/§13] cargo clippy (memory-safety/unsafe ERRORS are HARD; cosmetic warnings are not blockers)');
{
  const r = sh('cargo', ['clippy', '--quiet', '--workspace']);
  // clippy lints are warnings by default; only genuine ERRORS (e.g.
  // not_unsafe_ptr_arg_deref, type errors) violate §9B memory-safety.
  const errs = (r.out.match(/^error(\[|:)/gm) || []).length;
  if (errs > 0) hard(`cargo clippy found ${errs} error(s) — §9B memory-safety/unsafe violated`);
}

console.log('◆ [§9B/§13] falsifiable-guardrail (every #[test] must be able to go RED)');
{
  const r = sh('node', ['scripts/guardrail-falsifiable-proof.mjs']);
  if (r.code !== 0) hard('guardrail-falsifiable-proof failed — a #[test] cannot go RED (§9C/§13 false-green)');
}

// ---------------------------------------------------------------------------
// §9B — supply chain / crypto assurance (REAL deny + audit + RED leg)
// ---------------------------------------------------------------------------
console.log('◆ [§9B] supply-chain gate (cargo-deny advisories/bans/licenses + cargo-audit + RED leg)');
{
  const r = sh('bash', ['scripts/ci-supply-chain.sh']);
  if (r.code !== 0) hard('ci-supply-chain.sh failed — banned crate / unaudited advisory / RED leg broken (§9B)');
}

// ---------------------------------------------------------------------------
// Sovereign-core invariants (fast, structural) — SOVEREIGN-EVENT-EXCHANGE-BLUEPRINT
// P0. Wires the existing MESH guard scripts (were manual-only) into the per-commit
// gate. Only the FAST ones run here (pure grep / cargo metadata); the slow
// wasm-empty-import proof runs in CI (see .github/workflows/ci.yml sovereign-guards).
// ---------------------------------------------------------------------------
console.log('◆ [sovereign] no-courier-scoring · G1 wire-codec · G4 scope-subset · crdt-fence · kernel-fence (structural invariants)');
for (const [script, why] of [
  ['scripts/ci-no-courier-scoring.sh', 'a struct field names a courier/agent score/rating/rank — trust is a signed capability, never a reputation metric'],
  ['scripts/ci-crdt-fence.sh', 'a money/order crate depends on a CRDT-merge crate (MESH-08 periphery fence)'],
  ['scripts/ci-kernel-fence.sh', 'proto-cap depends on dowiz-kernel (MESH-02 layer-purity fence)'],
  ['scripts/ci-no-serde-json-wire.sh', 'G1 — SignedFrame is serialized with serde_json on the wire instead of the canonical binary codec (wire_codec)'],
  ['scripts/ci-no-flat-scope.sh', 'G4 — Scope/Effect reverted to flat 2-arg equality, making UCAN attenuation a no-op (must be the set-subset model)'],
]) {
  const r = sh('bash', [script]);
  if (r.code !== 0) hard(`${script} failed — ${why}`);
}

// ---------------------------------------------------------------------------
// §17 — critical thinking: detectable fallacies in staged DOC/PR text
// ---------------------------------------------------------------------------
console.log('◆ [§17] fallacy lint on staged docs/claims (ad hominem, correlation≠causation, appeal-to-authority, false dilemma)');
{
  // staged textual files only
  const staged = sh('git', ['diff', '--cached', '--name-only', '--diff-filter=ACM']).out
    .split('\n').filter((f) => /\.(md|mjs|ts|rs)$/.test(f) && !/node_modules|target|\.git\//.test(f));
  const FALLACY = [
    [/\b(because|since)\s+\w+(\s+\w+){0,3}\s+(is (popular|widely used|recommended))\b/i, 'appeal-to-authority/bandwagon (§17)'],
    [/\b(correlation|associated with)\b.{0,40}\b(therefore|thus|so|proves?|causes?)\b/i, 'correlation≠causation (§17)'],
    [/\b(either .+ or .+)\b(?!.*\bunless\b)/i, 'possible false-dilemma (§17) — confirm only two options exist'],
    [/\byou('?re| are) (wrong|stupid|lazy)\b/i, 'ad hominem (§17)'],
  ];
  let hits = 0;
  for (const f of staged) {
    const p = ROOT + '/' + f;
    let txt; try { txt = readFileSync(p, 'utf8'); } catch { continue; }
    for (const [re, label] of FALLACY) {
      const m = txt.match(re);
      if (m) { esc(`${f}: detectable fallacy — ${label} ("${m[0].slice(0,60)}…")`); hits++; }
    }
  }
  // never hard-fail on subjective style; only escalate for human review
  if (hits) console.warn(`   ${hits} fallacy-candidate(s) escalated to human (§17 self-check + arbitration)`);
}

// ---------------------------------------------------------------------------
// §16 — learning loop: a code-changing commit must record a "learned gap"
//       in memory or a skill note (enforceable: look for a ponytail:/LESSON marker)
// ---------------------------------------------------------------------------
console.log('◆ [§16] Feynman self-retro: code-changing commit records a learned gap (ponytail:/LESSON marker)');
{
  const codeChanged = sh('git', ['diff', '--cached', '--name-only', '--diff-filter=ACM']).out
    .split('\n').some((f) => /\.(rs|mjs|ts|js|toml)$/.test(f) && !/node_modules|target|\.git\//.test(f));
  if (codeChanged) {
    const commitMsg = sh('git', ['log', '-1', '--pretty=%B']).out;
    const hasLesson = /(ponytail:|LESSON|learned|gap closed|retro:|takeaway)/i.test(commitMsg);
    if (!hasLesson) esc('code-changing commit has no self-retro marker (ponytail:/LESSON) — §16 Feynman loop not closed; add one line on the gap learned');
  }
}

// ---------------------------------------------------------------------------
// §23 — unpleasant truth over flattery: ban unbacked superlatives / noble-lie phrasing
// ---------------------------------------------------------------------------
console.log('◆ [§23] no-flattery / no-noble-lie lint on staged docs/claims');
{
  const staged = sh('git', ['diff', '--cached', '--name-only', '--diff-filter=ACM']).out
    .split('\n').filter((f) => /\.(md|mjs|ts)$/.test(f));
  const FLATTERY = [
    [/\b(best|perfect|flawless|unrivaled|the only tool that)\b/i, 'unbacked superlative (§23 — verify or cut; no flattery)'],
    [/\b(no need to worry|trust me|it just works|magic)\b/i, 'noble-lie / hand-wave phrasing (§23 — state the verified truth)'],
  ];
  let hits = 0;
  for (const f of staged) {
    const p = ROOT + '/' + f;
    let txt; try { txt = readFileSync(p, 'utf8'); } catch { continue; }
    for (const [re, label] of FLATTERY) {
      const m = txt.match(re);
      if (m) { esc(`${f}: ${label} ("${m[0].slice(0,50)}…")`); hits++; }
    }
  }
  if (hits) console.warn(`   ${hits} flattery/noble-lie candidate(s) escalated to human (§23)`);
}

// ---------------------------------------------------------------------------
// Summary
// ---------------------------------------------------------------------------
console.log(`◆ law-hooks: HARD=${HARD} ESCALATE=${ESC}`);
if (HARD > 0) { console.error('✗ commit REFUSED: a hard law (§9/§13) was violated by real tool evidence.'); process.exit(1); }
if (ESC > 0) { console.warn('⚠ commit ALLOWED but tracked for human arbitration (docs/design/ESCALATIONS.md).'); process.exit(2); }
console.log('✓ all law-checks passed (practice matches the laws)');
process.exit(0);
