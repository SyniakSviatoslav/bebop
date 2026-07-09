# Dev-system hardening + anti-hallucination plan (bebop)

Date: 2026-07-09 · Author: Hermes agent · Status: DRAFT plan, no code yet
Focus: (1) DEFER practical telemetry + Dowiz-in-action; (2) fix + upgrade the internal
dev/verification loop and kill agent (self-)hallucination — both mine and the project's.

Verification basis for this analysis (falsifiable, run today):
- `npm test` → **410 pass / 0 fail** (authoritative runner).
- `node scripts/verify-doc-claims.mjs` → **FAILS**: "test count honest" check RED
  (README says 351, actual 410). Pre-commit hook is therefore currently RED.
- `grep` of `verify-doc-claims.mjs` for analytics → **0** hits (none of the new
  analytics modules are covered by the anti-hallucination gate).
- AGENTS.md line 55 claims "165 falsifiable tests" (stale).
- dowiz has `scripts/guardrail-falsifiable-proof.mjs` (asserts every enforced proof can
  go RED); bebop-repo has NO equivalent — only `verify-doc-claims.mjs`.

---

## PART 0 — DEFERRED (recorded, NOT acted on now)

These are real EV but explicitly out of scope per the operator's directive
("practical telemetry and Dowiz in action → record in plans, defer; focus on dev-system
+ anti-hallucination first"):

| # | Item | Why deferred | Re-open when |
|---|------|--------------|--------------|
| D1 | Wire `telemetry-ica-loop` (ICA→cycle-consistency) to the REAL Dowiz telemetry stream | needs a live/staged telemetry source + flag-OFF shadow harness | operator approves a feature flag + a staging telemetry feed |
| D2 | Wire ICA pipeline into `governor.ts` (today only `cycleConsistency` + `pcaAnomaly` are wired flag-OFF) | module is verified but un-wired into runtime | after D1 shadow run proves stable |
| D3 | `selectZenoh` + `prove` into kernel dispatch (flag-OFF, RED+GREEN) | **DONE 2026-07-09** — wired into the dispatch SHELL (`runDispatch`, not the pure kernel — selection does IO); `cfg.meshMode` flag-OFF; fail-closed to LocalMesh twin; RED+GREEN+falsifiable in `src/loop.test.ts` | ✅ landed |
| D4 | Dowiz ETA model (quantileLoss + huber, prediction intervals) | needs a real ETA seam in `apps/api` | when ETA module located + scoped |
| D5 | RAG noise-cleaning (pcaFit/pcaProject before recall in `knowledge.ts`) | feature, not a correctness blocker | after D1 |
| D6 | Causal graph over module-import adjacency (counterfactual "points of failure") | R&D, needs real trace | separate causal-discovery arc |

D1/D2 are the highest-EV of the deferred set (they close the loop on the two modules
built today). They stay frozen until the dev-system below is solid — shipping a
half-wired runtime harness on top of a RED pre-commit would reintroduce exactly the
false-green risk this plan exists to kill.

---

## PART 1 — ANTI-HALLUCINATION: fix the gate that is ALREADY RED (no-risk, do first)

The project's strongest anti-hallucination tool (`verify-doc-claims.mjs` → pre-commit)
is failing right now. A guardrail that's red-but-ignored is worse than none — it teaches
the system that red is fine. Concrete fixes:

### 1.1 Correct the stale test counts (make the gate GREEN again)
- README.md:351 → `410 TS tests (RED+GREEN), 0 fail`.
- AGENTS.md:55 → `npm test — 410 falsifiable tests`.
- Falsifiable check: after edit, `node scripts/verify-doc-claims.mjs` must exit 0.
- WHY max EV / no risk: pure doc correction; unblocks the pre-commit gate so it can
  catch the NEXT false claim instead of being permanently red.

### 1.2 Make the gate parse the count from the authoritative runner, not a hardcoded regex on README
- Today the gate greps README's *claimed* number and compares to `npm test`. If README
  drifts again it goes red — fine — but the *source of truth* should be `package.json`'s
  `test` script, not a prose line. Add a check: README/AGENTS count must equal
  `node --test --import tsx $(find src -name '*.test.ts')` pass total. (Same math, but
  assert on both doc surfaces, not just README.)
- RED case: lower the README/AGENTS number by 1 → gate exits 1.

### 1.3 Extend the gate to the analytics modules (close the blind spot)
The new L5 stack (matrix/anomaly/loss/cycle-consistency/ica/telemetry-ica-loop) is
invisible to `verify-doc-claims.mjs`. Add checks J–L:
- J: `governor.ts` exposes `pcaAnomaly` + `cycleBroken` state fields AND they are
  flag-OFF (absent in default `GovernorState` unless configured).
- K: `src/integration/analytics/telemetry-ica-loop.ts` exists + its test asserts the
  EV (sparse single-source localization) AND the RED Gaussian blind-spot.
- L: `AGENTS.md` "symmetrical loops" rule present; `cycle-consistency-theorem.md`
  exists and is referenced.
- Falsifiable: deleting `cycleBroken` from governor → J exits 1.

---

## PART 2 — ANTI-HALLUCINATION: port dowiz's `guardrail-falsifiable-proof` into bebop

bebop has the *doc* honesty gate but NOT the *proof* honesty gate. dowiz's
`guardrail-falsifiable-proof.mjs` enforces Verified-by-Math principle 3: **every
enforced proof must be able to go RED** (a test that cannot fail is a false-positive
metric and does NOT validate). This is the deeper layer — it stops "green tests that
prove nothing."

### 2.1 Port + adapt `guardrail-falsifiable-proof.mjs` to bebop
- Reuse the dowiz logic (scan `src/**/*.test.ts` for `test(` blocks; assert each
  load-bearing behavior test has a paired RED assertion or a `not ok`/negative branch).
- Bebop-specific: every analytics module already ships RED+GREEN pairs (matrix/anomaly/
  loss/cycle-consistency/ica/telemetry-ica-loop) — so the port should PASS on the
  current tree, and RED on any future test that only asserts the happy path.
- Wire into pre-commit AFTER `verify-doc-claims.mjs` (both must pass to commit).
- Falsifiable: add a test that only asserts `assert.ok(true)` for a behavior change →
  guardrail exits 1.

### 2.2 Add a "compaction / stale-summary" guard for agent runs
The CLASS of bug that bit this session: a compaction summary (or an injected "prior
context") can assert a file state that is no longer true, and I then "act on" the
summary instead of re-reading. Mitigations:
- 2.2a: in `AGENTS.md`, add a hard rule: **"re-read the file before acting on any
  summarized/compaction claim; a summary is a hint, never a source of truth."** (This is
  already implied by "Read before edit" but make it explicit for summaries.)
- 2.2b: when verifying, always run the ACTUAL command and paste its output — never paste
  a remembered number. (Operator discipline already requires this; encode it as a
  gate-able rule.)

---

## PART 3 — DEV-SYSTEM: make verification the DEFAULT, not the afterthought

### 3.1 Single authoritative verify command
- `npm test` already is `node --test --import tsx $(find src -name '*.test.ts')` — good.
- Add `npm run verify` = `npm run typecheck && npm test && node scripts/verify-doc-claims.mjs`
  (and later `guardrail-falsifiable-proof`). One command = full gate. Cheap to run, run
  after every change.
- NOTE: `pnpm run test` (per dowiz HERMES.md) would MISS bebop's `src/integration/**`
  if it used the wrong glob — bebop's `npm test` already uses `find`, so it's safe.
  Keep `find`, never switch to a hardcoded `src/**` glob that node --test may not expand.

### 3.2 Pre-commit must be enforceable, not just present
- `.git/hooks/pre-commit` IS active (not `.sample`) — good. But no CI equivalent runs on
  push in bebop-repo (only dowiz has `run-armaments.sh`). Add a GitHub Action that runs
  `npm run verify` on PRs so a red gate can't be pushed around by `git commit --no-verify`.
- Falsifiable: a PR that lowers the test count must fail CI.

### 3.3 Kill silent "unverified" drift
- The repeated "[System: Verification status: unverified]" reminders this session were
  stale (files were green). Root cause: the reminder snapshot didn't refresh. Add a
  post-edit step (could be a hook or just discipline): after editing, ALWAYS run
  `npm run verify` in the same turn and paste output. The reminder is advisory; the
  pasted command output is the proof.

---

## PART 4 — DEV-SYSTEM: reduce MY hallucination surface (agent-side)

Things I can do better as the agent, encoded as rules:

1. **Re-read, don't trust the summary.** Any "prior context / compaction" block is a
   hint. Before editing a file the summary claims exists/changed, `read_file` it.
2. **Paste real command output, never recalled numbers.** "410 pass" comes from running
   `npm test`, not memory.
3. **Ship RED before GREEN in the same file.** Every new behavior test gets its failing
   assertion first (or a sibling RED test) so the green is falsifiable.
4. **Stale doc counts are a red-line for the doc gate, not a TODO.** Fix them the moment
   the gate catches them (1.1).
5. **Don't over-claim wiring.** "verified" ≠ "wired into runtime". Be explicit about
   flag-OFF / un-wired (as done for telemetry-ica-loop today).
6. **Defer loudly.** When an item is deferred (Part 0), say WHY and the re-open
   condition, so it isn't silently dropped or silently "finished".

---

## Sequencing

1. **1.1 + 1.2** — correct counts, make gate green (minutes, no risk). DO FIRST.
2. **1.3** — extend gate to analytics (closes today's blind spot).
3. **3.1 + 2.1** — `npm run verify` + port falsifiable-proof guardrail.
4. **2.2 + 3.2 + 3.3 + Part 4** — rule/hook/CI hardening (anti-stale, anti-summary-trust).
5. Deferred (Part 0) only after 1–4 land and the gate is green+enforced.

## Honest gaps in THIS plan
- No code written yet — this is the analysis + plan the operator asked for.
- Porting `guardrail-falsifiable-proof.mjs` needs a read of the dowiz original to adapt
  the AST/regex scan; that read is step 2.1's first action.
- CI action (3.2) requires a GitHub workflow file + the repo's CI to exist for bebop
  (dowiz has `run-armaments.sh`; bebop may need a new one).
