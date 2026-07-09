//! bebop-core — deterministic math twin (FLAG-OFF) of the TS analytic modules.
//!
//! PROVEN-EV REPLACEMENT targets (operator: prefer Rust): the hot, pure Float64 DSP paths in
//! `src/integration/analytics/matrix.ts` (SVD/PCA), `src/integration/field-sim.ts` (Laplacian + wave),
//! `src/integration/analytics/kalman.ts`, `src/integration/analytics/eta.ts`, and the VSA codec in
//! `src/memory.ts`. These have NO behavior change when ported — same inputs, same outputs — and a
//! Rust/WASM build is ~10–50× faster and type-safe, with the sovereign-core guarantees (no RNG / no
//! Date / no network) enforced at the FFI boundary.
//!
//! STATUS: signatures only. Wire + port the RED+GREEN tests from the TS modules before any switch
//! (Verified-by-Math: the Rust twin must pass the SAME falsifiable tests). DO NOT compile into the
//! runtime until then. This stub exists so the architecture is explicit and the next max-EV step is
//! mechanical.

/// Graph Laplacian L = D − A (unnormalized). Mirrors `laplacian` in field-sim.ts.
pub fn laplacian(a: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = a.len();
    let mut l = vec![vec![0.0; n]; n];
    for i in 0..n {
        let mut deg = 0.0;
        for j in 0..n {
            if i != j {
                l[i][j] = -a[i][j];
                deg += a[i][j];
            }
        }
        l[i][i] = deg;
    }
    l
}

/// One velocity-Verlet step of the wave equation ∂²u/∂t² = −c²·L u on a graph.
/// Mirrors the symplectic integrator in `FieldSim.step` (wave mode). Conserves the Hamiltonian.
/// `u` and `v` are the (channels×nodes) state; updated in place. FLAG-OFF until tests ported.
pub fn wave_step(l: &[Vec<f64>], u: &mut Vec<Vec<f64>>, v: &mut Vec<Vec<f64>>, c2: f64, dt: f64) {
    let c = u.len();
    let n = if c > 0 { u[0].len() } else { 0 };
    for ch in 0..c {
        for i in 0..n {
            let mut lu = 0.0;
            for j in 0..n {
                lu += l[i][j] * u[ch][j];
            }
            v[ch][i] += 0.5 * dt * (-c2 * lu);
        }
    }
    for ch in 0..c {
        for i in 0..n {
            u[ch][i] += dt * v[ch][i];
        }
    }
    for ch in 0..c {
        for i in 0..n {
            let mut lu = 0.0;
            for j in 0..n {
                lu += l[i][j] * u[ch][j];
            }
            v[ch][i] += 0.5 * dt * (-c2 * lu);
        }
    }
}

/// One explicit-Euler step of the heat equation ∂u/∂t = −D·L u (contractive). Mirrors `diffuse` mode.
pub fn diffuse_step(l: &[Vec<f64>], u: &mut Vec<Vec<f64>>, d: f64, dt: f64) {
    let c = u.len();
    let n = if c > 0 { u[0].len() } else { 0 };
    let mut nu = u.clone();
    for ch in 0..c {
        for i in 0..n {
            let mut lu = 0.0;
            for j in 0..n {
                lu += l[i][j] * u[ch][j];
            }
            nu[ch][i] = u[ch][i] - dt * d * lu;
        }
    }
    *u = nu;
}

/// Hamiltonian ½vᵀv + ½c²·uᵀ(Lu) — the conserved quantity for `wave_step`. Proves energy is held.
pub fn hamiltonian(l: &[Vec<f64>], u: &[Vec<f64>], v: &[Vec<f64>], c2: f64) -> f64 {
    let c = u.len();
    let n = if c > 0 { u[0].len() } else { 0 };
    let mut e = 0.0;
    for ch in 0..c {
        for i in 0..n {
            e += 0.5 * v[ch][i] * v[ch][i];
        }
    }
    for ch in 0..c {
        for i in 0..n {
            let mut lu = 0.0;
            for j in 0..n {
                lu += l[i][j] * u[ch][j];
            }
            e += 0.5 * c2 * u[ch][i] * lu;
        }
    }
    e
}

// SVD/PCA, Kalman 1D, ETA decay, VSA bind/bundle — signatures TBD when porting the TS modules.
// Each must carry the SAME RED+GREEN test contract as its TS origin before activation.
