//! bebop2-core — from-scratch, zero-dep, post-quantum.
//!
//! Pure `core`+`alloc`. Compiles to wasm with an EMPTY import section (no clock/RNG/socket
//! reachable). This IS the "machine code" layer: deterministically verifiable, executed bit-exact.
//!
//! Architecture principle (physicality + first-principles): vectors/waves are NOT arrays.
//! A vector = projection onto a basis; a wave/tensor = eigenmode of a linear operator.
//! Prefer spectral/basis representations (field_*, VSA hyperplanes) over dense tensor buffers.

// `no_std` ONLY for the wasm32 target — that is where the "machine code" / empty-import
// property must hold (no clock/RNG/socket reachable). Native + test builds use std so
// tests can allocate + panic. The empty-import gate (reloop v2) enforces no_std on wasm.
#![cfg_attr(target_arch = "wasm32", no_std)]
// `alloc` is in scope for both std and no_std builds (brought in explicitly so hash.rs's
// `alloc::vec::Vec` / `alloc::string::String` helpers resolve under native doctest builds too).
extern crate alloc;

pub mod field;      // graph-PDE spectral kernel (Laplacian eigenmodes) — replaces dense tensors
pub mod vsa;        // vector symbolic archive (hyperplane bundling, not dense matrices)
pub mod algebra;     // cosine / cross / sinc — basis projections
pub mod kalman;      // Kalman filter (trajectory integrals, not vector math)
pub mod lyapunov;    // Lyapunov derivative (stability, not ad-hoc vectors)
pub mod chebyshev;    // Chebyshev spectral propagator
pub mod fft;         // FFT (frequency-domain eigen-decomposition)
pub mod active;       // active inference (free-energy, spectral)

// crypto — all from scratch, zero-dep, post-quantum.
pub mod pq_kem;      // ML-KEM-768 (FIPS 203)
pub mod pq_dsa;      // ML-DSA-65 (FIPS 204)
pub mod aead;        // XChaCha20-Poly1305 (RFC 8439)
pub mod kdf;         // Argon2id
pub mod hash;        // SHA-512 + SHA3
pub mod sign;        // Ed25519 (hybrid classical fallback)
pub mod rng;         // CSPRNG from hardware entropy (in-tree, no getrandom dep)

// KAT vectors (committed; parent-embedded short ones in vectors.rs, agent-fetched long
// ones in vectors_long.rs). Read by #[cfg(test)] in each crypto module.
pub mod kat;

// ── C8 FIX (carried from fable audit) ───────────────────────────────────────────────
/// Correct range reduction for exp: `r = x - round(x/ln2)*ln2`, symmetric for ALL signs.
/// The old `|r| <= ln2/2` form was WRONG for negative arguments (hottest spectral path).
#[inline]
pub fn fexp(x: f64) -> f64 {
    // C8 FIX: symmetric range reduction `r = x - round(x/ln2)*ln2` — correct for ALL
    // signs (old `|r| <= ln2/2` form silently broke for x<0, the hottest spectral path).
    let ln2 = 0.6931471805599453_f64;
    // round()/floor()/trunc() are std-only; in no_std core round via `as i64` cast (language primitive).
    let q = x / ln2;
    let fl = q as i64;
    let frac = q - (fl as f64);
    let k = if frac >= 0.5 {
        (fl + 1) as i32
    } else if frac <= -0.5 {
        (fl - 1) as i32
    } else {
        fl as i32
    };
    let r = x - (k as f64) * ln2; // |r| <= ln2/2 ≈ 0.3466
    // e^r via Taylor (fast, deterministic, no RNG, no pow()).
    let mut t = 1.0_f64;
    let mut term = 1.0_f64;
    let mut n = 1u32;
    while n <= 20 {
        term *= r / (n as f64);
        t += term;
        n += 1;
    }
    // 2^k * e^r. A raw `1u64 << k` panics on overflow for k >= 64 (u64 shift
    // range), which the mass-conservation tests hit with large diffusion coeffs.
    // Use the exact integer power of two via f64 (saturates to INF at extreme k,
    // never panics; deterministic).
    let two_k = if k >= 0 {
        if k >= 1024 { f64::INFINITY } else { 2.0f64.powi(k) }
    } else {
        let ak = (-k) as i32;
        if ak >= 1024 { 0.0 } else { 2.0f64.powi(ak).recip() }
    };
    two_k * t
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn c8_fexp_negative_reduction_correct() {
        // RED+GREEN: old code broke for x<0. Reference: e^-1 ≈ 0.36787944117.
        let v = fexp(-1.0);
        assert!((v - (-1.0f64).exp()).abs() < 1e-12, "fexp(-1) wrong: {v}");
        // symmetry: fexp(x)*fexp(-x) == 1 for all x (old code failed this for negatives).
        for x in [-3.0, -1.5, -0.25, 0.0, 0.5, 2.0, 7.0] {
            let p = fexp(x) * fexp(-x);
            assert!((p - 1.0).abs() < 1e-10, "fexp symmetry broken at {x}: {p}");
        }
    }
    #[test]
    fn c8_fexp_matches_std_for_positives() {
        for x in [0.0, 0.5, 1.0, 2.5, 5.0, 10.0] {
            let d = fexp(x);
            let s = x.exp();
            // relative tolerance — absolute 1e-12 is unrealistic for |e^x|~22000.
            assert!((d - s).abs() / s.abs() < 1e-12, "fexp({x}) = {d}, std = {s}");
        }
    }
}
