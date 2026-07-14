#!/usr/bin/env bash
# RED+GREEN regression for the G4 guard.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
GUARD="$REPO_ROOT/scripts/ci-no-flat-scope.sh"

echo "== GREEN: clean tree must pass the G4 guard =="
bash "$GUARD" >/dev/null 2>&1 || { echo "GREEN FAILED"; exit 1; }
echo "GREEN OK"

echo "== RED: a flat Scope::new(Resource::.., Action::..) must trip the guard =="
TMP="$(mktemp -d)"
mkdir -p "$TMP/bebop2/proto-cap/src"
printf 'fn bad() { let _ = bebop_proto_cap::Scope::new(Resource::Route, Action::Send); }\n' > "$TMP/bebop2/proto-cap/src/x.rs"
if GUARD_ROOT="$TMP" bash "$GUARD" >/dev/null 2>&1; then
  echo "RED FAILED: guard did not catch flat constructor"
  rm -rf "$TMP"
  exit 1
fi
echo "RED OK (guard tripped)"
rm -rf "$TMP"
echo "G4 regression: PASS (RED+GREEN)"
