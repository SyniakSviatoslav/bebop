# AGENTS.md — bebop2 operating rules (binding for every agent/lane)

> Greenfield from-scratch PQ crypto + deterministic core. Zero-dependency, `no_std + alloc`.
> These rules are standing orders; they override convenience and "it's probably fine".

## 0. Global default workflow (multipilot-native, ON by default)
For ANY build task or loop, the default operating mode is a 3-phase pipeline:
1. **FABLE RESEARCH FIRST** — run a claude-fable deep-research pass to produce a *plan blueprint*
   (exact params, algorithm skeleton, function signatures, falsifiable-KAT strategy) BEFORE coding.
2. **3-MODEL DO** — the doer agent/model executes the build. The doer NEVER reviews its own work.
3. **3-MODEL REVIEW/AUDIT (post)** — after finishing, run ANOTHER 3-model review (independent
   reviewer + independent overlap) of the completed task. This is the multipilot native approach.
   **Invariant: doer ≠ reviewer ≠ overlap (no agent/model reviews its own output).**
Override only on an explicit per-task operator instruction.

## 1. Three-model peer review (NEVER self-review) — "threelaterition"
No agent may build AND certify its own work. Every non-trivial change goes through a 3-stage
pipeline; the gate enforces it on commit (`.git/hooks/pre-commit` → `scripts/three-model-review.sh`):

1. **BUILD**   — implementer writes + verifies the code (tests + build green). Does NOT self-certify.
2. **REVIEW**  — a SECOND, independent agent reviews the diff for correctness/security.
3. **OVERLAP** — a THIRD agent (≠ #1, ≠ #2) cross-checks the reviewer against spec/docs, catching
                 shared blind spots where builder & reviewer both assume the same wrong thing.

The §A.3.1 Poly1305 tag was "green" on a roundtrip test that reused the same broken path both ways —
independence of the reviewer is the only reliable antidote. The commit is refused unless BOTH a
distinct `reviewer` and a distinct `overlap` attestation exist, each with a non-empty findings
summary. Builder = reviewer, or reviewer = overlap, fails the gate.

Workflow (builder):
```
bash scripts/three-model-review.sh prepare <builder-id>
# independent reviewer agent:
bash scripts/three-model-review.sh attest reviewer <reviewer-agent> <findings.md>
# independent overlap agent:
bash scripts/three-model-review.sh attest overlap  <overlap-agent>  <findings.md>
```
CI may set `CI_THREE_MODEL_REVIEW=allow` only if it runs its own equivalent review job.

## 2. Verified-by-Math (VbM) — only falsifiable proof validates
A change is validated only if: (1) it works (exercised against reality), (2) it is proven with math
(a deterministic assertion/count with a defined threshold), and (3) the proof is **falsifiable** —
there exists an input under which it goes RED. Ship the RED case alongside the green. A test that
cannot fail is a false-positive metric and does NOT validate. Enforced by
`scripts/guardrail-falsifiable-proof.mjs` (pre-commit) and `scripts/verify-doc-claims.mjs`.

## 3. Red-line areas need per-change confirmation
auth / money / RLS / migrations / bulk-edit / crypto-constant changes are red-line. Don't silently
ship them; flag for human confirmation.

## 4. Trust the failing test over the narrative
When a test is red, the bug is real even if the code "looks right". Investigate with an independent
oracle (e.g. a Python reference implementation) before concluding the test is wrong.

## Build/test
- `cargo test` — 416 Rust tests, RED+GREEN, 0 fail
- `cargo test -p bebop2-core` (full suite), `cargo clippy -p bebop2-core --all-targets`
- Crypto KATs live in `bebop2/core/src/kat/`; RFC 8439 §2.5.2 + Appendix A.3 are the Poly1305 anchors.

---

## Operating rules — memory-first + push-plans-first (operator, 2026-07-11)

1. **Update living memory FIRST.** Before writing/planning any code, record new changes, plans,
   decisions, and ground-truth facts to the canonical corpus. Source of truth = the corpus, not chat.
   - bebop/bebop2 (protocol) → `/root/.claude/projects/-root-bebop-repo/` corpus.
   - dowiz (product) → `/root/.claude/projects/-root-dowiz/memory/MEMORY.md`.
2. **Push plans to remote FIRST.** Plans/roadmaps/decision docs are committed + pushed to `origin`
   before execution — so they can never be lost to a crashed session or stale context.
3. **Ground truth outranks plans.** Re-verify code claims (`grep`/`git`/`cargo test`) before trusting a
   pasted "verified" status. Plan = desired state; live repo = what IS. Keep DONE (verified) vs PLANNED
   separate. bebop is PARKED as a protocol until dowiz carries it (cold-start depends on a working product).
4. **Structure before code:** PARALLEL-SAFE (independent files, zero-pivot, non-red-line → own branch)
   vs SEQUENTIAL GATES (red-line, external validation, tier deps). Shared Tier spine with dowiz.
