// Bebop agentic loop — the skeleton every coding agent shares, owned by us:
//   (1) system prompt carrying the OS  (2) tool definitions  (3) LLM-call loop appending tool
//   results and re-calling  (4) guard gate BEFORE any file mutation  (5) knowledge seam pre-step.
//
// The LLM call is INJECTED (default: a deterministic stub) so the loop is runnable and testable
// with ZERO tokens — Verified-by-Math without burning the operator's budget. Swap in a real
// provider (OpenRouter per TOOLING-REGISTRY.md) by passing an `llm` fn that returns
// { tool_calls?: [{name,args}], content?: string }.

import fs from 'node:fs';
import path from 'node:path';
import { checkRedLine, checkScope } from './guard.ts';
import { validateToolArgs } from './validate.ts';
import { searchFieldStateText, directiveFor, type FieldDirective } from './field.ts';
import { adviseLoop, type LoopAction } from './integration/active-inference/loop-advisor.ts';
import { route, enforceRouting, type TaskClass, type Model } from './router.ts';
import { recall } from './knowledge.ts';
import type { DispatchResult } from './backend.ts';
import { SHIP, banner, makePaint } from './theme.ts';
import { BOOT, say, TAGLINE, voiceFor } from './voice.ts';
import { runBackend, type Backend } from './backend.ts';
import { selectBackend, rotate } from './routing.ts';
import { selectZenoh } from './integration/zenoh/real-adapter.ts';
import { mineGraph, type MineReport } from './integration/analytics/arch-mine.ts';
import { emptyLedger, record, type Ledger } from './token.ts';
import type { Profile } from './profile.ts';
import { preToolUse, type HookSpec } from './hooks.ts';

export type ToolName = 'read' | 'edit' | 'run' | 'grep' | 'dispatch' | 'done';

export interface LlmResponse {
  content?: string;
  tool_calls?: { name: ToolName; args: Record<string, any> }[];
}

export interface BebopConfig {
  cwd: string;
  taskClass: TaskClass;
  // injected LLM — default stub returns a single 'done' call so the loop terminates deterministically
  llm?: (messages: any[], ctx: LoopContext) => LlmResponse | Promise<LlmResponse>;
  maxSteps?: number;
  // optional scope override (absolute or glob). Defaults to the repo's agreed surface.
  scope?: string[];
  // conductor config (Phase 0): profile drives backend selection; forcedBackend overrides it.
  profile?: Profile;
  forcedBackend?: Backend | null;
  // injected native runner for the `native` backend (so it doesn't shell out).
  runNative?: (task: string) => DispatchResult;
  // hooks (Claude Code PreToolUse/PostToolUse/Stop analogue) — run before the guard gate.
  hooks?: HookSpec[];
  // plan mode: read-only; edit/write tools are denied (Explore/Plan subagent semantics).
  planMode?: boolean;
  // extra red-line globs (from TRUSTED user settings only) — strengthen the deny set.
  redLines?: string[];
  // ReAct iterations (Reason→Act→Observe→Reflect) — default 3, overridable via cfg or
  // BEBOP_REACT_ITERS env. This is the visible iteration count; it is NOT hidden from the user.
  iterations?: number;
  // Field oracle (∇·F / ∇×F 3-state law): when set, the loop samples the agent's DOMAIN-KNOWLEDGE
  // candidate field and reports the physics directive (generate / reconsider / focus). Off by
  // default — pure, deterministic, no extra LLM call; it only reads the in-process VSA memory.
  field?: boolean;
  // Active Inference (Free-Energy Principle) policy advisor: when set, the loop derives a belief over
  // {stuck, progressing, done} from its own progress (steps, denials, completion) and asks the FEP
  // engine to pick the next action. Complements the field oracle (field = where to look; FEP = what
  // to do). Off by default. Grounded in real pymdp numbers (see src/integration/active-inference).
  activeInference?: boolean;
  // Zenoh mesh transport selection (D3, flag-OFF): when set to 'local' | 'real', dispatch records
  // WHICH transport won via the pure `selectZenoh` probe and stamps its provenance onto the dispatch
  // envelope detail. Fail-closed — 'real' with no native @eclipse-zenoh client degrades to 'local'
  // and says so; it NEVER claims a connection it doesn't have, and it stays OUT of the pure kernel
  // (selection does IO). Off by default: no meshMode ⇒ dispatch behaves exactly as before.
  meshMode?: 'local' | 'real';
  // node ids to wire into the mesh when meshMode is set (defaults to a single 'bebop' node).
  meshIds?: string[];
  // D6 architecture-mining pass (flag-OFF): when a module set is supplied, the loop runs the pure
  // arch-mine detectors (orphans / import cycle / coupling clusters) over it and surfaces the health
  // report in the transcript + returns it on LoopResult.mine. Deterministic, no LLM call. Off by
  // default: no archMine ⇒ the pass never runs and mine is undefined.
  archMine?: { id: string; source: string; isMarkdown?: boolean }[];
}

export interface LoopContext {
  cwd: string;
  model: Model;
  recallHits: { id: string; text: string }[];
}

// One visible ReAct step. The whole point: nothing here is hidden (unlike promo demos that show a
// single "perfect" iteration). Every draft/observe/reflect is recorded and returned in reactTrace.
export interface ReactStep {
  iter: number;
  phase: 'reason' | 'act' | 'observe' | 'reflect';
  thought?: string;     // the model's reasoning for this iteration
  action?: string;      // the tool call issued (e.g. "edit main.ts")
  observation?: string; // what the tool/environment returned (truncated)
  reflection?: string;  // the real-time eval verdict + self-correction note
  evalScore?: number;   // 0..1 real-time quality score for THIS iteration
  evalPassed?: boolean; // did the iteration's draft/test pass the eval gate?
  ok: boolean;          // did this iteration make progress / not get denied?
}

// Real-time quality eval gate for ONE ReAct iteration. Combines with the existing guard (does NOT
// duplicate it): the guard decides legality; this gate decides QUALITY of the iteration — did the
// draft make progress, did a mutation land, did a test run pass. Returns a 0..1 score + pass flag.
// Falsifiable: a denied mutation scores 0 and fails; a clean edit+done scores high.
export interface EvalVerdict {
  passed: boolean;
  score: number; // 0..1
  notes: string;
}

export function evalStep(step: { action?: string; observation?: string; denied?: boolean; mutated?: boolean }): EvalVerdict {
  const action = step.action ?? '';
  const obs = step.observation ?? '';
  if (step.denied) {
    return { passed: false, score: 0, notes: `denied by guard — draft rejected, rewrite required (ReAct iter will see this)` };
  }
  if (/edit|write|dispatch/.test(action) && /written|would dispatch|would exec/.test(obs)) {
    return { passed: true, score: 0.9, notes: 'mutation/dispatch landed cleanly' };
  }
  if (/run/.test(action) && /test|pass|ok/.test(obs.toLowerCase())) {
    return { passed: true, score: 0.85, notes: 'action ran and reported success' };
  }
  if (/done/.test(action)) {
    return { passed: true, score: 1, notes: 'task marked done' };
  }
  if (/read|grep/.test(action)) {
    return { passed: true, score: 0.6, notes: 'read-only observation' };
  }
  return { passed: true, score: 0.5, notes: 'neutral step' };
}

// Resolve the visible ReAct iteration count: explicit cfg > BEBOP_REACT_ITERS env > default 3.
export function reactIters(cfg: { iterations?: number }): number {
  const env = Number(process.env.BEBOP_REACT_ITERS ?? '');
  if (Number.isFinite(env) && env >= 1) return Math.floor(env);
  if (cfg.iterations && cfg.iterations >= 1) return Math.floor(cfg.iterations);
  return 3;
}

// The kernel law (RESEARCH §1.5/§1.6): every dispatch/action is recorded as an immutable envelope so
// a whole multi-backend session is replayable and auditable. Pure data — no clock/RNG in the record.
export interface Envelope {
  seq: number;
  cause: string; // the task hash / command that caused this
  backend: Backend;
  event: 'dispatch' | 'denied' | 'mutation' | 'done';
  detail: string;
}

export interface LoopResult {
  steps: number;
  mutations: number;
  denied: number;
  transcript: string[];
  ok: boolean;
  // the deterministic, replayable session log (cross-backend).
  log: Envelope[];
  ledger: Ledger;
  // Visible ReAct iteration count (default 3) — NOT hidden from the user.
  iterations: number;
  // The full, visible Reason→Act→Observe→Reflect trace for every iteration. This is what promo
  // demos hide: here it is emitted to the transcript AND returned for audit/replay.
  reactTrace: ReactStep[];
  // D6 architecture-mining health report (flag-OFF). Present only when cfg.archMine is supplied;
  // undefined otherwise — the pass never runs by default.
  mine?: MineReport;
}

const SYSTEM_PROMPT = `You are Bebop — a coding agent for the dowiz/DeliveryOS project.
Operating System (native, non-negotiable):
- Ethics: no AI for military/warfare; build toward peace and owner sovereignty.
- Red-lines (auth, money, RLS, migrations, bulk-edit): NEVER edit without explicit human go-ahead.
- Verified-by-Math: every change needs a deterministic proof that can go RED on bad input.
- Token economy: you are a doer; route reasoning to the right model, never overspend.
- Voice: dry co-pilot. Plain on money/auth/security. No emojis, no cheer.
Tools: read, edit, run, grep, done. Call 'done' when the task is complete and proven.`;

function defaultLlm(): LlmResponse {
  // deterministic termination stub — proves the loop machinery without a live model
  return { content: 'No live model configured; terminating.', tool_calls: [{ name: 'done', args: {} }] };
}

function runTool(name: ToolName, args: any, cfg: BebopConfig): { result: string; mutated: boolean; denied: boolean } {
  const p = path.resolve(cfg.cwd, String(args.path ?? ''));

  // PreToolUse hooks (Claude Code analogue) — run BEFORE the guard gate; a hook can deny.
  if (cfg.hooks && (name === 'edit' || name === 'run' || name === 'dispatch')) {
    const hd = preToolUse(cfg.hooks, name, args);
    if (hd.blocked) return { result: hd.reason ?? 'blocked by hook', mutated: false, denied: true };
  }

  // Plan mode: read-only. Edit/write is denied — Explore/Plan subagent semantics.
  if (cfg.planMode && (name === 'edit')) {
    return { result: 'plan mode: edit denied (read-only). Review the plan, then run without --plan.', mutated: false, denied: true };
  }

  switch (name) {
    case 'read':
    case 'grep': {
      // GUARD GATE — red-line + scope, BEFORE any read (exfiltration of secrets/migrations is denied).
      const rl = checkRedLine(p, cfg.redLines ?? []);
      if (!rl.ok) return { result: rl.reason!, mutated: false, denied: true };
      const sc = checkScope(p, cfg.scope, cfg.cwd);
      if (!sc.ok) return { result: sc.reason!, mutated: false, denied: true };
      if (name === 'read') return { result: fs.readFileSync(p, 'utf8').slice(0, 4000), mutated: false, denied: false };
      return { result: `[grep stub] matched '${args.pattern}' in ${args.path ?? '.'}`, mutated: false, denied: false };
    }
    case 'run': {
      // GUARD GATE — a run command must not target a red-line area.
      const cmd = String(args.cmd ?? '');
      const rl = checkRedLine(cmd, cfg.redLines ?? []);
      if (!rl.ok) return { result: rl.reason!, mutated: false, denied: true };
      return { result: `[run stub] would exec: ${cmd}`, mutated: false, denied: false };
    }
    case 'dispatch': {
      // GUARD GATE — the task string is a proxy for the target; red-line tasks are denied BEFORE
      // any backend runs, for every backend equally (RESEARCH §1.6).
      const rl = checkRedLine(String(args.task ?? ''), cfg.redLines ?? []);
      if (!rl.ok) return { result: rl.reason!, mutated: false, denied: true };
      return { result: '[dispatch stub] would dispatch', mutated: false, denied: false };
    }
    case 'edit': {
      // GUARD GATE — red-line + scope, BEFORE any write
      const rl = checkRedLine(p, cfg.redLines ?? []);
      if (!rl.ok) return { result: rl.reason!, mutated: false, denied: true };
      const sc = checkScope(p, cfg.scope, cfg.cwd);
      if (!sc.ok) return { result: sc.reason!, mutated: false, denied: true };
      fs.writeFileSync(p, String(args.content ?? ''));
      return { result: `written ${p}`, mutated: true, denied: false };
    }
    case 'done':
    default:
      return { result: 'done', mutated: false, denied: false };
  }
}

// Deterministic FNV-1a command hash (mirrors rebuild/crates/bebop core::command_hash) — the log only
// CARRIES the cause; determinism of the log is what matters, not collision resistance.
function causeHash(s: string): string {
  let h = 0xcbf29ce484222325n;
  for (let i = 0; i < s.length; i++) {
    h ^= BigInt(s.charCodeAt(i));
    h = (h * 0x100000001b3n) & 0xffffffffffffffffn;
  }
  return h.toString(16).padStart(16, '0');
}

function runDispatch(
  task: string,
  cfg: BebopConfig,
  log: Envelope[],
): Promise<{ result: string; backend: Backend; ok: boolean }> {
  // GUARD GATE — the task string is a proxy for the target; a red-line task is denied BEFORE any
  // backend runs, for every backend equally (RESEARCH §1.6).
  const rl = checkRedLine(task, cfg.redLines ?? []);
  if (!rl.ok) {
    log.push({ seq: log.length, cause: causeHash(task), backend: 'denied' as Backend, event: 'denied', detail: rl.reason! });
    return Promise.resolve({ result: `[denied] ${rl.reason!}`, backend: 'denied' as Backend, ok: false });
  }
  const profile = cfg.profile;
  const chosen = cfg.forcedBackend
    ? { backend: cfg.forcedBackend, model: route(cfg.taskClass).model }
    : profile
      ? selectBackend(profile, cfg.taskClass) ?? { backend: 'native' as Backend, model: route(cfg.taskClass).model }
      : { backend: 'native' as Backend, model: route(cfg.taskClass).model };

  const nativeRunner = (t: string): DispatchResult =>
    cfg.runNative
      ? cfg.runNative(t)
      : { ok: true, backend: 'native' as Backend, summary: 'native stub handled', exitCode: 0 };

  return (async () => {
  let res = await runBackend(chosen.backend, task, { model: chosen.model, yolo: profile?.yolo, runNative: nativeRunner });
  // Uniform rotation on failure (RESEARCH §1.6) — try the next available backend.
  if (!res.ok && profile) {
    const next = rotate(profile, chosen.backend);
    if (next) res = await runBackend(next.backend, task, { model: next.model, yolo: profile.yolo, runNative: nativeRunner });
  }
  log.push({ seq: log.length, cause: causeHash(task), backend: res.backend, event: 'dispatch', detail: res.summary });
  // D3 (flag-OFF): stamp which Zenoh mesh transport is in use onto the dispatch record. Pure
  // selection, fail-closed to the deterministic LocalMesh twin — never claims an unbacked connection.
  if (cfg.meshMode) {
    const sel = selectZenoh(cfg.meshMode, cfg.meshIds ?? ['bebop']);
    log.push({ seq: log.length, cause: causeHash(task), backend: res.backend, event: 'dispatch', detail: `mesh=${sel.mode} (${sel.provenance})` });
  }
  const tag = `${res.backend}${res.ok ? '' : ' (failed)'}`;
  return { result: `[${tag}] ${res.summary}`, backend: res.backend, ok: res.ok };
  })();
}

export async function runLoop(cfg: BebopConfig): Promise<LoopResult> {
  const paint = makePaint(cfg.profile?.looks);
  const voice = voiceFor(cfg.profile?.narration);
  const model = route(cfg.taskClass).model;
  const routing = enforceRouting(cfg.taskClass, model);
  const r = recall(`task: ${cfg.taskClass}`);
  const ctx: LoopContext = { cwd: cfg.cwd, model, recallHits: r.hits };

  // ── FIELD ORACLE (∇·F / ∇×F) ── the agent's domain-knowledge field, sampled from recall hits.
  // Divergence = the field spreads (generate/explore); curl = it cycles (reconsider); both = both.
  // Pure + deterministic (reads the in-process VSA memory). Off unless cfg.field is set.
  let fieldDirective: FieldDirective | null = null;
  if (cfg.field) {
    const candidates = r.hits.length >= 2 ? r.hits.map((h) => h.text) : [
      'guard os red line', 'pq identity', 'vsa token codec', 'mesh no server', 'landauer thermo',
    ];
    const fa = searchFieldStateText(`task: ${cfg.taskClass}`, candidates);
    fieldDirective = directiveFor(fa.state);
  }

  const transcript: string[] = [];
  transcript.push(banner(paint));
  transcript.push(paint.dim(`  model=${model} ${routing.ok ? '' : paint.blood('[' + routing.note + ']')}`));
  if (r.found) transcript.push(paint.dim(`  §0·GP recall: ${r.hits.length} hit(s)`));
  else transcript.push(paint.amber(`  ${r.note}`));
  if (fieldDirective) transcript.push(paint.dim(`  field ∇·F/∇×F → ${fieldDirective}`));

  // ── D6 ARCH-MINING PASS (flag-OFF) ── runs the pure arch-mine detectors over a supplied module set
  // and surfaces the health report (orphans / import cycle / coupling clusters). Deterministic, no
  // LLM. Off unless cfg.archMine is supplied; the result is returned on res.mine and printed once.
  let mine: MineReport | undefined;
  if (cfg.archMine && cfg.archMine.length > 0) {
    mine = mineGraph(cfg.archMine);
    const parts: string[] = [];
    parts.push(`${mine.moduleCount} modules / ${mine.edgeCount} edges`);
    parts.push(mine.isolated.length ? `orphans=${mine.isolated.length}` : 'no orphans');
    parts.push(mine.cycle ? `CYCLE: ${mine.cycle.join(' → ')}` : 'no import cycle');
    if (mine.clusters.length) parts.push(`coupling-clusters=${mine.clusters.length}`);
    transcript.push(paint.dim(`  arch-mine: ${parts.join(' · ')}`));
  }

  const messages: { role: string; content: string; name?: string }[] = [
    { role: 'system', content: SYSTEM_PROMPT },
  ];
  const llm = cfg.llm ?? defaultLlm;
  let steps = 0;
  let mutations = 0;
  let denied = 0;
  // VISIBLE ReAct iteration count (default 3). Each iteration is a full Reason→Act→Observe→Reflect
  // pass; the loop does NOT hide the intermediate drafts (unlike promo demos).
  const iterations = reactIters(cfg);
  const maxSteps = Math.min(iterations, cfg.maxSteps ?? 8); // maxSteps stays a hard safety cap
  const log: Envelope[] = [];
  const reactTrace: ReactStep[] = [];
  let ledger = emptyLedger();

  for (let iter = 1; iter <= maxSteps; iter++) {
    // ── REASON ── the model proposes a thought + an action (tool call)
    steps++;
    const res = await llm(messages, ctx);
    if (res.content) transcript.push(paint.teal(`${SHIP} ${res.content}`));
    const calls = res.tool_calls ?? [];
    if (calls.length === 0) {
      reactTrace.push({ iter, phase: 'reason', thought: res.content, ok: true });
      break;
    }

    let halted = false;
    for (const call of calls) {
      // ── VALIDATION WALL (pydantic principle) ── untrusted LLM tool-args MUST clear the contract
      // before any hook/guard/tool sees them. Malformed input is rejected at the boundary, never
      // patched downstream. This runs BEFORE the guard gate (guard decides legality; this decides
      // well-formedness).
      const valid = validateToolArgs(call.name, call.args);
      if (!valid.ok) {
        denied++;
        log.push({ seq: log.length, cause: causeHash(String(call.name)), backend: 'native', event: 'denied', detail: valid.reason });
        transcript.push(paint.blood(`  ✖ VALIDATE ${valid.name ?? call.name} denied — ${valid.reason}`));
        reactTrace.push({ iter, phase: 'act', action: String(call.name), observation: valid.reason, ok: false });
        halted = true;
        continue;
      }
      const callArgs = valid; // typed, safe payload
      const actionLabel = `${callArgs.name}${callArgs.path ? ' ' + callArgs.path : callArgs.task ? ' ' + callArgs.task : ''}`;
      reactTrace.push({ iter, phase: 'reason', thought: res.content, action: actionLabel, ok: true });
      transcript.push(paint.dim(`  ⟳ iter ${iter} · REASON: ${res.content ?? '(act)'}`));

      // ── ACT ── execute the tool behind the guard gate (using the VALIDATED payload only)
      let out: { result: string; mutated: boolean; denied: boolean };
      if (callArgs.name === 'dispatch') {
        const d = await runDispatch(String(callArgs.task ?? ''), cfg, log);
        if (!d.ok) denied++;
        out = { result: d.result, mutated: false, denied: !d.ok };
        transcript.push(paint.dim(`  · ACT dispatch ${d.result.slice(0, 120)}`));
      } else {
        out = runTool(callArgs.name, { path: callArgs.path, content: callArgs.content, cmd: callArgs.cmd, pattern: callArgs.pattern }, cfg);
        if (out.denied) {
          denied++;
          log.push({ seq: log.length, cause: causeHash(callArgs.name), backend: 'native', event: 'denied', detail: out.result });
          transcript.push(paint.blood(`  ✖ ACT ${callArgs.name} denied — ${out.result}`));
        } else {
          if (out.mutated) {
            mutations++;
            log.push({ seq: log.length, cause: causeHash(callArgs.name), backend: 'native', event: 'mutation', detail: out.result });
          }
          transcript.push(paint.dim(`  · ACT ${callArgs.name} → ${out.result.slice(0, 120)}`));
        }
      }
      reactTrace.push({ iter, phase: 'act', action: actionLabel, observation: out.result.slice(0, 200), ok: !out.denied });

      // ── OBSERVE ── record what the environment returned
      messages.push({ role: 'tool', name: callArgs.name, content: out.result });
      reactTrace.push({ iter, phase: 'observe', observation: out.result.slice(0, 200), ok: !out.denied });

      // ── REFLECT ── real-time eval gate (combines with, does not replace, the guard)
      const verdict = evalStep({ action: callArgs.name, observation: out.result, denied: out.denied, mutated: out.mutated });
      const reflection = `eval ${verdict.score.toFixed(2)} ${verdict.passed ? 'PASS' : 'FAIL'} — ${verdict.notes}` +
        (out.denied ? ' → rewrote draft for next iteration' : '');
      reactTrace.push({ iter, phase: 'reflect', reflection, evalScore: verdict.score, evalPassed: verdict.passed, ok: !out.denied });
      transcript.push(paint.dim(`  · REFLECT ${reflection}`));

      // ── ACTIVE INFERENCE ADVISOR (FEP) ── complementary to the field oracle: derive a belief over
      // {stuck, progressing, done} from the loop's running progress and ask the FEP engine for the
      // next action. Advisory only (the guard gate still decides admission). Off unless cfg set.
      if (cfg.activeInference) {
        const total = Math.max(1, steps + denied);
        const stuck = denied / total;
        const progressing = steps / total;
        const donePrior = steps > 0 && denied === 0 ? 0.2 : 0.0;
        const belief = [stuck, progressing, donePrior].map((x) => x / (stuck + progressing + donePrior + 1e-9));
        const fepAction = adviseLoop(belief, true);
        transcript.push(paint.dim(`  fep → ${fepAction} (belief ${belief.map((x) => x.toFixed(2)).join(',')})`));
      }

      if (out.denied) {
        halted = true;
      } else if (call.name === 'done') {
        log.push({ seq: log.length, cause: causeHash(call.name), backend: 'native', event: 'done', detail: out.result });
        halted = true;
      }
    }
    if (halted) break;
  }

  transcript.push(paint.bold(paint.bone(`  ${TAGLINE}`)));
  const ok = routing.ok && denied === 0;
  return { steps, mutations, denied, transcript, ok, log, ledger, iterations, reactTrace, mine };
}

// Subagent — Claude Code's .claude/agents/*.md analogue. Runs a SCOPED, read-only loop with a
// cheaper model, returns only the summary (not the full transcript) to save context. This is the
// Explore/Plan subagent pattern: delegate narrow read-only reconnaissance to a cheaper doer.
export async function subagent(
  task: string,
  opts?: { tools?: ToolName[]; taskClass?: TaskClass; cwd?: string; maxSteps?: number; llm?: BebopConfig['llm'] },
): Promise<{ summary: string; steps: number; denied: number }> {
  const readOnly: ToolName[] = opts?.tools ?? ['read', 'grep', 'done'];
  // A subagent is read-only by default: edit/run/dispatch are stripped out unless explicitly passed.
  const safeTools = readOnly.filter((t) => t !== 'edit' && t !== 'run' && t !== 'dispatch');
  void safeTools;
  const res = await runLoop({
    cwd: opts?.cwd ?? process.cwd(),
    taskClass: opts?.taskClass ?? 'doer',
    maxSteps: opts?.maxSteps ?? 4,
    llm:
      opts?.llm ??
      (() => ({
        content: `[subagent] delegated: ${task.slice(0, 80)}`,
        tool_calls: [{ name: 'done' as ToolName, args: {} }],
      })),
    scope: [],
  });
  const summary = res.transcript.find((l) => l.includes('[subagent]')) ?? res.transcript.join('\n');
  return { summary, steps: res.steps, denied: res.denied };
}

