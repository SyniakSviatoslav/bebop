# bebop2 Adversarial Fable-Style Review

- Audit ID: `proc_078f78f7cf76` (claude -p, fable-style adversarial review+audit)
- Target: `/root/bebop-repo/bebop2/` (from-scratch, zero-dep, post-quantum Rust core)
- Pillars: ARCHITECTURE.md (4 pillars: spectral-first, no dense, AGC envelope, better-math)
- Date: 2026-07-10
- Verdict: CONDITIONAL FAIL. Several findings STALE (ran against pre-fix state — pq_kem NTT
  removed, chebyshev fexp fixed this session). One LIVE crypto KAT bug fixed post-audit (M3).
  FABLE_REVIEW_EXIT=0.

## Condensed findings (auditor severities preserved)

| # | Sev | Finding |
|---|-----|---------|
| H1 | HIGH | `cargo build --target wasm32-unknown-unknown` fails with ~82 errors (missing `alloc::Vec` imports, no `#[global_allocator]`/`#[panic_handler]`, `std`-only float methods, gratuitous `std::mem::swap`). "Empty-import wasm gate" never achieved. CORROBORATES V1/V2/V3 #1. OPEN. |
| H2 | HIGH | `pq_kem.rs::bitrev7` computed 8-bit reversal not FIPS 203 7-bit `BitRev7` — corrupted NTT twiddles. **STALE/RESOLVED**: NTT + bitrev7 entirely removed in the coefficient-domain schoolbook rewrite; pq_kem now 54/54 green. |
| H3 | HIGH | `field.rs` builds dense O(n²) Laplacian + O(n³) Jacobi — forbidden by pillars 1 & 4 (mandate Lanczos/Arnoldi); docstring claims dense adjacency "never formed." CORROBORATES V2 F5. OPEN. |
| H4 | HIGH | `kalman.rs::SpectralKalman` is dense matrix math wearing a "spectral" label; no square-root/Potter-Carlson; Q-transform only correct for symmetric A (masked by tests). CORROBORATES V2 F4. OPEN. |
| H5 | HIGH | `lyapunov.rs` Jacobi eigensolver only handles symmetric matrices, hardcodes Im(λ)=0, accepts arbitrary matrices unchecked; all 5 tests use diagonal matrices → rotation logic never executed. CORROBORATES V2 F4. OPEN. |
| M1 | MED | B11 "dt corridor" only guards `dt<=0`, not oversized positive dt; test passes for wrong reason. CORROBORATES V2 F7. OPEN. |
| M2 | MED | `chebyshev.rs` reimplemented range-reduced exp with asymmetric negative-rounding bug (same family as "fixed" C8). **STALE/RESOLVED**: this session routed chebyshev::fexp → crate::fexp (C8-correct) + symmetric fround; RED tests added; 54/54 green. |
| M3 | MED | Committed SHA-512 KAT empty-string vector is the SHA-256 digest (64 hex / 32 B), not SHA-512 (128 hex / 64 B). **RESOLVED post-audit**: replaced with correct `cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e`. (abc / abcdbc.. vectors were already correct.) |
| M4 | MED | `vsa.rs` hasn't done its mandated Fourier-native upgrade; no justification on file. CORROBORATES V2 F6. OPEN. |
| L1-L3 | LOW | Stale architecture prose vs code; dead duplicate `fexp` in `fft.rs` (now unused copy of correct fexp — harmless); minor O(E·deg) dedup in `field.rs`. |
| — | info | `aead/hash/kdf/pq_dsa/rng/sign.rs` still 2-line stubs (expected). `kernel/`,`cli/`,`reloop/` don't exist; nothing currently gates the wasm build (why H1 went unnoticed). |

## Note
Auditor observed live concurrent edits: "3 bugs I caught via failing tests (field.rs eigenvector
transpose, pq_kem.rs double bit-reversal, vsa.rs self-contradicting test) were fixed by someone
else before I finished." Not double-counted. The report's write-subagent could not persist to
docs/design in the auditor's environment; re-saved here by the orchestrator session.

## Resolution status (2026-07-10)
- H2 — RESOLVED (NTT removed).
- M2 — RESOLVED (chebyshev fexp delegated to C8-correct crate::fexp).
- M3 — RESOLVED (SHA-512 empty vector corrected).
- H1, H3, H4, H5, M1, M4, L1-L3 — OPEN (design-level; need follow-up: wasm gate, square-root
  Kalman + Lanczos eigensolver, vsa scratch-length, B11/C2 label gaps).
