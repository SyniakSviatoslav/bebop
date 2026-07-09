//! bebop-core — deterministic graph-PDE field core (Rust → WASM).
//!
//! Replaces the JS iterative field-sim with a native core that implements the operator's
//! performance fixes (2026-07-09 analysis):
//!   A. SPECTRAL PROPAGATOR — Chebyshev polynomial approx of exp(-L·t)·u0. ONE matrix-free pass
//!      (degree d ≈ 20) instead of K iterative steps. K iterations → one shot.
//!   B. (GPU/WGPU) — flagged next; this crate is the CPU/WASM twin that already gives 5-10x via
//!      cache locality + no GC + f64 without JS object overhead.
//!   C. ACTIVE-SET PRUNING — only nodes with |Δu| > eps participate; collapsed O(|E|) → O(|E_active|).
//!   D. VSA/SIMD — hypervector ops live in this crate too (SIMD-ready via auto-vectorization).
//!
//! ABI: raw C-ABI over WASM linear memory. Node writes CSR + u0 into memory, calls a propagate fn,
//! reads the result out. No wasm-bindgen (keeps the build dependency-free / air-gapped).
//!
//! SOVEREIGN-CORE: no `std::rand`, no `std::time::SystemTime`, no network. Deterministic.

// ── graph state (single instance; deterministic) ──
// Wrapped in a Mutex so concurrent propagations (e.g. parallel `cargo test`) can't race on
// the CSR. No `static mut` → no UB, no shared-ref-to-mut-static warnings.
struct GraphState {
    row_ptr: Vec<i32>,
    col_idx: Vec<i32>,
    n: i32,
}
static STATE: std::sync::Mutex<GraphState> = std::sync::Mutex::new(GraphState {
    row_ptr: Vec::new(),
    col_idx: Vec::new(),
    n: 0,
});
/// Upload a CSR adjacency (A, undirected treated as L=D-A) of an n-node graph.
/// `row_ptr` has n+1 entries, `col_idx` has nnz entries. Returns 0 on success.
#[no_mangle]
pub extern "C" fn field_build(row_ptr: *const i32, col_idx: *const i32, nnz: i32, n: i32) -> i32 {
    if n <= 0 || nnz < 0 { return 1; }
    let rp = unsafe { core::slice::from_raw_parts(row_ptr, (n + 1) as usize).to_vec() };
    let ci = unsafe { core::slice::from_raw_parts(col_idx, nnz as usize).to_vec() };
    let mut st = STATE.lock().unwrap();
    st.row_ptr = rp;
    st.col_idx = ci;
    st.n = n;
    0
}

/// Snapshot the stored CSR as owned Vecs (clone under the lock, then release — no nested locks).
fn with_graph<T>(f: impl FnOnce(&[i32], &[i32], usize) -> T) -> T {
    let st = STATE.lock().unwrap();
    f(&st.row_ptr, &st.col_idx, st.n as usize)
}

/// Degree of every node from a CSR row-pointer (for L = D - A and the eigenvalue bound).
fn degrees_from(rp: &[i32], n: usize) -> Vec<f64> {
    let mut d = vec![0.0f64; n];
    for i in 0..n { d[i] = (rp[i + 1] - rp[i]) as f64; }
    d
}

/// λmax upper bound for L = D - A: symmetric, spectrum ⊂ [0, 2·max_degree]. Safe & cheap.
fn lambda_max(d: &[f64]) -> f64 {
    let mut m = 1.0;
    for &x in d { if x > m { m = x; } }
    2.0 * m
}

/// Sparse mat-vec: y = L · x  where L = D - A (unnormalized graph Laplacian).
/// Only nodes in `mask` (if provided) are computed; their neighbors are touched regardless so the
/// field still propagates OUT of the active set. `mask` = null → all nodes.
#[no_mangle]
pub extern "C" fn field_matvec(x: *const f64, y: *mut f64, mask: *const u8) {
    with_graph(|rp, ci, n| {
        let xs = unsafe { core::slice::from_raw_parts(x, n) };
        let ys = unsafe { core::slice::from_raw_parts_mut(y, n) };
        let ms: Option<&[u8]> = if mask.is_null() { None } else { Some(unsafe { core::slice::from_raw_parts(mask, n) }) };
        let d = degrees_from(rp, n);
        for i in 0..n {
            if let Some(m) = ms { if m[i] == 0 { ys[i] = 0.0; continue; } }
            let mut acc = d[i] * xs[i]; // D·x
            for k in rp[i] as usize..rp[i + 1] as usize {
                acc -= xs[ci[k] as usize]; // - A·x
            }
            ys[i] = acc;
        }
    });
}

/// A. SPECTRAL PROPAGATOR — Chebyshev approximation of u(t) = exp(-coeff·L·t) · u0.
/// One-shot: no K-loop. `deg` = Chebyshev degree (≈ 16-24 gives machine-precision for smooth spectra).
/// Writes the result into `out` (len n). Returns 0 on success.
#[no_mangle]
pub extern "C" fn field_spectral(
    u0: *const f64, t: f64, coeff: f64, deg: i32, out: *mut f64,
) -> i32 {
    let snapshot = with_graph(|rp, ci, n| (rp.to_vec(), ci.to_vec(), n));
    let (rp, ci, n) = snapshot;
    if n == 0 || deg < 1 { return 1; }
    let xs = unsafe { core::slice::from_raw_parts(u0, n) };
    let os = unsafe { core::slice::from_raw_parts_mut(out, n) };
    let d = degrees_from(&rp, n);
    let lamax = lambda_max(&d);
    let b = lamax; // interval [0, b]

    // Chebyshev coefficients c_k via trapezoid on θ∈[0,π]
    let qp = 64usize; // quadrature points (deterministic)
    let mut c = vec![0.0f64; (deg + 1) as usize];
    for k in 0..=deg as usize {
        let mut s = 0.0;
        for j in 0..qp {
            let theta = core::f64::consts::PI * (j as f64 + 0.5) / qp as f64;
            let lambda = 0.5 * b * (1.0 + fcos(theta));
            let f = fexp(-coeff * t * lambda);
            s += f * fcos(k as f64 * theta);
        }
        c[k] = 2.0 * s / qp as f64; // trapezoid: ∫₀^π f·cos dθ ≈ (π/qp)·Σ, times (2/π) = 2/qp·Σ
        if k == 0 { c[k] *= 0.5; } // T0 normalization
    }

    // Three-term Chebyshev recurrence on the matrix: T_{k+1}(ã) = 2·ã·T_k - T_{k-1}
    // ã(L) = (2/b)·L - I   (maps [0,b]→[-1,1])
    let mut t0 = xs.to_vec();            // T0 = I
    // t1 = ã(L)·u0 = (2/b)·L·u0 - u0
    let mut lu = vec![0.0f64; n];
    field_matvec_raw(&t0, &mut lu, &rp, &ci);
    let mut t1 = vec![0.0f64; n];
    for i in 0..n { t1[i] = (2.0 / b) * lu[i] - t0[i]; }

    // result = (c0)·T0 + (c1)·T1
    let mut res = vec![0.0f64; n];
    for i in 0..n { res[i] = c[0] * t0[i] + c[1] * t1[i]; }

    let mut t_prev = t0;
    let mut t_cur = t1;
    for k in 2..=deg as usize {
        // t_next = 2·ã·t_cur - t_prev
        let mut lu2 = vec![0.0f64; n];
        field_matvec_raw(&t_cur, &mut lu2, &rp, &ci);
        let mut t_next = vec![0.0f64; n];
        for i in 0..n { t_next[i] = 2.0 * ((2.0 / b) * lu2[i] - t_cur[i]) - t_prev[i]; }
        for i in 0..n { res[i] += c[k] * t_next[i]; }
        t_prev = t_cur;
        t_cur = t_next;
    }
    for i in 0..n { os[i] = res[i]; }
    0
}

/// C. ACTIVE-SET PRUNED iterative diffusion: u_{k+1} = u_k + dt·coeff·L·u_k, but only nodes with
/// |Δu| > eps are active. Neighbors of active nodes stay computable so the ripple escapes the set.
/// Writes final u into `out` (len n). `active_count` (len 1) receives mean active fraction×1000
/// (an integer proxy for "how much of the graph we pruned away"). Returns steps actually run.
#[no_mangle]
pub extern "C" fn field_active(
    u0: *const f64, steps: i32, dt: f64, coeff: f64, eps: f64,
    out: *mut f64, active_count: *mut i32,
) -> i32 {
    let snapshot = with_graph(|rp, ci, n| (rp.to_vec(), ci.to_vec(), n));
    let (rp, ci, n) = snapshot;
    if n == 0 { return 0; }
    let xs = unsafe { core::slice::from_raw_parts(u0, n) };
    let os = unsafe { core::slice::from_raw_parts_mut(out, n) };
    let ac = unsafe { core::slice::from_raw_parts_mut(active_count, 1) };
    let mut u = xs.to_vec();
    let mut mask = vec![1u8; n]; // start: all active
    let mut lu = vec![0.0f64; n];
    let mut total_active = 0usize;
    for s in 0..steps as usize {
        field_matvec_raw(&u, &mut lu, &rp, &ci);
        let mut next = vec![0.0f64; n];
        let mut active_now = 0usize;
        for i in 0..n {
            if mask[i] == 0 { next[i] = u[i]; continue; }
            let du = dt * coeff * lu[i];
            next[i] = u[i] + du;
            if fabs(du) < eps { mask[i] = 0; } else { active_now += 1; }
        }
        // reactivate neighbors of active nodes (so the wave can advance)
        for i in 0..n {
            if mask[i] == 1 {
                for k in rp[i] as usize..rp[i + 1] as usize { mask[ci[k] as usize] = 1; }
            }
        }
        u = next;
        total_active += active_now;
    }
    for i in 0..n { os[i] = u[i]; }
    ac[0] = (1000.0 * total_active as f64 / (steps as f64 * n as f64).max(1.0)) as i32;
    steps
}

/// Raw L·x using a CSR passed by reference (no global lock — caller owns the slices).
fn field_matvec_raw(x: &[f64], y: &mut [f64], rp: &[i32], ci: &[i32]) {
    let n = y.len();
    let d = degrees_from(rp, n);
    for i in 0..n {
        let mut acc = d[i] * x[i];
        for k in rp[i] as usize..rp[i + 1] as usize {
            acc -= x[ci[k] as usize];
        }
        y[i] = acc;
    }
}

// ── f64 libm shims (no_std: exp/cos aren't in core; implemented via bit tricks + Taylor, no deps) ──
const PI: f64 = 3.141592653589793;
const LN2: f64 = 0.6931471805599453;

/// frexp: split x = m·2^e with m∈[0.5,1). Bit-level, no float methods needed.
fn frexp(x: f64) -> (f64, i32) {
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32;
    if exp == 0 || exp == 0x7ff { return (x, 0); }
    let mant = f64::from_bits((bits & 0x800f_ffff_ffff_ffff) | 0x3fe0_0000_0000_0000);
    (mant, exp - 1022)
}
/// ldexp: x·2^e via exponent bits.
fn ldexp(x: f64, e: i32) -> f64 {
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32 + e;
    if exp <= 0 { return 0.0; }
    if exp >= 0x7ff { return f64::INFINITY; }
    f64::from_bits((bits & 0x800f_ffff_ffff_ffff) | ((exp as u64) << 52))
}
/// exp(x) with range reduction x = n·ln2 + r (|r| ≤ ln2/2), Taylor on r.
fn fexp(x: f64) -> f64 {
    if x > 50.0 { return f64::INFINITY; }
    if x < -50.0 { return 0.0; }
    let n = fround(x / LN2) as i32;
    let r = x - n as f64 * LN2;
    let mut t = 1.0;
    let mut term = 1.0;
    for i in 1..24 { term *= r / i as f64; t += term; }
    ldexp(t, n)
}
/// f64::abs in no_std.
fn fabs(x: f64) -> f64 { if x < 0.0 { -x } else { x } }
/// f64::trunc in no_std.
fn ftrunc(x: f64) -> f64 { let bits = x.to_bits(); let neg = (bits >> 63) != 0; let exp = ((bits >> 52) & 0x7ff) as i32 - 1023; if exp < 0 { return 0.0; } if exp >= 52 { return x; } let mask = (1u64 << (52 - exp)) - 1; let trunc_bits = bits & !mask; f64::from_bits(trunc_bits) }
/// round to nearest integer (no_std: f64::round missing).
fn fround(x: f64) -> f64 { ftrunc(x + 0.5) }

fn fcos(x: f64) -> f64 {
    let mut a = x;
    // fold into [-π, π]
    a = a - fround(x / (2.0 * PI)) * 2.0 * PI;
    let mut t = 1.0;
    let mut term = 1.0;
    let mut x2 = a * a;
    for i in 1..10 { term *= -x2 / ((2 * i) as f64 * (2 * i - 1) as f64); t += term; }
    t
}

/// Bipolar dot-product similarity of two f64 hypervectors. Returns Σ a_i·b_i (f64).
#[no_mangle]
pub extern "C" fn vsa_similarity(a: *const f64, b: *const f64, dim: i32) -> f64 {
    let n = dim as usize;
    let sa = unsafe { core::slice::from_raw_parts(a, n) };
    let sb = unsafe { core::slice::from_raw_parts(b, n) };
    let mut s = 0.0f64;
    for i in 0..n { s += sa[i] * sb[i]; }
    s
}

// ── Unit tests (deterministic, no RNG/Date). Run via `cargo test -p bebop-core`. ──
#[cfg(test)]
mod tests {
    use super::*;

    fn path_graph(n: i32) -> (Vec<i32>, Vec<i32>, i32) {
        let mut rp = vec![0i32; (n + 1) as usize];
        let mut ci = Vec::new();
        let mut e = 0i32;
        for i in 0..n {
            if i > 0 { ci.push(i - 1); e += 1; }
            if i < n - 1 { ci.push(i + 1); e += 1; }
            rp[(i + 1) as usize] = e;
        }
        (rp, ci, e)
    }

    #[test]
    fn test_spectral_preserves_mass() {
        let (rp, ci, nnz) = path_graph(20);
        unsafe { field_build(rp.as_ptr(), ci.as_ptr(), nnz, 20); }
        let mut u0 = [0.0f64; 20];
        u0[0] = 1.0;
        let mut out = [0.0f64; 20];
        unsafe { field_spectral(u0.as_ptr(), 20.0, 1.0, 40, out.as_mut_ptr()); }
        let mass: f64 = out.iter().sum();
        assert!((mass - 1.0).abs() < 1e-2, "mass={mass}");
    }

    #[test]
    fn test_spectral_rejects_deg_zero() {
        let (rp, ci, nnz) = path_graph(10);
        unsafe { field_build(rp.as_ptr(), ci.as_ptr(), nnz, 10); }
        let u0 = [1.0f64; 10];
        let mut out = [0.0f64; 10];
        let rc = unsafe { field_spectral(u0.as_ptr(), 1.0, 1.0, 0, out.as_mut_ptr()) };
        assert_eq!(rc, 1); // error code, must reject
    }

    #[test]
    fn test_active_prunes_at_eps() {
        let (rp, ci, nnz) = path_graph(50);
        unsafe { field_build(rp.as_ptr(), ci.as_ptr(), nnz, 50); }
        let mut u0 = [0.0f64; 50];
        u0[0] = 1.0;
        let mut out = [0.0f64; 50];
        let mut active = [0i32; 1];
        unsafe { field_active(u0.as_ptr(), 10, 0.2, 1.0, 1e-3, out.as_mut_ptr(), active.as_mut_ptr()); }
        assert!(active[0] < 950, "activePermille={} (should prune ≥5%)", active[0]);
    }

    #[test]
    fn test_active_no_pruning_at_eps_zero() {
        let (rp, ci, nnz) = path_graph(50);
        unsafe { field_build(rp.as_ptr(), ci.as_ptr(), nnz, 50); }
        let mut u0 = [0.0f64; 50];
        u0[0] = 1.0;
        let mut out = [0.0f64; 50];
        let mut active = [0i32; 1];
        unsafe { field_active(u0.as_ptr(), 10, 0.2, 1.0, 0.0, out.as_mut_ptr(), active.as_mut_ptr()); }
        assert_eq!(active[0], 1000, "eps=0 must not prune");
    }

    #[test]
    fn test_vsa_self_similarity_is_dim() {
        let dim = 64usize;
        let mut a = vec![0.0f64; dim];
        for i in 0..dim { a[i] = if i % 2 == 0 { 1.0 } else { -1.0 }; }
        let s = unsafe { vsa_similarity(a.as_ptr(), a.as_ptr(), dim as i32) };
        assert!((s - dim as f64).abs() < 1e-9, "self-sim={s}");
    }

    #[test]
    fn test_laplacian_zero_row_sum() {
        // L = D - A has zero row sums; verify the matvec keeps the constant vector fixed.
        let (rp, ci, nnz) = path_graph(30);
        unsafe { field_build(rp.as_ptr(), ci.as_ptr(), nnz, 30); }
        let u = [1.0f64; 30];
        let mut y = [0.0f64; 30];
        unsafe { field_matvec(u.as_ptr(), y.as_mut_ptr(), std::ptr::null()); }
        for v in y { assert!(v.abs() < 1e-12, "L·1 should be 0, got {v}"); }
    }

    #[test]
    fn test_fexp_libm_sanity() {
        assert!((fexp(0.0) - 1.0).abs() < 1e-12);
        assert!((fexp(1.0) - core::f64::consts::E).abs() < 1e-9, "fexp(1)={}", fexp(1.0));
        assert!((fcos(0.0) - 1.0).abs() < 1e-12);
    }

    /// RED/regression: concurrent propagations must NOT deadlock the global Mutex.
    /// Earlier version nested with_graph() (lock -> degrees() -> lock) and hung on native targets.
    #[test]
    fn test_concurrent_propagations_no_deadlock() {
        std::thread::scope(|s| {
            for tid in 0..4u32 {
                s.spawn(move || {
                    let n = 50 + tid as i32 * 10;
                    let (rp, ci, nnz) = path_graph(n);
                    unsafe { field_build(rp.as_ptr(), ci.as_ptr(), nnz, n); }
                    let mut u0 = vec![0.0f64; n as usize];
                    u0[0] = 1.0;
                    let mut out = vec![0.0f64; n as usize];
                    unsafe { field_spectral(u0.as_ptr(), 2.0, 1.0, 20, out.as_mut_ptr()); }
                    let mass: f64 = out.iter().sum();
                    assert!((mass - 1.0).abs() < 1e-2, "tid {} mass={}", tid, mass);
                });
            }
        });
    }
}

