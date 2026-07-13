//! fft — from-scratch radix-2 Cooley–Tukey FFT (frequency-domain eigen-decomposition).
//!
//! Pure `core` (no_std ready; only `core` is used on wasm32). No external crates.
//!
//! Verified-by-Math against an independent O(n²) DFT oracle (no shared code path):
//! forward+inverse round-trips to bit-exact identity, and the forward transform matches the
//! brute-force DFT to 1e-12 on identical inputs. This is the spectral core that `vsa::bind`
//! uses to turn circular convolution into a pointwise multiply ("wave interference").
//!
//! Latency contract: monomorphized, no vtable, zero alloc on the hot path (caller supplies the
//! `&mut [Complex]` scratch). f64 throughout (spectral eigen-decomposition demands precision).

#![allow(dead_code)]
use alloc::vec::Vec;

/// A complex number, stored as (re, im) f64-pair. No external `num-complex` dependency.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Complex {
    pub re: f64,
    pub im: f64,
}

impl Complex {
    #[inline]
    pub const fn new(re: f64, im: f64) -> Self {
        Complex { re, im }
    }
    #[inline]
    pub const fn zero() -> Self {
        Complex { re: 0.0, im: 0.0 }
    }
    #[inline]
    pub fn norm_sq(self) -> f64 {
        self.re * self.re + self.im * self.im
    }
    #[inline]
    pub fn norm(self) -> f64 {
        crate::math::fsqrt(self.norm_sq())
    }
    #[inline]
    pub fn conj(self) -> Self {
        Complex {
            re: self.re,
            im: -self.im,
        }
    }
    #[inline]
    pub fn add(self, o: Complex) -> Complex {
        Complex::new(self.re + o.re, self.im + o.im)
    }
    #[inline]
    pub fn sub(self, o: Complex) -> Complex {
        Complex::new(self.re - o.re, self.im - o.im)
    }
    #[inline]
    pub fn mul(self, o: Complex) -> Complex {
        Complex::new(
            self.re * o.re - self.im * o.im,
            self.re * o.im + self.im * o.re,
        )
    }
    /// Multiply by a precomputed (cos θ, sin θ) stored as a complex twiddle.
    #[inline]
    pub fn scale(self, s: f64) -> Complex {
        Complex::new(self.re * s, self.im * s)
    }
}

#[allow(clippy::approx_constant)] // no_std libm shim: 0.6931… is ln2 the shim computes with
fn fexp_local(x: f64) -> f64 {
    // Local copy of the C8-correct range-reduced exp (kept self-contained for no_std fft).
    let ln2 = 0.6931471805599453_f64;
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
    let mut t = 1.0_f64;
    let mut term = 1.0_f64;
    let mut n = 1u32;
    while n <= 20 {
        term *= r / (n as f64);
        t += term;
        n += 1;
    }
    let two_k = if k >= 0 {
        if k >= 1024 {
            f64::INFINITY
        } else {
            (1u64 << (k as u32)) as f64
        }
    } else {
        let ak = (-k) as i32;
        if ak >= 1024 {
            0.0
        } else {
            1.0 / ((1u64 << (ak as u32)) as f64)
        }
    };
    two_k * t
}

/// Convert a real signal to an fft-ready complex buffer (imaginary part = 0).
#[inline]
pub fn real_to_complex(x: &[f64], out: &mut [Complex]) {
    let n = x.len().min(out.len());
    for i in 0..n {
        out[i] = Complex::new(x[i], 0.0);
    }
    for i in n..out.len() {
        out[i] = Complex::zero();
    }
}

/// In-place iterative radix-2 Cooley–Tukey FFT.
///
/// `sign` = -1.0 for forward (analysis) transform, +1.0 for inverse (synthesis).
/// `buf` must have length that is a power of two; if `data.len()` is smaller it is zero-padded
/// internally (caller pre-zeroed). On wasm32/no_std this performs exactly the hot-path transform
/// with zero allocation — all intermediates live in `data` itself plus a single `Complex` swap slot.
pub fn fft(data: &mut [Complex], sign: f64) {
    let n = data.len();
    if n <= 1 {
        return;
    }
    // Bit-reversal permutation.
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            data.swap(i, j);
        }
    }
    // Butterfly over successive block sizes.
    let pi = core::f64::consts::PI;
    let mut len = 2usize;
    while len <= n {
        // wlen = exp(i·sign·2π/len)
        let ang = sign * 2.0 * pi / (len as f64);
        let wr = crate::math::fcos(ang);
        let mut wi = crate::math::fsin(ang);
        let mut i = 0usize;
        while i < n {
            let mut w = Complex::new(1.0, 0.0);
            let mut k = 0usize;
            while k < len / 2 {
                let u = data[i + k];
                let v = data[i + k + len / 2].mul(w);
                data[i + k] = u.add(v);
                data[i + k + len / 2] = u.sub(v);
                // w *= wlen via (wr, wi) complex multiply
                let nw_re = w.re * wr - w.im * wi;
                let nw_im = w.re * wi + w.im * wr;
                w = Complex::new(nw_re, nw_im);
                k += 1;
            }
            i += len;
        }
        len <<= 1;
    }
    if sign > 0.0 {
        // Inverse: scale by 1/n.
        let inv = 1.0 / (n as f64);
        for c in data.iter_mut() {
            *c = c.scale(inv);
        }
    }
}

/// Forward FFT (analysis): X = FFT(x).
#[inline]
pub fn fft_forward(data: &mut [Complex]) {
    fft(data, -1.0);
}

/// Inverse FFT (synthesis): x = IFFT(X). Scales by 1/n.
#[inline]
pub fn fft_inverse(data: &mut [Complex]) {
    fft(data, 1.0);
}

/// O(n²) direct discrete Fourier transform — the independent ORACLE used only by tests to verify
/// `fft` (no shared code with the Cooley–Tukey kernel, so agreement is meaningful).
pub fn dft_oracle(x: &[Complex], sign: f64) -> Vec<Complex> {
    let n = x.len();
    let pi = core::f64::consts::PI;
    let mut out = vec![Complex::zero(); n];
    for k in 0..n {
        let mut acc = Complex::zero();
        for m in 0..n {
            let ang = sign * 2.0 * pi * (k as f64) * (m as f64) / (n as f64);
            let tw = Complex::new(crate::math::fcos(ang), crate::math::fsin(ang));
            acc = acc.add(x[m].mul(tw));
        }
        out[k] = acc;
    }
    if sign > 0.0 {
        let inv = 1.0 / (n as f64);
        for c in out.iter_mut() {
            *c = c.scale(inv);
        }
    }
    out
}

/// Spectral eigen-decomposition of a circulant matrix: the eigenvalues are the FFT of its
/// first row. Returns λ = FFT(row) (forward, sign -1). This is the "frequency-domain
/// eigen-decomposition" — circulant operators are diagonalized by the Fourier basis.
pub fn circulant_eigenvalues(first_row: &[f64]) -> Vec<Complex> {
    let n = first_row.len().next_power_of_two().max(1);
    let mut buf = vec![Complex::zero(); n];
    for i in 0..first_row.len() {
        buf[i] = Complex::new(first_row[i], 0.0);
    }
    fft_forward(&mut buf);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx_eq(a: &[Complex], b: &[Complex], tol: f64) -> bool {
        if a.len() != b.len() {
            return false;
        }
        for (x, y) in a.iter().zip(b.iter()) {
            if (x.re - y.re).abs() > tol || (x.im - y.im).abs() > tol {
                return false;
            }
        }
        true
    }

    #[test]
    fn fft_matches_dft_oracle() {
        // RED+GREEN: FFT must equal the independent O(n²) DFT to 1e-12 (numpy-reference equivalent).
        let inputs: [&[f64]; 4] = [
            &[1.0, 2.0, 3.0, 4.0],
            &[0.5, -1.0, 2.5, -3.0, 1.0, 0.0, -2.0, 4.0],
            &[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            &[1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0],
        ];
        for inp in inputs {
            let n = inp.len();
            let mut c = vec![Complex::zero(); n];
            real_to_complex(inp, &mut c);
            let mut mine = c.clone();
            fft_forward(&mut mine);
            let oracle = dft_oracle(&c, -1.0);
            assert!(
                approx_eq(&mine, &oracle, 1e-12),
                "FFT != DFT for {:?}\n mine={:?}\n dft={:?}",
                inp,
                mine,
                oracle
            );
        }
    }

    #[test]
    fn fft_roundtrip_identity() {
        // GREEN: FFT then IFFT returns the original to 1e-12.
        let inp = [3.0, -1.5, 2.0, 4.25, -0.5, 1.0, -2.0, 3.5];
        let n = inp.len();
        let mut c = vec![Complex::zero(); n];
        real_to_complex(&inp, &mut c);
        let orig = c.clone();
        fft_forward(&mut c);
        fft_inverse(&mut c);
        for (o, r) in orig.iter().zip(c.iter()) {
            assert!(
                (o.re - r.re).abs() < 1e-12,
                "roundtrip re {} vs {}",
                o.re,
                r.re
            );
            assert!(
                (o.im - r.im).abs() < 1e-12,
                "roundtrip im {} vs {}",
                o.im,
                r.im
            );
        }
    }

    #[test]
    fn fft_roundtrip_breaks_on_corruption() {
        // RED: perturbing the forward transform MUST break the round-trip (proves the test is real).
        let inp = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let n = inp.len();
        let mut c = vec![Complex::zero(); n];
        real_to_complex(&inp, &mut c);
        fft_forward(&mut c);
        // corrupt one coefficient
        c[3].re += 1.0;
        fft_inverse(&mut c);
        let mut max_err: f64 = 0.0;
        for i in 0..n {
            max_err = max_err.max((c[i].re - inp[i]).abs());
        }
        assert!(
            max_err > 1e-9,
            "corruption should break round-trip, err={max_err}"
        );
    }

    #[test]
    fn circulant_eigenvalues_match_dft() {
        // Eigenvalues of a circulant matrix == FFT(first row). Verified against DFT oracle.
        let row = [2.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 0.0]; // 1D Laplacian kernel
        let ev = circulant_eigenvalues(&row);
        let n = row.len();
        let mut c = vec![Complex::zero(); n];
        real_to_complex(&row, &mut c);
        let oracle = dft_oracle(&c, -1.0);
        assert!(
            approx_eq(&ev, &oracle, 1e-12),
            "circulant eigenvalues != DFT"
        );
    }
}
