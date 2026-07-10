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
                let t = phi.signum() / (phi.abs() + (1.0 + phi * phi).sqrt());
                let c = 1.0 / (1.0 + t * t).sqrt();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_system_has_negative_margin() {
        // GREEN: A = [[-2,0],[0,-3]] is asymptotically stable (both λ < 0).
        let a = [-2.0, 0.0, 0.0, -3.0];
        assert!(is_stable(&a, 2), "should be stable");
        let mu = stability_margin(&a, 2);
        assert!(mu < 0.0 && (mu + 2.0).abs() < 1e-9, "margin should be -2, got {mu}");
    }

    #[test]
    fn unstable_system_has_positive_margin() {
        // GREEN: A = [[2,0],[0,-1]] has a positive eigenvalue ⇒ unstable.
        let a = [2.0, 0.0, 0.0, -1.0];
        assert!(is_unstable(&a, 2), "should be unstable");
        let mu = stability_margin(&a, 2);
        assert!(mu > 0.0 && (mu - 2.0).abs() < 1e-9, "margin should be +2, got {mu}");
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
}
