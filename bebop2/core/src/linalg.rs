//! linalg — the ONE authoritative general eigensolver for bebop2.
//!
//! KILLS THE DUAL-AUTHORITY HAZARD. The Faddeev–LeVerrier + Durand–Kerner
//! eigensolver previously lived ONLY inside `bebop_proto_cap::tests::mesh_consensus`
//! (and was mirrored, byte-for-byte, in the dowiz kernel `spectral` engine). That is a
//! silent-drift hazard: an edit to one copy is invisible to the other until a consumer breaks.
//! It is now consolidated HERE as the single source of truth. Every consumer
//! (`mesh_consensus`, future spectral callers) MUST route through
//! [`eigenvalues`] and is parity-gated against it — no second, unsupervised copy is
//! allowed to drift.
//!
//! Algorithm (zero-dep, deterministic, NO RNG):
//!   1. [`charpoly`] — Faddeev–LeVerrier → characteristic polynomial
//!      `p(λ) = det(λI − A)` (constant term first, matching [`roots`]).
//!   2. [`roots`] — Durand–Kerner → all `n` complex roots of `p`
//!      (deterministic seed `0.4 + 0.9i`, fixed iteration count, no RNG).
//!
//! This is the SAME math proven in dowiz `kernel/src/spectral.rs` and in
//! `bebop_proto_cap/tests/mesh_consensus.rs`'s local solver, now DRY and canonical.
//!
//! NOT feature-gated behind `host`: it is plain `f64` + `alloc` only (no `crate::math`,
//! no `crate::fft`), so it stays reachable from `bebop_proto_cap`, which compiles
//! `bebop2-core` with `default-features = false, features = ["std", "test_keygen"]`.

use alloc::vec::Vec;

/// Minimal complex number (zero-dep). Mirrors `crate::fft::Complex` but is NOT gated
/// behind `host`, so `linalg` stays usable from `bebop_proto_cap`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Complex {
    /// Real part.
    pub re: f64,
    /// Imaginary part.
    pub im: f64,
}

impl Complex {
    /// Construct a complex number.
    #[inline]
    pub fn new(re: f64, im: f64) -> Self {
        Self { re, im }
    }
    /// Modulus `|z|`.
    #[inline]
    pub fn abs(&self) -> f64 {
        self.re.hypot(self.im)
    }
    #[inline]
    fn add(self, o: Complex) -> Complex {
        Complex::new(self.re + o.re, self.im + o.im)
    }
    #[inline]
    fn sub(self, o: Complex) -> Complex {
        Complex::new(self.re - o.re, self.im - o.im)
    }
    #[inline]
    fn mul(self, o: Complex) -> Complex {
        Complex::new(
            self.re * o.re - self.im * o.im,
            self.re * o.im + self.im * o.re,
        )
    }
    #[inline]
    fn div(self, o: Complex) -> Complex {
        let d = o.re * o.re + o.im * o.im;
        Complex::new(
            (self.re * o.re + self.im * o.im) / d,
            (self.im * o.re - self.re * o.im) / d,
        )
    }
}

/// `C = A·B` for two `n×n` matrices (row-major `&[Vec<f64>]`).
#[inline]
fn matmul(a: &[Vec<f64>], b: &[Vec<f64>], n: usize) -> Vec<Vec<f64>> {
    let mut c = vec![vec![0.0; n]; n];
    for i in 0..n {
        for k in 0..n {
            let aik = a[i][k];
            if aik == 0.0 {
                continue;
            }
            for j in 0..n {
                c[i][j] += aik * b[k][j];
            }
        }
    }
    c
}

/// Trace of an `n×n` matrix.
#[inline]
fn trace(a: &[Vec<f64>], n: usize) -> f64 {
    (0..n).map(|i| a[i][i]).sum()
}

/// Characteristic polynomial `p(λ) = det(λI − A)` via Faddeev–LeVerrier.
///
/// Returned **highest-degree-first** (i.e. `p(λ) = c₀·λⁿ + c₁·λⁿ⁻¹ + … + cₙ`, where
/// `c₀ = 1`). This matches the `bebop_proto_cap::tests::mesh_consensus` `charpoly`
/// convention exactly. For an `n×n` input the returned vector has length `n+1`.
/// (The overall sign is irrelevant to [`roots`]/`eigenvalues` since negating the whole
/// polynomial does not change its roots.)
pub fn charpoly(a: &[Vec<f64>]) -> Vec<f64> {
    let n = a.len();
    if n == 0 {
        return vec![1.0];
    }
    let mut c = vec![0.0; n + 1];
    c[n] = 1.0;
    let mut m = vec![vec![0.0; n]; n];
    for i in 0..n {
        m[i][i] = 1.0;
    }
    c[n - 1] = -trace(&matmul(a, &m, n), n);
    for k in 2..=n {
        let am = matmul(a, &m, n);
        let add = c[n - k + 1];
        let mut mk = am;
        for i in 0..n {
            mk[i][i] += add;
        }
        m = mk;
        c[n - k] = -trace(&matmul(a, &m, n), n) / (k as f64);
    }
    (0..=n).map(|i| c[n - i]).collect()
}

/// `seed^k` for a complex `seed` (deterministic Durand–Kerner root seed).
#[inline]
fn seed_pow(s: Complex, k: u32) -> Complex {
    let mut r = Complex::new(1.0, 0.0);
    for _ in 0..k {
        r = r.mul(s);
    }
    r
}

/// All complex roots of a polynomial (constant-term first) via Durand–Kerner.
///
/// Deterministic seed `0.4 + 0.9i`, fixed iteration cap (200), no RNG. Converges to
/// the characteristic polynomial's `n` roots — i.e. the eigenvalues of the original matrix.
pub fn roots(coeffs: &[f64]) -> Vec<Complex> {
    let deg = coeffs.len().saturating_sub(1);
    if deg == 0 {
        return vec![];
    }
    if deg == 1 {
        // 1×1 matrix A=[[a]]: charpoly is [1, -a] → root a = -coeffs[1].
        return vec![Complex::new(-coeffs[1], 0.0)];
    }
    let p: Vec<Complex> = coeffs.iter().map(|&x| Complex::new(x, 0.0)).collect();
    let peval = |x: Complex| -> Complex {
        let mut r = Complex::new(0.0, 0.0);
        for &co in &p {
            r = r.mul(x).add(co);
        }
        r
    };
    let seed = Complex::new(0.4, 0.9);
    let mut rts: Vec<Complex> = (0..deg).map(|k| seed_pow(seed, k as u32)).collect();
    for _ in 0..200 {
        let mut maxd = 0.0f64;
        for i in 0..deg {
            let xi = rts[i];
            let mut denom = Complex::new(1.0, 0.0);
            for j in 0..deg {
                if j != i {
                    denom = denom.mul(xi.sub(rts[j]));
                }
            }
            if denom.abs() == 0.0 {
                continue;
            }
            let delta = peval(xi).div(denom);
            rts[i] = xi.sub(delta);
            let ad = delta.abs();
            if ad > maxd {
                maxd = ad;
            }
        }
        if maxd < 1e-12 {
            break;
        }
    }
    rts
}

/// Eigenvalues of a real `n×n` matrix `A` (row-major as `&[Vec<f64>]`).
///
/// Returns the `n` complex eigenvalues (counting multiplicity), computed by
/// Faddeev–LeVerrier (`charpoly`) + Durand–Kerner (`roots`). This is the SINGLE
/// authoritative eigensolver for bebop2 — do not re-implement it elsewhere; parity-gate
/// any independent method against this function instead.
///
/// # Panics
/// Debug builds assert `A` is square (`n×n`).
pub fn eigenvalues(m: &[Vec<f64>]) -> Vec<Complex> {
    let n = m.len();
    debug_assert!(
        n == 0 || m.iter().all(|r| r.len() == n),
        "eigenvalues: expected a square n×n matrix"
    );
    let coeffs = charpoly(m);
    if n > 0 && coeffs[1..].iter().all(|c| c.abs() < 1e-12) {
        return vec![Complex::new(0.0, 0.0); n];
    }
    roots(&coeffs)
}

#[cfg(all(test, feature = "host"))]
mod tests {
    use super::*;

    #[test]
    fn charpoly_2x2_swap() {
        // A = [[0,1],[1,0]]: p(λ) = det(λI−A) = λ² − 1 → coeffs highest-first [ 1, 0, -1 ].
        let a = vec![vec![0.0, 1.0], vec![1.0, 0.0]];
        let c = charpoly(&a);
        assert_eq!(c.len(), 3);
        assert!(
            (c[0] - 1.0).abs() < 1e-12,
            "λ² term should be 1, got {}",
            c[0]
        );
        assert!((c[1]).abs() < 1e-12, "λ term should be 0, got {}", c[1]);
        assert!(
            (c[2] + 1.0).abs() < 1e-12,
            "const term should be -1, got {}",
            c[2]
        );
    }

    #[test]
    fn eigenvalues_2x2_swap_is_pm1() {
        let a = vec![vec![0.0, 1.0], vec![1.0, 0.0]];
        let mut ev: Vec<f64> = eigenvalues(&a).iter().map(|e| e.re).collect();
        ev.sort_by(|x, y| x.partial_cmp(y).unwrap());
        assert!(
            (ev[0] + 1.0).abs() < 1e-9 && (ev[1] - 1.0).abs() < 1e-9,
            "eigs {{1,-1}}, got {ev:?}"
        );
    }

    #[test]
    fn eigenvalues_2x2_complex_pair() {
        // Rotation [[0,-1],[1,0]]: p(λ)=λ²+1 → {i, -i}.
        let a = vec![vec![0.0, -1.0], vec![1.0, 0.0]];
        let mut ev: Vec<(f64, f64)> = eigenvalues(&a).iter().map(|e| (e.re, e.im)).collect();
        ev.sort_by(|x, y| x.partial_cmp(y).unwrap());
        assert!(
            (ev[0].0).abs() < 1e-9 && (ev[0].1 + 1.0).abs() < 1e-9,
            "got {ev:?}"
        );
        assert!(
            (ev[1].0).abs() < 1e-9 && (ev[1].1 - 1.0).abs() < 1e-9,
            "got {ev:?}"
        );
    }
}
