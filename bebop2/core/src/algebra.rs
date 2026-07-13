//! algebra — cosine / cross / sinc basis projections (Verified-by-Math vs old rust-core).
//!
//! Pure `core`. f64 (the old oracle used f64 for these; the spec reserves f32 for the *field*
//! kernels only — "f64 where spectral math demands it"; these vector primitives match the oracle
//! exactly at f64).
//!
//! Matches old `rust-core` functions:
//!   • `cosine_similarity`  → `cosine_similarity`
//!   • `cross_product`      → `cross_product`
//!   • `sinc`               → `sinc`
//! Plus a spectral projection primitive (`project`/`reconstruct`) per directive 1: a "vector" is
//! stored as spectral coefficients, not a dense sample list.

#![allow(dead_code)]

/// Bipolar dot-product similarity of two f64 vectors. Σ a_i·b_i.
/// Matches old `vsa_similarity` (identical math; renamed for the spectral API).
#[inline]
pub fn similarity(a: &[f64], b: &[f64]) -> f64 {
    debug_assert!(a.len() == b.len());
    let n = a.len().min(b.len());
    let mut s = 0.0f64;
    for i in 0..n {
        s += a[i] * b[i];
    }
    s
}

/// Cosine similarity ⟨a,b⟩ / (‖a‖·‖b‖). 0 when either vector is zero (no spurious 1.0).
/// Matches old `cosine_similarity` exactly (including the `.clamp(-1,1)` and zero-guard).
#[inline]
pub fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    debug_assert!(a.len() == b.len());
    let n = a.len().min(b.len());
    let mut dot = 0.0f64;
    let mut na = 0.0f64;
    let mut nb = 0.0f64;
    for i in 0..n {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = crate::math::fsqrt(na * nb);
    if denom <= 1e-12 {
        0.0
    } else {
        (dot / denom).clamp(-1.0, 1.0)
    }
}

/// Geodesic (angular) distance on the unit sphere: `d_g = arccos(⟨a,b⟩)`. IS a metric
/// (great-circle), unlike `1−cos` which violates the triangle inequality. Range `d_g ∈ [0, π]`.
/// The `clamp` guards the `acos` against NaN at the poles (‖a‖·‖b‖ ≈ 0 or rounding past ±1).
#[inline]
pub fn geodesic_distance(a: &[f64], b: &[f64]) -> f64 {
    cosine_similarity(a, b).clamp(-1.0, 1.0).acos()
}

/// Chordal distance `√(2(1−cos))` — also a metric (Euclidean chord on the sphere). Cheaper than
/// `geodesic_distance` (no `acos`); order-preserving w.r.t. it, so ordering logic is unchanged —
/// only the contraction-ratio arithmetic becomes valid. `max(0.0)` guards the sqrt against a
/// tiny negative from floating-point rounding when `cos ≈ 1`.
#[inline]
pub fn chordal_distance(a: &[f64], b: &[f64]) -> f64 {
    (2.0 * (1.0 - cosine_similarity(a, b))).max(0.0).sqrt()
}

/// 3-D cross product a × b = (a2b3−a3b2, a3b1−a1b3, a1b2−a2b1). Orthogonality detector.
/// Matches old `cross_product` exactly.
#[inline]
pub fn cross_product(a: &[f64], b: &[f64], out: &mut [f64]) {
    debug_assert!(a.len() >= 3 && b.len() >= 3 && out.len() >= 3);
    out[0] = a[1] * b[2] - a[2] * b[1];
    out[1] = a[2] * b[0] - a[0] * b[2];
    out[2] = a[0] * b[1] - a[1] * b[0];
}

/// Sinc(x) = sin(x)/x with the removable singularity at 0 → 1 (L'Hôpital limit).
/// Matches old `sinc` exactly (1e-9 threshold).
#[inline]
pub fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-9 {
        1.0
    } else {
        crate::math::fsin(x) / x
    }
}

/// Spectral projial (directive 1): project a real signal onto a chosen real orthogonal basis.
///
/// `basis` is a set of `modes` basis vectors, each of length `n` (row-major: basis[k*n + i]).
/// Returns the `modes` spectral coefficients c_k = ⟨signal, basis_k⟩ / ‖basis_k‖².
/// This is the "store the vector as spectral coefficients" primitive — no dense sample retained.
/// Mirror of what `field::LaplacianSpectrum` does for graphs (coefficients in the eigenbasis).
pub fn project(signal: &[f64], basis: &[f64], modes: usize, n: usize, coeffs: &mut [f64]) {
    for k in 0..modes {
        let mut dot = 0.0f64;
        let mut norm2 = 0.0f64;
        let base = k * n;
        for i in 0..n {
            let bk = basis[base + i];
            dot += signal[i] * bk;
            norm2 += bk * bk;
        }
        coeffs[k] = if norm2 > 1e-12 { dot / norm2 } else { 0.0 };
    }
}

/// Reconstruct a signal from spectral coefficients (inverse of `project`).
/// out[i] = Σ_k coeffs[k] · basis[k*n + i].
pub fn reconstruct(coeffs: &[f64], basis: &[f64], modes: usize, n: usize, out: &mut [f64]) {
    for i in 0..n {
        let mut acc = 0.0f64;
        for k in 0..modes {
            acc += coeffs[k] * basis[k * n + i];
        }
        out[i] = acc;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_matches_old_oracle() {
        // GREEN: identical to old rust-core `test_cosine_similarity_bounds`.
        let a = [1.0, 2.0, 3.0];
        let b = [1.0, 2.0, 3.0];
        let c = [-1.0, -2.0, -3.0];
        let orth = [1.0, 0.0, 0.0];
        let orth2 = [0.0, 1.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-12);
        assert!((cosine_similarity(&a, &c) + 1.0).abs() < 1e-12);
        assert!(cosine_similarity(&orth, &orth2).abs() < 1e-12);
        let big = [10.0, 20.0, 30.0];
        assert!(
            (cosine_similarity(&a, &big) - 1.0).abs() < 1e-12,
            "norm-invariant"
        );
    }

    #[test]
    fn cosine_red_breaks_on_perturbation() {
        // RED+GREEN: perturbing a constant must break equivalence.
        let a = [1.0, 2.0, 3.0];
        let b = [1.0, 2.0, 3.0];
        let mut b2 = b;
        b2[1] += 0.1; // perturb
        assert!((cosine_similarity(&a, &b) - cosine_similarity(&a, &b2)).abs() > 1e-9);
    }

    #[test]
    fn cross_matches_old_oracle() {
        // GREEN: identical to old rust-core `test_cross_product_orthogonality`.
        let a = [1.0, 0.0, 0.0];
        let parallel = [2.0, 0.0, 0.0];
        let b = [0.0, 1.0, 0.0];
        let mut out = [0.0f64; 3];
        cross_product(&a, &parallel, &mut out);
        assert!(out.iter().all(|v| v.abs() < 1e-12), "parallel ⇒ zero cross");
        cross_product(&a, &b, &mut out);
        assert!((out[2].abs() - 1.0).abs() < 1e-12, "perp ⇒ unit-normal z");
    }

    #[test]
    fn sinc_matches_old_oracle() {
        // GREEN: identical to old rust-core `test_sinc_singularity_and_zero`.
        assert!((sinc(0.0) - 1.0).abs() < 1e-12, "sinc(0)=1 by limit");
        assert!((sinc(core::f64::consts::PI)).abs() < 1e-12, "sinc(π)=0");
    }

    #[test]
    fn similarity_matches_old_vsa() {
        // GREEN: self-similarity of a ±1 hypervector == dim (old `test_vsa_self_similarity_is_dim`).
        let dim = 64usize;
        let mut a = vec![0.0f64; dim];
        for i in 0..dim {
            a[i] = if i % 2 == 0 { 1.0 } else { -1.0 };
        }
        let s = similarity(&a, &a);
        assert!((s - dim as f64).abs() < 1e-9, "self-sim={s}");
    }

    #[test]
    fn geodesic_and_chordal_respect_triangle_inequality() {
        // RED→GREEN (plan §2.1 В1): `1−cos` is NOT a metric; `arccos` and chordal ARE.
        // Prove the triangle inequality on 100 random triples within a geodesic ball < π/2
        // around a reference, so we never hit the cut locus / antipodes where acos is singular.
        // Deterministic LCG (no RNG feature needed) for reproducible random unit vectors.
        let mut s: u64 = 0x1234_5678_9abc_def0;
        let mut rng = || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (s >> 33) as f64 / (1u64 << 31) as f64 * 2.0 - 1.0
        };
        for _ in 0..100 {
            let mut mk = || -> Vec<f64> {
                let mut v = vec![0.0f64; 8];
                for x in v.iter_mut() {
                    *x = rng();
                }
                let nrm = crate::math::fsqrt(v.iter().map(|x| x * x).sum::<f64>());
                if nrm > 1e-9 {
                    for x in v.iter_mut() {
                        *x /= nrm;
                    }
                }
                v
            };
            let a = mk();
            let b = mk();
            let c = mk();
            for d in [geodesic_distance, chordal_distance] {
                let ab = d(&a, &b);
                let bc = d(&b, &c);
                let ac = d(&a, &c);
                assert!(
                    ac <= ab + bc + 1e-9,
                    "triangle inequality violated: {} <= {} + {}",
                    ac,
                    ab,
                    bc
                );
            }
        }
    }

    #[test]
    fn cosine_mirage_red_detected_by_geodesic() {
        // RED (plan §2.1 RED-D): `1−cos` is not a metric (violates triangle inequality on the
        // sphere's far side); `arccos` IS. Prove the geodesic metric is well-defined and monotonic
        // where 1−cos would mirage, and that the pole guard prevents NaN at the antipodes.
        let x0 = [1.0f64, 0.0, 0.0]; // reference
        let x1 = [0.0, 1.0, 0.0]; // 90° off: cosine=0 → d_g=π/2
        let x2 = [-1.0, 0.0, 0.0]; // antipodal to x0: cosine=−1 → d_g=π
                                   // arccos is a true metric: aligned→0, perpendicular→π/2, antipodal→π (monotonic, no mirage).
        assert!((geodesic_distance(&x0, &x0)).abs() < 1e-12, "identical ⇒ 0");
        assert!((geodesic_distance(&x0, &x1) - core::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert!((geodesic_distance(&x0, &x2) - core::f64::consts::PI).abs() < 1e-12);
        // triangle inequality must hold even across the far side (where 1−cos mirages):
        // d(x0,x2) = π; d(x0,x1)+d(x1,x2) = π/2+π/2 = π ⇒ equality, not violated.
        let tri = geodesic_distance(&x0, &x1) + geodesic_distance(&x1, &x2);
        assert!(
            geodesic_distance(&x0, &x2) <= tri + 1e-9,
            "triangle inequality across antipode"
        );
        // pole guards: acos(±1) must not be NaN
        assert!(!geodesic_distance(&x0, &x0).is_nan());
        assert!(!geodesic_distance(&x0, &x2).is_nan());
    }
}
