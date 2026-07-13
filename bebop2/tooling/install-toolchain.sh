#!/usr/bin/env bash
# DK-01 — WASM-component toolchain installer.
#
# Ensures the two prerequisites for building Port-as-WASM-component artifacts
# (DK-02) are present:
#   1. The `wasm32-wasip2` rustup target (component build target).
#   2. `cargo-component` (the `wasm32-wasip2` component adapter linker).
#
# Network IS available. We prefer the prebuilt release binary for
# `cargo-component` (fast), and fall back to `cargo install cargo-component`
# only if the download fails. `wasmtime` is intentionally NOT installed here:
# the host mapping lives in `bebop2/wasm-host` and is feature-gated
# (`feature="wasm"`) so the DEFAULT `cargo test --workspace` stays offline-clean.
#
# This script is idempotent: running it twice is a no-op if everything is present.
set -euo pipefail

# Resolve the directory this script lives in, so it can be invoked from anywhere.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

log() { printf '\033[36m[install-toolchain]\033[0m %s\n' "$*"; }
die() { printf '\033[31m[install-toolchain][FATAL]\033[0m %s\n' "$*" >&2; exit 1; }

# ── 1. rustup target ───────────────────────────────────────────────────────────
WASI_TARGET="wasm32-wasip2"
if rustup target list --installed 2>/dev/null | grep -qx "${WASI_TARGET}"; then
    log "target ${WASI_TARGET} already installed"
else
    log "installing rustup target ${WASI_TARGET} ..."
    rustup target add "${WASI_TARGET}" || die "rustup target add ${WASI_TARGET} failed"
fi

# ── 2. cargo-component ─────────────────────────────────────────────────────────
if command -v cargo-component >/dev/null 2>&1; then
    log "cargo-component already on PATH: $(command -v cargo-component)"
    log "version: $(cargo-component --version 2>&1 || true)"
else
    log "cargo-component not found; installing prebuilt release binary ..."
    # Prefer the latest release that publishes a linux-gnu asset. v0.20.0 is the
    # most recent published prebuilt tarball at time of writing; fall back to
    # `cargo install` if it disappears.
    CC_VERSION="${CARGO_COMPONENT_VERSION:-0.20.0}"
    CC_URL="https://github.com/bytecodealliance/cargo-component/releases/download/v${CC_VERSION}/cargo-component-x86_64-unknown-linux-gnu"
    DEST="${CARGO_INSTALL_ROOT:-/usr/local/bin}/cargo-component"
    if curl -fsSL "${CC_URL}" -o "${DEST}" 2>/dev/null && [ -s "${DEST}" ]; then
        chmod +x "${DEST}"
        log "installed cargo-component ${CC_VERSION} -> ${DEST}"
    else
        log "prebuilt binary download failed; falling back to 'cargo install cargo-component' (slower) ..."
        cargo install cargo-component --version "=${CC_VERSION}" || die "cargo install cargo-component failed"
    fi
    command -v cargo-component >/dev/null 2>&1 || die "cargo-component still not on PATH after install"
    log "version: $(cargo-component --version 2>&1 || true)"
fi

log "toolchain ready. Build a port component with: tooling/build-wasm-component.sh <crate-dir>"
# Signal success to callers (e.g. CI) without printing to stderr.
exit 0
