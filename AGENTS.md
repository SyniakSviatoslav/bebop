# AGENTS.md — bebop2 operating rules (binding for every agent/lane)

> Greenfield from-scratch PQ crypto + deterministic core. Zero-dependency, `no_std + alloc`.
> These rules are standing orders; they override convenience and "it's probably fine".

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
- `cargo test` — 371 Rust tests (275 bebop + 19 bebop-core + 77 bebop2-core), RED+GREEN, 0 fail
- `cargo test -p bebop2-core` (full suite), `cargo clippy -p bebop2-core --all-targets`
- Crypto KATs live in `bebop2/core/src/kat/`; RFC 8439 §2.5.2 + Appendix A.3 are the Poly1305 anchors.
