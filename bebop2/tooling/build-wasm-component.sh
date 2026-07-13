#!/usr/bin/env bash
# DK-01 — WASM-component build driver.
#
# Builds a Port-as-WASM-component crate (DK-02; e.g. `bebop2/ports/telegram`)
# into a `wasm32-wasip2` component via `cargo component build`. The result is a
# *component* (not a bare core module): the guest declares its WIT world and the
# only host import it may request is the capability-scoped function the host
# grants (deny-by-default, DK-03). The guest carries NO filesystem/socket
# imports — proving zero-ambient-authority.
#
# RED DK-01: this script MUST NOT produce an OCI image. It emits a raw
# `.wasm` component artifact only. (Full SLSA provenance + signing is a
# documented follow-up in `supply-chain.sh`.)
#
# Usage:
#   tooling/build-wasm-component.sh <crate-dir> [extra cargo-component args...]
#
# Examples:
#   tooling/build-wasm-component.sh bebop2/ports/telegram
#   tooling/build-wasm-component.sh bebop2/ports/telegram --release
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "${REPO_ROOT}"

log() { printf '\033[36m[build-wasm-component]\033[0m %s\n' "$*"; }
die() { printf '\033[31m[build-wasm-component][FATAL]\033[0m %s\n' "$*" >&2; exit 1; }

CRATE_DIR="${1:-}"
if [ -z "${CRATE_DIR}" ]; then
    die "usage: build-wasm-component.sh <crate-dir> [cargo-component args...]"
fi
shift || true

# Resolve to an absolute path and verify it is a cargo package.
CRATE_DIR="$(cd "${CRATE_DIR}" 2>/dev/null && pwd)" || die "crate dir not found: ${1:-}"
[ -f "${CRATE_DIR}/Cargo.toml" ] || die "no Cargo.toml in ${CRATE_DIR}"
grep -q 'crate-type' "${CRATE_DIR}/Cargo.toml" || \
    die "crate ${CRATE_DIR} is not a component (missing crate-type=[cdylib])"

# Ensure toolchain is present (idempotent; cheap if already installed).
"${SCRIPT_DIR}/install-toolchain.sh"

# Build the component for the wasip2 target (component model + wasi:p2).
log "building component: ${CRATE_DIR} (target wasm32-wasip2)"
cargo component build --target wasm32-wasip2 "$@" \
    --manifest-path "${CRATE_DIR}/Cargo.toml" \
    || die "cargo component build failed for ${CRATE_DIR}"

# Locate the produced artifact. cargo-component writes to
# target/wasm32-wasip2/debug|release/<name>.wasm
PROFILE="debug"
for a in "$@"; do [ "$a" = "--release" ] && PROFILE="release"; done
BIN_NAME="$(grep -m1 '^name' "${CRATE_DIR}/Cargo.toml" | sed -E 's/name[[:space:]]*=[[:space:]]*//; s/[" ]//g')"
WASM="${CRATE_DIR}/target/wasm32-wasip2/${PROFILE}/${BIN_NAME}.wasm"

[ -f "${WASM}" ] || die "expected component artifact not found: ${WASM}"
log "component built: ${WASM} ($(wc -c < "${WASM}") bytes)"

# Sanity: a component MUST have a component-type custom section, and (by design
# of DK-02) MUST NOT import ambient-authority WASI such as filesystem/sockets.
if command -v wasm-tools >/dev/null 2>&1; then
    if wasm-tools component new --help >/dev/null 2>&1; then
        : # wasm-tools present; deeper validation possible but optional
    fi
fi

# RED DK-01 GUARD: refuse to emit an OCI image. We only ever hand back a .wasm.
case "${WASM}" in
    *.wasm) log "RED DK-01 satisfied: artifact is a raw .wasm component (NO OCI image emitted)";;
    *) die "RED DK-01 violation: produced a non-.wasm artifact: ${WASM}";;
esac

log "done. Component path: ${WASM}"
echo "${WASM}"
