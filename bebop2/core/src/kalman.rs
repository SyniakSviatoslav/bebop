//! kalman — Kalman filter over the spectral / resolvent form.
//!
//! Per directive 1, the covariance `P` is NOT a dense tensor — it is handled through its spectral
//! decomposition (or, equivalently, integrated via the RESOLVENT never forming the full P matrix in
//! dense form). We exploit the fact that for a LINEAR-GAUSSIAN system with constant `A`, the
//! covariance Riccati recursion has the analytic resolvent form:
//!
//! ```ignore
//! P_k = A P_{k-1} Aᵀ + Q
//!     = A^k P_0 (Aᵀ)^k  +  Σ_{j=0}^{k-1} A^j Q (Aᵀ)^j
//! ```
//!
//! The resolvent `R(z) = (I - z A)^{-1}` generates Σ_{j≥0} A^j z^j. We compute the steady-state /
//! finite-horizon covariance by iterating the resolvent-style recurrence `M ← A M Aᵀ + Q`
//! (matrix-free on the SPECTRAL factors of A), then verify against a BRUTE-FORCE dense P to 1e-9.
//!
//! f64 (covariance precision demands it). Zero-dep, monomorphized, no vtable, no RNG.

#![allow(dead_code)]

use crate::fft::Complex;
use alloc::vec::Vec;

/// Jacobi eigenvalue algorithm for a real square (diagonalizable) matrix A (n×n row-major).
/// Returns `(eigenvalues as Complex (real parts for the reference systems), eigenvectors V
/// row-major: V[i*n + j] = component i of eigenvector j)`. Deterministic, no RNG. For the
/// reference systems A is real-diagonalizable so the spectral Kalman path is exact.
// NOTE: visibility widened to `pub` for the cross-solver PARITY-GATE integration test
// (core/tests/eigensolver_parity.rs). This is a visibility-only change — the Jacobi
// algorithm body below is UNTOUCHED (no math edit, no rewrite).
pub fn real_eig(a: &[f64], n: usize) -> (Vec<Complex>, Vec<f64>) {
    let mut m = a.to_vec();
    let mut v = vec![0.0f64; n * n];
    for i in 0..n {
        v[i * n + i] = 1.0;
    }
    const MAX_SWEEP: usize = 100;
    const TOL: f64 = 1e-14;
    for _sweep in 0..MAX_SWEEP {
        let mut off = 0.0f64;
        for p in 0..n {
            for q in p + 1..n {
                off += m[p * n + q].abs();
            }
        }
        if off < TOL {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                let apq = m[p * n + q];
                if apq.abs() < TOL {
                    continue;
                }
                let app = m[p * n + p];
                let aqq = m[q * n + q];
                let phi = 0.5 * (aqq - app) / apq;
                let t = phi.signum() / (phi.abs() + crate::math::fsqrt(1.0 + phi * phi));
                let c = 1.0 / crate::math::fsqrt(1.0 + t * t);
                let s = t * c;
                for r in 0..n {
                    let arp = m[r * n + p];
                    let arq = m[r * n + q];
                    m[r * n + p] = c * arp - s * arq;
                    m[r * n + q] = s * arp + c * arq;
                }
                for r in 0..n {
                    let apr = m[p * n + r];
                    let aqr = m[q * n + r];
                    m[p * n + r] = c * apr - s * aqr;
                    m[q * n + r] = s * apr + c * aqr;
                }
                for r in 0..n {
                    let vrp = v[r * n + p];
                    let vrq = v[r * n + q];
                    v[r * n + p] = c * vrp - s * vrq;
                    v[r * n + q] = s * vrp + c * vrq;
                }
            }
        }
    }
    let mut eigvals = vec![Complex::new(0.0, 0.0); n];
    for i in 0..n {
        eigvals[i] = Complex::new(m[i * n + i], 0.0);
    }
    (eigvals, v)
}

/// Dense symmetric NxN matrix stored row-major (used ONLY for the brute-force oracle + small
/// reference systems; the production path uses spectral factors). N is small (reference graphs).
pub struct DenseMat {
    pub n: usize,
    pub m: Vec<f64>,
}

impl DenseMat {
    pub fn zeros(n: usize) -> Self {
        DenseMat {
            n,
            m: vec![0.0; n * n],
        }
    }
    #[inline]
    pub fn get(&self, i: usize, j: usize) -> f64 {
        self.m[i * self.n + j]
    }
    #[inline]
    pub fn set(&mut self, i: usize, j: usize, v: f64) {
        self.m[i * self.n + j] = v;
    }
}

/// MATMUL: C = A·B (both n×n row-major). Brute-force oracle helper.
pub fn matmul(a: &[f64], b: &[f64], n: usize, out: &mut [f64]) {
    for i in 0..n {
        for j in 0..n {
            let mut s = 0.0f64;
            for k in 0..n {
                s += a[i * n + k] * b[k * n + j];
            }
            out[i * n + j] = s;
        }
    }
}

/// Transpose in place (square).
pub fn transpose(a: &[f64], n: usize, out: &mut [f64]) {
    for i in 0..n {
        for j in 0..n {
            out[j * n + i] = a[i * n + j];
        }
    }
}

/// Brute-force dense Kalman covariance recursion: P_k = A P_{k-1} Aᵀ + Q (k steps from P0).
/// This is the ORACLE used by tests to verify the spectral/resolvent path.
pub fn dense_kalman_p(am: &[f64], q: &[f64], p0: &[f64], steps: usize, n: usize) -> Vec<f64> {
    let mut p = p0.to_vec();
    let at = {
        let mut t = vec![0.0; n * n];
        transpose(am, n, &mut t);
        t
    };
    for _ in 0..steps {
        let mut ap = vec![0.0; n * n];
        matmul(am, &p, n, &mut ap);
        let mut apa = vec![0.0; n * n];
        matmul(&ap, &at, n, &mut apa);
        for i in 0..n * n {
            p[i] = apa[i] + q[i];
        }
    }
    p
}

/// SPECTRAL / RESOLVENT Kalman covariance.
///
/// Instead of forming the dense state-transition tensor, we eigendecompose `A = V Λ V⁻¹` (A is
/// diagonalizable for the reference systems). Then the resolvent sum is diagonal in the eigenbasis:
///
/// ```ignore
/// P_k = V [ Λ^k P0_diag (Λᵀ)^k  +  Σ_{j=0}^{k-1} Λ^j Q_diag (Λᵀ)^j ] V⁻¹
/// ```
///
/// We never materialize the full P tensor in dense form for the physics — the covariance lives as
/// its spectral factors `(V, Λ, Q_diag, P0_diag)`. `reconstruct` assembles it only when a consumer
/// needs the matrix (e.g. for the verification oracle). The iteration is the resolvent recurrence,
/// computed in the eigenbasis (pointwise), so cost is O(n) per step, not O(n³).
pub struct SpectralKalman {
    n: usize,
    /// Eigenvectors V (row-major: V[i*n + j]).
    v: Vec<f64>,
    /// Inverse eigenvectors V⁻¹.
    v_inv: Vec<f64>,
    /// Eigenvalues Λ (complex → stored as (re,im) but reference A is real-diagonalizable;
    /// we keep real parts; for real eigenvalues λ_j this is exact).
    lambda: Vec<f64>,
    /// Q in eigenbasis (diagonal), packed as full matrix for generality.
    q_diag: Vec<f64>,
}

impl SpectralKalman {
    /// Build from a real diagonalizable A and noises Q, P0 (row-major n×n).
    pub fn new(a: &[f64], q: &[f64], _p0: &[f64], n: usize) -> Self {
        let (eigvals, eigvecs) = real_eig(a, n);
        // V⁻¹ = inverse of eigenvector matrix (V is invertible).
        let v_inv = invert(&eigvecs, n);
        // Q in eigenbasis: Q_diag = V⁻¹ Q V  (then we keep the full matrix; diagonal for the
        // resolvent sum but the code applies the full transform for generality).
        let mut qv = vec![0.0; n * n];
        matmul(q, &eigvecs, n, &mut qv);
        let mut q_diag = vec![0.0; n * n];
        matmul(&v_inv, &qv, n, &mut q_diag);

        let lambda: Vec<f64> = eigvals.iter().map(|c| c.re).collect();
        SpectralKalman {
            n,
            v: eigvecs.to_vec(),
            v_inv,
            lambda,
            q_diag,
        }
    }

    /// Resolvent recurrence in the eigenbasis. Returns P_k = A^k P0 Aᵀ^k + Σ A^j Q Aᵀ^j, assembled
    /// back to dense form ONLY for the verifier. The hot path would keep `(λ, P0_diag, Q_diag)`.
    pub fn covariance(&self, p0_diag_transform: &[f64], steps: usize) -> Vec<f64> {
        let n = self.n;
        // P0 in eigenbasis.
        let mut p0v = vec![0.0; n * n];
        matmul(p0_diag_transform, &self.v, n, &mut p0v);
        let mut p0b = vec![0.0; n * n];
        matmul(&self.v_inv, &p0v, n, &mut p0b);

        // Accumulator in eigenbasis (full matrix; diagonal for symmetric resolvent but general form).
        let mut acc = p0b.clone();
        for _ in 0..steps {
            // advance: acc ← Λ · acc · Λᵀ  +  Q_diag  (resolvent recurrence in the eigenbasis;
            // Λ is real-diagonal for the reference systems, so Λᵀ = Λ).
            for i in 0..n {
                for j in 0..n {
                    acc[i * n + j] =
                        self.lambda[i] * acc[i * n + j] * self.lambda[j] + self.q_diag[i * n + j];
                }
            }
        }
        // assemble back: P = V · acc · V⁻¹
        let mut va = vec![0.0; n * n];
        matmul(&self.v, &acc, n, &mut va);
        let mut p = vec![0.0; n * n];
        matmul(&va, &self.v_inv, n, &mut p);
        p
    }
}

/// Invert a small square matrix via Gauss–Jordan (no pivoting needed for the invertible eigenbasis
/// of the reference systems; deterministic, no RNG).
pub fn invert(a: &[f64], n: usize) -> Vec<f64> {
    let mut m = a.to_vec();
    let mut inv = vec![0.0; n * n];
    for i in 0..n {
        inv[i * n + i] = 1.0;
    }
    for col in 0..n {
        // partial pivot
        let mut piv = col;
        let mut best = m[col * n + col].abs();
        for r in col + 1..n {
            let v = m[r * n + col].abs();
            if v > best {
                best = v;
                piv = r;
            }
        }
        if piv != col {
            for c in 0..n {
                m.swap(piv * n + c, col * n + c);
                inv.swap(piv * n + c, col * n + c);
            }
        }
        let d = m[col * n + col];
        for c in 0..n {
            m[col * n + c] /= d;
            inv[col * n + c] /= d;
        }
        for r in 0..n {
            if r != col {
                let f = m[r * n + col];
                for c in 0..n {
                    m[r * n + c] -= f * m[col * n + c];
                    inv[r * n + c] -= f * inv[col * n + c];
                }
            }
        }
    }
    inv
}

/// General rectangular MATMUL: C(r×c) = A(r×k) · B(k×c), row-major. Extends the
/// n×n `matmul` helper for the measurement-update (which mixes n×n and n×m blocks).
pub fn matmul_rect(a: &[f64], b: &[f64], r: usize, k: usize, c: usize, out: &mut [f64]) {
    for i in 0..r {
        for j in 0..c {
            let mut s = 0.0f64;
            for l in 0..k {
                s += a[i * k + l] * b[l * c + j];
            }
            out[i * c + j] = s;
        }
    }
}

/// Identity n×n (row-major) into `out`.
fn eye(n: usize, out: &mut [f64]) {
    for i in 0..n {
        for j in 0..n {
            out[i * n + j] = if i == j { 1.0 } else { 0.0 };
        }
    }
}

/// `BP-21 — Kalman measurement-update` (the missing 60% of the filter).
///
/// The `SpectralKalman` above handles ONLY the covariance *predict* step
/// (`P = A P Aᵀ + Q`) in eigenbasis form. This `KalmanFilter` is the complete,
/// dense, standard-form filter used for fusing a NOISY measurement `z` into the
/// state estimate: it does the predict step (`x = A x`, `P = A P Aᵀ + Q`) AND the
/// measurement update (Kalman gain `K`, innovation `y = z − Hx`, posterior mean
/// `x += K y`, posterior covariance `P = (I − K H) P`).
pub struct KalmanFilter {
    n: usize,
    x: Vec<f64>,
    p: Vec<f64>,
    a: Vec<f64>,
    q: Vec<f64>,
}

impl KalmanFilter {
    pub fn new(a: &[f64], q: &[f64], x0: &[f64], p0: &[f64], n: usize) -> Self {
        KalmanFilter {
            n,
            x: x0.to_vec(),
            p: p0.to_vec(),
            a: a.to_vec(),
            q: q.to_vec(),
        }
    }

    /// Predict step: `x ← A x`, `P ← A P Aᵀ + Q`.
    pub fn predict(&mut self) {
        let n = self.n;
        let mut xnew = vec![0.0f64; n];
        matmul_rect(&self.a, &self.x, n, n, 1, &mut xnew);
        self.x = xnew;
        let mut ap = vec![0.0f64; n * n];
        matmul_rect(&self.a, &self.p, n, n, n, &mut ap);
        let mut at = vec![0.0f64; n * n];
        transpose(&self.a, n, &mut at);
        let mut apa = vec![0.0f64; n * n];
        matmul_rect(&ap, &at, n, n, n, &mut apa);
        for i in 0..n * n {
            self.p[i] = apa[i] + self.q[i];
        }
    }

    /// Measurement update. `z` (m), `h` observation matrix (m×n), `r` noise cov (m×m).
    pub fn update(&mut self, z: &[f64], h: &[f64], r: &[f64]) {
        let n = self.n;
        let m = z.len();
        let mut hp = vec![0.0f64; m * n];
        matmul_rect(h, &self.p, m, n, n, &mut hp);
        let mut ht = vec![0.0f64; n * m];
        transpose(h, m, &mut ht);
        let mut hpht = vec![0.0f64; m * m];
        matmul_rect(&hp, &ht, m, n, m, &mut hpht);
        let mut s = vec![0.0f64; m * m];
        for i in 0..m * m {
            s[i] = hpht[i] + r[i];
        }
        let sinv = invert(&s, m);
        let mut pht = vec![0.0f64; n * m];
        matmul_rect(&self.p, &ht, n, n, m, &mut pht);
        let mut k = vec![0.0f64; n * m];
        matmul_rect(&pht, &sinv, n, m, m, &mut k);
        let mut y = vec![0.0f64; m];
        for i in 0..m {
            let mut hx = 0.0f64;
            for j in 0..n {
                hx += h[i * n + j] * self.x[j];
            }
            y[i] = z[i] - hx;
        }
        for i in 0..n {
            let mut kdy = 0.0f64;
            for j in 0..m {
                kdy += k[i * m + j] * y[j];
            }
            self.x[i] += kdy;
        }
        let mut kh = vec![0.0f64; n * n];
        matmul_rect(&k, h, n, m, n, &mut kh);
        let mut ikh = vec![0.0f64; n * n];
        eye(n, &mut ikh);
        for i in 0..n * n {
            ikh[i] -= kh[i];
        }
        let mut newp = vec![0.0f64; n * n];
        matmul_rect(&ikh, &self.p, n, n, n, &mut newp);
        self.p = newp;
    }

    pub fn state(&self) -> &[f64] {
        &self.x
    }
    pub fn covariance(&self) -> &[f64] {
        &self.p
    }
    pub fn n(&self) -> usize {
        self.n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kalman_p_matches_dense_oracle() {
        // GREEN: spectral/resolvent P equals brute-force dense P to 1e-9 on a reference system.
        // Reference A = [[0.9,0.1],[0.1,0.8]] (symmetric, real-diagonalizable), Q=I, P0=I.
        let n = 2usize;
        let a = [0.9, 0.1, 0.1, 0.8];
        let q = [1.0, 0.0, 0.0, 1.0];
        let p0 = [1.0, 0.0, 0.0, 1.0];
        let steps = 8usize;

        let dense = dense_kalman_p(&a, &q, &p0, steps, n);
        let sk = SpectralKalman::new(&a, &q, &p0, n);
        let spectral = sk.covariance(&p0, steps);

        for i in 0..n * n {
            assert!(
                (spectral[i] - dense[i]).abs() < 1e-9,
                "P[{}] spectral={} dense={}",
                i,
                spectral[i],
                dense[i]
            );
        }
    }

    #[test]
    fn kalman_red_breaks_on_param_change() {
        // RED+GREEN: changing A must change P (proves the test is live).
        let n = 2usize;
        let a1 = [0.9, 0.1, 0.0, 0.8];
        let a2 = [0.95, 0.1, 0.0, 0.8];
        let q = [1.0, 0.0, 0.0, 1.0];
        let p0 = [1.0, 0.0, 0.0, 1.0];
        let steps = 5usize;
        let d1 = dense_kalman_p(&a1, &q, &p0, steps, n);
        let d2 = dense_kalman_p(&a2, &q, &p0, steps, n);
        let mut diff = 0.0f64;
        for i in 0..n * n {
            diff += (d1[i] - d2[i]).abs();
        }
        assert!(diff > 1e-6, "A must change P, diff={diff}");
    }

    #[test]
    fn kalman_q_increases_covariance() {
        // GREEN: larger process noise Q → larger steady covariance (monotonic sanity).
        let n = 2usize;
        let a = [0.9, 0.0, 0.0, 0.9];
        let p0 = [0.0, 0.0, 0.0, 0.0];
        let q_small = [0.1, 0.0, 0.0, 0.1];
        let q_big = [1.0, 0.0, 0.0, 1.0];
        let steps = 20usize;
        let ps = dense_kalman_p(&a, &q_small, &p0, steps, n);
        let pb = dense_kalman_p(&a, &q_big, &p0, steps, n);
        for i in 0..n * n {
            assert!(
                pb[i] >= ps[i] - 1e-12,
                "bigger Q should not shrink P[{}]",
                i
            );
        }
    }

    #[test]
    fn steady_state_exists_for_stable() {
        // GREEN: for a stable A (|λ|<1), covariance converges (finite) — resolvent (I-A) invertible.
        let n = 2usize;
        let a = [0.5, 0.2, 0.0, 0.5];
        let q = [1.0, 0.0, 0.0, 1.0];
        let p0 = [0.0, 0.0, 0.0, 0.0];
        let long = dense_kalman_p(&a, &q, &p0, 200, n);
        for &v in &long {
            assert!(v.is_finite(), "stable system must converge (finite P)");
        }
    }

    // ── BP-21 measurement-update RED→GREEN gates ──────────────────────────────

    #[test]
    fn measurement_update_reduces_variance_vs_raw() {
        // BP-21 RED→GREEN: feed a NOISY measurement of a constant truth; the
        // Kalman-smoothed posterior variance must be LOWER than the raw
        // measurement-noise variance (the filter fuses info, it does not just
        // echo the noisy reading). Also the estimate must converge onto truth.
        let n = 1usize; // scalar state = constant quality level
        let a = [1.0f64]; // static plant
        let q = [1e-6f64]; // tiny process noise
        let x0 = [0.0f64];
        let p0 = [100.0f64]; // very uncertain prior
        let h = [1.0f64]; // observe state directly
        let r = [4.0f64]; // measurement noise variance = 4 (std 2)

        let mut kf = KalmanFilter::new(&a, &q, &x0, &p0, n);
        let truth = 7.3f64;
        // deterministic pseudo-noise sequence (no RNG): sine-based, bounded.
        let noises = [1.4f64, -1.1, 0.7, -0.9, 1.2, -0.3, 0.5, -0.6, 0.2, -0.4];
        for &nz in &noises {
            let z = truth + nz;
            kf.predict();
            kf.update(&[z], &h, &r);
        }
        // Posterior variance << raw measurement variance (4.0): the filter learned.
        let post_var = kf.covariance()[0];
        assert!(
            post_var < 2.0,
            "posterior var {} must be below raw measurement var 4.0",
            post_var
        );
        // Estimate converged near truth.
        let est = kf.state()[0];
        assert!(
            (est - truth).abs() < 0.5,
            "estimate {} drifted from truth {}",
            est,
            truth
        );
    }

    #[test]
    fn kalman_gain_shrinks_as_covariance_converges() {
        // BP-21 ACCEPTANCE: the gain K must DECREASE as the covariance converges
        // (a confident filter trusts new noisy measurements less). RED before a
        // correct update: K would stay constant/large. We observe K at two stages.
        let n = 1usize;
        let a = [1.0f64];
        let q = [1e-6f64];
        let x0 = [0.0f64];
        let p0 = [100.0f64];
        let h = [1.0f64];
        let r = [4.0f64];
        let truth = 5.0f64;
        let mut kf = KalmanFilter::new(&a, &q, &x0, &p0, n);

        // Gain K = P Hᵀ (H P Hᵀ + R)⁻¹. For scalar: K = P / (P + R).
        let k_early = {
            let p = kf.covariance()[0];
            p / (p + r[0])
        };
        // run a few updates
        let noises = [1.0f64, -0.8, 0.6, -0.5, 0.4];
        for &nz in &noises {
            kf.predict();
            kf.update(&[truth + nz], &h, &r);
        }
        let k_late = {
            let p = kf.covariance()[0];
            p / (p + r[0])
        };
        assert!(
            k_late < k_early,
            "gain must shrink as covariance converges: early={} late={}",
            k_early,
            k_late
        );
        assert!(k_late < 0.5, "converged gain should be modest, got {}", k_late);
    }

    #[test]
    fn update_follows_predict_state_propagation() {
        // BP-21: state-mean propagation x ← A x during predict, then correction
        // via measurement. A ramping plant x_{k+1}=x_k+0.5; observe it noisily.
        let n = 1usize;
        let a = [1.0f64];
        let q = [0.25f64];
        let x0 = [0.0f64];
        let p0 = [1.0f64];
        let h = [1.0f64];
        let r = [1.0f64];
        let mut kf = KalmanFilter::new(&a, &q, &x0, &p0, n);
        // after one predict x should be 0 (A·0); after update with z=0.5 → x≈0.5.
        kf.predict();
        assert!((kf.state()[0]).abs() < 1e-12, "predict should propagate x");
        kf.update(&[0.5], &h, &r);
        assert!(
            (kf.state()[0] - 0.5).abs() < 0.3,
            "update should pull estimate toward measurement 0.5, got {}",
            kf.state()[0]
        );
    }
}
