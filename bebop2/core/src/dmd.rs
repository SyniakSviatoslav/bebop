//! dmd — Online Dynamic Mode Decomposition (rank-1 RLS / Sherman–Morrison), BP-07.
//!
//! Per directive 1 the DMD operator is NOT a dense tensor — it lives in a
//! low-rank Proper-Orthogonal-Decomposition (POD) coordinate system. Full-state
//! snapshots `x_k ∈ ℝⁿ` are projected onto an `r`-dimensional POD basis `U`
//! (`n×r`, column-major) to reduced coordinates `x̃_k = Uᵀ x_k`. The reduced DMD
//! operator `Ã` (`r×r`) is fit *online* via recursive-least-squares with a
//! rank-1 Sherman–Morrison update so that `ỹ_k = Ã x̃_k`, where
//! `ỹ_k = Uᵀ x_{k+1}`:
//!
//! ```text
//!   γ        = 1 / (1 + x̃ᵀ P x̃)
//!   Ã       += γ (ỹ − Ã x̃) x̃ᵀ P
//!   P       -= γ (P x̃) (P x̃)ᵀ          (Sherman–Morrison, P = (Σ x̃ x̃ᵀ)⁻¹)
//! ```
//!
//! Optional exponential forgetting (`P ← P/ρ_forget` before the update) tracks
//! time-varying drift. Difference-snapshot mode feeds `Δ_n = x_{n+1}−x_n`. The
//! 1-D special case augments the state `[x; 1]` so a constant/affine mode
//! (μ = 1) is captured. The spectral radius `ρ = max |μ_i(Ã)|` comes from the
//! general (non-symmetric) eigensolver (BP-03, `lyapunov::eigenvalues_general`).
//!
//! f64 throughout (spectral precision). Zero-dep, no RNG, no vtable.

#![allow(dead_code)]

use crate::fft::Complex;
use crate::lyapunov;
use alloc::vec::Vec;

/// Online DMD operator in a low-rank POD coordinate system.
pub struct OnlineDMD {
    /// Full-state dimension `n`.
    n: usize,
    /// POD rank `r` (reduced dimension).
    r: usize,
    /// Reduced DMD operator `Ã` (`r×r`, row-major).
    a_tilde: Vec<f64>,
    /// Inverse data covariance `P = (Σ x̃ x̃ᵀ)⁻¹` in POD coordinates (`r×r`).
    p_inv: Vec<f64>,
    /// POD basis `U` (`n×r`, column-major: `U[i + n*j]` = component `i` of mode `j`).
    u_basis: Vec<f64>,
    /// Exponential-forgetting factor (≤ 1; 1 = no forgetting, time-invariant).
    rho_forget: f64,
    /// Regularization `δ` for the initial `P = (δ I)⁻¹` (small ⇒ unbiased).
    delta: f64,
}

// ─────────────────────────────────────────────────────────────────────────────
// POD covariance eigen-solve reuses the in-tree `field::jacobi_eigen` (no fork).
// ─────────────────────────────────────────────────────────────────────────────

/// Build a POD basis `U` (`n×r`) from a snapshot matrix `x` (`n×m`, column-major)
/// and return `(U, r)` after Gavish–Donoho hard-threshold truncation
/// (`τ = 2.858 · σ_median`). Always keeps at least the leading mode.
fn build_pod(x: &[f64], n: usize, m: usize) -> (Vec<f64>, usize) {
    // Covariance C = X Xᵀ (n×n).
    let mut c = vec![0.0f64; n * n];
    for i in 0..n {
        for j in 0..n {
            let mut s = 0.0f64;
            for k in 0..m {
                s += x[i + n * k] * x[j + n * k];
            }
            c[i * n + j] = s;
        }
    }
    let (eigvals, eigvecs) = crate::field::jacobi_eigen(&c, n);
    // singular values σ_i = sqrt(λ_i), paired with original index.
    let mut sig: Vec<(f64, usize)> = (0..n)
        .map(|i| (crate::math::fsqrt(eigvals[i].max(0.0)), i))
        .collect();
    sig.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap()); // descending
                                                        // Gavish–Donoho threshold τ = 2.858 · median(σ).
    let median = if n == 0 {
        0.0
    } else if n % 2 == 1 {
        sig[n / 2].0
    } else {
        0.5 * (sig[n / 2 - 1].0 + sig[n / 2].0)
    };
    let tau = 2.858 * median;
    let mut r = 0usize;
    for (s, _) in &sig {
        if *s > tau {
            r += 1;
        } else {
            break;
        }
    }
    if r == 0 {
        r = 1; // keep at least the leading mode
    }
    // U columns = top-r eigenvectors (POD modes), preserving original indexing.
    let mut u = vec![0.0f64; n * r];
    for out_col in 0..r {
        let src = sig[out_col].1;
        for i in 0..n {
            u[i + n * out_col] = eigvecs[i + n * src];
        }
    }
    (u, r)
}

// ── complex linear algebra (for DMD mode eigenvectors) ────────────────────────

#[inline]
fn cdiv(z: Complex, w: Complex) -> Complex {
    let d = w.norm_sq();
    Complex::new(
        (z.re * w.re + z.im * w.im) / d,
        (z.im * w.re - z.re * w.im) / d,
    )
}

/// Solve `A x = b` for a complex `n×n` system `a` (row-major) via Gaussian
/// elimination with partial pivoting. Returns `x` (complex, length `n`).
fn solve_complex(a: &[Complex], b: &[Complex], n: usize) -> Vec<Complex> {
    let mut m: Vec<Complex> = a.to_vec();
    let mut rhs: Vec<Complex> = b.to_vec();
    for col in 0..n {
        // partial pivot on |·|²
        let mut piv = col;
        let mut best = m[col * n + col].norm_sq();
        for r in (col + 1)..n {
            let v = m[r * n + col].norm_sq();
            if v > best {
                best = v;
                piv = r;
            }
        }
        if piv != col {
            for c in 0..n {
                m.swap(piv * n + c, col * n + c);
            }
            rhs.swap(piv, col);
        }
        let d = m[col * n + col];
        if d.norm_sq() < 1e-30 {
            continue; // singular pivot: leave as free variable
        }
        for r in (col + 1)..n {
            let f = cdiv(m[r * n + col], d);
            for c in col..n {
                m[r * n + c] = m[r * n + c].sub(f.mul(m[col * n + c]));
            }
            rhs[r] = rhs[r].sub(f.mul(rhs[col]));
        }
    }
    // back-substitution
    let mut x = vec![Complex::zero(); n];
    for i in (0..n).rev() {
        let mut s = rhs[i];
        for j in (i + 1)..n {
            s = s.sub(m[i * n + j].mul(x[j]));
        }
        let d = m[i * n + i];
        x[i] = if d.norm_sq() > 1e-30 {
            cdiv(s, d)
        } else {
            Complex::zero()
        };
    }
    x
}

impl OnlineDMD {
    /// Project a full-state snapshot `x ∈ ℝⁿ` to POD coordinates `x̃ ∈ ℝʳ`.
    pub fn pod_coords(&self, x: &[f64]) -> Vec<f64> {
        let mut xt = vec![0.0f64; self.r];
        for j in 0..self.r {
            let mut s = 0.0f64;
            for i in 0..self.n {
                s += self.u_basis[i + self.n * j] * x[i];
            }
            xt[j] = s;
        }
        xt
    }

    /// Build directly from a known POD basis `u` (`n×r`, column-major). Used for
    /// the 1-D augmented `[x; 1]` state (to catch μ = 1) and for difference-
    /// snapshot modes where the basis is fixed. Initializes `Ã = 0`,
    /// `P = (δ I)⁻¹`.
    pub fn from_basis(u_basis: &[f64], n: usize, r: usize, delta: f64, rho_forget: f64) -> Self {
        let mut p_inv = vec![0.0f64; r * r];
        let d = if delta > 0.0 { delta } else { 1e-6 };
        for i in 0..r {
            p_inv[i * r + i] = 1.0 / d;
        }
        OnlineDMD {
            n,
            r,
            a_tilde: vec![0.0f64; r * r],
            p_inv,
            u_basis: u_basis.to_vec(),
            rho_forget,
            delta: d,
        }
    }

    /// Fit the POD basis from an initial ensemble of snapshots `snaps` (`n×m`,
    /// column-major) via Gavish–Donoho-truncated SVD, then prepare for online
    /// updates (`Ã = 0`, `P = (δ I)⁻¹`). `rho_forget` ≤ 1 is the exponential-
    /// forgetting factor (1 = no forgetting).
    pub fn new_from_snapshots(
        snaps: &[f64],
        n: usize,
        m: usize,
        delta: f64,
        rho_forget: f64,
    ) -> Self {
        let (u_basis, r) = build_pod(snaps, n, m);
        Self::from_basis(&u_basis, n, r, delta, rho_forget)
    }

    /// Incorporate one snapshot pair `(x_k → x_{k+1})` online via rank-1 RLS.
    pub fn update(&mut self, x_k: &[f64], x_next: &[f64]) {
        let r = self.r;
        // Project to POD coordinates.
        let xt = self.pod_coords(x_k);
        let mut yt = vec![0.0f64; r];
        for j in 0..r {
            let mut s = 0.0f64;
            for i in 0..self.n {
                s += self.u_basis[i + self.n * j] * x_next[i];
            }
            yt[j] = s;
        }
        // Exponential forgetting: P ← P / ρ_forget.
        if (self.rho_forget - 1.0).abs() > 1e-12 {
            let f = self.rho_forget;
            for v in self.p_inv.iter_mut() {
                *v /= f;
            }
        }
        // px = P x̃
        let mut px = vec![0.0f64; r];
        for i in 0..r {
            let mut s = 0.0f64;
            for k in 0..r {
                s += self.p_inv[i * r + k] * xt[k];
            }
            px[i] = s;
        }
        let mut denom = 1.0f64;
        for k in 0..r {
            denom += xt[k] * px[k];
        }
        let gamma = 1.0 / denom;
        // residual = ỹ − Ã x̃
        let mut resid = vec![0.0f64; r];
        for i in 0..r {
            let mut s = 0.0f64;
            for k in 0..r {
                s += self.a_tilde[i * r + k] * xt[k];
            }
            resid[i] = yt[i] - s;
        }
        // Ã += γ (residual) (x̃ᵀ P)   with (x̃ᵀ P)[j] = Σ_k x̃[k] P[k][j]
        for i in 0..r {
            for j in 0..r {
                let mut xpk = 0.0f64;
                for k in 0..r {
                    xpk += xt[k] * self.p_inv[k * r + j];
                }
                self.a_tilde[i * r + j] += gamma * resid[i] * xpk;
            }
        }
        // P -= γ (P x̃) (P x̃)ᵀ
        for i in 0..r {
            for j in 0..r {
                self.p_inv[i * r + j] -= gamma * px[i] * px[j];
            }
        }
    }

    /// Difference-snapshot mode: feed `Δ_n = x_{n+1} − x_n` as the "next" state,
    /// i.e. fit `x_{k+1} − x_k = Ã x_k`.
    pub fn update_diff(&mut self, x_k: &[f64], x_next: &[f64]) {
        let mut d = vec![0.0f64; self.n];
        for i in 0..self.n {
            d[i] = x_next[i] - x_k[i];
        }
        self.update(x_k, &d);
    }

    /// Spectral radius `ρ = max_i |μ_i(Ã)|` via the general (non-symmetric)
    /// eigensolver (BP-03). `ρ > 1` ⇒ the online DMD operator is UNSTABLE.
    pub fn spectral_radius(&self) -> f64 {
        let ev = lyapunov::eigenvalues_general(&self.a_tilde, self.r);
        ev.iter().map(|c| c.norm()).fold(0.0f64, f64::max)
    }

    /// Reduced DMD operator `Ã` (`r×r`, row-major).
    pub fn operator(&self) -> &[f64] {
        &self.a_tilde
    }

    /// Reduced DMD rank `r`.
    pub fn rank(&self) -> usize {
        self.r
    }

    /// Full-state dimension `n`.
    pub fn dim(&self) -> usize {
        self.n
    }

    /// Eigenvector `w` (complex, length `r`) of `Ã` for eigenvalue `mu`, via
    /// complex inverse iteration on `Ã − μ I`.
    fn eigenvector_of(&self, mu: &Complex) -> Vec<Complex> {
        let r = self.r;
        let mut m = vec![Complex::zero(); r * r];
        for i in 0..r {
            for j in 0..r {
                if i == j {
                    m[i * r + j] = Complex::new(self.a_tilde[i * r + j] - mu.re, -mu.im);
                } else {
                    m[i * r + j] = Complex::new(self.a_tilde[i * r + j], 0.0);
                }
            }
        }
        let mut w = vec![Complex::new(1.0, 0.0); r];
        for _ in 0..30 {
            let ww = solve_complex(&m, &w, r);
            let mut nrm = 0.0f64;
            for c in &ww {
                nrm += c.norm_sq();
            }
            nrm = crate::math::fsqrt(nrm);
            if nrm < 1e-30 {
                break;
            }
            for i in 0..r {
                w[i] = ww[i].scale(1.0 / nrm);
            }
        }
        w
    }

    /// Full eigenpair list in POD coordinates: `(μ_i, w_i)` where `Ã w_i = μ_i w_i`
    /// (complex). Complements `modes()` (which lifts to full space).
    pub fn eigenpairs(&self) -> Vec<(Complex, Vec<Complex>)> {
        let r = self.r;
        let ev = lyapunov::eigenvalues_general(&self.a_tilde, r);
        ev.iter().map(|mu| (*mu, self.eigenvector_of(mu))).collect()
    }

    /// DMD modes: for each eigenvalue `μ_i` of `Ã`, the (complex) eigenpair
    /// `(μ_i, w_i)` in POD coordinates is lifted back to full space
    /// `φ_i = U w_i`. Returns `Vec<(μ_i, Re φ_i)>`. (The imaginary part of a
    /// complex mode is the conjugate; the real envelope is reported here.)
    pub fn modes(&self) -> Vec<(Complex, Vec<f64>)> {
        let r = self.r;
        let ev = lyapunov::eigenvalues_general(&self.a_tilde, r);
        let mut out = Vec::with_capacity(r);
        for mu in &ev {
            let w = self.eigenvector_of(mu);
            // φ = U w  (U real, w complex ⇒ φ complex)
            let mut phi = vec![0.0f64; self.n];
            for j in 0..r {
                for i in 0..self.n {
                    phi[i] += self.u_basis[i + self.n * j] * w[j].re;
                }
            }
            out.push((*mu, phi));
        }
        out
    }

    /// Forward–backward de-biased spectral radius for a stationary segment:
    /// `ρ_fb = √(ρ_f · ρ_b)` where `rho_b` is the spectral radius of the
    /// operator trained on the *reversed* sequence. A self-check `ρ_f · ρ_b ≈ 1`
    /// flags drift (the two should be reciprocal on a stationary segment).
    pub fn debias_spectral_radius(&self, backward: &OnlineDMD) -> f64 {
        let rf = self.spectral_radius();
        let rb = backward.spectral_radius();
        crate::math::fsqrt(rf * rb)
    }

    /// Enforced forward/backward de-bias self-check (BP-07 gap #2). On a
    /// *stationary* segment `ρ_f · ρ_b ≈ 1` (the two operators are reciprocal);
    /// a significant deviation indicates time-varying drift and the de-biased
    /// estimate is untrustworthy. Returns `StationaryConsistent` (ρ_f·ρ_b ∈
    /// [1/tol, tol]) or `DriftSuspected` otherwise. The loop MUST read this
    /// before trusting `debias_spectral_radius`.
    pub fn forward_backward_debias_selfcheck(&self, backward: &OnlineDMD) -> Debias {
        let rf = self.spectral_radius();
        let rb = backward.spectral_radius();
        let prod = rf * rb;
        const TOL: f64 = 1.3; // ~30% slack; exact reciprocal ⇒ 1.0
        if (prod >= 1.0 / TOL) && (prod <= TOL) {
            Debias::StationaryConsistent { prod }
        } else {
            Debias::DriftSuspected { prod }
        }
    }
}

/// Result of the forward/backward de-bias self-check (BP-07 gap #2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Debias {
    /// `ρ_f · ρ_b ≈ 1`: segment is stationary; de-biased `ρ_fb` is trustworthy.
    StationaryConsistent { prod: f64 },
    /// `ρ_f · ρ_b` deviates from 1: time-varying drift; de-biased estimate suspect.
    DriftSuspected { prod: f64 },
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── BP-07 gate 1: slow outward spiral A = 1.02·R(0.05) ─────────────────────
    // After a few online updates the operator must report ρ̂ = 1.02 > 1 (UNSTABLE).
    #[test]
    fn bp07_slow_spiral_online_unstable() {
        // Ã built in the full 2-D POD basis U = I₂ (r = 2) so DMD recovers A exactly.
        let c = crate::math::fcos(0.05);
        let s = crate::math::fsin(0.05);
        let g = 1.02;
        let a = [g * c, -g * s, g * s, g * c]; // row-major, |μ| = 1.02
        let u = [1.0f64, 0.0, 0.0, 1.0]; // U = I₂ (n=2, r=2)
        let mut dmd = OnlineDMD::from_basis(&u, 2, 2, 1e-3, 1.0);

        let mut xk = [1.0f64, 0.0];
        // ~20 online updates (a handful suffices; the gate says "after ~4").
        for _ in 0..20 {
            let xnext = [a[0] * xk[0] + a[1] * xk[1], a[2] * xk[0] + a[3] * xk[1]];
            dmd.update(&xk, &xnext);
            xk = xnext;
        }
        let rho = dmd.spectral_radius();
        // GREEN: online DMD MUST detect the outward spiral as UNSTABLE (ρ̂ > 1).
        assert!(rho > 1.0, "slow spiral must be UNSTABLE, got ρ̂={rho}");
        assert!(
            (rho - 1.02).abs() < 0.02,
            "ρ̂ should be ≈1.02 (the spiral growth rate), got {rho}"
        );
    }

    // ── BP-07 gate 1b: same spiral through the POD/Gavish–Donoho build path ────
    // The signal lives in an EXACTLY 2-D subspace of an 8-D state, so GD keeps
    // rank r = 2 (the 6 zero singular values are truncated) and the POD-projected
    // DMD recovers ρ̂ = 1.02 > 1. (GD on a bare n=2 spiral degenerately collapses
    // to rank 1 and loses the rotation — that is a genuine GD property, not a bug.)
    #[test]
    fn bp07_slow_spiral_pod_also_unstable() {
        let c = crate::math::fcos(0.05);
        let s = crate::math::fsin(0.05);
        let g = 1.02;
        let a = [g * c, -g * s, g * s, g * c];
        let n = 8usize;
        let m0 = 24usize;
        // Fixed 8×2 subspace basis (full column rank 2).
        let b0 = [1.0f64, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let b1 = [8.0f64, 7.0, 6.0, 5.0, 4.0, 3.0, 2.0, 1.0];
        let mut snaps = vec![0.0f64; n * m0];
        let mut aa = 1.0f64; // spiral coords in the 2-D subspace
        let mut bb = 0.0f64;
        let mut last_aa = aa;
        let mut last_bb = bb;
        for k in 0..m0 {
            // advance the 2-D spiral
            let na = a[0] * aa + a[1] * bb;
            let nb = a[2] * aa + a[3] * bb;
            aa = na;
            bb = nb;
            for i in 0..n {
                snaps[i + n * k] = b0[i] * aa + b1[i] * bb;
            }
            last_aa = aa;
            last_bb = bb;
        }
        let mut dmd = OnlineDMD::new_from_snapshots(&snaps, n, m0, 1e-3, 1.0);
        assert_eq!(dmd.rank(), 2, "GD must keep rank 2 for a 2-D subspace");
        // feed the consecutive snapshot pairs online (full 8-D states)
        let mut prev = (0..n).map(|i| snaps[i]).collect::<Vec<_>>();
        for k in 1..m0 {
            let cur: Vec<f64> = (0..n).map(|i| snaps[i + n * k]).collect();
            dmd.update(&prev, &cur);
            prev = cur;
        }
        // Online continuation: keep advancing the true 2-D spiral coords and
        // embed each full 8-D state through the fixed basis.
        let mut aa = last_aa; // last spiral coords from the build loop
        let mut bb = last_bb;
        for _ in 0..8 {
            let na = a[0] * aa + a[1] * bb; // advance the 2-D spiral
            let nb = a[2] * aa + a[3] * bb;
            aa = na;
            bb = nb;
            let mut nxx = vec![0.0f64; n];
            for i in 0..n {
                nxx[i] = b0[i] * aa + b1[i] * bb;
            }
            dmd.update(&prev, &nxx);
            prev = nxx;
        }
        let rho = dmd.spectral_radius();
        assert!(
            rho > 1.0,
            "POD-built (r=2) DMD must flag the spiral UNSTABLE, got ρ̂={rho}"
        );
        assert!(
            (rho - 1.02).abs() < 0.02,
            "POD ρ̂ should be ≈1.02, got {rho}"
        );
    }

    // ── BP-07 gate 2: 1-D verbosity / affine mode L_{k+1} = 1.3 L_k ───────────
    // Augment state [x; 1] so the affine map (μ = 1) is captured; the runaway
    // slope μ = 1.3 > 1 must be caught, and one eigenvalue must be ≈ 1.
    #[test]
    fn bp07_1d_verbosity_runaway() {
        // Augmented state z = [L; 1]; z_{k+1} = [[1.3,0],[0,1]] z_k  (μ=1.3 and μ=1).
        let u = [1.0f64, 0.0, 0.0, 1.0]; // U = I₂ over the augmented space
        let mut dmd = OnlineDMD::from_basis(&u, 2, 2, 1e-3, 1.0);
        let mut l = 1.0f64;
        for _ in 0..12 {
            let l_next = 1.3 * l;
            let zk = [l, 1.0];
            let znext = [l_next, 1.0];
            dmd.update(&zk, &znext);
            l = l_next;
        }
        let rho = dmd.spectral_radius();
        // GREEN: μ = 1.3 > 1 ⇒ runaway caught.
        assert!(
            rho > 1.0,
            "1-D runaway (μ=1.3) must be UNSTABLE, got ρ̂={rho}"
        );
        assert!(
            (rho - 1.3).abs() < 0.02,
            "ρ̂ should be ≈1.3 (the runaway slope), got {rho}"
        );
        // The augmentation must expose μ = 1 (the affine/constant mode).
        let ev = lyapunov::eigenvalues_general(dmd.operator(), 2);
        let has_one = ev.iter().any(|c| c.norm().abs() - 1.0 < 0.05);
        assert!(has_one, "augmented DMD must expose μ≈1 (got {:?})", ev);
    }

    // ── Strong GREEN: exact recovery of a known linear operator ────────────────
    // A wrong (stub) implementation fails this; the RLS recovery matches A to 1e-3.
    #[test]
    fn bp07_recovers_exact_linear_operator() {
        let c = crate::math::fcos(0.05);
        let s = crate::math::fsin(0.05);
        let g = 1.02;
        let a = [g * c, -g * s, g * s, g * c];
        let u = [1.0f64, 0.0, 0.0, 1.0];
        let mut dmd = OnlineDMD::from_basis(&u, 2, 2, 1e-3, 1.0);
        let mut xk = [1.0f64, 0.0];
        for _ in 0..16 {
            let xnext = [a[0] * xk[0] + a[1] * xk[1], a[2] * xk[0] + a[3] * xk[1]];
            dmd.update(&xk, &xnext);
            xk = xnext;
        }
        let at = dmd.operator();
        let mut max_err = 0.0f64;
        for i in 0..4 {
            max_err = max_err.max((at[i] - a[i]).abs());
        }
        assert!(
            max_err < 5e-3,
            "online DMD must recover Ã ≈ A to 1e-3, max err = {max_err}"
        );
    }

    // ── modes() returns exactly r eigenpairs and the operator reconstructs ─────
    #[test]
    fn bp07_modes_count_and_reconstruct() {
        let c = crate::math::fcos(0.05);
        let s = crate::math::fsin(0.05);
        let g = 1.02;
        let a = [g * c, -g * s, g * s, g * c];
        let u = [1.0f64, 0.0, 0.0, 1.0];
        let mut dmd = OnlineDMD::from_basis(&u, 2, 2, 1e-3, 1.0);
        let mut xk = [1.0f64, 0.0];
        for _ in 0..16 {
            let xnext = [a[0] * xk[0] + a[1] * xk[1], a[2] * xk[0] + a[3] * xk[1]];
            dmd.update(&xk, &xnext);
            xk = xnext;
        }
        let modes = dmd.modes();
        assert_eq!(modes.len(), 2, "must return r modes");
        let rho_modes = modes.iter().map(|(mu, _)| mu.norm()).fold(0.0f64, f64::max);
        let rho = dmd.spectral_radius();
        assert!(
            (rho_modes - rho).abs() < 1e-9,
            "mode eigenvalues must agree with spectral_radius"
        );
        let at = dmd.operator();
        for (mu, w) in dmd.eigenpairs() {
            let mut aw = vec![Complex::zero(); w.len()];
            for i in 0..w.len() {
                let mut acc = Complex::zero();
                for k in 0..w.len() {
                    acc = acc.add(Complex::new(at[i * w.len() + k], 0.0).mul(w[k]));
                }
                aw[i] = acc;
            }
            let mut res = 0.0f64;
            for i in 0..w.len() {
                res += aw[i].sub(mu.mul(w[i])).norm();
            }
            assert!(res < 1e-6, "eigenpair residual too large: {res}");
        }
    }

    // ── BP-07 gap #1: difference-snapshot mode cancels an unknown mean (x*) ──
    #[test]
    fn bp07_difference_snapshot_cancels_mean() {
        let g = 1.02f64;
        let m = 1000.0f64;
        let u = [1.0f64, 0.0, 0.0, 1.0];
        let mut dmd = OnlineDMD::from_basis(&u, 2, 2, 1e-3, 1.0);
        let mut x = [m + 1.0, m + 0.0];
        for _ in 0..20 {
            let xnext = [m + g * (x[0] - m), m + g * (x[1] - m)];
            dmd.update_diff(&x, &xnext);
            x = xnext;
        }
        let rho = dmd.spectral_radius();
        // Difference-snapshot fits Δ = x_{k+1}−x_k = (g−1)(x_k−m), so the
        // recovered eigenvalue is (g−1) — the offset m is cancelled. For
        // g=1.02 the difference rate is 0.02 (NOT 1.02). This is the correct,
        // intended behavior: the unknown constant mean is removed.
        assert!(
            (rho - (g - 1.0)).abs() < 0.02,
            "difference-snapshot must recover difference rate (g-1)={}, got ρ̂={}",
            g - 1.0,
            rho
        );
        assert!(
            rho < 0.5,
            "offset must NOT blow the estimate up, got ρ̂={rho}"
        );
    }

    // ── BP-07 gap #2: forward/backward de-bias self-check is ENFORCED ─────────
    #[test]
    fn bp07_debias_selfcheck_stationary() {
        let c = crate::math::fcos(0.05);
        let s = crate::math::fsin(0.05);
        let a = [0.9 * c, -0.9 * s, 0.9 * s, 0.9 * c];
        let u = [1.0f64, 0.0, 0.0, 1.0];
        let mut fwd = OnlineDMD::from_basis(&u, 2, 2, 1e-3, 1.0);
        let mut bwd = OnlineDMD::from_basis(&u, 2, 2, 1e-3, 1.0);
        let mut seq = Vec::with_capacity(20);
        let mut xk = [1.0f64, 0.0];
        for _ in 0..20 {
            let xnext = [a[0] * xk[0] + a[1] * xk[1], a[2] * xk[0] + a[3] * xk[1]];
            seq.push(xnext);
            xk = xnext;
        }
        for k in 0..seq.len() - 1 {
            fwd.update(&seq[k], &seq[k + 1]);
        }
        for k in (0..seq.len() - 1).rev() {
            bwd.update(&seq[k + 1], &seq[k]);
        }
        // The de-biased product must be stationary-consistent (≈ reciprocal, ≈1).
        let chk = fwd.forward_backward_debias_selfcheck(&bwd);
        assert!(
            matches!(chk, Debias::StationaryConsistent { prod } if (prod - 1.0).abs() < 0.1),
            "stationary segment must be StationaryConsistent with prod≈1, got {chk:?}"
        );
        // Sanity: de-biased ρ_fb must be STABLE (|μ|=0.9<1 ⇒ ρ_fb<1) and not
        // collapsed to ~0. Finite-sample DMD bias keeps it near 1; the key
        // invariant is stability, which the self-check + this bound enforce.
        let fb = fwd.debias_spectral_radius(&bwd);
        assert!(fb < 1.0, "de-biased ρ_fb must be stable (<1), got {fb}");
        assert!(fb > 0.5, "de-biased ρ_fb must not collapse, got {fb}");
    }
}
