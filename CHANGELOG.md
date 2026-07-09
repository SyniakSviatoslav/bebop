# Changelog

All notable changes to Bebop are documented here. Format: keep it falsifiable — every line is
backed by a RED+GREEN test in `src/**/*.test.ts` (authoritative runner:
`node --test --import tsx 'src/**/*.test.ts'`).

## [0.3.5] — 2026-07-09 — "Release workflow fix: portable test command (Node 20/22)"

### Fixed
- **GitHub Releases stuck at v0.2.0**: the `release.yml` workflow ran on Node 20.19, where
  `node --test` does NOT expand the quoted glob `'src/**/*.test.ts'` (needs `globstar`, off in
  CI's non-interactive bash). The Falsifiable-tests step failed with
  `Could not find 'src/**/*.test.ts'`, so NO v0.3.x Release was ever published — even though
  `ci.yml` (Node 22) was green. Root cause of "repo shows current release 0.2".
  - `package.json` `test` script now expands via `find src -name '*.test.ts'` (shell-independent,
    works on Node 20 and 22 — no globstar needed).
  - `release.yml` aligned to `node-version: '22'` (matches the green CI run).

### Verification (fresh, on main)
- `npm test` (find-based, the exact release command) → 351 pass / 0 fail / 0 skipped.
- `pnpm run typecheck` → 0 errors. `npm run boot` (Guard-OS self-cert) → certified.
- This is the precise command+Node the release workflow now runs; it passes locally, so the
  release run will publish v0.3.5 (latest) instead of stalling at 0.2.0.



### Fixed
- **CI was RED on main**: `src/harvest.test.ts` (test "harvest() report bundles candidates +
  patterns + cross + existing skills") hard-coded `rep.existingSkills.includes('review')`, depending
  on a `review` skill being installed in `~/.hermes/skills/` on the build machine. The runner lacks
  that skill → `expected true, actual false`, breaking `npm test` under CI Node 22/24 (passes locally
  only by accident of which skills are installed). `harvest(concepts, skills?)` already accepts an
  injected skill list, so the test now passes a deterministic `[review, deploy]` list and asserts the
  exact set — environment-independent. Real test-brittleness fix, no production behavior changed.
  - Test: `harvest.test.ts` now 9/9 deterministically.

### Verification (fresh, on main)
- `npm test` → 351 pass / 0 fail / 0 skipped. `pnpm run typecheck` → 0 errors. `npm run boot`
  (Guard-OS self-cert gate) → certified. Fix is independent of installed skills, so it holds on CI
  Node 22/24.



### Fixed
- **Scope gate anchored to `process.cwd()` instead of `cfg.cwd` (loop + dispatch attack surface).**
  `runTool()` in `src/loop.ts` called `checkScope(p, cfg.scope)` WITHOUT `cfg.cwd`, so relative
  scope globs (`tools/bebop/**`) were matched against `process.cwd()`. Whenever `cfg.cwd !==
  process.cwd()` — the entire purpose of the `cwd` config, and exactly what `bebop run` does
  (`cwd: path.resolve(HERE,'..')`, i.e. the repo root is one level above the process cwd) — a
  LEGITIMATELY in-scope edit was WRONGLY DENIED (fail-closed but broken), and if `process.cwd()`
  were a parent of `cfg.cwd` the gate would go OVER-PERMISSIVE. Surfaced by the agent-driven
  red-team subagent (loop/dispatch surface, left as a BUG, fixed here). Fix: pass `cfg.cwd` to
  `checkScope` at both the read/grep and edit call sites.
  - Tests: `redteam-run.test.ts` (GREEN relative-scope in-scope edit now admitted when
    `cfg.cwd != process.cwd()`; RED out-of-scope edit still denied with no over-permissive leak).

### Verification (fresh, on main)
- `npm test` → 351 pass / 0 fail (v0.3.2 was 350). `pnpm run typecheck` → 0 errors.
- Agent-driven red-team (3 subagents vs live CLI) is now fully RED+GREEN locked: F6/F7/F8/F9 (prior
  release) + F10 (this release). No outstanding real weaknesses reported.

## [0.3.2] — 2026-07-09 — "Agent-driven red-team: 3 real bugs fixed, honest next-phase scaffolds"

### Fixed (each with a falsifiable RED+GREEN test)
- **Governor NaN poison leak (recall/governor attack surface).** `Governor.step` fed a
  non-finite sample (NaN/Infinity) integrated NaN into the PID accumulator, permanently poisoning
  `authority` so later *finite* samples still yielded `authority=NaN` — silent L5 authority rot that
  the self-evolution gate trusts. Now `step` rejects non-finite samples: floors `authority` to `uMin`,
  sets `poisoned=true`, and does NOT integrate the bad sample. Recovery proven: a healthy sample after
  a NaN clears the poisoned flag and yields finite authority.
  - Tests: `redteam-recall.test.ts` (GREEN `Governor.step` with NaN → finite + `poisoned`; GREEN
    recovers on next finite sample).
- **`recall('')` fabricates a confident hit (honest-degradation violation).** `recallScored("")`
  substring-matched every node (`""` is a substring of all concepts) and returned `found=true` with all
  corpus nodes. Now an empty/whitespace query returns `found=false`, `hits=[]` (honest degradation).
  - Tests: `redteam-recall.test.ts` (GREEN `recall("")` / `recall("   ")` → no hits).
- **Resonance pre-check over-conservative (self-evolution).** `perturb = 1.4 + len/40` tripped
  ζ<0.707 (under-damped) for ANY idea longer than ~32 chars, making ordinary self-evolution near-
  impossible to admit. Re-scaled to a normalized change magnitude: well-damped for normal ideas,
  risky only for genuinely bulk (≫300-char) mutations. The bulk-rejection property is preserved.
  - Tests: `redteam-self.test.ts` (GREEN short idea admitted; RED genuine bulk quarantined),
    `consciousness.test.ts` (bulk fixture updated to ≥350 chars to reflect the corrected threshold).
- **Self-evolution fail-open on junk (quality).** The triviality gate was pure length (`< 4`), so
  any 4+ non-alphanumeric chars (`????`, control chars) were admitted as corpus mutations. Now also
  requires ≥1 alphanumeric token.
  - Tests: `redteam-self.test.ts` (GREEN junk rejected; GREEN alphanumeric short idea still admitted).

### Added (honest next-phase scaffolding — env-gated, fail-closed, NOT faked)
- **Real Zenoh mesh adapter** (`src/integration/zenoh/real-adapter.ts`): `selectZenoh(mode, ids)`
  returns the in-process `LocalMesh` twin by default; when the native `@eclipse-zenoh/zenoh-ts` client
  is present `mode:'real'` uses it. Requesting `real` without the native client FAILS CLOSED to the
  local twin (never claims a connection). Unknown mode throws.
  - Tests: `zenoh/real-adapter.test.ts` (5 GREEN falsifiable proofs).
- **Real zkVM prover adapter** (`src/integration/zkvm/prover-adapter.ts`): `prove(...)` returns the
  tamper-evident `decide()` digest by default; with `BEBOP_RISC0_PROVER` set + a real prover it
  returns a genuine STARK receipt. Requesting `prove` without a prover FAILS CLOSED to the digest
  (NO fabricated seal). Unknown mode throws.
  - Tests: `zkvm/prover-adapter.test.ts` (4 GREEN falsifiable proofs).

### Verification (fresh, on main)
- `npm test` → 350 pass / 0 fail (baseline 305 → +45 red-team + adapter + corrected tests).
- `pnpm run typecheck` → 0 errors. `pnpm run build` → bebop_core.wasm 183197 bytes (unchanged).
- Agent-driven red-team: 3 autonomous subagents exercised the LIVE CLI (self-evolution, loop/dispatch,
  recall/governor) and surfaced the 3 real bugs above; each is now locked by a RED+GREEN test.

## [0.3.0] — 2026-07-09 — "Sovereign Node: integrations composed into the one gate"

### Added
- **zkVM `decide()` journal — on by default at the kernel gate.** Every admitted command now emits a
  `JOURNAL` envelope with a tamper-evident digest over `(state, commandHash, seq)`. The kernel's
  `applyCommandChecked` journals unconditionally (`journal=true` default). Replay-verifiable via
  `verifyJournal` / `compose.verifyJournalChain`; tampering any entry fails the chain.
  - Tests: `core.test.ts` (GREEN digest verifies; RED tampered state fails), `compose.test.ts`
    (GREEN chain replays; RED tamper breaks it).
- **TigerBeetle money boundary composed into the kernel gate.** `applyCommandChecked(.., money=true)`
  runs the `moneyTransferChecker` structural law (`amount>0`, `debit≠credit`, idempotent) *in addition
  to* the caller's policy checker — fail-closed. Mint/burn/replay are refused at the universal gate.
  - Tests: `core.test.ts` (GREEN legal transfer; RED mint `amount<=0`, RED replay).
- **Active Inference advisor in the dispatch loop.** `adviseLoop` (FEP policy selector over
  `{stuck, progressing, done}`) surfaces an advisory action when `cfg.activeInference` is set; the
  guard still decides admission. Advisory-only, never overrides the gate.
  - Tests: `loop.test.ts` (GREEN advisor surfaces when flag on; RED stays off when flag off),
    `loop-advisor.test.ts`.
- **Optical field recall in `knowledge.ts`.** `recall(query, { opticalRecall: true })` re-ranks
  candidates by SVETlANNa/Meep field correlation (placed behind a thin-lens mask) as a *third, advisory*
  signal — graph score and vector sim dominate; optical never filters and never promotes a weak hit
  above a strong one.
  - Tests: `knowledge.test.ts` (GREEN candidate id-set preserved; RED graph score dominates optical).
- **Tamper-evident self-evolution audit.** `bebop self evolve` now records each approved corpus
  mutation as a kernel `PUBLISH` command (journaled) and exposes `verifySelfEvolution()` — the agent
  can prove its own evolution history is unbroken (falsifiable: tamper breaks the replay).
  - Tests: `consciousness.test.ts` (GREEN clean chain verifies; RED tampered digest fails).
- **Sovereign Node composition layer** (`src/integration/compose.ts`) is now the canonical apply path:
  it delegates to the kernel's single gate (zkVM journal + optional TigerBeetle money), so there is one
  decision path, not two.

### Changed
- **`npm test` now covers the integration layer.** The script glob changed from `src/*.test.ts` to
  `src/**/*.test.ts`, so `self maintain` and CI exercise the full RED+GREEN suite (was silently missing
  `src/integration/**`). Authoritative runner confirmed at **305 tests, 0 fail**.
- README + README.uk: added the "Sovereign Node" integrations table and corrected the test count.

### Security / hardening
- **Attack-team (3 red-team subagents) ran after wiring — concrete findings fixed:**

  **F1 — consciousness self-evolution gate drift (red-team).** The kernel gate previously ran
  *after* the corpus mutation (a post-hoc audit append, not the admission authority). Fixed:
  `selfEvolve` now computes `applyCommandChecked` *before* `mem.remember` and aborts the mutation if
  `quarantined` — the kernel verdict is the single source of truth for self-evolution admission.
  - Tests: `consciousness.test.ts` (GREEN admitted → JOURNAL envelope + state advances; RED quarantined
    → state unchanged, DENIED envelope emitted).

  **F2 — optical recall poisoning (red-team).** `recallLocal` assigned every graph-hit a flat
  `score: 1`, so the optical tertiary signal became the de-facto primary ranker; a planted linked
  memory node could reach recall #1 above the genuine hit. Fixed: graph hits now carry their REAL
  spreading-activation energy as `score` (exact match = 1, one hop ≤ decay), so the graph ranks the
  set; optical only re-orders *within equal primary scores*. Also fixed a latent `hits.indexOf(a)`
  comparator bug (it read the live, reshuffling array) by keying on a stable original index.
  - Repro (RED→GREEN): attacker node linked into the corpus now ranks #2 (score 0.5) behind the
    genuine `kernel law` node (score 1.0); previously optical promoted it to #1.

  **F3 — `adviseLoop` belief not validated (red-team).** Degenerate/negative/un-normalized beliefs
  silently produced actionable output (e.g. `[1,1,1]` → `'done'`). Fixed: `adviseLoop` now requires
  a finite, non-negative, non-zero-sum belief and normalizes it; otherwise it throws (no silent
  directive). Confirmed the FEP advisor still cannot admit/deny a command (advisory-only).

  **F4 — money checker fail-open crash (red-team).** `BigInt()` on malformed input threw out of the
  checker, failing the whole command stream open (DoS). Fixed: parse is wrapped in try/catch returning
  `{ ok:false, reason }`, and non-string bigint fields are rejected as malformed. Conservation is
  enforced at shell apply-time via `applyMoneyTransfer`/`moneyConserved`.

  **F5 — journal keyless + no cumulative binding (honest framing).** The zkVM "tamper-evident" digest
  is a keyless FNV-style recomputation: it detects *accidental* bit-flips of stored digests but is
  forgeable by anyone who controls the stored `(state, digest)` (no secret/key/SNARK). Documented as
  **accidental-tamper detection, not cryptographic integrity**. Forward-chained binding + a real
  key/MAC/SNARK receipt remain the upgrade path (tracked, not yet wired — `rzup` unavailable).
  RED tests prove the detection still fires (tamper fails the chain).

### Known limitations (documented, not papered over)
- Stateless kernel → replay/double-spend is only prevented within a single evolving-state lineage
  threaded by the caller; a caller that resets state re-feeds commands. Money idempotency is not yet
  tied to a persisted ledger. (RED-test falsifiable: the detection + gate behavior is proven; the
  persistence gap is a known TODO, not a silent claim.)
- `verifyJournal` is NOT a substitute for a signature/MAC; treat digests as tamper-*detection*, not
  tamper-*proof* against an adversary who controls storage.

---

## [0.2.0] — prior release
- Deterministic Rust/WASM guard kernel, PSQ node identity, living (VSA) memory, L5 telemetry governor,
  freestyle bebop soul self-loop. See git history for detail.
