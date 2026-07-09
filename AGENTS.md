# AGENTS.md — Bebop

Bebop is a standalone AGPL-3.0 coding-agent CLI. Operating rules for any agent (Claude Code,
Hermes, Codex, OpenCode, Aider, or Bebop itself) working in this repo.

## Hard rules (from docs/RULES.md — non-negotiable)
- **Constant Doubt**: no statement in docs is true unless backed by a live probe or a
  deterministic test. Unverified = false. Ship the RED case alongside the GREEN.
- **Verified-by-Math**: every behavior change ships with a falsifiable RED+GREEN test.
- **Red lines** (per-change human gate, never auto-touch without confirmation): auth, money,
  RLS/migrations, secrets, bulk edits.

## Universal rule — symmetrical loops (cycle consistency) wherever they add EV
- **Definition**: a symmetrical loop = an invertible `Decompose → Reconstruct` pair over a
  state snapshot `X`, asserting `Reconstruct(Decompose(X)) ≈ X` (i.e. `F(G(X)) == X`). The
  residual `‖X − X̂‖` is the *symmetry gap*; its per-feature `rⱼ` localizes which module broke.
- **Where it adds EV (use it)**: any function with a cheap, deterministic `Decompose/Reconstruct`
  pair — state-delta round-trips, telemetry/feature-vector reconciliation, serialization
  (encode/decode), config→effect→config, plan→state→plan (the "hallucination filter").
  It automates regression, degradation, and property-based self-testing.
- **Where it does NOT add EV (do NOT rely on it alone)**: semantic truth and hard red-line
  boundaries. A symmetric-but-wrong map (`x→2x→x/2`) has gap 0 yet is wrong — see
  `src/integration/analytics/cycle-consistency.test.ts` (RED blind-spot case). Pair every
  loop with ≥1 ground-truth oracle for money/RLS/drone-physics/contract correctness.
- **Implementation**: `src/integration/analytics/cycle-consistency.ts` (deterministic PCA
  round-trip, no RNG/training). Proof + bounds in `docs/design/cycle-consistency-theorem.md`.
- **Deployment**: flag-OFF by default; shadow (log drift) before gate (block). Never a
  replacement for tests — a complement. Wire via `GovernorConfig.cycleConsistency`.

## Universal rule — L5 Neuro-Symbolic Gate (advisor proposes, kernel decides)
- **Definition**: any stochastic advisor (LLM / GNN / heuristic) is a *consultant* that PROPOSES an
  authority + `predictedQuality`. The deterministic kernel (`Governor`) is the only actor that writes
  `authority` to the actuator plane; a symbolic arbiter (`clamp`, factor-kill, resonance-cap,
  safe-state floor, poison guard, cycle/PCA breach gate) sits between them and **mathematically
  cannot emit an out-of-contract command**. See `docs/design/adr-003-neuro-symbolic-gate-2026-07-09.md`.
  Plan-step validity is verified structurally via Logical CoT (PDDL-INSTRUCT, arXiv:2509.13351):
  `src/integration/logicalCot.ts` proves each step's preconditions/effects/invariants before admission —
  see `docs/design/adr-004-logical-cot-pddl-instruct-2026-07-09.md`.
- **Where it adds EV**: stops advisor hallucinations from reaching actuators; the empirical proof the
  gate works is `bridgeMetrics().hallucinationRate` (N7) — how often the kernel overrode the advisor.
- **Where it does NOT add EV**: making the advisor "smarter" via runtime RLHF/PPO. Sovereign-core
  forbids SGD/RNG/Date at runtime (air-gapped) — training is offline-only. A GNN advisor (N6) slots in
  behind `GnnAdvisor` with **zero kernel change**; its proposals are gated by `dualTrackGate` against
  the deterministic Truth Layer graph so a hallucinated edge/route is rejected (`no-such-edge`).
- **Implementation**: `src/governor.ts` (gate) + `src/integration/analytics/dual-track.ts` (seam).
  Causal blast-radius surfaced via `pointsOfFailure` (N4).

## Universal rule — As-above-so-below checker (verify-then-admit at every scale)
- **Definition**: the SAME fail-closed `Checker` abstraction that admits a command in the kernel
  (`applyCommandChecked` in `kernel.ts`) recurs at every scale — kernel (decide/fold), agent
  (copilot distinct-backend checker), plan (`logicalCot` step-wise logic auditor), tool-args
  (`validate` boundary contract), draft (`speculate` guard-verifies). The primitive is always
  *verify-then-admit, quarantine-on-failure*; the verifier is NEVER the same component that produced.
- **Where it adds EV**: one uniform, auditable trust boundary instead of N ad-hoc checks; a violation
  at any scale has the same shape and the same fail-closed semantics (localize → quarantine → re-plan).
- **Where it does NOT add EV**: when the verifier and producer are collapsed into one component (defeats
  independence). Enforce Cross-pattern B (propose-don't-execute) so the producer is always stochastic/
  LLM and the verifier always deterministic/distinct.
- **Implementation**: `kernel.ts applyCommandChecked` + `copilot.ts` + `logicalCot.ts` + `validate.ts`
  + `speculate.ts`. See `docs/design/bebop-fundamental-principles-2026-07-09.md` (Cross-pattern A).

## Universal rule — Propose-don't-execute (stochastic layer never gets the actuator)
- **Definition**: any stochastic/advisor component (LLM, GNN, heuristic) MAY propose/name an intent but
  NEVER writes to the actuator plane. Execution is always a deterministic function over a verified state.
  Concretely: advisor proposes → deterministic verifier checks → deterministic executor applies (or the
  planner returns NO PATH when preconditions are unmet).
- **Where it adds EV**: it is the single topology that makes every other safety property possible —
  kernel (ADR-003), dual-track (N6), copilot, speculate, logicalCot, GOAP (N8c) all rely on it. A
  hallucinated plan physically cannot act because the executor enumerates transitions, not the model.
- **Where it does NOT add EV**: letting an LLM call a tool directly (bypassing the gate) — that collapses
  to propose-and-execute and re-introduces the failure mode. Enforced by `guard.ts` + `loop.ts` GUARD GATE.
- **Implementation**: see `src/kernel.ts` (advisor proposes, kernel decides), `src/integration/analytics/
  dual-track.ts`, `src/copilot.ts`, `src/speculate.ts`, `src/integration/logicalCot.ts`,
  `src/integration/analytics/goap.ts`. See principles doc (Cross-pattern B).

## Universal rule — Flag-OFF → shadow → gate (no feature goes live silently)
- **Definition**: every new analytic/integration is FLAG-OFF by default (inert unless a caller supplies
  its cfg). Deployment ladder: OFF → **shadow** (run in background, log drift, no blocking) → **gate**
  (block on breach) — and red-line actions are NEVER gated on a statistical loop alone.
- **Where it adds EV**: bounds the blast radius of a wrong/unproven module; proves the false-positive rate
  in shadow before any blocking. Matches the cycle-consistency theorem deployment (§6).
- **Where it does NOT add EV**: gating a safety-critical/red-line action on a single statistical signal
  (use the deterministic contract checks + a ground-truth oracle instead).
- **Implementation**: 8 FLAG-OFF seams (cycle-consistency, ica, kalman, degradation, mesh, arch-mine,
  field, active-inference, dual-track, logicalCot, redteam, modelGateway, multipilot, shadow, etc.). See principles doc
  (Cross-pattern C).

## Universal rule — Multipilot (brain-inside-brain, multidimensional verification)
- **Definition**: for any agentic prompt (reasoning / review / reverse-engineering / research / planning),
  run ≥3 INDEPENDENT verifier loops in parallel over the same artifact and overlay their verdicts as a
  tensor (disagreement = dimension). One checker (copilot) is necessary but not sufficient; N≥3 independent
  checkers catch the failure modes any single one blind-spots (cf. cycle-consistency's self-inverse blind
  spot). The orchestrator promotes the artifact only when the overlay converges; divergence is surfaced
  for human triage, never silently averaged away.
- **Where it adds EV**: turns single-point review into a multidimensional integrity signal; each verifier
  is a distinct axis (e.g. structural/logical, adversarial/red-team, oracle/truth), so a hallucination that
  fools axis 1 is caught by axis 2 or 3. Default for ALL agentic surfaces.
- **Where it does NOT add EV**: colluding checkers (same model/prompt) — independence is the whole point.
  Each loop MUST differ in method or model; identical checkers add latency, not integrity.
- **Implementation**: `src/integration/multipilot.ts` — `multipilot(artifact, loops[])` runs N independent
  verifier fns, returns the per-axis verdicts + an `overlay` (converged / divergent) + a recommended action.
  FLAG-OFF seam; composable with copilot/redteam/logicalCot. See principles doc (Cross-pattern + the
  "tensor overlay" directive 2026-07-09).

## Repo layout
- `bebop.ts` — CLI entry (subcommands: boot, run, agents, use, recall, route, map, diagrams,
  **docs**, mcp, self, init, and the `/`-slash commands).
- `src/` — guard OS (`guard.ts`), Rust/WASM kernel (`core-wasm.ts` + `crates/core`), living
  memory, governor, routing, backends, MCP server, skills/hooks/subagents.
- `docs/` — the in-repo wiki (features, integrations, diagrams, footage, narration).
- `scripts/` — diagram + footage + i18n generators.

## Documentation pipeline (`bebop docs`)
The polished, repeatable doc-release flow. Run before any main release:
- `bebop docs build` — typecheck + tests + wasm + diagrams + map + i18n parity (no LLM needed).
- `bebop docs check` — release-readiness audit (gifs resolve, manifests valid, version semver,
  OpenWiki wired). Exits non-zero if anything is off.
- `bebop docs init` / `bebop docs update` — generate/refresh the **OpenWiki** agent-facing wiki
  in `openwiki/` (needs an LLM key: set `OPENWIKI_PROVIDER` + `OPENWIKI_API_KEY`).

## Agent-facing wiki (OpenWiki)
This repo uses [OpenWiki](https://github.com/langchain-ai/openwiki) to maintain a structured,
agent-readable wiki under `openwiki/`. **When you need durable repo context that isn't in this
file, consult `openwiki/` first** rather than re-deriving it. The wiki is regenerated on a daily
CI schedule (`openwiki-update.yml`) and is kept in sync with `git` diffs — treat it as living
documentation, not gospel; verify non-trivial claims against code.

## Verify before claiming done
- `npm run verify` — one-shot full gate: typecheck + tests + doc-claim honesty + falsifiable-proof.
- `npm run boot` — guard-OS self-certification (must go RED to be trusted).
- `npm test` — 525 falsifiable tests.
- `npm run typecheck` — clean.
- After any doc change: `bebop docs check`.
- `node scripts/verify-doc-claims.mjs` — doc claims must match live code (pre-commit + CI).
- `node scripts/guardrail-falsifiable-proof.mjs` — every test must have a RED path (pre-commit + CI).

## Anti-hallucination discipline (agent + human)
- **Re-read before acting on any summary.** A compaction summary, injected "prior context", or a
  remembered file state is a HINT, never a source of truth. Before editing a file a summary claims
  exists/changed, `read_file` it. Before trusting a count/state, run the command.
- **Paste the REAL command output, never a recalled number.** "410 tests pass" comes from running
  `npm test`, not memory. After editing, run `npm run verify` in the same turn and paste the output.
- **Ship RED before GREEN.** Every load-bearing test needs a falsifiable (non-tautological) assertion;
  `guardrail-falsifiable-proof.mjs` enforces this on every commit.
- **A red gate is a red-line, not a TODO.** A stale doc count that trips `verify-doc-claims.mjs` is
  fixed immediately, not deferred — a guardrail that is red-but-ignored teaches the system red is fine.
- **Defer loudly.** Deferred work states WHY and the re-open condition, so it is neither silently
  dropped nor silently claimed done.
- **Don't over-claim wiring.** "verified" ≠ "wired into runtime". Flag-OFF / un-wired is explicit
  (e.g. telemetry-ica-loop and the ICA→governor stage are proven but not live-promoted).
