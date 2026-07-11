#!/usr/bin/env bash
# Companion to scripts/three-model-review.sh — manages the review record that the pre-commit
# gate consumes. Implements the 3-model pipeline: BUILD → REVIEW → OVERLAP, no agent reviews itself.
#
# Stages:
#   prepare <builder-id>            create/reset .review/staged.json with the builder identity
#   attest  <role> <agent-id> <findings-file>
#                                    add an attestation (role = reviewer | overlap). <findings-file>
#                                    is a path to a text file with the agent's findings/verdict.
#   show                             print the current staged record (for debugging)
#   reset                            delete the staged record
#
# Identity rules (enforced by the gate, checked here too):
#   builder ≠ reviewer ≠ overlap — three distinct agents/models.
#
# NOTE: agent-id should be a concrete identity, e.g. "claude-opus", "hermes-tencent-hy3",
# "opencode", "codex". Putting the same id in two roles fails the gate.

set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
REVIEW_DIR="$ROOT/.review"
STAGE_FILE="$REVIEW_DIR/staged.json"
mkdir -p "$REVIEW_DIR"

cmd="${1:-}"
case "$cmd" in
  prepare)
    [ $# -ge 2 ] || { echo "usage: three-model-review.sh prepare <builder-id>"; exit 1; }
    BUILDER="$2"
    cat > "$STAGE_FILE" <<JSON
{
  "builder": "$BUILDER",
  "reviewer": null,
  "overlap": null
}
JSON
    echo "◆ review record prepared for builder='$BUILDER' (roles reviewer/overlap still required)"
    ;;
  attest)
    [ $# -ge 4 ] || { echo "usage: three-model-review.sh attest <reviewer|overlap> <agent-id> <findings-file>"; exit 1; }
    ROLE="$2"; AGENT="$3"; FINDINGS="$4"
    [ "$ROLE" = "reviewer" ] || [ "$ROLE" = "overlap" ] || { echo "role must be 'reviewer' or 'overlap'"; exit 1; }
    [ -f "$FINDINGS" ] || { echo "findings file not found: $FINDINGS"; exit 1; }
    [ -f "$STAGE_FILE" ] || { echo "no staged record — run 'prepare <builder-id>' first"; exit 1; }
    # Reject self-attestation up front (builder === agent).
    BUILDER="$(node -e "process.stdout.write(require('$STAGE_FILE').builder || '')" 2>/dev/null || true)"
    [ "$AGENT" != "$BUILDER" ] || { echo "✗ self-attestation refused: agent '$AGENT' is the builder"; exit 1; }
    # Reject duplicate role (reviewer must differ from overlap).
    if [ "$ROLE" = "reviewer" ]; then
      OTHER="$(node -e "process.stdout.write(require('$STAGE_FILE').overlap?.agent || '')" 2>/dev/null || true)"
    else
      OTHER="$(node -e "process.stdout.write(require('$STAGE_FILE').reviewer?.agent || '')" 2>/dev/null || true)"
    fi
    [ -z "$OTHER" ] || [ "$AGENT" != "$OTHER" ] || { echo "✗ '$ROLE' agent '$AGENT' equals the other reviewer agent — need 3 DISTINCT agents"; exit 1; }
    node -e "
      const fs=require('fs');
      const r=require('$STAGE_FILE');
      r['$ROLE']={ agent:'$AGENT', findings:fs.readFileSync('$FINDINGS','utf8'), at:new Date().toISOString() };
      fs.writeFileSync('$STAGE_FILE', JSON.stringify(r,null,2));
    "
    echo "✓ attested role='$ROLE' agent='$AGENT'"
    ;;
  show)
    [ -f "$STAGE_FILE" ] && cat "$STAGE_FILE" || echo "(no staged record)"
    ;;
  reset)
    rm -f "$STAGE_FILE"
    echo "◆ staged record removed"
    ;;
  *)
    echo "usage: three-model-review.sh <prepare|attest|show|reset> ..."
    exit 1
    ;;
esac
