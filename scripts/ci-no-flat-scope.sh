#!/usr/bin/env bash
# G4 guard (2026-07-14): scope/effect attenuation MUST be a real SET subset,
# not flat equality. The flat model used `Scope::new(Resource::X, Action::Y)`
# (2-arg) + `is_subset_of ==` equality, which made UCAN attenuation a no-op.
# This guard fails the build if any code re-introduces the 2-arg flat
# constructor for Scope/Effect (the set model uses `Scope::new(vec![...])`
# / `Scope::single(...)` / `Effect::single(...)`).
set -euo pipefail

REPO_ROOT="${GUARD_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
TARGETS=(bebop2/proto-cap bebop2/proto-wire)

PAT='(Scope|Effect)::new\([[:space:]]*(Resource|r|resource)[,:space:]'
# Matches the flat 2-arg constructor:  Scope::new(Resource::X, Action::Y)
FLAT_RE='(Scope|Effect)::new\([[:space:]]*Resource::'

found=0
for t in "${TARGETS[@]}"; do
  dir="$REPO_ROOT/$t"
  [ -d "$dir" ] || continue
  while IFS= read -r f; do
    while IFS= read -r line; do
      if [[ "$line" =~ $FLAT_RE ]]; then
        echo "G4 VIOLATION (flat scope/effect constructor): $f"
        echo "  $line"
        echo "  -> use Scope::single(..) / Effect::single(..) / Scope::new(vec![..]) (set model)"
        found=1
      fi
    done < <(grep -nE "$FLAT_RE" "$f" || true)
  done < <(grep -rlE "$FLAT_RE" "$dir" --include='*.rs' || true)
done

if [ "$found" -ne 0 ]; then
  echo "FAIL: G4 guard — flat scope/effect constructor reintroduced (attenuation would be a no-op)."
  exit 1
fi
echo "G4 guard: OK (scope/effect use the set model; attenuation is real subset)."
