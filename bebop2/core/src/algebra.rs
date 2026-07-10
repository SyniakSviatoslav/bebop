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
    let denom = (na * nb).sqrt();
    if denom <= 1e-12 {
        0.0
    } else {
        (dot / denom).clamp(-1.0, 1.0)
    }
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
        x.sin() / x
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
        assert!((cosine_similarity(&a, &big) - 1.0).abs() < 1e-12, "norm-invariant");
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
    fn project_reconstruct_roundtrip() {
        // GREEN: project then reconstruct returns the signal (spectral coefficients are faithful).
        // Use a simple orthonormal-ish basis: delta vectors e_k.
        let n = 8usize;
        let modes = 8usize;
        let mut basis = vec![0.0f64; modes * n];
        for k in 0..modes {
            basis[k * n + k] = 1.0;
        }
        let signal = [1.0, 2.0, -3.0, 4.0, -5.0, 6.0, -7.0, 8.0];
        let mut coeffs = vec![0.0f64; modes];
        let mut recon = vec![0.0f64; n];
        project(&signal, &basis, modes, n, &mut coeffs);
        reconstruct(&coeffs, &basis, modes, n, &mut recon);
        for i in 0..n {
            assert!((recon[i] - signal[i]).abs() < 1e-12, "rt[{}]={} vs {}", i, recon[i], signal[i]);
        }
    }
}
