//! active — active inference free-energy (spectral).
//!
//! In active inference a belief b (a categorical distribution over states) is governed by the
//! generative model's precision (inverse temperature) β and a Laplacian "surprise-diffusion"
//! coupling. Free energy  F = E[−ln p(o|s)] − H[b]  (energy − entropy). For a LINEAR-GAUSSIAN
//! belief the entropy is (1/2) ln|2πe P| over the covariance P; the GENERATIVE model's precision
//! operator is the graph LAPLACIAN (per architecture: "free-energy / precision = Laplacian of the
//! generative model = spectral; beliefs diffuse"). So:
//!
//! ```ignore
//! F = ½ 〈b, L b〉  −  H[b]            (energy = spectral curvature of the belief over the graph)
//!   = ½ Σ_k λ_k |〈b, φ_k〉|²  −  H[b]  (SPECTRAL form — the "wave" decomposition)
//! ```
//!
//! We compute F entirely in the spectral (eigenmode) basis: the energy is a pointwise sum over
//! eigenvalues λ_k, never forming the dense Laplacian. This matches a brute-force dense
//! `½ bᵀ L b − H` to 1e-9 on a reference graph. H[b] = −Σ_i b_i ln b_i (cross-entropy / entropy
//! for a normalized belief).
//!
//! f64 (precision math). Zero-dep, monomorphized, no RNG, no vtable.

#![allow(dead_code)]

use crate::field::LaplacianSpectrum;

/// Shannon entropy of a (normalized) belief distribution: H = −Σ b_i ln b_i.
/// For a zero entry, 0·ln0 := 0 (limit). Matches the standard discrete-entropy definition.
#[inline]
pub fn entropy(b: &[f64]) -> f64 {
    let mut h = 0.0f64;
    for &p in b {
        if p > 0.0 {
            h -= p * crate::math::fln(p);
        }
    }
    h
}

/// Brute-force (dense) free energy: F = ½ bᵀ L b − H[b], where L = D − A from CSR.
/// This is the ORACLE used by the spectral path's verification test.
pub fn free_energy_dense(b: &[f64], row_ptr: &[i32], col_idx: &[i32], degrees: &[f64]) -> f64 {
    let n = b.len();
    // L·b  (L = D − A)
    let mut lb = vec![0.0f64; n];
    for i in 0..n {
        let mut acc = degrees[i] * b[i];
        for k in row_ptr[i] as usize..row_ptr[i + 1] as usize {
            acc -= b[col_idx[k] as usize];
        }
        lb[i] = acc;
    }
    let mut energy = 0.0f64;
    for i in 0..n {
        energy += b[i] * lb[i];
    }
    energy *= 0.5;
    energy - entropy(b)
}

/// SPECTRAL free energy (directive 1): F = ½ Σ_k λ_k |⟨b, φ_k⟩|² − H[b].
/// The energy is computed ONLY from the Laplacian spectrum — no dense L is ever multiplied out
/// except to verify. This is the "tensor → spectrum" replacement for the precision operator.
pub fn free_energy_spectral(spec: &LaplacianSpectrum, b: &[f64]) -> f64 {
    let n = spec.n;
    let mut energy = 0.0f64;
    for k in 0..spec.num_modes {
        // coefficient c_k = ⟨b, φ_k⟩
        let mut c = 0.0f64;
        for i in 0..n {
            c += b[i] * spec.modes[k * n + i] as f64;
        }
        energy += spec.eigenvalues[k] * c * c;
    }
    energy *= 0.5;
    energy - entropy(b)
}

/// Precision-weighted belief update (one spectral diffusion step): the belief relaxes toward the
/// generative model by diffusing along the Laplacian (beliefs diffuse — architecture). Returns the
/// updated belief b' = b − dt·β·L b, normalized to sum 1 (a valid categorical belief). Uses the
/// f32 CSR matvec on the spectral kernel. B11: stable dt corridor default 0.02.
pub fn belief_diffuse_step(
    spec: &LaplacianSpectrum,
    b: &[f32],
    dt: f32,
    beta: f32,
    out: &mut [f32],
) {
    let n = b.len();
    let mut lb = vec![0.0f32; n];
    spec.matvec_f32(b, &mut lb, None);
    let dt = if dt <= 0.0 {
        crate::field::DT_STABLE
    } else {
        dt
    };
    let mut sum = 0.0f32;
    for i in 0..n {
        let v = b[i] - dt * beta * lb[i];
        out[i] = v;
        sum += v;
    }
    // normalize to a valid belief (sum = 1) — saturate-safe division.
    if sum.abs() > 1e-9 {
        let inv = 1.0 / sum;
        for v in out.iter_mut() {
            *v *= inv;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path_edges(n: u32) -> (Vec<(u32, u32)>, usize) {
        let edges: Vec<(u32, u32)> = (0..n - 1).map(|i| (i, i + 1)).collect();
        (edges, n as usize)
    }

    fn degrees(rp: &[i32], n: usize) -> Vec<f64> {
        (0..n).map(|i| (rp[i + 1] - rp[i]) as f64).collect()
    }

    #[test]
    fn free_energy_spectral_matches_dense() {
        // GREEN: spectral F equals dense ½ bᵀ L b − H to 1e-9 on a reference path graph.
        let (edges, nn) = path_edges(12);
        let spec = LaplacianSpectrum::from_edges(&edges, nn, nn);
        // a normalized belief
        let mut b = vec![0.0f64; nn];
        for i in 0..nn {
            b[i] = crate::math::fsin((i as f64 + 1.0) * 0.3).abs() + 0.05;
        }
        let s: f64 = b.iter().sum();
        for v in b.iter_mut() {
            *v /= s;
        }
        let spectral = free_energy_spectral(&spec, &b);
        let dense = free_energy_dense(&b, &spec.row_ptr, &spec.col_idx, &{
            let d: Vec<f64> = spec.degrees.iter().map(|&x| x as f64).collect();
            d
        });
        assert!(
            (spectral - dense).abs() < 1e-9,
            "spectral F={spectral} vs dense F={dense}"
        );
    }

    #[test]
    fn entropy_of_uniform() {
        // GREEN: H(uniform over n) = ln n.
        let n = 8usize;
        let b = vec![1.0 / n as f64; n];
        let h = entropy(&b);
        assert!(
            (h - (n as f64).ln()).abs() < 1e-12,
            "H(uniform)=ln n, got {h}"
        );
    }

    #[test]
    fn free_energy_red_breaks_on_belief_change() {
        // RED+GREEN: perturbing the belief MUST change F (proves the test is live).
        let (edges, nn) = path_edges(10);
        let spec = LaplacianSpectrum::from_edges(&edges, nn, nn);
        let mut b1 = vec![0.0f64; nn];
        let mut b2 = vec![0.0f64; nn];
        for i in 0..nn {
            b1[i] = (i as f64 + 1.0) / (nn as f64);
            b2[i] = b1[i];
        }
        let s: f64 = b1.iter().sum();
        for i in 0..nn {
            b1[i] /= s;
            b2[i] /= s;
        }
        b2[3] += 0.01; // perturb
        let s2: f64 = b2.iter().sum();
        for v in b2.iter_mut() {
            *v /= s2;
        }
        let f1 = free_energy_spectral(&spec, &b1);
        let f2 = free_energy_spectral(&spec, &b2);
        assert!(
            (f1 - f2).abs() > 1e-9,
            "belief must change F, diff={}",
            (f1 - f2).abs()
        );
    }

    #[test]
    fn belief_diffuse_stays_normalized() {
        // GREEN: after a diffusion step the belief is still a valid normalized distribution.
        let (edges, nn) = path_edges(16);
        let spec = LaplacianSpectrum::from_edges(&edges, nn, 4);
        let mut b = vec![0.0f32; nn];
        b[0] = 1.0; // impulse belief
        let mut out = vec![0.0f32; nn];
        belief_diffuse_step(&spec, &b, crate::field::DT_STABLE, 1.0, &mut out);
        let s: f32 = out.iter().sum();
        assert!(
            (s - 1.0).abs() < 1e-5,
            "belief must stay normalized, sum={s}"
        );
        for &v in &out {
            assert!(v >= -1e-6, "belief must stay non-negative, got {v}");
        }
    }
}
