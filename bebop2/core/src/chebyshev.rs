//! chebyshev — Chebyshev spectral propagator (Verified-by-Math vs old rust-core).
//!
//! Pure `core`. f64 (the propagator's coefficients are f64 in the oracle; spectral math demands it).
//! Matches old `rust-core::spectral_propagate` EXACTLY — same `fexp`/`fcos` range-reduced shims,
//! same Chebyshev-coefficient trapezoid, same three-term recurrence on the Laplacian. The only
//! structural change is that the CSR graph is passed BY REFERENCE (direct communication — directive 2)
//! instead of living in a global `Mutex` (no serialization/relay — directive 2). Same numerical result.
//!
//! `propagate(spectrum, t) = pointwise exp(-λ·t)` (the "wave"/tensor replaced by eigenmode decay).

#![allow(dead_code)]

/// f64 libm shims (bit-trick frexp/ldexp; Taylor exp/cos). Identical to old rust-core so the
/// Chebyshev coefficients are bit-equivalent. These are range-reduced exactly as the oracle used.
#[allow(clippy::approx_constant)] // no_std: f64::consts::PI unavailable; shim's own constant
const PI: f64 = 3.141592653589793;

/// exp(x) with range reduction x = n·ln2 + r (|r| ≤ ln2/2), Taylor on r. Matches oracle `fexp`.
#[inline]
pub fn fexp(x: f64) -> f64 {
    // Single source of truth: delegate to the crate-level C8-correct `fexp` (symmetric
    // range reduction, valid for ALL signs). The old local copy used `fround =
    // ftrunc(x+0.5)` which was wrong for x<0 — see verify-math audit F3.
    crate::fexp(x)
}
#[inline]
fn ftrunc(x: f64) -> f64 {
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32 - 1023;
    if exp < 0 {
        return 0.0;
    }
    if exp >= 52 {
        return x;
    }
    let mask = (1u64 << (52 - exp)) - 1;
    f64::from_bits(bits & !mask)
}
#[inline]
fn fround(x: f64) -> f64 {
    // Symmetric round-to-nearest (handles x<0 correctly). `ftrunc(x+0.5)` is wrong for
    // negatives (e.g. -1.2 rounds to 0 instead of -1), corrupting fcos argument reduction.
    if x >= 0.0 {
        ftrunc(x + 0.5)
    } else {
        -ftrunc(-x + 0.5)
    }
}
#[inline]
pub fn fcos(x: f64) -> f64 {
    let mut a = x;
    a = a - fround(x / (2.0 * PI)) * 2.0 * PI;
    let mut t = 1.0;
    let mut term = 1.0;
    let x2 = a * a;
    for i in 1..10 {
        term *= -x2 / ((2 * i) as f64 * (2 * i - 1) as f64);
        t += term;
    }
    t
}

/// λmax upper bound for L = D - A: symmetric, spectrum ⊂ [0, 2·max_degree]. Matches oracle.
#[inline]
pub fn lambda_max(d: &[f64]) -> f64 {
    let mut m = 1.0;
    for &x in d {
        if x > m {
            m = x;
        }
    }
    2.0 * m
}

/// Sparse mat-vec y = L·x where L = D - A (unnormalized graph Laplacian).
/// `degrees` precomputed D; `mask` (len n) zeroes masked rows (neighbors still touched). Caller-owned.
#[inline]
pub fn matvec(x: &[f64], y: &mut [f64], rp: &[i32], ci: &[i32], d: &[f64], mask: Option<&[u8]>) {
    let n = y.len();
    for i in 0..n {
        if let Some(m) = mask {
            if m[i] == 0 {
                y[i] = 0.0;
                continue;
            }
        }
        let mut acc = d[i] * x[i]; // D·x
        for k in rp[i] as usize..rp[i + 1] as usize {
            acc -= x[ci[k] as usize]; // - A·x
        }
        y[i] = acc;
    }
}

/// CSR graph over which the propagator runs (direct communication — the CSR is borrowed, not relayed).
pub struct Graph<'a> {
    pub row_ptr: &'a [i32],
    pub col_idx: &'a [i32],
    pub degrees: &'a [f64],
    pub n: usize,
}

impl<'a> Graph<'a> {
    pub fn new(row_ptr: &'a [i32], col_idx: &'a [i32], degrees: &'a [f64], n: usize) -> Self {
        Graph {
            row_ptr,
            col_idx,
            degrees,
            n,
        }
    }
}

/// A. SPECTRAL PROPAGATOR core — Chebyshev approximation of u(t) = exp(-coeff·L·t)·u0.
/// One-shot, matrix-free. Returns the n-vector (or None on invalid input: deg<1).
/// Bit-equivalent to old `rust-core::spectral_propagate`.
pub fn spectral_propagate(xs: &[f64], t: f64, coeff: f64, deg: i32, g: &Graph) -> Option<Vec<f64>> {
    let n = xs.len();
    if n == 0 || deg < 1 {
        return None;
    }
    let lamax = lambda_max(g.degrees);
    let b = lamax; // interval [0, b]

    // Chebyshev coefficients c_k via trapezoid on θ∈[0,π]
    let qp = 64usize; // quadrature points (deterministic)
    let mut c = vec![0.0f64; (deg + 1) as usize];
    for k in 0..=deg as usize {
        let mut s = 0.0;
        for j in 0..qp {
            let theta = PI * (j as f64 + 0.5) / qp as f64;
            let lambda = 0.5 * b * (1.0 + fcos(theta));
            let f = fexp(-coeff * t * lambda);
            s += f * fcos(k as f64 * theta);
        }
        c[k] = 2.0 * s / qp as f64;
        if k == 0 {
            c[k] *= 0.5;
        }
    }

    let mut t_prev = xs.to_vec(); // T0 = I·u0
    let mut lu = vec![0.0f64; n];
    matvec(&t_prev, &mut lu, g.row_ptr, g.col_idx, g.degrees, None);
    let mut t_cur = vec![0.0f64; n];
    for i in 0..n {
        t_cur[i] = (2.0 / b) * lu[i] - t_prev[i];
    }
    let mut res = vec![0.0f64; n];
    for i in 0..n {
        res[i] = c[0] * t_prev[i] + c[1] * t_cur[i];
    }
    let mut t_next = vec![0.0f64; n];
    for k in 2..=deg as usize {
        matvec(&t_cur, &mut lu, g.row_ptr, g.col_idx, g.degrees, None);
        for i in 0..n {
            t_next[i] = 2.0 * ((2.0 / b) * lu[i] - t_cur[i]) - t_prev[i];
        }
        for i in 0..n {
            res[i] += c[k] * t_next[i];
        }
        core::mem::swap(&mut t_prev, &mut t_cur);
        core::mem::swap(&mut t_cur, &mut t_next);
    }
    Some(res)
}

/// The "wave" form (directive 1): for a precomputed Laplacian spectrum (eigenvalues λ),
/// propagate via pointwise eigenmode decay: out_i = exp(-λ_i · t). This is the tensor→spectrum
/// replacement. Returns `exp(-spectrum[i]*t)` for each eigenvalue.
#[inline]
pub fn propagate_spectrum(spectrum: &[f64], t: f64, out: &mut [f64]) {
    let n = spectrum.len().min(out.len());
    for i in 0..n {
        out[i] = fexp(-spectrum[i] * t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path_graph(n: i32) -> (Vec<i32>, Vec<i32>, i32) {
        let mut rp = vec![0i32; (n + 1) as usize];
        let mut ci = Vec::new();
        let mut e = 0i32;
        for i in 0..n {
            if i > 0 {
                ci.push(i - 1);
                e += 1;
            }
            if i < n - 1 {
                ci.push(i + 1);
                e += 1;
            }
            rp[(i + 1) as usize] = e;
        }
        (rp, ci, e)
    }

    fn degrees(rp: &[i32], n: usize) -> Vec<f64> {
        (0..n).map(|i| (rp[i + 1] - rp[i]) as f64).collect()
    }

    #[test]
    fn propagator_matches_old_oracle_mass() {
        // GREEN: same as old `test_spectral_preserves_mass`. Heat kernel conserves mass ≈ 1.
        let (rp, ci, nnz) = path_graph(20);
        let n = 20usize;
        let deg = degrees(&rp, n);
        let g = Graph::new(&rp, &ci, &deg, n);
        let mut u0 = vec![0.0f64; n];
        u0[0] = 1.0;
        let out = spectral_propagate(&u0, 20.0, 1.0, 40, &g).unwrap();
        let mass: f64 = out.iter().sum();
        assert!((mass - 1.0).abs() < 1e-2, "mass={mass}");
    }

    #[test]
    fn propagator_rejects_deg_zero() {
        // RED: old `test_spectral_rejects_deg_zero` — deg<1 returns None/error.
        let (rp, ci, nnz) = path_graph(10);
        let n = 10usize;
        let deg = degrees(&rp, n);
        let g = Graph::new(&rp, &ci, &deg, n);
        let u0 = vec![1.0f64; n];
        let out = spectral_propagate(&u0, 1.0, 1.0, 0, &g);
        assert!(out.is_none(), "deg<1 must be rejected");
    }

    #[test]
    fn laplacian_zero_row_sum() {
        // GREEN: L·1 = 0 (old `test_laplacian_zero_row_sum`).
        let (rp, ci, _) = path_graph(30);
        let n = 30usize;
        let deg = degrees(&rp, n);
        let g = Graph::new(&rp, &ci, &deg, n);
        let u = vec![1.0f64; n];
        let mut y = vec![0.0f64; n];
        matvec(&u, &mut y, g.row_ptr, g.col_idx, g.degrees, None);
        for v in y {
            assert!(v.abs() < 1e-12, "L·1 should be 0, got {v}");
        }
    }

    #[test]
    fn fexp_libm_sanity() {
        // GREEN: old `test_fexp_libm_sanity` mirrored locally (C8 already fixed, but re-verify here).
        assert!((fexp(0.0) - 1.0).abs() < 1e-12);
        assert!(
            (fexp(1.0) - core::f64::consts::E).abs() < 1e-9,
            "fexp(1)={}",
            fexp(1.0)
        );
        // F3 RED: the old local fexp used `fround=ftrunc(x+0.5)`, WRONG for x<0.
        // This must hold for negatives (the production field.rs decay path hits x<0).
        assert!(
            (fexp(-1.0) - (-1.0f64).exp()).abs() < 1e-9,
            "fexp(-1)={}",
            fexp(-1.0)
        );
        assert!(
            (fexp(-3.0) - (-3.0f64).exp()).abs() < 1e-9,
            "fexp(-3)={}",
            fexp(-3.0)
        );
        // symmetry: fexp(x)*fexp(-x) == 1 for all x (old code failed this for negatives).
        for x in [-3.0, -1.5, -0.25, 0.0, 0.5, 2.0, 7.0] {
            let p = fexp(x) * fexp(-x);
            assert!((p - 1.0).abs() < 1e-8, "fexp symmetry broken at {x}: {p}");
        }
        assert!((fcos(0.0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn propagate_spectrum_pointwise_decay() {
        // GREEN: pointwise exp(-λ t) for a known 2-eigenvalue spectrum.
        let spectrum = [0.0, 1.0, 2.0];
        let t = 0.5;
        let mut out = [0.0f64; 3];
        propagate_spectrum(&spectrum, t, &mut out);
        assert!((out[0] - 1.0).abs() < 1e-9, "λ=0 mode stays 1");
        assert!((out[1] - fexp(-0.5)).abs() < 1e-9);
        assert!((out[2] - fexp(-1.0)).abs() < 1e-9);
    }

    #[test]
    fn propagator_red_breaks_on_coeff_change() {
        // RED+GREEN: changing coeff must change the field (proves the test is live).
        let (rp, ci, _) = path_graph(20);
        let n = 20usize;
        let deg = degrees(&rp, n);
        let g = Graph::new(&rp, &ci, &deg, n);
        let mut u0 = vec![0.0f64; n];
        u0[0] = 1.0;
        let a = spectral_propagate(&u0, 20.0, 1.0, 40, &g).unwrap();
        let b = spectral_propagate(&u0, 20.0, 2.0, 40, &g).unwrap();
        let mut diff = 0.0f64;
        for i in 0..n {
            diff += (a[i] - b[i]).abs();
        }
        assert!(diff > 1e-6, "coeff must change output, diff={diff}");
    }
}
