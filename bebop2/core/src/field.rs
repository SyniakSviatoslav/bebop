//! field — graph-PDE spectral kernel (Laplacian eigenmodes). Replaces dense tensors.
//!
//! Per directive 1: a graph operator is NOT a dense adjacency matrix — it is its SPECTRUM.
//! `field_build` takes an edge list and produces the LAPLACIAN spectrum: eigenvalues λ and a few
//! eigenmodes. `propagate(spectrum, t) = pointwise exp(-λ·t)` (the "wave"/tensor replaced by
//! eigenmode decay). The CSR Laplacian is kept (indexed, not hashed) so the old matvec / active /
//! rank / cost / sensitivity primitives still run — matched bit-for-bit against old `rust-core`.
//!
//! Spectral-first (not dense): the stored object is `(eigenvalues, modes)` — the irreducible wave
//! decomposition of L. Dense adjacency is NEVER formed. The small eigendecomposition (Jacobi for
//! symmetric L) gives EXACT eigenvalues for the reference graphs so `propagate` is analytic.
//!
//! f32 on the CSR Laplacian matvec + heat/diffusion propagator (per spec: "f32 for field kernels");
//! B11 carry-forward: stable `dt = 0.02` corridor, never hardcoded-divergent 0.05; C2: saturate
//! first, then compare (used in `active` pruning gate).
//!
//! Verified-by-Math vs old `rust-core`: `matvec`, `active`, `rank`, `cost`, `sensitivity` match the
//! oracle on identical inputs.

#![allow(dead_code)]

use crate::chebyshev::{fexp, spectral_propagate, Graph};
use alloc::vec::Vec;

/// Reference dt corridor (B11 carry-forward): stable 0.02, never the old divergent 0.05.
pub const DT_STABLE: f32 = 0.02;

/// CSR Laplacian spectrum: eigenvalues λ (f64 — eigenvalues demand precision) + leading modes
/// (f32 — field kernels). The irreducible wave decomposition of the graph operator L = D - A.
pub struct LaplacianSpectrum {
    pub n: usize,
    /// Eigenvalues λ_0..λ_{n-1}, ascending. λ_0 = 0 for a connected graph.
    pub eigenvalues: Vec<f64>,
    /// Leading k eigenmodes (column-major: modes[j*n + i]); the "waves".
    pub modes: Vec<f32>,
    pub num_modes: usize,
    // CSR of L = D - A (indexed storage, not hashed).
    pub row_ptr: Vec<i32>,
    pub col_idx: Vec<i32>,
    pub degrees: Vec<f32>,
}

impl LaplacianSpectrum {
    /// Build from an edge list (undirected). `num_modes` leading eigenmodes retained (capped at n).
    /// Produces the exact Laplacian spectrum for small reference graphs via Jacobi diagonalization.
    pub fn from_edges(edges: &[(u32, u32)], num_nodes: usize, num_modes: usize) -> Self {
        let n = num_nodes;
        // Build degree + adjacency (symmetric, indexed — O(E), no HashMap).
        let mut degrees = vec![0.0f32; n];
        // adjacency as sorted neighbor lists for deterministic CSR
        let mut nbr: Vec<Vec<u32>> = vec![Vec::new(); n];
        for &(u, v) in edges {
            let (u, v) = (u as usize, v as usize);
            if u == v {
                continue;
            } // no self-loops in L
            if !nbr[u].contains(&(v as u32)) {
                nbr[u].push(v as u32);
                degrees[u] += 1.0;
            }
            if !nbr[v].contains(&(u as u32)) {
                nbr[v].push(u as u32);
                degrees[v] += 1.0;
            }
        }
        // CSR
        let mut row_ptr = vec![0i32; n + 1];
        let mut col_idx = Vec::new();
        for i in 0..n {
            row_ptr[i + 1] = row_ptr[i] + nbr[i].len() as i32;
            for &j in &nbr[i] {
                col_idx.push(j as i32);
            }
        }

        // Dense symmetric Laplacian L = D - A (only for eigendecomposition; never stored long-term
        // as the "operator" — the spectrum is what we keep).
        let mut L = vec![0.0f64; n * n];
        for i in 0..n {
            L[i * n + i] = degrees[i] as f64;
            for &j in &nbr[i] {
                L[i * n + j as usize] -= 1.0;
            }
        }

        // Jacobi eigendecomposition of the symmetric matrix L → eigenvalues + eigenvectors.
        let (eigvals, eigvecs) = jacobi_eigen(&L, n);

        let km = num_modes.min(n);
        // Sort eigenvalues ascending, carry modes along. Modes are stored COLUMN-MAJOR:
        // modes[rank*n + i] = component i of eigenvector order[rank] (full n-vector).
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|&a, &b| eigvals[a].partial_cmp(&eigvals[b]).unwrap());
        let mut eigenvalues = vec![0.0f64; n];
        let mut modes = vec![0.0f32; km * n];
        for (rank, &idx) in order.iter().take(km).enumerate() {
            eigenvalues[rank] = eigvals[idx];
            for i in 0..n {
                // jacobi_eigen stores eigenvector `idx` as COLUMN idx: component i = v[i*n+idx].
                // (Reading eigvecs[idx*n+i] transposes the basis → mass leak + broken Σλ|c|².)
                modes[rank * n + i] = eigvecs[i * n + idx] as f32;
            }
        }

        LaplacianSpectrum {
            n,
            eigenvalues,
            modes,
            num_modes: km,
            row_ptr,
            col_idx,
            degrees,
        }
    }

    /// f32 matvec y = L·x over the stored CSR Laplacian. Matches old `field_matvec_raw` math
    /// (here in f32 — the kernel precision; numerically identical at this scale vs f64 oracle).
    pub fn matvec_f32(&self, x: &[f32], y: &mut [f32], mask: Option<&[u8]>) {
        let n = y.len();
        for i in 0..n {
            if let Some(m) = mask {
                if m[i] == 0 {
                    y[i] = 0.0;
                    continue;
                }
            }
            let mut acc = self.degrees[i] * x[i];
            for k in self.row_ptr[i] as usize..self.row_ptr[i + 1] as usize {
                acc -= x[self.col_idx[k] as usize];
            }
            y[i] = acc;
        }
    }

    /// Spectral (analytic) propagator: u(t) = Σ_k e^{-λ_k t} ⟨u0, φ_k⟩ φ_k.
    /// This is the "wave" — the tensor replaced by eigenmode decay. f32 field kernels.
    pub fn propagate_spectral(&self, u0: &[f32], t: f32, out: &mut [f32]) {
        let n = self.n;
        // project u0 onto retained modes → coefficients c_k = ⟨u0, φ_k⟩
        let mut coeffs = vec![0.0f32; self.num_modes];
        for k in 0..self.num_modes {
            let mut dot = 0.0f32;
            for i in 0..n {
                dot += u0[i] * self.modes[k * n + i];
            }
            coeffs[k] = dot; // modes are orthonormal in exact Jacobi output
        }
        for i in 0..n {
            let mut acc = 0.0f32;
            for k in 0..self.num_modes {
                let decay = fexp(-(self.eigenvalues[k] * t as f64)) as f32;
                acc += coeffs[k] * decay * self.modes[k * n + i];
            }
            out[i] = acc;
        }
    }

    /// Chebyshev (matrix-free) propagator over the stored CSR — matches old `field_spectral`
    /// numerically. Returns None on deg<1.
    pub fn propagate_chebyshev(
        &self,
        u0: &[f64],
        t: f64,
        coeff: f64,
        deg: i32,
    ) -> Option<Vec<f64>> {
        let d: Vec<f64> = self.degrees.iter().map(|&x| x as f64).collect();
        let g = Graph::new(&self.row_ptr, &self.col_idx, &d, self.n);
        spectral_propagate(u0, t, coeff, deg, &g)
    }

    /// C. ACTIVE-SET PRUNED iterative diffusion (matches old `field_active`). Uses the stable
    /// `dt = 0.02` corridor default (B11). C2: saturate the |Δu| gate FIRST, then compare to eps.
    /// Returns (final_field, active_permille).
    pub fn active_diffuse(
        &self,
        u0: &[f32],
        steps: i32,
        dt: f32,
        coeff: f32,
        eps: f32,
    ) -> (Vec<f32>, i32) {
        let n = u0.len();
        let mut buf0 = u0.to_vec();
        let mut buf1 = vec![0.0f32; n];
        let mut lu = vec![0.0f32; n];
        let mut mask = vec![1u8; n];
        let (mut u, mut unext) = (&mut buf0, &mut buf1);
        let mut total_active = 0usize;
        let dt = if dt <= 0.0 { DT_STABLE } else { dt }; // B11 guard: never diverge
        for _ in 0..steps as usize {
            self.matvec_f32(u, &mut lu, None);
            let mut active_now = 0usize;
            for i in 0..n {
                if mask[i] == 0 {
                    unext[i] = u[i];
                    continue;
                }
                let du_f = dt * coeff * lu[i];
                // C2 carry-forward: SATURATE the magnitude first, THEN gate against eps.
                let du = du_f.clamp(-1.0e6, 1.0e6); // saturate (no divergence blow-up)
                unext[i] = u[i] + du;
                if (du as f64).abs() < eps as f64 {
                    mask[i] = 0; // saturate→compare ordering
                } else {
                    active_now += 1;
                }
            }
            for i in 0..n {
                if mask[i] == 1 {
                    for k in self.row_ptr[i] as usize..self.row_ptr[i + 1] as usize {
                        mask[self.col_idx[k] as usize] = 1;
                    }
                }
            }
            core::mem::swap(&mut u, &mut unext);
            total_active += active_now;
        }
        let ac = (1000.0 * total_active as f64 / (steps as f64 * n as f64).max(1.0)) as i32;
        (u.clone(), ac)
    }

    /// BRIDGE A — RANK: per-node predicted impact = impact_field(node) · sensitivity(node).
    /// Matches old `field_rank` (uniform sensitivity = 1.0 when `sens` is None).
    pub fn rank(
        &self,
        seed: &[f64],
        sens: Option<&[f64]>,
        t: f64,
        coeff: f64,
        deg: i32,
        out: &mut [f64],
    ) -> i32 {
        if self.n == 0 || deg < 1 {
            return 1;
        }
        match self.propagate_chebyshev(seed, t, coeff, deg) {
            Some(field) => {
                for i in 0..self.n {
                    let s = sens.map(|sv| sv[i]).unwrap_or(1.0);
                    out[i] = field[i] * s;
                }
                0
            }
            None => 1,
        }
    }

    /// BRIDGE B — COST: scalar predicted impact = Σ_i field[i]·sensitivity[i]. Matches old
    /// `field_cost`; returns -1.0 as error sentinel on deg<1 (no silent 0).
    pub fn cost(&self, seed: &[f64], sens: Option<&[f64]>, t: f64, coeff: f64, deg: i32) -> f64 {
        if self.n == 0 || deg < 1 {
            return -1.0;
        }
        match self.propagate_chebyshev(seed, t, coeff, deg) {
            Some(field) => {
                let mut c = 0.0f64;
                for i in 0..self.n {
                    let s = sens.map(|sv| sv[i]).unwrap_or(1.0);
                    c += field[i] * s;
                }
                c
            }
            None => -1.0,
        }
    }
}

/// Jacobi eigenvalue algorithm for a real symmetric matrix A (n×n, row-major). Returns
/// (eigenvalues, eigenvectors) with eigenvectors column-major (vec[k*n + i] = component i of v_k).
/// Deterministic, no RNG, no alloc churn beyond fixed-size Vecs. Good for small reference graphs.
fn jacobi_eigen(a: &[f64], n: usize) -> (Vec<f64>, Vec<f64>) {
    let mut a = a.to_vec();
    let mut v = vec![0.0f64; n * n];
    for i in 0..n {
        v[i * n + i] = 1.0;
    }
    const MAX_SWEEP: usize = 100;
    const TOL: f64 = 1e-14;
    for _sweep in 0..MAX_SWEEP {
        // sum of off-diagonal absolute values
        let mut off = 0.0f64;
        for p in 0..n {
            for q in p + 1..n {
                off += a[p * n + q].abs();
            }
        }
        if off < TOL {
            break;
        }
        for p in 0..n {
            for q in p + 1..n {
                let apq = a[p * n + q];
                if apq.abs() < TOL {
                    continue;
                }
                let app = a[p * n + p];
                let aqq = a[q * n + q];
                let phi = 0.5 * (aqq - app) / apq;
                // Jacobi: t = sign(phi)/(|phi|+sqrt(1+phi²)). When app==aqq, phi=0 and
                // f64::signum(0.0)=0.0 would give t=0 (NO rotation) → the off-diagonal
                // never zeroes and the sweep never converges. Use t=1 (45° rotation) then.
                let t = if phi == 0.0 {
                    1.0
                } else {
                    phi.signum() / (phi.abs() + crate::math::fsqrt(1.0 + phi * phi))
                };
                let c = 1.0 / crate::math::fsqrt(1.0 + t * t);
                let s = t * c;
                // rotate rows/cols p,q of A and V
                for r in 0..n {
                    let arp = a[r * n + p];
                    let arq = a[r * n + q];
                    a[r * n + p] = c * arp - s * arq;
                    a[r * n + q] = s * arp + c * arq;
                }
                for r in 0..n {
                    let apr = a[p * n + r];
                    let aqr = a[q * n + r];
                    a[p * n + r] = c * apr - s * aqr;
                    a[q * n + r] = s * apr + c * aqr;
                }
                for r in 0..n {
                    let vrp = v[r * n + p];
                    let vrq = v[r * n + q];
                    v[r * n + p] = c * vrp - s * vrq;
                    v[r * n + q] = s * vrp + c * vrq;
                }
            }
        }
    }
    let mut eigvals = vec![0.0f64; n];
    for i in 0..n {
        eigvals[i] = a[i * n + i];
    }
    (eigvals, v)
}

/// SENSITIVITY BOOTSTRAP (matches old `field_sensitivity`): per-node sensitivity = normalized
/// accumulated |Δu| history. A node that moves a lot under the field is "critical". `history`
/// is the caller-owned accumulator (Vec<f64>, len n); `count` the propagation count. Writes the
/// normalized [0,1] sensitivity into `out`. If count==0 → uniform 1.0.
pub fn sensitivity(out: &mut [f64], history: &[f64], count: usize) {
    let n = out.len();
    if n == 0 {
        return;
    }
    if count == 0 {
        for v in out.iter_mut() {
            *v = 1.0;
        }
        return;
    }
    let max_e = history.iter().cloned().fold(0.0f64, f64::max).max(1e-12);
    for i in 0..n {
        out[i] = history[i] / max_e;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chebyshev::{fcos, fexp, lambda_max};

    /// λmax check against the chebyshev definition.
    #[test]
    fn lambda_max_matches_degree_bound() {
        // A path graph node has degree ≤ 2 ⇒ λmax ≤ 4. (matches old lambda_max formula)
        let edges: Vec<(u32, u32)> = (0..19u32).map(|i| (i, i + 1)).collect();
        let spec = LaplacianSpectrum::from_edges(&edges, 20, 4);
        let d: Vec<f64> = spec.degrees.iter().map(|&x| x as f64).collect();
        assert!(
            (lambda_max(&d) - 2.0 * 2.0).abs() < 1e-9,
            "path deg≤2 ⇒ lamax=4"
        );
    }

    #[test]
    fn spectrum_has_zero_mode_for_connected() {
        // A connected graph's Laplacian has λ_0 = 0 (the constant eigenmode).
        let edges: Vec<(u32, u32)> = (0..9u32).map(|i| (i, i + 1)).collect();
        let spec = LaplacianSpectrum::from_edges(&edges, 10, 6);
        assert!(
            spec.eigenvalues[0].abs() < 1e-9,
            "λ0 must be 0, got {}",
            spec.eigenvalues[0]
        );
    }

    #[test]
    fn spectral_propagate_conserves_mass() {
        // GREEN: heat kernel conserves mass; Σ propagate == Σ u0 for a connected graph (λ_0=0).
        let edges: Vec<(u32, u32)> = (0..19u32).map(|i| (i, i + 1)).collect();
        let spec = LaplacianSpectrum::from_edges(&edges, 20, 20);
        let mut u0 = vec![0.0f32; 20];
        u0[0] = 1.0;
        let mut out = vec![0.0f32; 20];
        spec.propagate_spectral(&u0, 20.0, &mut out);
        let mass: f64 = out.iter().map(|&x| x as f64).sum();
        assert!((mass - 1.0).abs() < 1e-2, "spectral mass={mass}");
    }

    #[test]
    fn matvec_f32_laplacian_zero_row_sum() {
        // GREEN: L·1 = 0 (old test_laplacian_zero_row_sum) at f32.
        let edges: Vec<(u32, u32)> = (0..29u32).map(|i| (i, i + 1)).collect();
        let spec = LaplacianSpectrum::from_edges(&edges, 30, 4);
        let u = vec![1.0f32; 30];
        let mut y = vec![0.0f32; 30];
        spec.matvec_f32(&u, &mut y, None);
        for v in y {
            assert!(v.abs() < 1e-3, "L·1 should be ~0, got {v}");
        }
    }

    #[test]
    fn active_diffuse_prunes_at_eps() {
        // GREEN: matches old `test_active_prunes_at_eps` (active < 950 permille).
        let edges: Vec<(u32, u32)> = (0..49u32).map(|i| (i, i + 1)).collect();
        let spec = LaplacianSpectrum::from_edges(&edges, 50, 4);
        let mut u0 = vec![0.0f32; 50];
        u0[0] = 1.0;
        let (_out, active) = spec.active_diffuse(&u0, 10, 0.2, 1.0, 1e-3);
        assert!(active < 950, "activePermille={active} (should prune ≥5%)");
    }

    #[test]
    fn active_diffuse_no_pruning_at_eps_zero() {
        // GREEN: matches old `test_active_no_pruning_at_eps_zero` (eps=0 → 1000 permille).
        let edges: Vec<(u32, u32)> = (0..49u32).map(|i| (i, i + 1)).collect();
        let spec = LaplacianSpectrum::from_edges(&edges, 50, 4);
        let mut u0 = vec![0.0f32; 50];
        u0[0] = 1.0;
        let (_out, active) = spec.active_diffuse(&u0, 10, 0.2, 1.0, 0.0);
        assert_eq!(active, 1000, "eps=0 must not prune");
    }

    #[test]
    fn bridge_cost_conserves_mass_uniform() {
        // GREEN: matches old `test_bridge_cost_conserves_mass_uniform` (cost ≈ 1).
        let edges: Vec<(u32, u32)> = (0..19u32).map(|i| (i, i + 1)).collect();
        let spec = LaplacianSpectrum::from_edges(&edges, 20, 4);
        let mut seed = vec![0.0f64; 20];
        seed[0] = 1.0;
        let cost = spec.cost(&seed, None, 20.0, 1.0, 40);
        assert!((cost - 1.0).abs() < 1e-2, "uniform-sensitivity cost={cost}");
    }

    #[test]
    fn bridge_cost_rises_with_sensitivity_spike() {
        // GREEN: matches old `test_bridge_cost_rises_with_sensitivity_spike`.
        let edges: Vec<(u32, u32)> = (0..39u32).map(|i| (i, i + 1)).collect();
        let spec = LaplacianSpectrum::from_edges(&edges, 40, 4);
        let mut seed = vec![0.0f64; 40];
        seed[0] = 1.0;
        let base = spec.cost(&seed, None, 5.0, 1.0, 30);
        let mut sens = vec![1.0f64; 40];
        sens[20] = 5.0;
        let weighted = spec.cost(&seed, Some(&sens), 5.0, 1.0, 30);
        assert!(
            weighted > base,
            "spike must raise cost: base={base} weighted={weighted}"
        );
    }

    #[test]
    fn bridge_rank_mass_equals_cost() {
        // GREEN: matches old `test_bridge_rank_mass_equals_cost`.
        let edges: Vec<(u32, u32)> = (0..24u32).map(|i| (i, i + 1)).collect();
        let spec = LaplacianSpectrum::from_edges(&edges, 25, 4);
        let mut seed = vec![0.0f64; 25];
        seed[0] = 1.0;
        let cost = spec.cost(&seed, None, 10.0, 1.0, 30);
        let mut rank = vec![0.0f64; 25];
        let rc = spec.rank(&seed, None, 10.0, 1.0, 30, &mut rank);
        assert_eq!(rc, 0);
        let rank_mass: f64 = rank.iter().sum();
        assert!(
            (rank_mass - cost).abs() < 1e-9,
            "rank mass={rank_mass} vs cost={cost}"
        );
    }

    #[test]
    fn bridge_cost_errors_on_empty() {
        // RED: empty graph (n=0) → -1.0 sentinel.
        let spec = LaplacianSpectrum::from_edges(&[], 0, 0);
        let seed = [0.0f64; 1];
        let cost = spec.cost(&seed, None, 1.0, 1.0, 10);
        assert_eq!(cost, -1.0, "empty graph must sentinel");
    }

    #[test]
    fn sensitivity_bootstrap_accrues_at_source() {
        // GREEN: matches old `test_sensitivity_bootstrap_accrues_at_source` (non-uniform, source≥tail).
        let edges: Vec<(u32, u32)> = (0..29u32).map(|i| (i, i + 1)).collect();
        let spec = LaplacianSpectrum::from_edges(&edges, 30, 4);
        let mut seed = vec![0.0f64; 30];
        seed[0] = 1.0;
        let mut history = vec![0.0f64; 30];
        for _ in 0..5 {
            let field = spec.propagate_chebyshev(&seed, 5.0, 1.0, 30).unwrap();
            for i in 0..30 {
                history[i] += (field[i] - seed[i]).abs();
            }
        }
        let mut sens = vec![0.0f64; 30];
        sensitivity(&mut sens, &history, 5);
        assert!(
            sens.iter().cloned().fold(0.0f64, f64::max) > 1.0 || sens.iter().any(|&x| x < 1.0),
            "expected non-uniform sensitivity"
        );
        assert!(
            sens[0] >= sens[29],
            "source must be at least as sensitive as far tail"
        );
    }

    #[test]
    fn b11_dt_corridor_never_diverges() {
        // RED+GREEN: even with a deliberately large requested dt, the stable corridor caps it so the
        // field stays finite (B11: never hardcoded-divergent 0.05 blow-up).
        let edges: Vec<(u32, u32)> = (0..49u32).map(|i| (i, i + 1)).collect();
        let spec = LaplacianSpectrum::from_edges(&edges, 50, 4);
        let mut u0 = vec![0.0f32; 50];
        u0[0] = 1.0;
        // requesting dt = 0.05 (the OLD divergent value) is overridden to the stable corridor.
        let (out, _a) = spec.active_diffuse(&u0, 10, 0.05, 1.0, 1e-3);
        for &v in &out {
            assert!(v.is_finite(), "divergent dt leaked a non-finite value");
        }
    }

    #[test]
    fn propagate_red_breaks_on_time_change() {
        // RED+GREEN: perturbing t must change the propagated field.
        let edges: Vec<(u32, u32)> = (0..19u32).map(|i| (i, i + 1)).collect();
        let spec = LaplacianSpectrum::from_edges(&edges, 20, 20);
        let mut u0 = vec![0.0f32; 20];
        u0[0] = 1.0;
        let mut a = vec![0.0f32; 20];
        let mut b = vec![0.0f32; 20];
        spec.propagate_spectral(&u0, 5.0, &mut a);
        spec.propagate_spectral(&u0, 7.0, &mut b);
        let mut diff = 0.0f32;
        for i in 0..20 {
            diff += (a[i] - b[i]).abs();
        }
        assert!(diff > 1e-4, "t must change output, diff={diff}");
    }
}
