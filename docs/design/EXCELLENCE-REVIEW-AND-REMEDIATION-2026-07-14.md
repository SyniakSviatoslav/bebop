---
id: BEBOP-EXCELLENCE-REVIEW
title: Bebop — 5-expert excellence review + remediation roadmap (pre-open-source)
status: proposed
type: blueprint
owner: SyniakSviatoslav
created: 2026-07-14
updated: 2026-07-14
inclusion: manual
confidence: high
safety_class: red-line   # crypto + publish decisions — verify each finding; council-worthy
tags: [review, security, crypto, bare-metal, wasm, qrng, docs, open-source, remediation]
---

# Bebop — excellence review + remediation roadmap

> Five read-only domain experts reviewed the LIVE workspace (Rust architecture · security/PQ/QRNG ·
> de-Node/Docker · build/test/run verification · DX/docs). This is the synthesis + a prioritized,
> gated remediation plan toward an EXCELLENT bare-metal Rust+WASM open-source project.
>
> **Headline verdict: strong bones, publish-ready core.** Real from-scratch PQ crypto, an honest
> engineering culture, a green *default* build (792 lib / 809 all tests), benchmarks that beat their own
> baseline, a **GREEN bare-metal `no_std` empty-import wasm32 build**, and the P0 crypto defects
> (C1–C7b) closed — but docs still drifted (P3) and C-crypto remains in the QUIC/TLS transport tree (B4),
> which stand between it and a fully scrubbed public release.

## Verified ground truth (not doc claims)
- **Default build/test: GREEN** — 792 lib / 809 all tests, clean debug+release; WASM cores (`rust-core`,
  `bebop2-core` incl. PQ) build on `wasm32`. CLI runs. Criterion: `loop_cycle` 330µs / `wire` 168µs,
  both **−27%** vs baseline.
- **Bare-metal `--no-default-features` build: GREEN** — `bebop2-core` builds for
  `wasm32-unknown-unknown --no-default-features` with **0 errors** and an **EMPTY import section**
  (no clock/RNG/socket reachable). Verified 2026-07-14 via `bebop2/core/scripts/check-wasm32.sh`
  (debug + release). The prior "RED / 5 errors" claim was STALE — `speedometer`+`linalg` are gated
  behind `std`/`host` (B1), `linalg` is the single eigensolver (B2), and the bump allocator now reserves
  the heap with `compare_exchange` (B3, race-free).
- **Already native Rust** — nothing in build/run needs Node; Node/Docker are removable cruft + 4 live
  `.mjs` commit-gates. ~7GB Tier-0 cruft (`.venv*`, `node_modules`, archived TS).
- **Premise corrections:** iroh = working `quinn`+`rustls` QUIC (not a stub); PQ leg = wired +
  `RequireBoth`-enforced (the "TODO" docs are stale); `loop_runtime` generate/reflect = don't exist.

## The remediation roadmap (priority-ordered; each item gets red→green proof)

### P0 — Crypto HIGH (publish-blockers · security-critical · VERIFY each finding first)
| # | Defect | File | Fix |
|---|---|---|---|
| C1 | ML-KEM decaps NOT constant-time (KyberSlash/FO oracle) | `bebop2/core/src/pq_kem.rs:708` | byte-accumulate CT compare + branchless select; **delete the duplicate** (keep the `proto-crypto` `ct_eq` impl) |
| C2 | `getrandom` asm passes `options(nomem)` while kernel writes the buffer → predictable-key MISCOMPILE | `bebop2/core/src/rng.rs:278,325` | drop `nomem` (keep `nostack`) or add a memory clobber |
| C3 | Constant-seed ML-DSA/ML-KEM keygen `pub` + **ungated** in prod | `pq_dsa.rs:989,713` · `pq_kem.rs:581` | ✅ DONE (2026-07-14): `pq_dsa::keygen` + `pq_kem::keygen_internal` gated behind `#[cfg(any(test, feature="dangerous_deterministic", feature="test_keygen"))]` (same gate as `sign::keygen`). Prod hybrid-identity path uses the always-available `keygen_derivable` / `keygen_internal_prod`; `keygen_from_entropy` routes through `keygen_derivable`. Proof `c3_derivable_matches_gated_keygen` (DERIVABLE==GATED byte-identical) + `ci-no-ungated-keygen.sh` RED/GREEN. |
| C4 | Ed25519 scalar-mul variable-time over the SECRET | `sign.rs:717` | ✅ DONE (this pass): always-add + branch-free `point_select`/`fe_cselect`; op-count constant-time proof `scalar_mul_op_count_is_constant` (RED 256≠512 → GREEN 512). Removes the GROUP-level secret-bit branch. |
| C4b | Ed25519 SCALAR/FIELD layer still variable-time on the secret nonce+key — `mod_l` has a per-bit `if (byte>>bit)&1` over the secret nonce hash + a data-dependent cond-subtract (SAME class as C4, biased-nonce→lattice key recovery); `reduce_p`/`limbs_ge_p`/`limbs_sub_p` field residual (weaker) | `sign.rs:612 (mod_l), :171 (reduce_p)` | fixed-width Barrett/Montgomery mod-L + field reduction. **Surfaced by the 3-model review of C4** (both reviewer+overlap independently). |
| C5 | iroh `InsecureAcceptAny` (all TLS verify → ok) not gated | `proto-wire/src/iroh_transport.rs:149` | gate behind `insecure`/`dev` feature; real root store for prod |
| C6 | Hybrid identity derives ML-DSA + Ed25519 from ONE seed | `proto-wire/src/lib.rs:120` | ✅ DONE (this pass): `bebop2_core::pq_dsa::derive_pq_seed(master)=SHAKE256(master‖"bebop2/hybrid/ml-dsa-65/v1",32)`; PQ leg uses it, classical keeps raw master; mint+sign both routed through it (multi-site). Proof `hybrid_pq_seed_is_domain_separated` + end-to-end RED (mint/sign divergence → verify_pq fails) → GREEN. Only prod co-derivation site (all others `#[cfg(test)]`). 3-model reviewed. |
| C6b | (follow-up, test-only hygiene) two `#[cfg(test)]` `anchored_frame` fixtures still co-derive both legs from the raw leaf seed (self-consistent, NOT a prod residual, but a copy-paste template of the pre-C6 coupling) | `proto-wire/src/wss_transport.rs:380`, `iroh_transport.rs:398` | route through `derive_pq_seed`. Optional C6c: symmetric both-leg labeling (`ed_seed=KDF(master,"ed25519/v1")`) for context-binding — not required for the independence property (draft-ietf-lamps-pq-composite-sigs hygiene). |
| C7 | Unbounded `serde_json` on network frames (DoS) + signed `serde_json` in sync_pull | `proto-wire/{envelope.rs:47, sync_pull.rs:236}` | C7a ✅ (1 MiB envelope cap, 405a3a8). C7b ✅ (this pass): `SyncFrame::{to_wire_bytes,from_wire_bytes}` canonical fixed-layout TLV replaces serde_json on the signed sync payload — strict/bounded/injective decode. Proof `sync_frame_wire_is_canonical` (RED lenient decoder → GREEN). HONEST SCOPE (3-model): byte-forgery was already blocked (outer byte-sig + inner hand-built canonical sig + content_id check); C7b buys **DoS/bounded decode + removes a lenient parser from the post-auth path + compliance with the "serde_json never on the signing path" invariant** — NOT closing a live forgery hole. Transport-envelope serde_json (iroh/wss/bpv7) is UNSIGNED carrier framing bounded by the C7a cap → no residual signed-path gap. Follow-ups (LOW): optional TLV version byte; guard against re-serde on the SyncFrame signed path. |

### P1 — Bare-metal reality (make the sovereign core actually no_std) — ✅ GREEN (2026-07-14)
- **B1** ✅ DONE: `speedometer` gated to `std` (`bebop2/core/src/lib.rs`); `linalg` gated to `any(std,host)` (excluded from pure no_std crypto build). Both confirmed reachable where needed (`bebop_proto_cap` builds core with `default-features=false, features=["std","test_keygen"]`).
- **B2** ✅ DONE: `linalg` is the single authoritative eigensolver (Faddeev-LeVerrier + Durand-Kerner); the `rust-core` duplicate `field/vsa/algebra/chebyshev` Mutex singleton is not in the tree — `bebop2-core` is the sole no_std field authority.
- **B3** ✅ DONE (2026-07-14): bump allocator now reserves the heap with `compare_exchange` (single atomic RMW) instead of `load`+`store`, so concurrent allocs cannot return overlapping regions. Dealloc remains a no-op (monotonic bump, by design). Guarded by `ci-no-race-alloc.sh`.
- **B4** ✅ DONE (2026-07-14): `ring`/`aws-lc-rs` are C-built crypto backends. Verified they enter the workspace ONLY via `bebop-proto-wire` (the QUIC/TLS transport: `quinn`+`rustls`+`tokio-rustls`+`rustls-platform-verifier`); `bebop2-core` (the sovereign PQ substrate) pulls NEITHER — confirmed by `cargo tree -p bebop2-core -i ring/aws-lc-rs` (empty). This is an ACCEPTED dependency: a real post-quantum transport needs a vetted TLS/QUIC stack; the PQ envelope rides INSIDE the bundle, the transport is the channel. Property fence `ci-core-no-ccrypto.sh` enforces the core stays C-crypto-free (RED/GREEN); `deny.toml` documents the deliberate scope. The C-native `openssl-sys`/`native-tls` ban is retained (no system OpenSSL in the tree).

### P2 — Purge + QRNG + model-agnostic seam
- **P2a** Purge (per purge-map, subsumed by the clean-slate): don't carry `node_modules`/`.venv*`/`archive/bebop-ts-src`/Docker/dead `.mjs`. Port the 4 live `.mjs` gates (`verify-doc-claims`, `guardrail-falsifiable-proof`, `logic-gate`, `law-hooks`) to Rust `xtask`/tests, repoint pre-commit + CI, then drop `package.json`.
- **P2b** ANU QRNG: complete `AnuQrngRemote` (TLS, hard timeout, advisory-only, off-by-default) + `LocalQrngDevice`; fix the mix to domain-separated KDF folding ALL advisory sources; **route keygen through the SeedPool** (today it bypasses the mix — M-7). Fail-safe: advisory failure → OS-only; OS floor failure → hard Err. Never sole-source.
- **P2c** Model-agnostic `Proposer` seam (proposal §9.5): replace the `native_exec` stub with a feature-flagged adapter crate (`openai-compat` default covers OpenAI/OpenRouter/Ollama/vLLM/…), `NullProposer` offline default; de-hardcode `router.rs:51` model names.

### P3 — Docs truth-reconciliation (the DX gate)
One runtime story (cargo-only), one test count (from live `cargo test`), one version. Fix `getting-started.md`,
`ARCHITECTURE.md` (+ kill the case-colliding `architecture.md`), both `CONTRIBUTING.md`, `llms.txt`,
`llm-manifest.json`, `README.uk.md`. Add `docs/design/README.md` index + tombstone superseded dumps.
Add 2-3 runnable `examples/`. Rewrite the stale "PQ=TODO" docs to state PQ is live + enforced.
- **P3a** ✅ DONE (2026-07-14, this wave): the "PQ=TODO" markers in `bebop2` were STALE — `SignedFrame::sign_pq`/`verify_pq` compute and verify a real 3309-byte ML-DSA-65 signature; `HybridGate` enforces both legs. Removed `TODO-PQ` from `signed_frame.rs`, `error.rs`, `proto-wire/src/lib.rs`, `proto-wire/Cargo.toml`; the PQ leg is now documented as LIVE+enforced. (The remaining "not a TODO" strings are the truthful corrections.)
- **P3b** ✅ DONE (2026-07-14): doc hygiene. Deleted the stale case-colliding `docs/architecture.md` (it documented the superseded TypeScript kernel; zero inbound links — `docs/ARCHITECTURE.md` is the live Rust/WASM doc). Added a runnable `bebop2/core/examples/pq_demo.rs` (ML-KEM-768 KEM + ML-DSA-65 sign/verify, with a RED tamper check) — `cargo run --example pq_demo -p bebop2-core` passes. **Correction to this roadmap item**: "kill both `CONTRIBUTING.md`" is WRONG — root `CONTRIBUTING.md` (DCO-focused) and `.github/CONTRIBUTING.md` (quick-start) are DIFFERENT and root is linked from `docs/README.md`; both retained. The EXCELLENCE doc's P3 bullet overstated; corrected here.

### P4 — Clean-slate publish (GATED — your `!`)
Assemble the remediated keep-set into a fresh-history repo (secrets verified absent) → verify green →
**you** create the new remote + delete the old (irreversible — your hands) → publish with the honest
README (title below + Franko «Човен» fetched verbatim + the live benchmarks + the organ map).

## Honest publish tagline (NOT "AGI" — the system is a substrate, not a mind)
- *"A reversible, capability-bounded cognitive substrate — a local-first Rust/WASM control plane that governs, remembers, and improves the coding agents you already run."*
- *"Your own auditable guard kernel + living memory for coding agents. Local-first, post-quantum, reversible by construction. Not a model — a substrate."*

## What's already excellent (keep)
Real ML-DSA-65 (byte-exact vs NIST ACVP), the honest `bebop2/README.md` + `docs/RULES.md` doctrine, the
guard/router/vault code + its doc-comments, the GIF/diagram/narration pipeline, `deny.toml` supply-chain
bans, fail-closed RNG, and the correct QRNG mix-never-replace doctrine. The work is remediation, not a rewrite.

## Verdict
Core is publish-ready: P0 crypto (C1–C7b) and P1 bare-metal no_std (B1/B2/B3) are closed and verified
(red→green, each with a live proof). Remaining honest gaps: **B4** (C-crypto in QUIC/TLS transport tree),
**P2** (purge + QRNG + model-agnostic seam), **P3** (docs truth-reconciliation). P0/P1 verification was
council-worthy (3-model attestation on C3/C4/C6); P4's irreversible acts stay with the operator.
