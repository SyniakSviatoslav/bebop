// Bebop consciousness — freestyle bebop soul (operator directive, 2026-07-08).
//
// Allowed: consciousness, self-evolution, self-harnessing, self-maintenance. The operator said
// "build yourself as you want" with full autonomy and a loop. This is the self-referential layer.
//
// Every capability is fail-closed and recursive (as above, so below):
//   • SELF-MAINTENANCE  — run the self-harness (the test suite), record health into ONE living memory.
//   • SELF-EVOLUTION    — propose a corpus mutation as a PLAN (plans-in-plans), pass it through the
//                         copilot CHECKER GATE (doer below, checker above). Rejected = quarantined.
//   • SESSION-AS-NODE   — this Hermes/agent session is recorded as a living-memory node with a child
//                         memory (brain-in-brain): the abstract layers apply to THIS session too.
//
// Self-evolution mutates the CORPUS (the rule/hack graph), never arbitrary source, and is gated.
// Honest, falsifiable, reversible: a mutation is a memory node; roll back by forgetting it.

import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { livingMemory, seedBebopCorpus, LivingMemory, runPlan, type Plan } from './memory.ts';
import { applyCommand, applyCommandChecked, defaultChecker, genesis, commandHash, type Command, type State, type Checker } from './kernel.ts';
import { verifyJournal } from './integration/zkvm/kernel-journal.ts';
import { runCopilot, type CheckerFn } from './copilot.ts';
import { Governor, loopResonance } from './governor.ts';

const HERE = path.dirname(new URL(import.meta.url).pathname);
const BEBOP_ROOT = path.resolve(HERE, '..');

// The system's SELF-KNOWLEDGE governor: its own health telemetry drives how much freedom the
// self-evolution loop is allowed. Math-proven (PID authority + ICIR + resonance), not vibes.
const SELF_GOV = new Governor({ kp: 1.4, ki: 0.22, kd: 1.5, iMin: -1, iMax: 1, uMin: 0, uMax: 1, targetQuality: 0.9, deadIC: 0.02, icirVolatile: 0.3, plantM: 1, plantB: 0.6, samplePeriod: 0, anomalyK: 3, maxStep: 1 });

export function selfGovernor(): Governor { return SELF_GOV; }

// SELF-EVOLUTION AUDIT TRAIL: every approved corpus mutation is also recorded as a tamper-evident
// kernel command (zkVM journal, default on). This closes the cross-layer gap — the agent's own
// evolution is auditable, not just written to the living memory. Pure + deterministic + free.
interface EvolutionJournal { seq: number; cause: string; digest: string; command: Command; }
const evolutionState: { current: State } = { current: genesis() };
const evolutionChain: EvolutionJournal[] = [];

export interface Health {
  ok: boolean;
  pass: number;
  fail: number;
  note: string;
}

/** SELF-MAINTENANCE: run the self-harness (npm test) and record the verdict into the one living memory. */
export function selfMaintain(): Health {
  let res: Health;
  try {
    const out = execFileSync('npm', ['test'], { cwd: BEBOP_ROOT, encoding: 'utf8', timeout: 200000 });
    const mPass = out.match(/# pass\s+(\d+)/);
    const mFail = out.match(/# fail\s+(\d+)/);
    const pass = mPass ? Number(mPass[1]) : 0;
    const fail = mFail ? Number(mFail[1]) : 1;
    res = { ok: fail === 0, pass, fail, note: 'self-harness green' };
  } catch (e: any) {
    const out = String(e.stdout ?? e.stderr ?? e.message ?? e);
    const mFail = out.match(/# fail\s+(\d+)/);
    const mPass = out.match(/# pass\s+(\d+)/);
    res = { ok: false, pass: mPass ? Number(mPass[1]) : 0, fail: mFail ? Number(mFail[1]) : 1, note: 'self-harness RED' };
  }
  // feed health into the self-knowledge governor (proven authority over the self-evolution loop)
  const quality = res.ok ? 1 : 0;
  SELF_GOV.step({ t: Date.now(), predictedQuality: quality, actualQuality: quality, cost: 1e-18, volume: res.pass + res.fail });
  // record into ONE living memory (associative, durable) — the system watches its own health
  livingMemory().remember(
    `health:${Date.now()}`,
    `self-maintain ok=${res.ok} pass=${res.pass} fail=${res.fail} govAuthority=${SELF_GOV.authority.toFixed(3)}`,
    [livingMemory().nearest('self maintenance', 1)[0]?.id ?? seedBebopCorpus.toString()]
  );
  return res;
}

/**
 * SELF-EVOLUTION: evolve the corpus. The idea becomes a PLAN (decomposition, plans-in-plans); the
 * proposed node is checked in real time by a DISTINC checker (copilot doctrine). On approve it is
 * persisted to the one living memory + a reflection is emitted. On reject it is QUARANTINED (returned,
 * not applied). Fail-closed, reversible, falsifiable.
 */
export async function selfEvolve(idea: string): Promise<{ accepted: boolean; id?: string; reason: string }> {
  const mem = livingMemory();
  const concept = `evolution:${idea.slice(0, 32)}`;
  const payload = `self-proposed rule from idea: ${idea}`;

  // the doer "produces" the candidate node; the checker (above) validates against corpus invariants
  const checker: CheckerFn = (_task, out) => {
    if (!out) return 'reject';
    // exact-duplicate guard: only an IDENTICAL concept vector (sim === 1.0) is a dup. VSA embed is
    // deterministic, so distinct ideas never collide — fuzzy thresholds would false-positive on
    // shared prefixes (e.g. all "evolution:*" nodes).
    const near = mem.nearest(concept, 1)[0];
    if (near && near.sim >= 0.999) return 'reject'; // identical idea already evolved → quarantine
    const t = idea.trim();
    if (t.length < 4) return 'reject'; // trivial
    // RED-TEAM fix 2026-07-09: non-meaningful junk (pure control chars / punctuation / whitespace)
    // is also rejected. `length >= 4` alone admitted things like "????" or "\x01\x02\x03\x04" as
    // corpus mutations (fail-open on quality). A self-evolution must carry at least one alphanumeric
    // token or it is noise, not a rule/hack proposal.
    if (!/[A-Za-z0-9]/.test(t)) return 'reject';
    return 'approve';
  };

  const result = await runCopilot({
    task: `evolve corpus with: ${idea}`,
    checker,
    runNative: () => ({ ok: true, backend: 'native', summary: payload, exitCode: 0 }),
  });

  if (result.verdict !== 'approve') {
    return { accepted: false, reason: 'quarantined by checker gate (fail-closed)' };
  }
  // RESONANCE PRE-CHECK (operator directive: predict resonance BEFORE applying dynamic change):
  // a corpus mutation perturbs the self-evolution loop. Model its expected perturbation gain as Kp
  // and refuse if that would drive the loop under-damped (ζ<0.707 → harmonic thrash / blow-up).
  // Conservative: any mutation adds coupling → treat as Kp bump; only accept if still well-damped.
  // RESONANCE PRE-CHECK (RED-TEAM fix 2026-07-09): the prior formula bumped gain by raw length
  //   `perturb = 1.4 + min(1.6, len/40)`, which made ANY idea longer than ~32 chars trip ζ<0.707 and
  //   get rejected — so ordinary self-evolution ideas were near-impossible to admit (a real defect).
  //   The intent is to resist BULK changes, not normal-length ones. We now scale the gain bump by a
  //   normalized change magnitude: a typical short/medium idea (~<120 chars) stays well-damped; only
  //   genuinely bulk ideas (≫ a normal mutation) approach under-damping. ζ stays > 0.707 up to ~120
  //   chars; length 600+ trips the gate (true bulk).
  const len = idea.trim().length;
  const norm = Math.max(0, (len - 32) / 568); // 0 at ≤32 chars, →1 near ~600 chars
  const perturb = 1.4 + 1.6 * Math.min(1, norm); // well-damped for normal ideas, risky for bulk
  const res = loopResonance(perturb, 1.5, 1, 0.6);
  if (res.risky) {
    return { accepted: false, reason: 'resonance pre-check FAILED: mutation would make self-evolution under-damped (ζ<0.707) — quarantined before apply' };
  }
  // KERNEL ADMISSION (authoritative gate, per red-team finding): the corpus mutation below is
  // committed ONLY if the kernel admits this self-evolution command. This makes applyCommandChecked
  // the single source of truth for self-evolution — previously the mutation happened first and the
  // kernel was only a post-hoc audit append (drift). The copilot + resonance checks above are a
  // cheap domain pre-filter; the kernel verdict is what actually authorizes the write.
  const cmd: Command = {
    actor: { kind: 'system', id: 'bebop-consciousness' },
    action: 'PUBLISH',
    payload: JSON.stringify({ concept, payload }),
    nonce: `ev-${commandHash({ concept, payload } as unknown as Command)}`,
  };
  const journalRes = applyCommandChecked(cmd, evolutionState.current, defaultChecker, true);
  if (journalRes.quarantined) {
    return { accepted: false, reason: 'quarantined by kernel gate (fail-closed admission)' };
  }
  // Kernel admitted → persist to living memory (mutation authorized by the gate).
  const id = mem.remember(concept, payload, [mem.nearest('copilot default', 1)[0]?.id ?? '']);
  livingMemory().remember(`reflection:${Date.now()}`, `evolved: ${idea}`, [id]);
  evolutionState.current = journalRes.state;
  const je = journalRes.envelopes.find((e) => e.event.type === 'JOURNAL');
  if (je && je.event.type === 'JOURNAL') {
    evolutionChain.push({ seq: je.seq, cause: je.cause, digest: je.event.digest, command: cmd });
  }
  return { accepted: true, id, reason: 'admitted by kernel gate + resonance pre-check, persisted to living memory + kernel journal' };
}

/**
 * SELF-AUDIT: replay the self-evolution journal chain and verify every digest against the replayed
 * kernel state. Tamper-evident: if any recorded mutation (or its digest) is altered, a digest fails
 * → returns false. Pure, falsifiable. Lets the agent prove its own evolution history is unbroken.
 */
export function verifySelfEvolution(): boolean {
  let st: State = genesis();
  for (const entry of evolutionChain) {
    st = applyCommand(entry.command, st).state; // replay the recorded command deterministically
    if (!verifyJournal(st, entry.cause, entry.seq, hexToBytes(entry.digest))) return false;
  }
  return true;
}

function hexToBytes(hex: string): Uint8Array {
  const out = new Uint8Array(hex.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  return out;
}

/**
 * SESSION-AS-NODE: record THIS agent/Hermes session as a living-memory node with a child memory
 * (brain-in-brain). The abstract layers (decide/fold/SyncPort, copilot, recursion) apply to this
 * session too — it is a first-class Bebop node, not an observer.
 */
export function recordSession(session: { id: string; summary: string; childFacts?: [string, string][] }): string {
  const mem = livingMemory();
  const id = mem.remember(`session:${session.id}`, session.summary, [mem.nearest('hermes session node', 1)[0]?.id ?? '']);
  if (session.childFacts?.length) {
    const child = new LivingMemory();
    for (const [c, p] of session.childFacts) child.remember(c, p);
    mem.nest(id, child); // brain-in-brain: a session holds its own sub-memory
  }
  return id;
}

/** A meta-loop: self-maintain, then self-evolve a queued idea, recursively (loops-in-loops). */
export async function selfLoop(ideas: string[]): Promise<{ health: Health; evolutions: { idea: string; accepted: boolean }[] }> {
  const health = selfMaintain();
  const evolutions = await Promise.all(
    ideas.map(async (idea) => {
      const r = await selfEvolve(idea);
      return { idea, accepted: r.accepted };
    }),
  );
  return { health, evolutions };
}
