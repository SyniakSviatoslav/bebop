#!/usr/bin/env node
// logic-gate.mjs — enforces the Global Logic Laws (docs/design/LOGIC-LAWS.md)
// as truth gates on documentation/claim statements.
//
// HONEST SCOPE (per operator directive 2026-07-12):
//   Reliable logical-contradiction detection is *undecidable in general*. This
//   gate therefore does NOT pretend to prove logic. It enforces the PROCESS:
//     - Constitution (Law §6): both bebop2/ and crates/bebop must exist.
//       This IS mechanically certain -> HARD FAIL (exit 1, commit refused).
//     - Sufficient Reason (PSR, §4): every truth-claim in a CANONICAL claim
//       file needs a ground (test/proof/citation). Absent -> ESCALATE.
//     - Non-Contradiction (LNC, §2) + paradox: suspected cases are ESCALATED
//       to a human arbiter (exit 2), NEVER auto-blocked, because false
//       positives would freeze legitimate work. The operator/3-model review
//       are the final LNC authority.
//
// Exit codes:
//   0 = clean (all claims grounded, no suspected contradiction).
//   1 = HARD: canonical component deleted (constitution violation). Refuse.
//   2 = escalations written (unbacked / suspected-contradiction / paradox).
//       Commit ALLOWED; tracked in .bebop/escalations.jsonl + ESCALATIONS.md.

import { execFileSync } from 'node:child_process';
import { readdirSync, readFileSync, existsSync, statSync, writeFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';

const ROOT = execFileSync('git', ['rev-parse', '--show-toplevel'], { encoding: 'utf8' }).trim();
const STATE_DIR = join(ROOT, '.bebop');
const STATE = join(STATE_DIR, 'escalations.jsonl');
const ESC_MD = join(ROOT, 'docs/design/ESCALATIONS.md');
const ESC_MARKER = '<!-- LOGIC-GATE:OPEN-ITEMS (regenerated each run; do not edit by hand) -->';

// --- 1. Constitution (Law §6): both components MUST exist -> hard fail --------
if (!existsSync(join(ROOT, 'bebop2'))) { console.error('✗ HARD: bebop2/ (protocol) missing — violates LOGIC-LAWS §6'); process.exit(1); }
if (!existsSync(join(ROOT, 'crates', 'bebop'))) { console.error('✗ HARD: crates/bebop/ (agent) missing — violates LOGIC-LAWS §6'); process.exit(1); }

// --- 2. Canonical claim files ONLY (research/audit notes are not claims) -------
const CANON = [
  'README.md',
  'AGENTS.md',
  'docs/ARCHITECTURE.md',
  'docs/design/LOGIC-LAWS.md',
  'bebop2/README.md',
  'crates/bebop/README.md',
  'docs/design/RED-TEAM-REVIEW-2026-07-12.md',
  'docs/design/REMEDIATION-BLUEPRINT-2026-07-12.md',
].map((p) => join(ROOT, p)).filter(existsSync);

// --- 3. Patterns -------------------------------------------------------------
const CLAIM_RE = /(verified|proven|proves?|satisfies|guarantees|ensures|post-quantum claim|byte-exact|RED[→\-]>GREEN|kills?|eliminat|is (true|correct|secure|safe|canonical|consistent|reproducible|accessible|usable|honest|falsifiable)|no (serde|openssl)|claimed|asserts|sound|fact|semantic version|conventional commit|testing pyramid|feedback loop|causes?|implies?|therefore|thus|leads to|proves that|correlation|adapts to (every|all|any) user|always (right|correct)|flawless|perfect|knows best)/i;
const GROUND_RE = /(\.(rs|mjs|js|json|toml|wasm)|\[[^\]]+\]\(|#\[test\]|test |proof|KAT|ACVP|NIST|per (ARCHITECTURE|RED-TEAM|ROADMAP|LOGIC-LAWS)|source:|Stanford|Britannica|Aristotle|Leibniz|Wikipedia|cargo test|github\.com)/i;
const NEG_RE = /\b(not|never|no longer|does ?n'?t|isn'?t|is not|cannot|eliminated|removed|killed|gone|absent|false)\b/i;
const POS_RE = /\b(is|are|does|verified|satisfies|proven|ensures|guarantees|present|enabled|true|correct|secure|canonical)\b/i;
const PARADOX_RE = /\b(this (statement|claim|sentence|doc(ument)?|line))\b.{0,40}\b(false|true|unprovable|cannot be (proven|verified))\b/i;
// Subjects we never treat as contradiction subjects (field labels / noise words).
const STOP_SUBJ = new Set(['Claim', 'RED', 'Wave', 'This', 'Note', 'Kind', 'Status', 'Arbiter', 'Resolution', 'ESC', 'Date', 'Subject', 'The', 'A', 'An', 'It', 'File', 'Line', 'Path', 'Code', 'Test', 'Doc', 'Docs', 'README', 'AGENTS', 'Bebop', 'Protocol', 'Agent', 'Phase', 'Plan', 'Section']);

function subjectOf(line) {
  // A capitalized noun phrase that looks like a real topic, not a label.
  const m = line.match(/\b([A-Z][A-Za-z0-9][A-Za-z0-9_\-/]{2,})\b/g);
  if (!m) return null;
  for (const tok of m) if (!STOP_SUBJ.has(tok) && /[a-z]/.test(tok)) return tok;
  return null;
}
function hash(s) { return createHash('sha1').update(s).digest('hex').slice(0, 12); }

// --- 4. Load persisted state (preserve human RESOLVED verdicts) --------------
let persisted = new Map();
if (existsSync(STATE)) {
  for (const ln of readFileSync(STATE, 'utf8').split('\n')) {
    if (!ln.trim()) continue;
    try { const e = JSON.parse(ln); persisted.set(e.id, e); } catch { /* ignore corrupt line */ }
  }
}

const open = [];           // newly detected this run
const seen = new Set();

function escalate(file, line, kind, claim) {
  const id = 'ESC-' + hash(file + ':' + line + ':' + kind + ':' + claim.slice(0, 60));
  if (seen.has(id)) return;
  seen.add(id);
  const prev = persisted.get(id);
  const entry = prev || { id, file: file.replace(ROOT + '/', ''), line, kind, claim: claim.slice(0, 200), status: 'OPEN', arbiter: 'operator', resolution: '' };
  if (prev && prev.status === 'RESOLVED') { /* keep resolved verdict */ }
  else entry.status = 'OPEN';
  open.push(entry);
  const tag = kind === 'paradox' ? 'paradox' : kind === 'contradiction' ? 'CONTRADICTION?' : 'unbacked';
  console.log(`⚠ ESCALATE(${tag}): ${entry.file}:${line} — "${claim.slice(0, 80).trim()}…"`);
}

// --- 5. Scan canonical files -------------------------------------------------
for (const f of CANON) {
  const lines = readFileSync(f, 'utf8').split('\n');
  const rel = f.replace(ROOT + '/', '');
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (!CLAIM_RE.test(line)) continue;
    // Skip template/example lines (e.g. "<filled by human>" in LOGIC-LAWS.md).
    if (/<[^>]+>/.test(line) && /filled by human|example|e\.g\.|TODO/i.test(line)) continue;

    // Paradox (self-referential truth claim) -> escalate.
    if (PARADOX_RE.test(line)) { escalate(f, i + 1, 'paradox', line); continue; }

    // Grounding (PSR): claim line or ±4 lines must carry a ground.
    const window = lines.slice(Math.max(0, i - 4), Math.min(lines.length, i + 5)).join('\n');
    if (!GROUND_RE.test(line) && !GROUND_RE.test(window)) {
      escalate(f, i + 1, 'unbacked', line);
    }
  }

  // Conservative LNC: same specific subject within 12 lines, one NEG one POS.
  const idx = lines.map((l, i) => (CLAIM_RE.test(l) ? i : -1)).filter((x) => x >= 0);
  for (let a = 0; a < idx.length; a++) {
    for (let b = a + 1; b < idx.length; b++) {
      if (idx[b] - idx[a] > 12) break;
      const la = lines[idx[a]], lb = lines[idx[b]];
      const sa = subjectOf(la), sb = subjectOf(lb);
      if (!sa || sa !== sb) continue;
      const aNeg = NEG_RE.test(la), bNeg = NEG_RE.test(lb);
      const aPos = POS_RE.test(la), bPos = POS_RE.test(lb);
      if ((aNeg && bPos) || (aPos && bNeg)) {
        escalate(f, idx[a] + 1, 'contradiction', `${sa}: "${la.trim().slice(0, 60)}" vs "${lb.trim().slice(0, 60)}"`);
      }
    }
  }
}

// --- 6. Persist (merge: keep RESOLVED verdicts, add new OPEN) ---------------
if (!existsSync(STATE_DIR)) mkdirSync(STATE_DIR, { recursive: true });
const merged = new Map(persisted);
for (const e of open) if (!merged.has(e.id) || merged.get(e.id).status !== 'RESOLVED') merged.set(e.id, e);
writeFileSync(STATE, [...merged.values()].map((e) => JSON.stringify(e)).join('\n') + '\n');

// --- 7. Render human summary into ESCALATIONS.md (replace after marker) ----
if (existsSync(ESC_MD)) {
  const body = readFileSync(ESC_MD, 'utf8');
  const opens = [...merged.values()].filter((e) => e.status === 'OPEN');
  const resolved = [...merged.values()].filter((e) => e.status === 'RESOLVED');
  const summary =
    `\n## Open escalations (${opens.length}) — human arbiter required\n\n` +
    (opens.length ? opens.map((e) =>
      `- **${e.id}** [${e.kind}] \`${e.file}:${e.line}\` — ${e.claim}\n  - Arbiter: ${e.arbiter} · Status: ${e.status}`).join('\n') + '\n'
      : '_none_\n') +
    `\n## Resolved (${resolved.length})\n` +
    (resolved.length ? resolved.map((e) => `- **${e.id}** [${e.kind}] \`${e.file}:${e.line}\` — ${e.resolution || '(no note)'}`.trimEnd()).join('\n') + '\n' : '_none_\n');
  const markerIdx = body.indexOf(ESC_MARKER);
  const updated = (markerIdx >= 0 ? body.slice(0, markerIdx) + ESC_MARKER : body + ESC_MARKER) + summary;
  writeFileSync(ESC_MD, updated);
}

// --- 8. Exit -----------------------------------------------------------------
if (open.length) {
  console.log(`\n◆ LOGIC-GATE: ${open.length} claim(s) escalated to human arbiter — commit ALLOWED, tracked.`);
  process.exit(2);
}
console.log('✓ LOGIC-GATE: canonical claims grounded; constitution intact.');
process.exit(0);
