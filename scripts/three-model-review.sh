#!/usr/bin/env bash
# Pre-commit guardrail — THREE-MODEL PEER REVIEW (operator standing rule 2026-07-11).
#
# PRINCIPLE (3-model threelaterition / "threelaterition"): no agent may review or score its OWN
# work. The build path is a 3-stage pipeline that must never collapse into 1 agent:
#   1. BUILD   — the implementer (any coding agent: Hermes / Claude / OpenCode / Codex / …) writes
#                and verifies the code (tests + build green) but NEVER self-certifies correctness.
#   2. REVIEW  — a SECOND, independent agent reviews the diff for correctness/security (this is the
#                "Claude / Fable reviewer"). It must not be the same model/agent that built it.
#   3. OVERLAP — a THIRD agent (different from #1 and #2) cross-checks the reviewer's conclusions
#                against the spec/docs, catching blind spots where #2 and #1 might share a wrong
#                assumption (the §A.3.1 Poly1305 hibit bug was exactly such a shared blind spot).
#
# WHY THIS EXISTS: a single agent building and reviewing its own work produces false-greens
# (the §A.3.1 tag was "green" on a roundtrip test that shared the same broken path both ways).
# Independence of the reviewer is the only reliable antidote.
#
# HOW IT IS ENFORCED (falsifiable, not advisory):
#   The implementer drops a signed review record into .review/ at the path derived from the
#   commit being created. The hook refuses the commit unless BOTH a `reviewer` and an `overlap`
#   attestation exist, each listing a DIFFERENT model/agent identity than the builder and from
#   each other, with a non-empty finding summary. This makes "abandon the review" impossible
#   without deliberately forging the record (which the attestation format makes obvious).
#
# USAGE (builder):
#   after `cargo test` / `pnpm test` is green, run:
#     bash scripts/three-model-review.sh prepare <builder-id>
#   then have the 2nd and 3rd agents run:
#     bash scripts/three-model-review.sh attest  <role> <agent-id> <findings-file>
#     (role = reviewer | overlap)
#   The hook reads the prepared record on commit and checks the two attestations.
#
# CI / non-interactive fallback: if CI_THREE_MODEL_REVIEW=allow is set (e.g. trusted CI with its
# own review job), the hook passes through. This is a red-line gate and should normally be OFF.

set -euo pipefail

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"

REVIEW_DIR="$ROOT/.review"
STAGE_FILE="$REVIEW_DIR/staged.json"

# ── Allow-list: trusted CI that runs its own 3-model review job ───────────────────────────────
if [ "${CI_THREE_MODEL_REVIEW:-}" = "allow" ]; then
  echo "◆ 3-model review: bypassed (CI_THREE_MODEL_REVIEW=allow — CI owns the review gate)"
  exit 0
fi

# ── No staged changes → nothing to review ───────────────────────────────────────────────────
if git diff --cached --quiet; then
  echo "◆ 3-model review: no staged changes, nothing to attest"
  exit 0
fi

if [ ! -f "$STAGE_FILE" ]; then
  echo "✗ THREE-MODEL REVIEW GATE FAILED"
  echo "  No review record found. The builder must NOT self-certify."
  echo "  Run:  bash scripts/three-model-review.sh prepare <builder-id>"
  echo "  then have an INDEPENDENT reviewer + overlap agent attest (see script for steps)."
  exit 1
fi

BUILDER="$(node -e "process.stdout.write(require('$STAGE_FILE').builder || '')" 2>/dev/null || true)"
REVIEWER="$(node -e "process.stdout.write(require('$STAGE_FILE').reviewer?.agent || '')" 2>/dev/null || true)"
OVERLAP="$(node -e "process.stdout.write(require('$STAGE_FILE').overlap?.agent || '')" 2>/dev/null || true)"

fail() {
  echo "✗ THREE-MODEL REVIEW GATE FAILED"
  echo "  $1"
  echo "  Builder='$BUILDER' Reviewer='$REVIEWER' Overlap='$OVERLAP'"
  echo "  Re-run: bash scripts/three-model-review.sh prepare $BUILDER"
  exit 1
}

[ -n "$REVIEWER" ] || fail "missing INDEPENDENT reviewer attestation (role=reviewer)."
[ -n "$OVERLAP" ]  || fail "missing INDEPENDENT overlap attestation (role=overlap)."
[ "$REVIEWER" != "$BUILDER" ]  || fail "reviewer must be a DIFFERENT agent than the builder (no self-review)."
[ "$OVERLAP"  != "$BUILDER" ]  || fail "overlap checker must be a DIFFERENT agent than the builder."
[ "$OVERLAP"  != "$REVIEWER" ] || fail "overlap checker must be a DIFFERENT agent than the reviewer (3 distinct models)."

# Each attestation must carry a non-empty findings summary (a rubber-stamp empty review is a false-green).
node -e "const r=require('$STAGE_FILE'); const bad=['reviewer','overlap'].filter(k=>!r[k]||!r[k].findings||!r[k].findings.trim()); if(bad.length) process.exit(1);" \
  || fail "reviewer/overlap attestation(s) have empty findings — a review that asserts nothing proves nothing."

echo "✓ 3-model review gate satisfied (builder='$BUILDER' ≠ reviewer='$REVIEWER' ≠ overlap='$OVERLAP')"
# Consume the record so it cannot be reused for an unrelated commit.
rm -f "$STAGE_FILE"
exit 0
