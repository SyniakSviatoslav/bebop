/**
 * audit.ts — reusable, TESTABLE audit checks (the pure logic behind scripts/verify-doc-claims.mjs,
 * scripts/guardrail-falsifiable-proof.mjs, scripts/invariant-advisor-gate.mjs).
 *
 * CONSOLIDATION (operator directive): the three pre-commit guardrails lived ONLY as .mjs CLI scripts —
 * their logic was unreachable from tests and from other modules. This module extracts the check
 * PRIMITIVES so they run INSIDE the test suite (RED+GREEN) and can be composed (e.g. a CI module that
 * runs all three + the doc-gate in one pass). The .mjs scripts remain as thin CLI wrappers that call
 * these — the shell boundary stays, the brain moves into a module.
 *
 * Deterministic, pure, falsifiable. FLAG-OFF: import + call the checks you need.
 */

import { readFileSync, existsSync, readdirSync, statSync } from 'node:fs';
import path from 'node:path';

export interface AuditCheck {
  name: string;
  ok: boolean;
  detail: string;
}

const RE_TAUTOLOGY = new RegExp([
  'assert\\s*\\(\\s*true\\s*\\)',
  'assert\\s*\\.\\s*ok\\s*\\(\\s*true\\s*\\)',
  'assert\\s*\\.\\s*(equal|strictEqual|deepEqual|deepStrictEqual)\\s*\\(\\s*(\\d+)\\s*,\\s*\\2\\s*\\)',
  "assert\\s*\\.\\s*(equal|strictEqual|deepEqual|deepStrictEqual)\\s*\\(\\s*(['\"][^'\"]*['\"])\\s*,\\s*\\2\\s*\\)",
  'assert\\s*\\.\\s*(equal|strictEqual|deepEqual|deepStrictEqual)\\s*\\(\\s*(\\w+)\\s*,\\s*\\2\\s*\\)',
].join('|'));
const RE_ASSERT_CALL = /\bassert\s*(?:\.\s*\w+\s*)?\([^;]*?\)/g;
const RE_ASSERTS = /\bassert\b/;

/** Verdict for one test file's source (mirrors scripts/guardrail-falsifiable-proof.mjs judge()). */
export function judgeFalsifiable(src: string): { falsifiable: boolean; reasons: string[] } {
  const hasAsserts = RE_ASSERTS.test(src);
  if (!hasAsserts) return { falsifiable: false, reasons: ['makes no assert.* calls — it proves nothing'] };
  const calls = src.match(RE_ASSERT_CALL) || [];
  const meaningful = calls.some((c) => !RE_TAUTOLOGY.test(c));
  const reasons: string[] = [];
  if (!meaningful) reasons.push('every assertion is a tautology (assert(true)/equal(x,x)) — a false-positive metric');
  return { falsifiable: reasons.length === 0, reasons };
}

/** Count test files that are FALSIFIABLE (Verified-by-Math): mirror of the pre-commit guardrail. */
export function countFalsifiable(root = process.cwd()): { total: number; falsifiable: number; nonFalsifiable: string[] } {
  const dir = path.join(root, 'src');
  const out = execFind(dir, /test\.ts$/);
  let falsifiable = 0;
  const non: string[] = [];
  for (const f of out) {
    const v = judgeFalsifiable(readFileSync(f, 'utf8'));
    if (v.falsifiable) falsifiable++; else non.push(path.relative(root, f));
  }
  return { total: out.length, falsifiable, nonFalsifiable: non };
}

/** Doc-claim honesty: README+AGENTS test-count strings must match the actual `npm test` pass count. */
export function docTestCountHonest(root = process.cwd(), actualPass: number): AuditCheck[] {
  const read = (p: string) => (existsSync(path.join(root, p)) ? readFileSync(path.join(root, p), 'utf8') : '');
  const checks: AuditCheck[] = [];
  for (const f of ['README.md', 'AGENTS.md']) {
    const src = read(f);
    const m = src.match(/(\d+)\s*TS tests/);
    const claimed = m ? Number(m[1]) : null;
    const ok = claimed === actualPass;
    checks.push({
      name: `test count honest: ${f}`,
      ok,
      detail: ok ? `${f} claims ${claimed}, actual ${actualPass}` : `${f} claims ${claimed}, actual ${actualPass} (drift!)`,
    });
  }
  return checks;
}

/** Advisor→verifier invariant (Cross-pattern B): every advisor entry has a deterministic verifier. */
export function advisorVerifierInvariant(root = process.cwd()): AuditCheck {
  const pairs: [string, string][] = [
    ['src/kernel.ts', 'applyCommandChecked'],
    ['src/integration/analytics/dual-track.ts', 'dualTrackGate'],
    ['src/copilot.ts', 'defaultChecker'],
    ['src/speculate.ts', 'verifyBlock'],
    ['src/integration/logicalCot.ts', 'verifyLogicalPlan'],
    ['src/integration/analytics/goap.ts', 'plan'],
  ];
  let ok = true;
  const missing: string[] = [];
  for (const [file, verifier] of pairs) {
    const p = path.join(root, file);
    if (!existsSync(p) || !readFileSync(p, 'utf8').includes(verifier)) { ok = false; missing.push(file); }
  }
  return { name: 'advisor→verifier invariant (Cross-pattern B)', ok, detail: ok ? 'all advisor entries matched by a verifier' : `missing verifier in: ${missing.join(', ')}` };
}

// local helpers (deterministic recursive finder; no shell exec dependency)
function execFind(dir: string, re: RegExp, acc: string[] = []): string[] {
  let entries: string[];
  try { entries = readdirSync(dir); } catch { return acc; }
  for (const e of entries) {
    const full = path.join(dir, e);
    let st; try { st = statSync(full); } catch { continue; }
    if (st.isDirectory()) execFind(full, re, acc);
    else if (re.test(e)) acc.push(full);
  }
  return acc;
}
