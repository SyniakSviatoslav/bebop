//! bebop2-core — from-scratch, zero-dep, post-quantum.
//!
//! Pure `core`+`alloc`. Compiles to wasm with an EMPTY import section (no clock/RNG/socket
//! reachable). This IS the "machine code" layer: deterministically verifiable, executed bit-exact.
//!
//! Architecture principle (physicality + first-principles): vectors/waves are NOT arrays.
//! A vector = projection onto a basis; a wave/tensor = eigenmode of a linear operator.
//! Prefer spectral/basis representations (field_*, VSA hyperplanes) over dense tensor buffers.
//!
//! `no_std` is gated on the `std` feature (default-on). Disable it
//! (`--no-default-features`) for a genuine no_std / empty-import wasm32 build. Native + test
//! builds keep `std` so tests can allocate + panic, and so the wasm32 build links with std
//! semantics. The empty-import gate (reloop v2) enforces no_std on wasm when std is off.

// `no_std` ONLY when the `std` feature is disabled (e.g. `--no-default-features
// --target wasm32-unknown-unknown`). Native + test builds enable `std` (the default feature)
// so tests can allocate + panic, and so the default wasm32 build links with std semantics.
#![cfg_attr(not(feature = "std"), no_std)]
// `alloc` is in scope for both std and no_std builds (brought in explicitly so hash.rs's
// `alloc::vec::Vec` / `alloc::string::String` helpers resolve under native doctest builds too).
// `#[macro_use]` re-exports `vec!`/`format!` crate-wide so modules don't each need the import.
#[macro_use]
extern crate alloc;

// ── Genuine no_std (--no-default-features) support ──────────────────────────────────────
// When `std` is off we must supply a global allocator + panic handler and provide the few
// `f64` libm intrinsics (sqrt/sin/cos/ln/powi/exp) that std normally provides as methods.
// None of this is compiled when `std` is on, so numeric correctness under std is untouched.
#[cfg(not(feature = "std"))]
mod no_std_runtime {
    use core::alloc::{GlobalAlloc, Layout};
    use core::panic::PanicInfo;
    use core::sync::atomic::{AtomicUsize, Ordering};

    // Bump allocator over a static 4 MiB heap. Sufficient for the crate's transient
    // allocation patterns (temporary Vecs freed on scope exit; we never shrink — fine for
    // the empty-import wasm use-case). Never frees; acceptable for short-lived wasm runs.
    // AtomicUsize is Sync+Send (unlike Cell) so it satisfies the GlobalAlloc bound.
    const HEAP_SIZE: usize = 4 * 1024 * 1024;
    static mut HEAP: [u8; HEAP_SIZE] = [0u8; HEAP_SIZE];
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    struct Bump;
    unsafe impl GlobalAlloc for Bump {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let start = NEXT.load(Ordering::Relaxed);
            let align = layout.align();
            let aligned = (start + align - 1) & !(align - 1);
            let end = aligned + layout.size();
            if end > HEAP_SIZE {
                return core::ptr::null_mut();
            }
            NEXT.store(end, Ordering::Relaxed);
            HEAP.as_mut_ptr().add(aligned)
        }
        unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
    }

    #[global_allocator]
    static ALLOC: Bump = Bump;

    #[panic_handler]
    fn panic(_info: &PanicInfo) -> ! {
        loop {}
    }
}

/// `f64` libm shims (sqrt/sin/cos/ln/powi/exp). Part of the analytic `host` kernel only —
/// gated behind `feature = "host"`. The pure PQ crypto core never calls these, so disabling
/// `host` keeps the wasm32 build free of every f64 trig dependency.
#[cfg(feature = "host")]
pub mod math {
    /// sqrt(x). std: `x.sqrt()`. no_std: Newton–Raphson from an ldexp seed.
    #[inline]
    pub fn fsqrt(x: f64) -> f64 {
        #[cfg(feature = "std")]
        {
            x.sqrt()
        }
        #[cfg(not(feature = "std"))]
        {
            crate::no_std_support::sqrt_newton(x)
        }
    }
    /// sin(x). std: `x.sin()`. no_std: range-reduced Taylor.
    #[inline]
    pub fn fsin(x: f64) -> f64 {
        #[cfg(feature = "std")]
        {
            x.sin()
        }
        #[cfg(not(feature = "std"))]
        {
            crate::no_std_support::sin_taylor(x)
        }
    }
    /// cos(x). std: `x.cos()`. no_std: range-reduced Taylor.
    #[inline]
    pub fn fcos(x: f64) -> f64 {
        #[cfg(feature = "std")]
        {
            x.cos()
        }
        #[cfg(not(feature = "std"))]
        {
            crate::no_std_support::cos_taylor(x)
        }
    }
    /// ln(x). std: `x.ln()`. no_std: Newton on e^y = x.
    #[inline]
    pub fn fln(x: f64) -> f64 {
        #[cfg(feature = "std")]
        {
            x.ln()
        }
        #[cfg(not(feature = "std"))]
        {
            crate::no_std_support::ln_newton(x)
        }
    }
    /// 2.0_f64.powi(k) for integer k (avoids std-only `f64::powi`).
    #[inline]
    pub fn pow2_i(k: i32) -> f64 {
        if k >= 0 {
            if k >= 1024 {
                f64::INFINITY
            } else {
                pow2_pos(k as u32)
            }
        } else {
            let ak = (-k) as u32;
            if ak >= 1024 {
                0.0
            } else {
                1.0 / pow2_pos(ak)
            }
        }
    }
    #[inline]
    fn pow2_pos(k: u32) -> f64 {
        let mut r = 1.0f64;
        let mut i = 0u32;
        while i < k {
            r *= 2.0;
            i += 1;
        }
        r
    }
}

// `no_std_support` (f64 Taylor/Newton shims) is only needed by the `host` analytic kernel;
// gating it behind `host` keeps the pure-crypto no_std build from pulling in f64 math.
#[cfg(all(not(feature = "std"), feature = "host"))]
mod no_std_support {
    // IEEE bit helpers (the same trick chebyshev.rs uses for ftrunc).
    #[inline]
    fn frexp(x: f64) -> (f64, i32) {
        let bits = x.to_bits();
        let exp = (((bits >> 52) & 0x7ff) as i32) - 1023;
        let mant = f64::from_bits((bits & 0x800f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000);
        (mant, exp)
    }
    #[inline]
    #[allow(clippy::approx_constant)] // no_std libm shim: 0.6931… is ln2 the shim computes with
    fn ldexp(mant: f64, exp: i32) -> f64 {
        if exp > 1023 {
            return f64::INFINITY;
        }
        if exp < -1023 {
            return 0.0;
        }
        f64::from_bits((mant.to_bits() & 0x800f_ffff_ffff_ffff) | (((exp + 1023) as u64) << 52))
    }
    // Symmetric round-to-nearest (handles negatives correctly) via bit tricks.
    #[inline]
    fn ftrunc(x: f64) -> f64 {
        let bits = x.to_bits();
        let exp = ((bits >> 52) & 0x7ff) as i32 - 1023;
        if exp < 0 {
            return 0.0;
        }
        if exp >= 52 {
            return x;
        }
        let mask = (1u64 << (52 - exp)) - 1;
        f64::from_bits(bits & !mask)
    }
    #[inline]
    fn fround(x: f64) -> f64 {
        if x >= 0.0 {
            ftrunc(x + 0.5)
        } else {
            -ftrunc(-x + 0.5)
        }
    }

    pub fn sqrt_newton(x: f64) -> f64 {
        if x < 0.0 {
            return f64::NAN;
        }
        if x == 0.0 {
            return 0.0;
        }
        let (m, e) = frexp(x); // x = m * 2^e, m in [0.5, 1)
        let _ = m;
        // Seed y ≈ 2^(e/2); Newton converges in a handful of steps from here.
        let mut y = ldexp(1.0, e / 2);
        for _ in 0..12 {
            y = 0.5 * (y + x / y);
        }
        y
    }
    pub fn sin_taylor(mut x: f64) -> f64 {
        let pi = 3.141592653589793;
        let k = fround(x / pi);
        x = x - k * pi;
        let x2 = x * x;
        let mut t = x;
        let mut term = x;
        let mut n = 1u32;
        while n < 14 {
            term *= -x2 / ((2 * n) as f64 * (2 * n + 1) as f64);
            t += term;
            n += 1;
        }
        t
    }
    pub fn cos_taylor(mut x: f64) -> f64 {
        let pi = 3.141592653589793;
        let k = fround(x / pi);
        x = x - k * pi;
        let x2 = x * x;
        let mut t = 1.0;
        let mut term = 1.0;
        let mut n = 1u32;
        while n < 14 {
            term *= -x2 / ((2 * n) as f64 * (2 * n - 1) as f64);
            t += term;
            n += 1;
        }
        t
    }
    pub fn ln_newton(x: f64) -> f64 {
        if x <= 0.0 {
            return f64::NAN;
        }
        let (m, e) = frexp(x); // x = m * 2^e, m in [0.5, 1)
        let m2 = m * 2.0; // m2 in [1, 2)
        let mut y = 0.0f64; // solve exp(y) = m2 via Newton
        for _ in 0..40 {
            let ey = exp_taylor(y);
            y = y + (m2 - ey) / ey;
        }
        y + (e as f64) * 0.6931471805599453
    }
    #[allow(clippy::approx_constant)] // no_std libm shim: 0.6931… is ln2 the shim computes with
    pub fn exp_taylor(x: f64) -> f64 {
        let ln2 = 0.6931471805599453;
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
        let r = x - (k as f64) * ln2;
        let mut t = 1.0f64;
        let mut term = 1.0f64;
        let mut n = 1u32;
        while n <= 20 {
            term *= r / (n as f64);
            t += term;
            n += 1;
        }
        crate::math::pow2_i(k) * t
    }
}

// ── Analytic / host-only kernel ────────────────────────────────────────────────
// `field`/`vsa`/`algebra`/`kalman`/`lyapunov`/`chebyshev`/`fft`/`active` are the f64-heavy
// analytic layer (spectral propagators, FFT, VSA, Kalman, active inference). They depend on
// `core::math` (f64 libm shims) and heap-allocated `Vec<f64>` spectra, neither of which
// wasm32-unknown-unknown can provide without std math / an allocator. They are NOT part of the
// PQ crypto core, so they are gated behind the `host` feature and excluded from the wasm32
// (no_std + no-alloc) build. The pure PQ crypto path below never references them.
#[cfg(feature = "host")]
pub mod active; // active inference (free-energy, spectral)
#[cfg(feature = "host")]
pub mod algebra; // cosine / cross / sinc — basis projections
#[cfg(feature = "host")]
pub mod chebyshev; // Chebyshev spectral propagator
#[cfg(feature = "host")]
pub mod dmd; // Online DMD (rank-1 RLS / Sherman–Morrison), BP-07
#[cfg(feature = "host")]
pub mod fft; // FFT (frequency-domain eigen-decomposition)
#[cfg(feature = "host")]
pub mod field; // graph-PDE spectral kernel (Laplacian eigenmodes) — replaces dense tensors
#[cfg(feature = "host")]
pub mod kalman; // Kalman filter (trajectory integrals, not vector math)
#[cfg(feature = "host")]
pub mod lyapunov; // Lyapunov derivative (stability, not ad-hoc vectors)
#[cfg(feature = "host")]
pub mod resonator;
#[cfg(feature = "host")]
pub mod vsa; // vector symbolic archive (hyperplane bundling, not dense matrices) // closed-loop controller: generate→reflect→supervise, Lyapunov freeze, rollback-to-best

// crypto — all from scratch, zero-dep, post-quantum.
pub mod aead; // XChaCha20-Poly1305 (RFC 8439)
pub mod hash; // SHA-512 + SHA3
pub mod kdf; // Argon2id
pub mod pq_dsa; // ML-DSA-65 (FIPS 204)
pub mod pq_kem; // ML-KEM-768 (FIPS 203)
pub mod rng;
pub mod sign; // Ed25519 (hybrid classical fallback) // CSPRNG from hardware entropy (in-tree, no getrandom dep)

// KAT vectors (committed; parent-embedded short ones in vectors.rs, agent-fetched long
// ones in vectors_long.rs). Read by #[cfg(test)] in each crypto module.
pub mod kat;

// ── C8 FIX (carried from fable audit) ───────────────────────────────────────────────
/// Correct range reduction for exp: `r = x - round(x/ln2)*ln2`, symmetric for ALL signs.
/// The old `|r| <= ln2/2` form was WRONG for negative arguments (hottest spectral path).
///
/// Host-only (part of the f64 analytic kernel): depends on `core::math::pow2_i`, which is
/// gated behind `feature = "host"`. Excluded from the wasm32 no_std/alloc-free crypto build.
#[cfg(feature = "host")]
#[inline]
#[allow(clippy::approx_constant)] // no_std libm shim: 0.6931… is ln2 the shim computes with
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
    // never panics; deterministic). Uses the core-only `pow2_i` shim so this compiles
    // under both std and no_std.
    let two_k = crate::math::pow2_i(k);
    two_k * t
}

#[cfg(all(test, feature = "host"))]
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
            assert!(
                (d - s).abs() / s.abs() < 1e-12,
                "fexp({x}) = {d}, std = {s}"
            );
        }
    }
}
