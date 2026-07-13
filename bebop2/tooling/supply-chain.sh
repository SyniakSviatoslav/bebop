#!/usr/bin/env bash
# DK-01 — supply-chain provenance + signing (documented stub).
#
# Produces an SBOM and a keyless signature for a built WASM-component artifact.
# Both steps are OPTIONAL / best-effort: if the underlying CLIs (`syft`,
# `cosign`) are not installed, the script documents the intended command and
# exits 0 (so local/dev builds are not blocked). CI can tighten this to
# hard-fail.
#
# innovate: marker — FULL SLSA provenance (a `predicateType` build provenance
# attestation, e.g. via `slsa-github-generator` or `cosign attest --type
# slsa`) is a documented FOLLOW-UP and intentionally NOT implemented here. This
# stub establishes the hook point so the supply-chain wiring is mechanical to
# add later without restructuring DK-02 / DK-03.
#
# RED DK-01: this script NEVER wraps the artifact in an OCI image. It signs the
# raw `.wasm` (cosign supports raw blobs). The component stays a bare `.wasm`.
#
# Usage:
#   tooling/supply-chain.sh <artifact.wasm>
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

log() { printf '\033[36m[supply-chain]\033[0m %s\n' "$*"; }
warn() { printf '\033[33m[supply-chain][SKIP]\033[0m %s\n' "$*" >&2; }

WASM="${1:-}"
if [ -z "${WASM}" ]; then
    echo "usage: supply-chain.sh <artifact.wasm>" >&2
    exit 1
fi
[ -f "${WASM}" ] || { echo "artifact not found: ${WASM}" >&2; exit 1; }

# ── 1. SBOM via syft (optional) ────────────────────────────────────────────────
if command -v syft >/dev/null 2>&1; then
    SBOM="${WASM}.sbom.json"
    log "generating SBOM: ${SBOM}"
    syft "file:${WASM}" -o spdx-json > "${SBOM}" || warn "syft failed; skipping SBOM"
else
    warn "syft not installed — SBOM step documented only."
    log "intended: syft file:${WASM} -o spdx-json > ${WASM}.sbom.json"
fi

# ── 2. Keyless signature via cosign (optional) ────────────────────────────────
if command -v cosign >/dev/null 2>&1; then
    log "keyless-signing ${WASM} (cosign, OIDC ambient creds)"
    cosign sign-blob --yes "${WASM}" \
        --output-signature "${WASM}.sig" \
        --output-certificate "${WASM}.pem" \
        || warn "cosign sign-blob failed (no OIDC identity?); skipping signature"
else
    warn "cosign not installed — keyless signature step documented only."
    log "intended: cosign sign-blob --yes ${WASM} \\"
    log "            --output-signature ${WASM}.sig \\"
    log "            --output-certificate ${WASM}.pem"
fi

# innovate: FULL SLSA provenance attestation is a follow-up. Hook point:
#   cosign attest --yes --type slsa --predicate provenance.json ${WASM}
# once a build-provenance generator is wired in CI.
warn "SLSA provenance attestation is a documented FOLLOW-UP (not implemented)."

log "supply-chain processing complete for ${WASM} (raw .wasm, NO OCI image)."
exit 0
