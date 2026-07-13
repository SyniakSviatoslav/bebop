//! Coherence — the quantum-oscillator / wave-interference layer over the field.
//!
//! Analogy made deterministic: the field core propagates a HEAT-KERNEL impulse
//! `u(t) = exp(-L·t)·u0`. That is a classical diffusion wavefront. Two
//! agents are two seeds; their wavefunctions SUPERPOSE. This module computes
//! the constructive interference `|ψ₁ + ψ₂|²` and destructive `|ψ₁ − ψ₂|²`
//! over the propagated field — real superposition on the existing kernel.
//!
//! NO LLM, NO rng, NO wall-clock. The interference math is exact linear
//! algebra on the propagated vectors. RED+GREEN tests prove aligned seeds
//! constructively reinforce a peak while anti-aligned seeds destructively
//! cancel a node to ~0.

/// Propagate a single impulse seed `u0` under the heat kernel for `t` steps
/// with coeff `coeff` over an undirected Laplacian built from `edges`.
/// Returns the n-vector `u(t)`. (Cheap re-implementation of the core's
/// active diffusion so this module stays dependency-light and testable.)
pub fn propagate(u0: &[f64], edges: &[(usize, usize)], t: f64, coeff: f64) -> Vec<f64> {
    let n = u0.len();
    if n == 0 {
        return vec![];
    }
    // Build degree (D) and adjacency for L = D - A.
    let mut deg = vec![0.0f64; n];
    for &(a, b) in edges {
        if a < n {
            deg[a] += 1.0;
        }
        if b < n {
            deg[b] += 1.0;
        }
    }
    // BP-04 (variant A): integrate the HEAT equation u̇ = −coeff·L·u (decay/diffusion),
    // NOT u̇ = +coeff·L·u (anti-diffusion / growth). Use a proper multi-step explicit
    // Euler with a stable step `dt_stable = 0.02` (B11 corridor) so the discretisation
    // stays in the mass-conserving, non-negative regime. steps = ceil(t / dt_stable).
    const DT_STABLE: f64 = 0.02;
    let mut u = u0.to_vec();
    if t <= 0.0 {
        return u;
    }
    let steps = (t / DT_STABLE).ceil().max(1.0) as usize;
    let dt = t / steps as f64;
    for _ in 0..steps {
        let mut lu = vec![0.0f64; n];
        for i in 0..n {
            let mut acc = deg[i] * u[i];
            for &(a, b) in edges {
                if a == i {
                    acc -= u[b.min(n - 1)];
                }
                if b == i {
                    acc -= u[a.min(n - 1)];
                }
            }
            lu[i] = acc;
        }
        for i in 0..n {
            // −coeff·L·u : diffusion (mass-conserving, neighbors stay non-negative).
            u[i] -= dt * coeff * lu[i];
        }
    }
    u
}

/// Coherent superposition of two seeds. Returns (constructive, destructive)
/// n-vectors: `|ψ₁+ψ₂|²` and `|ψ₁−ψ₂|²`.
pub fn interfere(
    seed1: &[f64],
    seed2: &[f64],
    edges: &[(usize, usize)],
    t: f64,
    coeff: f64,
) -> (Vec<f64>, Vec<f64>) {
    let p1 = propagate(seed1, edges, t, coeff);
    let p2 = propagate(seed2, edges, t, coeff);
    let n = p1.len().min(p2.len());
    let mut con = vec![0.0f64; n];
    let mut des = vec![0.0f64; n];
    for i in 0..n {
        let s = p1[i] + p2[i];
        let d = p1[i] - p2[i];
        con[i] = s * s; // |ψ₁+ψ₂|² constructive
        des[i] = d * d; // |ψ₁−ψ₂|² destructive
    }
    (con, des)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aligned_seeds_constructive_peak() {
        // RED+GREEN: two IDENTICAL seeds interfere constructively.
        // The peak of |ψ₁+ψ₂|² must exceed the isolated peak (scale ×4 at the
        // seeded node), while a non-seeded node stays low.
        let edges = [(0usize, 1), (1, 2), (2, 3)];
        let s1 = [1.0f64, 0.0, 0.0, 0.0];
        let s2 = [1.0f64, 0.0, 0.0, 0.0];
        let (con, _des) = interfere(&s1, &s2, &edges, 1.0, 0.5);
        // constructive at node 0 is exactly (p+p)² = 4·p² of a single propagated
        // seed (superposition math, sign-agnostic). Compare to the true kernel
        // amplitude so the assertion doesn't depend on anti-diffusion growth.
        let p = propagate(&s1, &edges, 1.0, 0.5);
        let expected = (2.0 * p[0]) * (2.0 * p[0]);
        assert!(
            (con[0] - expected).abs() < 1e-9,
            "constructive peak must be 4·p[0]²={}, got {}",
            expected,
            con[0]
        );
        // a far node should stay small
        assert!(con[3] < con[0], "energy should be concentrated near seed");
    }

    #[test]
    fn antialigned_seeds_destructive_cancel() {
        // RED: two OPPOSITE seeds at the SAME node cancel it to ~0
        // (destructive interference): |1 - (-1)|² at neighbors, |1+(-1)|²=0 at node.
        let edges = [(0usize, 1), (1, 2)];
        let s1 = [1.0f64, 0.0, 0.0];
        let s2 = [-1.0f64, 0.0, 0.0]; // opposite sign, same node
        let (con, des) = interfere(&s1, &s2, &edges, 0.5, 0.5);
        // at node 0 the constructive term is |p + (-p)|² = 0 (cancels, sign-agnostic)
        assert!(con[0] < 1e-6, "node 0 must cancel to ~0, got {}", con[0]);
        // destructive there is |p - (-p)|² = (2p)² > 0; compare to true amplitude
        let p = propagate(&s1, &edges, 0.5, 0.5);
        let expected = (2.0 * p[0]) * (2.0 * p[0]);
        assert!(
            (des[0] - expected).abs() < 1e-9 && des[0] > 0.0,
            "destructive must be 4·p[0]²={}, got {}",
            expected,
            des[0]
        );
    }

    #[test]
    fn propagate_is_deterministic() {
        // GREEN: same inputs → identical output (no rng/timestamp).
        let edges = [(0usize, 1), (1, 2)];
        let s = [1.0f64, 0.0, 0.0];
        let a = propagate(&s, &edges, 1.0, 0.5);
        let b = propagate(&s, &edges, 1.0, 0.5);
        assert_eq!(a, b);
    }

    #[test]
    fn diffusion_conserves_mass_and_stays_nonnegative() {
        // BP-04 RED→GREEN: seed [1,0,0,0] on a 4-node path.
        // BUGGY (anti-diffusion, u̇=+cLu): mass grows past 1 and neighbors go
        // negative (e.g. [1.5,−0.5,…]). FIXED (u̇=−cLu, multi-step Euler):
        // Σu ≈ 1 (mass-conserving, tol 1e-2), all entries ≥ 0, seed decays.
        let edges = [(0usize, 1), (1, 2), (2, 3)];
        let u0 = [1.0f64, 0.0, 0.0, 0.0];
        let u = propagate(&u0, &edges, 1.0, 0.5);

        let mass: f64 = u.iter().sum();
        assert!(
            (mass - 1.0).abs() < 1e-2,
            "diffusion must conserve mass Σu≈1, got {}",
            mass
        );
        for (i, &v) in u.iter().enumerate() {
            assert!(v >= -1e-9, "node {} must stay non-negative, got {}", i, v);
        }
        assert!(
            u[0] < u0[0],
            "seed amplitude must decay, got {} (was {})",
            u[0],
            u0[0]
        );
        // energy should have spread to the immediate neighbor
        assert!(
            u[1] > 0.0,
            "neighbor should receive positive amplitude, got {}",
            u[1]
        );
    }
}
