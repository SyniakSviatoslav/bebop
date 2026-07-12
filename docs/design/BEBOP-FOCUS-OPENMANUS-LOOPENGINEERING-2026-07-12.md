# Focus research 2: OpenManus · Loop Engineering → Bebop integration

Date: 2026-07-12 · Operator: SyniakSviatoslav · Status: RESEARCH + INTEGRATION PLAN (push-plans-first)

## TL;DR
- **OpenManus** → validates agent framework shape (Planner → Tool-call loop, multi-agent).
  Bebop already has `pddl.rs` (STRIPS planner) + `copilot.rs` (doer/checker) + `multipilot.rs`
  (N pilots). Gap: no unified **agentic loop driver**. Add `loop.rs`.
- **Loop Engineering** → "governed agentic loops for autonomous multi-step workflows".
  Maps to `governor.rs` (kill-switch) + `intent.rs` (GOAL/LOOP) + `lanes.rs` (parallel).
  Gap: `intent.rs` detects LOOP but does NOT run a loop (no iteration cap, no per-step verify,
  no rollback). Add `run_loop()` with max_iter + verify-gate + rollback-on-fail.
- Both are external/Python-ish; Bebop stays native Rust, offline, deterministic. Port CONCEPTS.

## 1. What each tool is (reverse-engineered)

### 1.1 OpenManus (AIWaves, openmanus.github.io)
Open-source agent framework. Core: Agent abstraction + Planner (decomposes task) + Tool-call
loop (agent calls tools, observes, repeats) + multi-agent coordination. Goal: let an LLM
autonomously complete open-ended tasks via iterative tool use.

### 1.2 Loop Engineering (cobusgreyling/loop-engineering + GitHub Topics)
"Practical patterns, starters & CLI tools for loop engineering with AI coding agents."
"Configurable runtime for governed agentic loops to reliably enable autonomous execution of
complex multi-step workflows." Key axes: (a) decide next move, (b) govern (kill/rollback),
(c) verify between iterations, (d) cap iterations.

## 2. Descartes-square (exact pros/cons, per J3)

### 2.1 OpenManus vs Bebop agent stack (copilot/multipilot/pddl)

| | OpenManus | Bebop (current) |
|---|---|---|
| **PRO** | Mature agent+planner+tool-loop; multi-agent OOTB | `pddl.rs` STRIPS planner; `copilot` doer/checker; `multipilot` N pilots + field gate |
| **CON** | Python; external LLM-bound; not deterministic | No unified loop driver; agents not auto-chained into a loop |
| **PRO** | Tool-call abstraction reusable | `mcp.rs` native tool surface; `field.rs` physics veto |
| **CON** | Opaque verification between steps | `verify-doc-claims` gate exists but not in agent loop |

**Steal:** unified loop driver that chains planner→doer→verify→repeat. **Avoid:** Python/LLM-bound.

### 2.2 Loop Engineering vs Bebop governor/intent/lanes

| | Loop Engineering | Bebop (current) |
|---|---|---|
| **PRO** | Explicit govern + verify + cap; "reliable autonomous" | `governor` kill-switch; `intent` GOAL/LOOP; `lanes` parallel |
| **CON** | Pattern lib, not a runtime primitive | `intent` detects LOOP but runs NO loop (gap) |
| **PRO** | Rollback on failure semantics | `agentic_git` content-addressed (COMMIT/LOG) — rollback-ready |
| **CON** | CLI/Python tooling | No `run_loop()` with max_iter + per-step verify |

**Steal:** `run_loop()` with max_iter + verify-gate + rollback-on-fail. **Avoid:** external CLI.

## 3. Integration plan (build)

### 3.1 loop.rs — governed agentic loop driver (native Rust, offline)
```
pub fn run_loop(
    intent: Intent,
    max_iter: usize,
    mut step: impl FnMut(usize) -> StepResult,
    verify: impl Fn(&StepResult) -> bool,
) -> LoopReport
```
- For LOOP intent (or explicit GOAL-with-loop): iterate up to `max_iter`.
- Each iteration: `step(i)` → `verify(&result)`; if verify FAILS → record failure, STOP
  (rollback semantic: caller decides via `agentic_git`). If PASS → continue.
- Returns `LoopReport { iterations, successes, last_failure: Option<usize>, done: bool }`.
- Deterministic, no RNG, no external LLM. Step/verify injected by caller.
- Tests: RED (verify always fails → loop stops at iter 0, reports failure) +
  GREEN (verify always passes → runs to max_iter, done=true).

### 3.2 Wire into intent.rs (auto-loop on LOOP detect)
- `intent::detect` already returns LOOP. `loop.rs::run_loop` consumes it.
- `lanes.rs` can spawn `run_loop` per lane (parallel autonomous workflows).

### 3.3 Agents / Skills
- Agent progress (level/xp) = living-memory nodes (Q3, planned).
- Skills = searchable store; loop can call skills as steps.

## 4. What NOT to do (YAGNI / ceilings)
- Don't adopt Python runtime. Native Rust only.
- Don't add LLM-in-the-loop (offline). Step/verify are injected by caller.
- Don't daemonize. CLI/library primitive.

## 5. Verification
- `cargo test -p bebop loop::` RED+GREEN.
- doc-claim verifier stays GREEN.
