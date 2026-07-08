#!/usr/bin/env bash
# Records ONE real bebop command to a short GIF for the wiki.
# Usage: scripts/record-feature.sh <name> "<bebop args>"
#   name  -> output docs/footage/feat-<name>.gif  (+ .cast)
#   args  -> the command after `bebop`, e.g. "govern 0.9,0.6,0.2,0.95"
#
# Constant Doubt: this runs the REAL bebop binary. A temporary, untrusted, model-only
# bebop.json is dropped into the repo root ONLY during the recording and removed after
# (trap), so it never pollutes git and never changes behavior (model-only is inert).
#
# Requires: asciinema (https://asciinema.org) + agg (https://github.com/asciinema/agg).
# Install once, anywhere:  pip install asciinema   &&   (download agg to PATH, see docs/footage/README.md)
#
set -u
NAME="${1:?usage: record-feature.sh <name> \"<bebop args>\"}"; shift
ARGS="$*"
REPO="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$REPO/docs/footage/feat-$NAME"

# Resolve asciinema: PATH first, then a few common venv locations (portable, no committed symlink).
if command -v asciinema >/dev/null 2>&1; then ASCIINEMA=asciinema
elif [ -x "$HOME/bebop-venv/bin/asciinema" ]; then ASCIINEMA="$HOME/bebop-venv/bin/asciinema"
elif [ -x /tmp/bebop-venv/bin/asciinema ]; then ASCIINEMA=/tmp/bebop-venv/bin/asciinema
else echo "asciinema not found on PATH or in a known venv; install: pip install asciinema" >&2; exit 1; fi

# Resolve agg (cast->gif). Prefer PATH, then a couple of common drop locations.
if command -v agg >/dev/null 2>&1; then AGG=agg
elif [ -x /tmp/agg ]; then AGG=/tmp/agg
else echo "agg not found on PATH or /tmp/agg; see docs/footage/README.md" >&2; exit 1; fi

mkdir -p "$REPO/docs/footage"

cleanup() { rm -f "$REPO/bebop.json" "$INNER"; }
trap cleanup EXIT
printf '{\n  "model": "anthropic/claude-3.5-haiku"\n}\n' > "$REPO/bebop.json"

INNER=$(mktemp /tmp/rec-inner.XXXXXX.sh)
cat > "$INNER" <<EOF
#!/usr/bin/env bash
export NO_ANIM=1
cd "$REPO"
echo "### bebop $ARGS"
sleep 0.6
# hard cap so servers (e.g. mcp) don't hang the recording forever
timeout 12 npx tsx bebop.ts $ARGS 2>&1 | head -22
sleep 1.0
EOF
chmod +x "$INNER"

"$ASCIINEMA" rec -t "bebop $ARGS" -c "bash $INNER" "$OUT.cast" --overwrite >/dev/null 2>&1
"$AGG" --theme nord --speed 1.25 --cols 96 --rows 18 "$OUT.cast" "$OUT.gif" >/dev/null 2>&1
printf 'wrote %s.gif (%s bytes)\n' "$NAME" "$(stat -c%s "$OUT.gif" 2>/dev/null)"
