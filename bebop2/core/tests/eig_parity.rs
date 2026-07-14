//! eig_parity — the SILENT-DRIFT KILLER for bebop2's eigensolver.
//!
//! bebop2 previously had the Faddeev–LeVerrier + Durand–Kerner eigensolver living ONLY
//! inside `bebop_proto_cap/tests/mesh_consensus.rs` (mirrored, byte-for-byte, from the
//! dowiz `kernel/src/spectral.rs` engine). That is a dual-authority hazard: an edit to one
//! copy is invisible to the other until a consumer breaks. `core::linalg::eigenvalues` is
//! now the SINGLE authoritative solver. This test is the parity gate that makes a silent
//! divergence impossible:
//!
//!   * METHOD A — [`bebop2_core::linalg::eigenvalues`] (Faddeev–LeVerrier + Durand–Kerner),
//!     the canonical solver all consumers must route through.
//!   * METHOD B — [`bebop2_core::lyapunov::eigenvalues_general`], a COMPLETELY INDEPENDENT
//!     algorithm (Hessenberg reduction → Francis double-shift QR → real Schur form). Different
//!     math family, same answer. A drift in EITHER method is caught here.
//!
//! They must agree to 1e-6 on every reference matrix. On divergence the test FAILS loudly
//! with the matrix name + max |Δλ|, so a silent drift is impossible.
//!
//! This is the bebop2 analogue of the dowiz `markov.rs` parity gate that killed the same
//! dual-authority hazard on the dowiz side.

use bebop2_core::linalg::{self, Complex};
use bebop2_core::lyapunov;

// ── helpers ──────────────────────────────────────────────────────────────

/// Build a row-major `&[Vec<f64>]` matrix from a flat row-major slice.
fn mat(rows: &[&[f64]]) -> Vec<Vec<f64>> {
    rows.iter().map(|r| r.to_vec()).collect()
}

/// Flatten `&[Vec<f64>]` to a row-major `Vec<f64>` (what `eigenvalues_general` expects).
fn flat(m: &[Vec<f64>]) -> Vec<f64> {
    m.iter().flat_map(|r| r.iter().copied()).collect()
}

/// Sorted real parts of a complex spectrum.
fn reals(ev: &[Complex]) -> Vec<f64> {
    let mut v: Vec<f64> = ev.iter().map(|c| c.re).collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

/// Sorted real parts of `lyapunov::eigenvalues_general`'s `fft::Complex` spectrum (host only).
#[cfg(feature = "host")]
fn reals_general(m: &[Vec<f64>]) -> Vec<f64> {
    let n = m.len();
    let mut v: Vec<f64> = lyapunov::eigenvalues_general(&flat(m), n)
        .iter()
        .map(|c| c.re)
        .collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v
}

/// Max absolute elementwise diff of two equally-sized sorted real vectors.
fn max_diff(a: &[f64], b: &[f64]) -> f64 {
    assert_eq!(a.len(), b.len(), "mismatched eigenvalue count");
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f64::max)
}

/// Assert two real-eigenvalue vectors agree within `tol`, else fail loudly.
fn assert_agree(name: &str, a: &[f64], b: &[f64], tol: f64) {
    let d = max_diff(a, b);
    assert!(
        d < tol,
        "[EIG-PARITY] {name}: eigensolver divergence max|Δλ| = {:.3e} (tol {:.1e})",
        d,
        tol
    );
}

// ── references (hand-derived, do NOT trust a green test whose values weren't derived) ──

#[test]
fn eig_parity_2x2_swap_pm1() {
    // A = [[0,1],[1,0]] → eigenvalues {1, -1} (hand-derived: swap matrix, trace 0, det -1).
    let a = mat(&[&[0.0, 1.0], &[1.0, 0.0]]);
    let ev_a = reals(&linalg::eigenvalues(&a));
    let reference = vec![-1.0, 1.0];
    let ev_b = reals_general(&a);
    assert_agree("2x2 swap", &ev_a, &reference, 1e-6);
    assert_agree("2x2 swap (QR)", &ev_b, &reference, 1e-6);
    assert_agree("2x2 swap A-vs-B", &ev_a, &ev_b, 1e-6);
}

#[test]
fn eig_parity_2x2_complex_pair() {
    // A = [[0,-1],[1,0]] → eigenvalues {i, -i} (rotation; FL+DK gives complex pair).
    let a = mat(&[&[0.0, -1.0], &[1.0, 0.0]]);
    let ev = linalg::eigenvalues(&a);
    // both purely imaginary, magnitudes 1, opposite signs
    let mut ims: Vec<f64> = ev.iter().map(|c| c.im).collect();
    ims.sort_by(|x, y| x.partial_cmp(y).unwrap());
    assert!(
        (ims[0] + 1.0).abs() < 1e-6 && (ims[1] - 1.0).abs() < 1e-6,
        "eigs {{i,-i}}, got {ev:?}"
    );
    // real parts ~0
    for c in &ev {
        assert!(c.re.abs() < 1e-6, "rotation has no real part, got {c:?}");
    }
    // QR method agrees (complex pair).
    let qr = lyapunov::eigenvalues_general(&flat(&a), 2);
    let mut qr_im: Vec<f64> = qr.iter().map(|c| c.im).collect();
    qr_im.sort_by(|x, y| x.partial_cmp(y).unwrap());
    assert!(
        (qr_im[0] + 1.0).abs() < 1e-6 && (qr_im[1] - 1.0).abs() < 1e-6,
        "QR disagrees on rotation eigs, got {qr:?}"
    );
}

#[test]
fn eig_parity_3x3_path_laplacian() {
    // Path-graph P3 Laplacian L = [[1,-1,0],[-1,2,-1],[0,-1,1]] → eigenvalues {0,1,3}
    // (hand-derived: trace 4, det 0, characteristic polynomial λ(λ-1)(λ-3)=0).
    let a = mat(&[&[1.0, -1.0, 0.0], &[-1.0, 2.0, -1.0], &[0.0, -1.0, 1.0]]);
    let ev_a = reals(&linalg::eigenvalues(&a));
    let reference = vec![0.0, 1.0, 3.0];
    let ev_b = reals_general(&a);
    assert_agree("3x3 path Laplacian", &ev_a, &reference, 1e-6);
    assert_agree("3x3 path Laplacian (QR)", &ev_b, &reference, 1e-6);
    assert_agree("3x3 path Laplacian A-vs-B", &ev_a, &ev_b, 1e-6);
}

#[test]
fn eig_parity_4x4_path_laplacian() {
    // Path-graph P4 Laplacian → eigenvalues {0, 2-√2, 2, 2+√2}
    // (hand-derived: path-graph Laplacian eigenvalues are 2 - 2cos(kπ/(n+1)), k=1..n).
    let a = mat(&[
        &[1.0, -1.0, 0.0, 0.0],
        &[-1.0, 2.0, -1.0, 0.0],
        &[0.0, -1.0, 2.0, -1.0],
        &[0.0, 0.0, -1.0, 1.0],
    ]);
    let r2 = 2.0f64.sqrt();
    let reference = vec![0.0, 2.0 - r2, 2.0, 2.0 + r2];
    let ev_a = reals(&linalg::eigenvalues(&a));
    let ev_b = reals_general(&a);
    assert_agree("4x4 path Laplacian", &ev_a, &reference, 1e-6);
    assert_agree("4x4 path Laplacian (QR)", &ev_b, &reference, 1e-6);
    assert_agree("4x4 path Laplacian A-vs-B", &ev_a, &ev_b, 1e-6);
}

#[test]
fn eig_parity_3x3_cycle() {
    // 3-cycle adjacency (undirected) A = [[0,1,1],[1,0,1],[1,1,0]] → eigenvalues {2,-1,-1}
    // (hand-derived: all-ones matrix has eigenvalues 3,0,0; A = J − I → 2,-1,-1).
    let a = mat(&[&[0.0, 1.0, 1.0], &[1.0, 0.0, 1.0], &[1.0, 1.0, 0.0]]);
    let ev_a = reals(&linalg::eigenvalues(&a));
    let reference = vec![-1.0, -1.0, 2.0];
    let ev_b = reals_general(&a);
    assert_agree("3x3 cycle", &ev_a, &reference, 1e-6);
    assert_agree("3x3 cycle (QR)", &ev_b, &reference, 1e-6);
    assert_agree("3x3 cycle A-vs-B", &ev_a, &ev_b, 1e-6);
}
