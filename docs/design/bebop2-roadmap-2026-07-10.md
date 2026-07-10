# bebop2 — Planned Features & Goal Anchor (2026-07-10)

> Purpose: preserve the main goal and the roadmap so the build cannot silently drift.
> Update this file whenever a milestone closes or a scope change is agreed.

## Unchanging main goal
Build `bebop2` — a from-scratch, **zero-dependency**, post-quantum Rust core for the bebop agent:
- ML-KEM-768 (FIPS 203) key encapsulation — **DONE & GREEN** (coefficient-domain schoolbook).
- ML-DSA-65 (FIPS 204) signatures — **STILL STUB** (pq_dsa.rs, 2 lines).
- Classical hybrid: XChaCha20-Poly1305 AEAD, Argon2id KDF, SHA-512/SHA3 hash, Ed25519 sign,
  in-tree CSPRNG — **ALL STILL STUBS** (aead/kdf/hash/sign/rng.rs, 2 lines each).
- Math/spectral kernel (field Laplacian spectrum, VSA, kalman, lyapunov, chebyshev, fft, active)
  — **DONE & GREEN** (54 lib tests), pending architecture hardening (see open items).

## Hard constraints (do NOT violate)
1. **Zero external crates.** Everything from scratch (Keccak, SHA-512, SHA3, XChaCha, Argon2id,
   Ed25519, ML-KEM, ML-DSA). No `getrandom`, no `rand`, no crypto deps.
2. **wasm32 / no_std / empty-import gate.** Final target must compile `--target wasm32-unknown-unknown`
   with `#[no_std]` + `#[panic_handler]` + `#[global_allocator]` and an EMPTY import section.
   Currently FAILS (~82–90 errors) — see OPEN below.
3. **Verified-by-Math.** Every fix ships a falsifiable RED+GREEN test. No false-green metrics.
4. Feature branch only; never push to `main`.

## Closed milestones (this session)
- pq_kem NTT proven broken by 3 independent audits → pivoted to coefficient-domain schoolbook
  `poly_mul` over R_q = Z_q[x]/(x^256+1). 54/54 lib tests pass.
- F3 (chebyshev.rs fexp asymmetric + lib.rs `1u64<<k` shift overflow) fixed; fexp x<0 + symmetry RED tests added.
- M3 (SHA-512 KAT empty vector was SHA-256 digest) corrected in kat/vectors.rs.
- V1/V2/V3/fable adversarial audit reports saved under docs/design/.

## OPEN — architecture hardening (NOT blocking current 54-green, but required before "empty-import wasm" claim)
- **H1** wasm32 ~82–90 compile errors: missing `alloc::Vec` imports, no `#[global_allocator]`/
  `#[panic_handler]`, `std`-only f64 trig (`sin/cos/sqrt/ln`), `std::mem::swap`. → add `no_std` cfgs,
  f64 trig shims, allocator.
- **H3** field.rs builds dense O(n²) Laplacian + O(n³) Jacobi — pillars 1&4 mandate Lanczos/Arnoldi.
- **H4** kalman.rs::SpectralKalman is dense matrix math in a "spectral" label; needs square-root /
  Potter-Carlson form; Q-transform only correct for symmetric A (masked by tests).
- **H5** lyapunov.rs Jacobi eigensolver symmetric-only, hardcodes Im(λ)=0, all 5 tests use diagonal
  matrices → rotation logic never executed.
- **M1** B11 dt-corridor guards only `dt<=0`, not oversized positive dt (test passes for wrong reason).
- **M4** vsa.rs bind/unbind scratch length silently changes convolution length (F6/V2).
- **L1–L3** stale architecture prose; dead `fexp` copy in fft.rs.

## Pending agent work (re-dispatched, implement-only)
- Symmetric crypto (aead/kdf/hash/sign/rng): agent produced ZERO code last attempt (budget burned on
  research). Re-dispatched with embedded KAT vectors + self-consistency gate. Files still 2-line stubs.
- pq_dsa.rs (ML-DSA-65): never written. Re-dispatched mirroring pq_kem coefficient-domain approach.

## Next actions (ordered)
1. Await/Land symmetric crypto + pq_dsa → full `cargo test -p bebop2-core` green.
2. Close H1 wasm32 gate (hard blocker for headline claim).
3. Close H3/H4/H5 (Lanczos eigensolver, square-root Kalman, asymmetric-matrix lyapunov tests).
4. Close M1/M4 (B11 CFL bound, vsa scratch-length).
5. Trilateration integration check once all crates green.

## Verification ground state (2026-07-10)
- `cargo test -p bebop2-core --lib` → 54 passed, 0 failed (post schoolbook + F3/M3 fixes).
- branch: feat/wire-native-core (upstream origin/feat/wire-native-core).
