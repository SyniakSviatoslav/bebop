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
- `npm test` — 434 falsifiable tests.
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
