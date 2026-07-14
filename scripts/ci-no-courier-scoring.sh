#!/usr/bin/env bash
# CI GUARD — NO-COURIER-SCORING (MESH-05).
# Build FAILS if any Rust struct field names a courier/agent score or
# reputation metric. We do not model, rank, or rate the mover.
set -euo pipefail
cd "$(dirname "$0")/.."
hit=0
# Whole-word score/rating/reputation/rank anywhere in a field-ish context,
# plus trust_score/trust_level (root-of-trust / trust_anchor are ALLOWED).
while IFS= read -r f; do
  # Only flag FIELD definitions (an ident followed by ':' and a type), never
  # comments or prose that merely mentions the word "score". Scoped to the mesh
  # protocol line (bebop2/): the physics field-engine crate/bebop is out of
  # scope and its own `score` metrics are legitimate (not courier/agent rating).
  if grep -nE '^\s*[A-Za-z_][A-Za-z0-9_]*\s*:\s' "$f" | grep -E '\b(score|rating|reputation|rank|trust_score|trust_level|courier_score|agent_rating)\b' >/dev/null; then
    echo "NO-COURIER-SCORING violation in $f:"; grep -nE '^\s*[A-Za-z_][A-Za-z0-9_]*\s*:\s' "$f" | grep -E '\b(score|rating|reputation|rank|trust_score|trust_level|courier_score|agent_rating)\b' | sed 's/^/  /'
    hit=1
  fi
done < <(grep -rlE '\bstruct\b' --include='*.rs' bebop2 2>/dev/null | grep -vE '^bebop2/core/' || true)
# Scope: the mesh/trust/protocol layer (proto-cap, proto-wire, delivery-domain, …) where a
# courier/agent/mover could be modeled. `bebop2/core/` is EXCLUDED — it is pure crypto + linear
# algebra (DMD/SVD/spectral) where `rank`/`score` are legitimate math terms, not mover ratings;
# scanning it would false-positive-block unrelated numerical work now that this is a HARD gate.
if [ "$hit" -eq 1 ]; then echo "FAIL: NO-COURIER-SCORING gate red"; exit 1; fi
echo "PASS: NO-COURIER-SCORING — no score/rating/reputation/rank fields."
