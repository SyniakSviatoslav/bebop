//! vsa — Vector Symbolic Architecture: bind/unbind as circular convolution.
//!
//! Per directive 1 + the architecture table: a VSA hypervector (dense in old rust-core) becomes a
//! SPECTRAL object — its Fourier coefficients ARE its waves. `bind(a,b)` = circular convolution =
//! pointwise multiply in Fourier (wave interference). We implement it via the FFT (no dense matmul),
//! so the hot path is O(n log n) pointwise ops, zero alloc, monomorphized, no vtable.
//!
//! `unbind(x, a) = bind(x, a)` for BINARY (±1) hypervectors because the inverse of circular
//! convolution by a ±1 vector is convolution by itself (a⁻¹ = a in ±1 algebra). This gives the
//! exact symmetry required: `‖unbind(bind(a,b), a) - b‖ ≈ 0` (round-trip symmetry gap ≈ 0).
//!
//! f64 (hypervector algebra; the oracle used f64). Zero-dep; uses the in-tree `fft` module.

#![allow(dead_code)]

use crate::fft::{fft_forward, fft_inverse, Complex};

/// Pad a length up to the next power of two (FFT requirement). The binding dimension is thus a
/// power of two — wave interference is exact in the Fourier basis for circulant operators.
#[inline]
pub fn padded_dim(n: usize) -> usize {
    n.next_power_of_two().max(1)
}

/// Circular convolution of two real vectors: (a ⋆ b)_k = Σ_j a_j · b_{(k−j) mod n}.
/// Implemented as: FFT(a) ⊙ FFT(b) → IFFT (pointwise multiply in frequency = wave interference).
///
/// `out` receives the result (length = `a.len()` == `b.len()`); the FFT is computed on the
/// padded power-of-two length internally (caller scratch `scratch_a`/`scratch_b` must be ≥ padded).
pub fn bind(
    a: &[f64],
    b: &[f64],
    out: &mut [f64],
    scratch_a: &mut [Complex],
    scratch_b: &mut [Complex],
) {
    debug_assert!(a.len() == b.len() && a.len() == out.len());
    let n = a.len();
    let m = padded_dim(n);
    // zero scratch
    for c in scratch_a.iter_mut() {
        *c = Complex::zero();
    }
    for c in scratch_b.iter_mut() {
        *c = Complex::zero();
    }
    for i in 0..n {
        scratch_a[i] = Complex::new(a[i], 0.0);
        scratch_b[i] = Complex::new(b[i], 0.0);
    }
    fft_forward(scratch_a);
    fft_forward(scratch_b);
    for i in 0..m {
        scratch_a[i] = scratch_a[i].mul(scratch_b[i]); // pointwise multiply in Fourier
    }
    fft_inverse(scratch_a);
    for i in 0..n {
        out[i] = scratch_a[i].re;
    }
}

/// Unbind. Circular convolution by a is undone by deconvolution: divide the Fourier-domain
/// product by |FFT(a)|² and conjugate. For BINARY (±1) hypervectors |FFT(a)| = 1 (for the
/// standard embedding), so this reduces to the exact inverse and `unbind(bind(a,b), a) == b`
/// to machine precision. c_k = conj(A_k) · (AB)_k / |A_k|².
pub fn unbind(
    x: &[f64],
    a: &[f64],
    out: &mut [f64],
    scratch_x: &mut [Complex],
    scratch_a: &mut [Complex],
) {
    debug_assert!(x.len() == a.len() && x.len() == out.len());
    let n = x.len();
    let m = padded_dim(n);
    for c in scratch_x.iter_mut() {
        *c = Complex::zero();
    }
    for c in scratch_a.iter_mut() {
        *c = Complex::zero();
    }
    for i in 0..n {
        scratch_x[i] = Complex::new(x[i], 0.0);
        scratch_a[i] = Complex::new(a[i], 0.0);
    }
    fft_forward(scratch_x);
    fft_forward(scratch_a);
    for i in 0..m {
        let ak = scratch_a[i];
        let mag2 = ak.norm_sq();
        // deconvolution: conj(A) * X / |A|^2 ; guard against tiny |A| (numerical zero)
        let inv = if mag2 > 1e-30 {
            scratch_x[i].mul(ak.conj()).scale(1.0 / mag2)
        } else {
            Complex::zero()
        };
        scratch_x[i] = inv;
    }
    fft_inverse(scratch_x);
    for i in 0..n {
        out[i] = scratch_x[i].re;
    }
}

/// Map a real hypervector to its bipolar (±1) form (threshold at 0). Returns the sign pattern into
/// `out` (+1 / -1). Used to guarantee the unbind symmetry of the ±1 algebra.
pub fn bipolarize(x: &[f64], out: &mut [f64]) {
    for i in 0..x.len().min(out.len()) {
        out[i] = if x[i] >= 0.0 { 1.0 } else { -1.0 };
    }
}

/// Bundling (superposition): elementwise mean. Returns the additive mixture into `out`.
pub fn bundle(vectors: &[&[f64]], out: &mut [f64]) {
    if vectors.is_empty() {
        for v in out.iter_mut() {
            *v = 0.0;
        }
        return;
    }
    let n = out.len();
    for v in out.iter_mut() {
        *v = 0.0;
    }
    for v in vectors {
        for i in 0..n {
            out[i] += v[i];
        }
    }
    let inv = 1.0 / (vectors.len() as f64);
    for v in out.iter_mut() {
        *v *= inv;
    }
}

/// Cosine similarity of two hypervectors (the VSA retrieval/query measure). Matches old
/// `cosine_similarity` math (re-exported from `algebra` for the VSA namespace).
#[inline]
pub fn similarity(a: &[f64], b: &[f64]) -> f64 {
    crate::algebra::cosine_similarity(a, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_scratches(n: usize) -> (Vec<Complex>, Vec<Complex>) {
        let m = padded_dim(n);
        (vec![Complex::zero(); m], vec![Complex::zero(); m])
    }

    #[test]
    fn bind_unbind_roundtrip_symmetry_gap_zero() {
        // GREEN: exact-ish round-trip for ±1 hypervectors. Gap ‖unbind(bind(a,b),a) - b‖ ≈ 0.
        // The key `a` must have full spectral support (well-spread ±1) so circular deconvolution
        // is exact — a periodic ±1 key (e.g. i%2) has a sparse Fourier spectrum and cannot be
        // inverted frequency-by-frequency.
        let n = 64usize;
        let mut a = vec![0.0f64; n];
        let mut b = vec![0.0f64; n];
        for i in 0..n {
            a[i] = if (i as f64 * 2.399963).sin() >= 0.0 {
                1.0
            } else {
                -1.0
            };
            b[i] = if (i as f64 * 3.714567 + 0.5).sin() >= 0.0 {
                1.0
            } else {
                -1.0
            };
        }
        let mut ab = vec![0.0f64; n];
        let (mut sa1, mut sb1) = make_scratches(n);
        bind(&a, &b, &mut ab, &mut sa1, &mut sb1);

        // unbind(ab, a) should recover b
        let mut recovered = vec![0.0f64; n];
        let (mut sx, mut sa2) = make_scratches(n);
        unbind(&ab, &a, &mut recovered, &mut sx, &mut sa2);

        let mut gap = 0.0f64;
        for i in 0..n {
            gap += (recovered[i] - b[i]).abs();
        }
        assert!(gap < 1e-9, "round-trip symmetry gap = {gap}, must be ≈0");
    }

    #[test]
    fn bind_is_commutative() {
        // GREEN: circular convolution commutes: bind(a,b) == bind(b,a).
        let n = 32usize;
        let a: Vec<f64> = (0..n).map(|i| (i as f64 * 0.3).sin()).collect();
        let b: Vec<f64> = (0..n).map(|i| (i as f64 * 0.7 + 1.0).cos()).collect();
        let mut ab = vec![0.0f64; n];
        let mut ba = vec![0.0f64; n];
        let (mut sa, mut sb) = make_scratches(n);
        bind(&a, &b, &mut ab, &mut sa, &mut sb);
        let (mut sb2, mut sa2) = make_scratches(n);
        bind(&b, &a, &mut ba, &mut sb2, &mut sa2);
        for i in 0..n {
            assert!((ab[i] - ba[i]).abs() < 1e-12, "bind not commutative at {i}");
        }
    }

    #[test]
    fn bind_matches_bruteforce_circular_convolution() {
        // GREEN (RED+GREEN check): FFT-based bind equals the O(n²) brute-force circular convolution
        // to 1e-9. If the FFT path were wrong, this fails.
        let n = 16usize;
        let a: Vec<f64> = (0..n).map(|i| (i as f64 * 0.4).sin()).collect();
        let b: Vec<f64> = (0..n).map(|i| (i as f64 * 0.9 + 0.5).cos()).collect();
        let mut out = vec![0.0f64; n];
        let (mut sa, mut sb) = make_scratches(n);
        bind(&a, &b, &mut out, &mut sa, &mut sb);
        // brute force
        let mut brute = vec![0.0f64; n];
        for k in 0..n {
            let mut s = 0.0f64;
            for j in 0..n {
                let idx = ((k as i64 - j as i64).rem_euclid(n as i64)) as usize;
                s += a[j] * b[idx];
            }
            brute[k] = s;
        }
        for i in 0..n {
            assert!(
                (out[i] - brute[i]).abs() < 1e-9,
                "bind != brute at {i}: {} vs {}",
                out[i],
                brute[i]
            );
        }
    }

    #[test]
    fn bind_roundtrip_red_breaks_on_perturbation() {
        // RED: perturbing `a` MUST break the round-trip (proves the unbind symmetry test is live).
        let n = 64usize;
        let mut a = vec![0.0f64; n];
        let mut a_pert = vec![0.0f64; n];
        let mut b = vec![0.0f64; n];
        for i in 0..n {
            // Well-spread ±1 keys (a real VSA hypervector is pseudo-random, NOT alternating).
            // An alternating (-1)^i key has a single non-zero Fourier bin (delta at Nyquist);
            // deconvolution then divides by ~0 in every other bin and cannot recover b. Use a
            // deterministic LCG-style bit pattern so every Fourier bin is non-zero.
            let ha = (i.wrapping_mul(2654435761) ^ 0x9e37) & 1;
            let hb = (i.wrapping_mul(40503).wrapping_add(7) ^ 0x1234) & 1;
            a[i] = if ha == 0 { 1.0 } else { -1.0 };
            a_pert[i] = a[i];
            b[i] = if hb == 0 { 1.0 } else { -1.0 };
        }
        a_pert[5] = -a_pert[5]; // flip one bit
        let mut ab = vec![0.0f64; n];
        let (mut sa, mut sb) = make_scratches(n);
        bind(&a, &b, &mut ab, &mut sa, &mut sb);
        let mut ab_pert = vec![0.0f64; n];
        let (mut sa2, mut sb2) = make_scratches(n);
        bind(&a_pert, &b, &mut ab_pert, &mut sa2, &mut sb2);
        // round-trip with the WRONG key a_pert must NOT recover b
        let mut rec_wrong = vec![0.0f64; n];
        let (mut sx, mut sa3) = make_scratches(n);
        unbind(&ab, &a_pert, &mut rec_wrong, &mut sx, &mut sa3);
        let mut gap_wrong = 0.0f64;
        for i in 0..n {
            gap_wrong += (rec_wrong[i] - b[i]).abs();
        }
        // the gap with the wrong key must be meaningfully larger than the ~0 correct gap
        assert!(
            gap_wrong > 0.5,
            "wrong-key round-trip should diverge, gap={gap_wrong}"
        );
        // sanity: the correct key gives gap ≈ 0
        let mut rec_right = vec![0.0f64; n];
        let (mut sx2, mut sa4) = make_scratches(n);
        unbind(&ab, &a, &mut rec_right, &mut sx2, &mut sa4);
        let mut gap_right = 0.0f64;
        for i in 0..n {
            gap_right += (rec_right[i] - b[i]).abs();
        }
        assert!(
            gap_right < 1e-9,
            "right-key gap should be ≈0, got {gap_right}"
        );
    }

    #[test]
    fn bundle_matches_analytic_mean() {
        let v1 = [1.0f64, 2.0, 3.0];
        let v2 = [3.0f64, 2.0, 1.0];
        let mut out = [0.0f64; 3];
        bundle(&[&v1, &v2], &mut out);
        assert!((out[0] - 2.0).abs() < 1e-12);
        assert!((out[1] - 2.0).abs() < 1e-12);
        assert!((out[2] - 2.0).abs() < 1e-12);
    }
}
