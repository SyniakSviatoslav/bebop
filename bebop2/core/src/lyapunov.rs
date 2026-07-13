//! lyapunov — Lyapunov derivative for stability (spectral).
//!
//! For a linear system ẋ = A x the (continuous) Lyapunov function candidate V(x) = xᵀ P x has
//! derivative  V̇ = xᵀ (Aᵀ P + P A) x. A PD `P` exists (V̇ < 0 everywhere) IFF all eigenvalues of
//! `A` have NEGATIVE real part → the system is ASYMPTOTICALLY STABLE. We compute the SPECTRAL
//! Lyapunov quantity directly from the eigenvalues (the "wave" decomposition of A): the sign of
//! `max Re(λ)` decides stability. This is the Verified-by-Math stability primitive.
//!
//! f64 (eigenvalue precision). Zero-dep, monomorphized, no RNG, no vtable.

#![allow(dead_code)]

use crate::fft::Complex;
use alloc::vec::Vec;

/// Eigenvalues of a real square matrix via the Jacobi method (mirrors the kalman path; self-
/// contained so lyapunov owns its eigen-decomposition). Returns complex eigenvalues (real parts
/// matter for stability). For the reference real systems these are real.
fn eigenvals(a: &[f64], n: usize) -> Vec<Complex> {
    let mut m = a.to_vec();
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
            }
        }
    }
    (0..n).map(|i| Complex::new(m[i * n + i], 0.0)).collect()
}

/// Spectral Lyapunov stability test for ẋ = A x.
///
/// Returns the SIGNED stability margin `μ = max_i Re(λ_i)`:
///   • μ < 0  ⇒ every eigenvalue has negative real part ⇒ ASYMPTOTICALLY STABLE (V̇ < 0).
///   • μ > 0  ⇒ at least one eigenvalue has positive real part ⇒ UNSTABLE (V̇ > 0 along its mode).
///   • μ ≈ 0  ⇒ marginal (neutrally stable / critical).
///
/// The sign is what the architecture requires ("lyapunov sign correct on a known unstable/stable
/// system"). Uses the spectral (eigenvalue) form — no dense P tensor constructed.
pub fn stability_margin(a: &[f64], n: usize) -> f64 {
    let ev = eigenvals(a, n);
    ev.iter().map(|c| c.re).fold(f64::NEG_INFINITY, f64::max)
}

/// Is the linear system ẋ = A x asymptotically stable? (all Re(λ) < 0, with a small tolerance).
#[inline]
pub fn is_stable(a: &[f64], n: usize) -> bool {
    stability_margin(a, n) < -1e-12
}

/// Is it unstable? (some Re(λ) > 0).
#[inline]
pub fn is_unstable(a: &[f64], n: usize) -> bool {
    stability_margin(a, n) > 1e-12
}

/// Discrete-time analogue: x_{k+1} = A x_k is stable iff all |λ| < 1 (spectral radius < 1).
/// Returns `ρ = max |λ|` and its stability flag. This is the resolvent (I - A) invertibility test.
pub fn spectral_radius(a: &[f64], n: usize) -> (f64, bool) {
    let ev = eigenvals(a, n);
    let rho = ev.iter().map(|c| c.norm()).fold(0.0f64, f64::max);
    (rho, rho < 1.0)
}

// ─────────────────────────────────────────────────────────────────────────────
// BP-03 — General real eigensolver (Francis double-shift QR) for NON-symmetric A.
//
// The symmetric `eigenvals`/`spectral_radius` above are correct for the
// symmetric (A+Aᵀ)/2 path (Path A, Kalman). For a general real (often
// non-symmetric) DMD operator Ã with complex eigenvalues, they silently
// misreport. `eigenvalues_general` computes the COMPLEX spectrum of a general
// real r×r matrix via Hessenberg reduction → Francis double-shift QR → real
// Schur form, then reads 1×1 (real) and 2×2 (complex pair) blocks.
// ─────────────────────────────────────────────────────────────────────────────

/// Eigenvalues of a real 2×2 block [[a,b],[c,d]] in closed form.
/// τ = a+d (trace), δ = ad−bc (det). disc = (τ/2)² − δ.
///   disc ≥ 0 → two real μ = τ/2 ± √disc.
///   disc < 0 → complex conjugate pair μ = τ/2 ± i√(−disc); |μ| = √δ.
fn eig2(a: f64, b: f64, c: f64, d: f64) -> Vec<Complex> {
    let tau = a + d;
    let det = a * d - b * c;
    let disc = tau * tau / 4.0 - det;
    if disc >= 0.0 {
        let s = crate::math::fsqrt(disc);
        vec![
            Complex::new(tau / 2.0 + s, 0.0),
            Complex::new(tau / 2.0 - s, 0.0),
        ]
    } else {
        let s = crate::math::fsqrt(-disc);
        vec![Complex::new(tau / 2.0, s), Complex::new(tau / 2.0, -s)]
    }
}

/// Reduce a real matrix to upper Hessenberg form by Householder similarities.
/// Returns the Hessenberg matrix (row-major, n×n).
fn to_hessenberg(a: &[f64], n: usize) -> Vec<f64> {
    let mut h = a.to_vec();
    for k in 0..n.saturating_sub(2) {
        // Largest subdiagonal magnitude in column k (rows k+1..n-1).
        let mut mx = 0.0f64;
        for i in k + 1..n {
            let t = h[i * n + k].abs();
            if t > mx {
                mx = t;
            }
        }
        if mx == 0.0 {
            continue; // column already deflated
        }
        // Householder vector u over rows k+1..n-1: u_i = h[i][k]/mx, then u_{k+1} += sign·‖u‖.
        let mut u = vec![0.0f64; n];
        let mut s2 = 0.0f64;
        for i in k + 1..n {
            let x = h[i * n + k] / mx;
            u[i] = x;
            s2 += x * x;
        }
        let mut sigma = crate::math::fsqrt(s2);
        if u[k + 1] < 0.0 {
            sigma = -sigma;
        }
        let head = u[k + 1] + sigma;
        u[k + 1] = head;
        let mut unorm2 = head * head;
        for i in k + 2..n {
            unorm2 += u[i] * u[i];
        }
        if unorm2 == 0.0 {
            continue;
        }
        let c = 2.0 / unorm2; // reflector P = I − c·u uᵀ
                              // Left: H ← P H  (zeroes column k below the subdiagonal).
        for j in 0..n {
            let mut dot = 0.0f64;
            for i in k + 1..n {
                dot += u[i] * h[i * n + j];
            }
            if dot != 0.0 {
                for i in k + 1..n {
                    h[i * n + j] -= c * dot * u[i];
                }
            }
        }
        // Right: H ← H P  (restores zeros in the rows).
        for i in 0..n {
            let mut dot = 0.0f64;
            for j in k + 1..n {
                dot += h[i * n + j] * u[j];
            }
            if dot != 0.0 {
                for j in k + 1..n {
                    h[i * n + j] -= c * dot * u[j];
                }
            }
        }
    }
    h
}

/// Implicit double-shift (Francis) QR iteration on an upper-Hessenberg matrix `h`
/// (row-major, n×n), returning its n eigenvalues as `Complex`. This is the classic
/// EISPACK `hqr` real-Schur iteration (the well-validated reference), ported 1:1.
/// It handles complex-conjugate pairs correctly by reading 2×2 blocks. `n` small (≤ ~32).
/// Does NOT compute eigenvectors. Never panics (gives up gracefully, leaving remaining
/// diagonal entries as real, if the iteration limit is hit).
fn hqr_eigenvalues(h: &mut [f64], n: usize) -> Vec<Complex> {
    let mut w = vec![Complex::zero(); n];
    let at = |h: &[f64], i: usize, j: usize| h[i * n + j];
    let mut nn = n as isize - 1;
    let mut t = 0.0f64;
    let max_iter = 60 * n;
    while nn >= 0 {
        let mut its = 0usize;
        loop {
            // Look for a small subdiagonal element to split off a 1×1 block.
            let mut l = nn;
            while l > 0 {
                let s = (at(h, (l - 1) as usize, (l - 1) as usize)).abs()
                    + (at(h, l as usize, l as usize)).abs();
                let s = if s == 0.0 { 1.0 } else { s };
                if (at(h, l as usize, (l - 1) as usize)).abs() <= 1e-14 * s {
                    break;
                }
                l -= 1;
            }
            let ni = nn as usize;
            let x = at(h, ni, ni);
            if l == nn {
                // One real root.
                w[ni] = Complex::new(x + t, 0.0);
                nn -= 1;
                break;
            }
            let y = at(h, ni - 1, ni - 1);
            let ww = at(h, ni, ni - 1) * at(h, ni - 1, ni);
            if l == nn - 1 {
                // Two roots: 2×2 block → closed form on the shifted block.
                let e = eig2(y, at(h, ni - 1, ni), at(h, ni, ni - 1), x);
                w[ni - 1] = Complex::new(e[0].re + t, e[0].im);
                w[ni] = Complex::new(e[1].re + t, e[1].im);
                nn -= 2;
                break;
            }
            if its >= max_iter {
                // Give up gracefully: emit remaining diagonal as real (no panic).
                for i in 0..=ni {
                    if w[i].re == 0.0 && w[i].im == 0.0 {
                        w[i] = Complex::new(at(h, i, i) + t, 0.0);
                    }
                }
                return w;
            }
            // Form (exceptional) shift.
            let mut p = 0.0f64;
            let mut q = 0.0f64;
            let mut r = 0.0f64;
            let mut xx = x;
            let mut yy = y;
            let mut wwv = ww;
            if its == 10 || its == 20 {
                t += xx;
                for i in 0..=ni {
                    h[i * n + i] -= xx;
                }
                let s = (at(h, ni, ni - 1)).abs() + (at(h, ni - 1, ni - 2)).abs();
                xx = 0.75 * s;
                yy = xx;
                wwv = -0.4375 * s * s;
            }
            its += 1;
            // Look for two consecutive small subdiagonal elements.
            let mut m = nn - 2;
            while m >= l {
                let mi = m as usize;
                let z = at(h, mi, mi);
                let rr = xx - z;
                let ss = yy - z;
                p = (rr * ss - wwv) / at(h, mi + 1, mi) + at(h, mi, mi + 1);
                q = at(h, mi + 1, mi + 1) - z - rr - ss;
                r = at(h, mi + 2, mi + 1);
                let s = p.abs() + q.abs() + r.abs();
                p /= s;
                q /= s;
                r /= s;
                if m == l {
                    break;
                }
                let a1 = (at(h, mi, mi - 1)).abs() * (q.abs() + r.abs());
                let a2 = p.abs()
                    * ((at(h, mi - 1, mi - 1)).abs() + z.abs() + (at(h, mi + 1, mi + 1)).abs());
                if a1 <= 1e-14 * a2 {
                    break;
                }
                m -= 1;
            }
            let mi = m as usize;
            for i in (mi + 2)..=ni {
                h[i * n + (i - 2)] = 0.0;
                if i != mi + 2 {
                    h[i * n + (i - 3)] = 0.0;
                }
            }
            // Double QR sweep on rows l..=nn.
            let mut k = mi;
            while k <= ni - 1 {
                let notlast = k != ni - 1;
                if k != mi {
                    p = at(h, k, k - 1);
                    q = at(h, k + 1, k - 1);
                    r = if notlast { at(h, k + 2, k - 1) } else { 0.0 };
                    xx = p.abs() + q.abs() + r.abs();
                    if xx != 0.0 {
                        p /= xx;
                        q /= xx;
                        r /= xx;
                    }
                }
                if xx == 0.0 {
                    break;
                }
                let mut s = (p * p + q * q + r * r).sqrt();
                if p < 0.0 {
                    s = -s;
                }
                if s == 0.0 {
                    k += 1;
                    continue;
                }
                if k == mi {
                    if l != m {
                        h[k * n + (k - 1)] = -at(h, k, k - 1);
                    }
                } else {
                    h[k * n + (k - 1)] = -s * xx;
                }
                p += s;
                let xr = p / s;
                let yr = q / s;
                let zr = r / s;
                q /= p;
                r /= p;
                // Row modification.
                for j in k..=ni {
                    let mut pp = at(h, k, j) + q * at(h, k + 1, j);
                    if notlast {
                        pp += r * at(h, k + 2, j);
                        h[(k + 2) * n + j] -= pp * zr;
                    }
                    h[(k + 1) * n + j] -= pp * yr;
                    h[k * n + j] -= pp * xr;
                }
                // Column modification.
                let jmax = if nn < (k + 3) as isize {
                    nn as usize
                } else {
                    k + 3
                };
                for i in l as usize..=jmax {
                    let mut pp = xr * at(h, i, k) + yr * at(h, i, k + 1);
                    if notlast {
                        pp += zr * at(h, i, k + 2);
                        h[i * n + (k + 2)] -= pp * r;
                    }
                    h[i * n + (k + 1)] -= pp * q;
                    h[i * n + k] -= pp;
                }
                k += 1;
            }
        }
    }
    w
}

/// Complex eigenvalues of a general real n×n matrix (n small, ≤ ~32).
///
/// n==1 trivial; n==2 closed form; n>2 Hessenberg reduction (Householder) →
/// implicit double-shift QR (EISPACK `hqr`) → eigenvalues read off 1×1 (real)
/// and 2×2 (complex-conjugate pair) blocks. Correct for NON-symmetric matrices
/// (complex/rotational modes), unlike the symmetric `eigenvals` above. Never
/// panics on defective/repeated eigenvalues.
pub fn eigenvalues_general(a: &[f64], n: usize) -> Vec<Complex> {
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![Complex::new(a[0], 0.0)];
    }
    if n == 2 {
        return eig2(a[0], a[1], a[2], a[3]);
    }
    let mut h = to_hessenberg(a, n);
    hqr_eigenvalues(&mut h, n)
}

/// Spectral radius from the general path (for non-symmetric Ã).
/// Returns `(ρ = max|μ|, ρ < 1)` — i.e. `(max modulus, is-contracting)`.
pub fn spectral_radius_general(a: &[f64], n: usize) -> (f64, bool) {
    let ev = eigenvalues_general(a, n);
    let rho = ev.iter().map(|c| c.norm()).fold(0.0f64, f64::max);
    (rho, rho < 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_system_has_negative_margin() {
        // GREEN: A = [[-2,0],[0,-3]] is asymptotically stable (both λ < 0).
        let a = [-2.0, 0.0, 0.0, -3.0];
        assert!(is_stable(&a, 2), "should be stable");
        let mu = stability_margin(&a, 2);
        assert!(
            mu < 0.0 && (mu + 2.0).abs() < 1e-9,
            "margin should be -2, got {mu}"
        );
    }

    #[test]
    fn unstable_system_has_positive_margin() {
        // GREEN: A = [[2,0],[0,-1]] has a positive eigenvalue ⇒ unstable.
        let a = [2.0, 0.0, 0.0, -1.0];
        assert!(is_unstable(&a, 2), "should be unstable");
        let mu = stability_margin(&a, 2);
        assert!(
            mu > 0.0 && (mu - 2.0).abs() < 1e-9,
            "margin should be +2, got {mu}"
        );
    }

    #[test]
    fn lyapunov_red_breaks_on_sign_flip() {
        // RED+GREEN: flipping the sign of A must flip the stability verdict.
        let stable = [-2.0, 0.0, 0.0, -3.0];
        let unstable = [2.0, 0.0, 0.0, 3.0];
        assert!(is_stable(&stable, 2));
        assert!(is_unstable(&unstable, 2));
        assert!(stability_margin(&stable, 2) < 0.0);
        assert!(stability_margin(&unstable, 2) > 0.0);
    }

    #[test]
    fn discrete_spectral_radius() {
        // GREEN: A = [[0.5,0],[0,0.9]] ⇒ ρ = 0.9 < 1 ⇒ discrete-stable.
        let a = [0.5, 0.0, 0.0, 0.9];
        let (rho, stable) = spectral_radius(&a, 2);
        assert!(stable, "ρ={rho} must be < 1");
        assert!((rho - 0.9).abs() < 1e-9, "ρ should be 0.9, got {rho}");
    }

    #[test]
    fn discrete_unstable_radius() {
        // GREEN: A = [[1.2,0],[0,0.3]] ⇒ ρ = 1.2 > 1 ⇒ discrete-unstable.
        let a = [1.2, 0.0, 0.0, 0.3];
        let (rho, stable) = spectral_radius(&a, 2);
        assert!(!stable, "ρ={rho} must be > 1");
        assert!((rho - 1.2).abs() < 1e-9, "ρ should be 1.2, got {rho}");
    }

    // ── BP-03: general (non-symmetric) eigensolver RED→GREEN gate ────────────────

    #[test]
    fn bp03_swap_2cycle_general_flags_noncontraction() {
        // Ã = [[0,1],[1,0]] (swap) has eigenvalues ±1 ⇒ ρ = 1 (non-contracting, an
        // attracting 2-cycle). The GENERAL path must report ρ = 1.0 exactly.
        let a = [0.0, 1.0, 1.0, 0.0];
        let (rho, stable) = spectral_radius_general(&a, 2);
        assert!(
            (rho - 1.0).abs() < 1e-12,
            "general ρ must be 1.0 for swap, got {rho}"
        );
        assert!(!stable, "swap 2-cycle must NOT be flagged contracting");
    }

    #[test]
    fn bp03_slow_spiral_general_complex_pair() {
        // Ã = 1.02·R(0.05): a slow outward spiral. Eigenvalues are a complex pair with
        // |μ| = √det = 1.02 > 1 ⇒ UNSTABLE. The symmetric Jacobi would misreport this.
        let c = crate::math::fcos(0.05);
        let s = crate::math::fsin(0.05);
        let g = 1.02;
        let a = [g * c, -g * s, g * s, g * c];
        let ev = eigenvalues_general(&a, 2);
        // Complex pair present.
        assert!(
            ev[0].im.abs() > 1e-9,
            "expected complex pair, got real {:?}",
            ev
        );
        let (rho, stable) = spectral_radius_general(&a, 2);
        assert!((rho - 1.02).abs() < 1e-6, "|μ| must be 1.02, got {rho}");
        assert!(!stable, "outward spiral (ρ=1.02) must be UNSTABLE");
    }

    #[test]
    fn bp03_general_matches_symmetric_regression() {
        // On a symmetric matrix both paths must agree (max real eigenvalue).
        // A = [[2,1],[1,2]] ⇒ eigenvalues 3 and 1.
        let a = [2.0, 1.0, 1.0, 2.0];
        let ev = eigenvalues_general(&a, 2);
        let mut reals = [ev[0].re, ev[1].re];
        reals.sort_by(|x, y| x.partial_cmp(y).unwrap());
        assert!(
            (reals[0] - 1.0).abs() < 1e-9,
            "min λ should be 1, got {}",
            reals[0]
        );
        assert!(
            (reals[1] - 3.0).abs() < 1e-9,
            "max λ should be 3, got {}",
            reals[1]
        );
    }

    #[test]
    fn bp03_general_3x3_and_no_panic_on_defective() {
        // 3×3 upper-triangular ⇒ eigenvalues are the diagonal {4, -1, 2}, ρ = 4.
        let a = [4.0, 1.0, 5.0, 0.0, -1.0, 2.0, 0.0, 0.0, 2.0];
        let (rho, _) = spectral_radius_general(&a, 3);
        assert!((rho - 4.0).abs() < 1e-6, "ρ should be 4, got {rho}");
        // Defective (Jordan block) [[2,1],[0,2]] — repeated eigenvalue 2, must not panic.
        let def = [2.0, 1.0, 0.0, 2.0];
        let ev = eigenvalues_general(&def, 2);
        assert!((ev[0].re - 2.0).abs() < 1e-9 && (ev[1].re - 2.0).abs() < 1e-9);
    }

    #[test]
    fn bp03_general_agrees_symmetric_3x3() {
        // The new (non-symmetric) path must agree with the symmetric Jacobi path on a
        // symmetric 3×3 — here a tridiagonal matrix with exact eigenvalues 2±√2, 2.
        let a = [2.0, 1.0, 0.0, 1.0, 2.0, 1.0, 0.0, 1.0, 2.0];
        let mut g: Vec<f64> = eigenvalues_general(&a, 3).iter().map(|c| c.re).collect();
        let mut s: Vec<f64> = eigenvals(&a, 3).iter().map(|c| c.re).collect();
        g.sort_by(|x, y| x.partial_cmp(y).unwrap());
        s.sort_by(|x, y| x.partial_cmp(y).unwrap());
        for i in 0..3 {
            assert!(
                (g[i] - s[i]).abs() < 1e-6,
                "λ{} mismatch: general {} vs symmetric {}",
                i,
                g[i],
                s[i]
            );
        }
        // All eigenvalues real (symmetric matrix).
        for c in eigenvalues_general(&a, 3) {
            assert!(c.im.abs() < 1e-6, "symmetric 3x3 must be real, got {:?}", c);
        }
    }

    #[test]
    fn bp03_dense_nonsymmetric_5x5_recovers_ground_truth() {
        // Regression for the broken-Francis-QR false-green: a FULLY DENSE, non-Hessenberg
        // 5×5 with genuine complex-conjugate eigenvalue pairs. Built from a known
        // eigendecomposition A = V·Λ·V⁻¹ so the exact spectrum is known by construction:
        //   Λ = diag(0.3+0.7i, 0.3−0.7i, −0.5+1.1i, −0.5−1.1i, 0.9).
        // V is a fixed dense real invertible matrix; V⁻¹ is computed IN-CODE (Gaussian
        // elimination) so the ground truth is exact — no hand-typed inverse, no rounding.
        // If the QR path is broken it returns all-real / trace-inconsistent garbage and
        // this test fails.

        // Dense, non-symmetric, invertible basis V (columns).
        let v = [
            [2.0, 1.0, 0.0, 1.0, 0.5],
            [1.0, 3.0, 1.0, 0.0, 0.3],
            [0.5, 1.0, 2.0, 1.0, 0.1],
            [0.0, 0.5, 1.0, 3.0, 0.4],
            [0.2, 0.1, 0.3, 0.2, 2.0],
        ];
        // In-code 5×5 inverse via Gauss–Jordan (exact up to f64).
        let mut m = [[0.0f64; 10]; 5];
        for i in 0..5 {
            for j in 0..5 {
                m[i][j] = v[i][j];
            }
            m[i][5 + i] = 1.0;
        }
        for col in 0..5 {
            // pivot
            let mut piv = col;
            let mut best = m[col][col].abs();
            for r in (col + 1)..5 {
                if m[r][col].abs() > best {
                    best = m[r][col].abs();
                    piv = r;
                }
            }
            m.swap(col, piv);
            let d = m[col][col];
            for j in 0..10 {
                m[col][j] /= d;
            }
            for r in 0..5 {
                if r != col {
                    let f = m[r][col];
                    for j in 0..10 {
                        m[r][j] -= f * m[col][j];
                    }
                }
            }
        }
        let mut vinv = [[0.0f64; 5]; 5];
        for i in 0..5 {
            for j in 0..5 {
                vinv[i][j] = m[i][5 + j];
            }
        }
        // Sanity: V·V⁻¹ ≈ I (confirms the inverse is correct).
        for i in 0..5 {
            for j in 0..5 {
                let mut s = 0.0f64;
                for k in 0..5 {
                    s += v[i][k] * vinv[k][j];
                }
                let exp = if i == j { 1.0 } else { 0.0 };
                assert!(
                    (s - exp).abs() < 1e-9,
                    "V·V⁻¹ should be I (entry {i},{j}={s})"
                );
            }
        }

        // Build A = V·Λ·V⁻¹ using the real 2×2 block representation of each complex pair.
        // Λ acts on V's columns as: pair (c0,c1) with val re±im i, real column c4 val 0.9.
        let re = [0.3f64, -0.5, 0.9];
        let im = [0.7f64, 1.1, 0.0];
        let mut a = [[0.0f64; 5]; 5];
        // real eigenpair (col 4, val 0.9)
        for i in 0..5 {
            for j in 0..5 {
                a[i][j] += v[i][4] * 0.9 * vinv[4][j];
            }
        }
        // complex pair 0: cols (0,1), val 0.3±0.7i  → block [[re,-im],[im,re]]
        let (re0, im0) = (re[0], im[0]);
        for i in 0..5 {
            for j in 0..5 {
                a[i][j] += v[i][0] * (re0 * vinv[0][j] + im0 * vinv[1][j])
                    + v[i][1] * (-im0 * vinv[0][j] + re0 * vinv[1][j]);
            }
        }
        // complex pair 1: cols (2,3), val -0.5±1.1i
        let (re1, im1) = (re[1], im[1]);
        for i in 0..5 {
            for j in 0..5 {
                a[i][j] += v[i][2] * (re1 * vinv[2][j] + im1 * vinv[3][j])
                    + v[i][3] * (-im1 * vinv[2][j] + re1 * vinv[3][j]);
            }
        }
        // Confirm A is dense (no reliance on triangular shortcut).
        let mut zeros = 0;
        for i in 0..5 {
            for j in 0..5 {
                if a[i][j] == 0.0 {
                    zeros += 1;
                }
            }
        }
        assert!(
            zeros <= 3,
            "A must be genuinely dense (too few nonzeros: {zeros})"
        );

        let mut flat = [0.0f64; 25];
        for i in 0..5 {
            for j in 0..5 {
                flat[i * 5 + j] = a[i][j];
            }
        }
        let ev = eigenvalues_general(&flat, 5);
        assert_eq!(ev.len(), 5);

        // Ground-truth spectrum (exact, by construction): two conjugate pairs + one real.
        let truth = [
            (0.3f64, 0.7f64),
            (0.3f64, -0.7f64),
            (-0.5f64, 1.1f64),
            (-0.5f64, -1.1f64),
            (0.9f64, 0.0f64),
        ];
        // Match each computed eigenvalue to its nearest ground-truth partner (Hungarian-lite:
        // greedy by closest complex distance), asserting every one is within tolerance.
        let mut used = [false; 5];
        for c in &ev {
            let mut best_i = 0usize;
            let mut best_d = f64::MAX;
            for (k, (tr, ti)) in truth.iter().enumerate() {
                if used[k] {
                    continue;
                }
                let d = (c.re - tr).powi(2) + (c.im - ti).powi(2);
                if d < best_d {
                    best_d = d;
                    best_i = k;
                }
            }
            used[best_i] = true;
            assert!(
                best_d.sqrt() < 1e-6,
                "eigenvalue {:?} not matched to any ground-truth (nearest dist {:.2e})",
                c,
                best_d.sqrt()
            );
        }
        // Trace consistency: Σλ ≈ trace(A) (a broken QR would violate this).
        let trace_a: f64 = (0..5).map(|i| a[i][i]).sum();
        let sum_ev: f64 = ev.iter().map(|c| c.re).sum();
        assert!(
            (sum_ev - trace_a).abs() < 1e-6,
            "eigenvalues not trace-consistent: Σλ={} trace(A)={}",
            sum_ev,
            trace_a
        );
        // Genuine complex pair must appear (the broken QR returned all-real).
        let n_complex = ev.iter().filter(|c| c.im.abs() > 1e-6).count();
        assert!(
            n_complex >= 2,
            "expected complex-conjugate pairs, got {:?}",
            ev
        );
    }
}
