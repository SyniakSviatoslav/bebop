//! algebra — cosine / basis-projection primitives reused by the BP-10 orthogonometer.
//!
//! Mirrors the API of `bebop2/core/src/algebra.rs` (`project` + `cosine_similarity`) so the
//! orthogonometer can `use crate::algebra::{project, cosine}` *within this crate*. The original
//! blueprint assumed `algebra.rs` lived here; it actually lives in `bebop2-core`. To keep the
//! `bebop` crate dependency-light and the BP-10 module self-contained, the same primitive API is
//! re-provided here (pure std, no external math crate needed).

/// Bipolar dot-product similarity of two f64 vectors. Σ a_i·b_i.
pub fn dot(a: &[f64], b: &[f64]) -> f64 {
    debug_assert!(a.len() == b.len());
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Cosine similarity ⟨a,b⟩ / (‖a‖·‖b‖). 0 when either vector is zero (no spurious 1.0).
/// Clamped to [-1, 1].
pub fn cosine(a: &[f64], b: &[f64]) -> f64 {
    cosine_similarity(a, b)
}

/// Cosine similarity — same as [`cosine`], named to match the `bebop2-core` algebra API.
pub fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    debug_assert!(a.len() == b.len());
    let n = a.len().min(b.len());
    let mut dot_p = 0.0f64;
    let mut na = 0.0f64;
    let mut nb = 0.0f64;
    for i in 0..n {
        dot_p += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = (na * nb).sqrt();
    if denom <= 1e-12 {
        0.0
    } else {
        (dot_p / denom).clamp(-1.0, 1.0)
    }
}

/// Spectral projection of `signal` onto a set of `modes` basis vectors (row-major:
/// `basis[k*n + i]`). `coeffs[k] = ⟨signal, basis_k⟩ / ‖basis_k‖²`.
/// This is the "store the vector as spectral coefficients" primitive from the blueprint.
pub fn project(signal: &[f64], basis: &[f64], modes: usize, n: usize, coeffs: &mut [f64]) {
    for k in 0..modes {
        let mut dot_p = 0.0f64;
        let mut norm2 = 0.0f64;
        let base = k * n;
        for i in 0..n {
            let bk = basis[base + i];
            dot_p += signal[i] * bk;
            norm2 += bk * bk;
        }
        coeffs[k] = if norm2 > 1e-12 { dot_p / norm2 } else { 0.0 };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_bounds() {
        let a = [1.0, 2.0, 3.0];
        let b = [1.0, 2.0, 3.0];
        let c = [-1.0, -2.0, -3.0];
        let orth = [1.0, 0.0, 0.0];
        let orth2 = [0.0, 1.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 1e-12);
        assert!((cosine_similarity(&a, &c) + 1.0).abs() < 1e-12);
        assert!(cosine_similarity(&orth, &orth2).abs() < 1e-12);
        let big = [10.0, 20.0, 30.0];
        assert!((cosine_similarity(&a, &big) - 1.0).abs() < 1e-12, "norm-invariant");
    }

    #[test]
    fn project_onto_axis() {
        // projecting [1,1] onto basis [1,0] yields coeff 1, reconstructs [1,0].
        let signal = [1.0, 1.0];
        let basis = [1.0, 0.0]; // single mode, n=2
        let mut coeffs = [0.0f64; 1];
        project(&signal, &basis, 1, 2, &mut coeffs);
        assert!((coeffs[0] - 1.0).abs() < 1e-12);
        let recon: Vec<f64> = (0..2).map(|i| coeffs[0] * basis[i]).collect();
        assert!((recon[0] - 1.0).abs() < 1e-12);
        assert!((recon[1]).abs() < 1e-12);
    }
}
