//! MATHX — §2 numerical/control-theory primitives from the master dossier,
//! implemented natively (0 deps) and RED+GREEN tested. Companion to `stabilizer`
//! (Lyapunov/MRAC) and `enrich` (Pareto/opt-algos). Every function is a closed
//! form or a deterministic finite scheme — no RNG, no clock.

/// ─────────────────────────────────────────────────────────────────────────
/// §2 DIVERGENCE of a 2-D vector field F=(fx,fy) at (x,y), central differences.
/// div F = ∂fx/∂x + ∂fy/∂y. `h` is the step. GREEN: a radial field F=(x,y) has
/// divergence 2 everywhere. RED: a rotational field F=(-y,x) has divergence 0.
/// ─────────────────────────────────────────────────────────────────────────
pub fn divergence_2d(
    fx: impl Fn(f64, f64) -> f64,
    fy: impl Fn(f64, f64) -> f64,
    x: f64,
    y: f64,
    h: f64,
) -> f64 {
    let dfx_dx = (fx(x + h, y) - fx(x - h, y)) / (2.0 * h);
    let dfy_dy = (fy(x, y + h) - fy(x, y - h)) / (2.0 * h);
    dfx_dx + dfy_dy
}

/// ─────────────────────────────────────────────────────────────────────────
/// §2 FIRST-ORDER TRANSFER FUNCTION step response. H(s)=K/(τs+1) driven by a
/// unit step → y(t)=K·(1−e^{−t/τ}). Closed form, exact. `tau`>0.
/// ─────────────────────────────────────────────────────────────────────────
pub fn first_order_step_response(k: f64, tau: f64, t: f64) -> f64 {
    if tau <= 0.0 {
        // degenerate: instantaneous system → jumps straight to K for t>=0
        return if t >= 0.0 { k } else { 0.0 };
    }
    k * (1.0 - (-t / tau).exp())
}

/// Time to reach a fraction `frac` (0<frac<1) of the final value K for a
/// first-order system: t = -τ·ln(1-frac). (e.g. frac=0.632 → ~τ.)
pub fn first_order_settling_time(tau: f64, frac: f64) -> Option<f64> {
    if tau <= 0.0 || !(0.0..1.0).contains(&frac) {
        return None;
    }
    Some(-tau * (1.0 - frac).ln())
}

/// ─────────────────────────────────────────────────────────────────────────
/// §2 LAGRANGE INTERPOLATION — evaluate the unique degree-(n-1) polynomial
/// through `pts=(xi,yi)` at `x`. GREEN: passes exactly through every node.
/// RED: duplicate x values → None (ill-posed, no polynomial).
/// ─────────────────────────────────────────────────────────────────────────
pub fn lagrange_interp(pts: &[(f64, f64)], x: f64) -> Option<f64> {
    let n = pts.len();
    if n == 0 {
        return None;
    }
    // reject duplicate abscissae (division by zero in the basis)
    for i in 0..n {
        for j in (i + 1)..n {
            if (pts[i].0 - pts[j].0).abs() < f64::EPSILON {
                return None;
            }
        }
    }
    let mut acc = 0.0;
    for (i, &(xi, yi)) in pts.iter().enumerate() {
        let mut li = 1.0;
        for (j, &(xj, _)) in pts.iter().enumerate() {
            if i != j {
                li *= (x - xj) / (xi - xj);
            }
        }
        acc += yi * li;
    }
    Some(acc)
}

/// ─────────────────────────────────────────────────────────────────────────
/// §2 LIMIT-CYCLE DETECTION — given a scalar trajectory (e.g. one state
/// coordinate over time), classify the asymptotic behaviour by looking at the
/// amplitude of the LAST third vs the middle third:
///   - Fixed point: late amplitude ≈ 0 (settled).
///   - Limit cycle: late amplitude bounded and ≈ mid amplitude (sustained osc).
///   - Divergent: late amplitude ≫ mid amplitude (growing).
/// ─────────────────────────────────────────────────────────────────────────
#[derive(Debug, PartialEq, Eq)]
pub enum Regime {
    FixedPoint,
    LimitCycle,
    Divergent,
    Undetermined,
}

fn amplitude(seg: &[f64]) -> f64 {
    if seg.is_empty() {
        return 0.0;
    }
    let mx = seg.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let mn = seg.iter().cloned().fold(f64::INFINITY, f64::min);
    mx - mn
}

pub fn classify_trajectory(traj: &[f64], settle_eps: f64) -> Regime {
    let n = traj.len();
    if n < 9 {
        return Regime::Undetermined;
    }
    let third = n / 3;
    let mid = &traj[third..2 * third];
    let late = &traj[2 * third..];
    let a_mid = amplitude(mid);
    let a_late = amplitude(late);
    if a_late < settle_eps {
        Regime::FixedPoint
    } else if a_late > 2.0 * a_mid.max(settle_eps) {
        Regime::Divergent
    } else {
        // bounded, sustained oscillation of comparable amplitude
        Regime::LimitCycle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn divergence_radial_vs_rotational() {
        // GREEN: radial field F=(x,y) → div = 2 everywhere
        let d = divergence_2d(|x, _| x, |_, y| y, 3.0, -1.0, 1e-4);
        assert!((d - 2.0).abs() < 1e-6, "radial div should be 2, got {d}");
        // RED: rotational field F=(-y,x) → div = 0 (incompressible)
        let r = divergence_2d(|_, y| -y, |x, _| x, 2.0, 5.0, 1e-4);
        assert!(r.abs() < 1e-6, "rotational div should be 0, got {r}");
    }

    #[test]
    fn first_order_step_shape() {
        // at t=0 → 0; at t→∞ → K; at t=τ → ~63.2% of K
        assert!((first_order_step_response(2.0, 1.0, 0.0)).abs() < 1e-12);
        assert!((first_order_step_response(2.0, 1.0, 100.0) - 2.0).abs() < 1e-6);
        let at_tau = first_order_step_response(1.0, 1.0, 1.0);
        assert!(
            (at_tau - 0.6321).abs() < 1e-3,
            "τ point ≈ 0.632, got {at_tau}"
        );
    }

    #[test]
    fn settling_time_inverts_response() {
        // reaching 63.2% takes ~1τ
        let t = first_order_settling_time(1.0, 0.632).unwrap();
        assert!((t - 1.0).abs() < 1e-2, "should be ~1τ, got {t}");
        // RED: invalid frac → None
        assert!(first_order_settling_time(1.0, 1.5).is_none());
        assert!(first_order_settling_time(0.0, 0.5).is_none());
    }

    #[test]
    fn lagrange_passes_through_nodes_and_interpolates() {
        // y = x^2 sampled at 0,1,2,3 → interp reproduces exactly at nodes AND
        // recovers the quadratic between them (4 pts → cubic exact for a quadratic)
        let pts = [(0.0, 0.0), (1.0, 1.0), (2.0, 4.0), (3.0, 9.0)];
        for &(x, y) in &pts {
            assert!((lagrange_interp(&pts, x).unwrap() - y).abs() < 1e-9);
        }
        // between nodes: x=1.5 → 2.25
        assert!((lagrange_interp(&pts, 1.5).unwrap() - 2.25).abs() < 1e-9);
        // RED: duplicate abscissa → None
        assert!(lagrange_interp(&[(1.0, 2.0), (1.0, 3.0)], 1.0).is_none());
    }

    #[test]
    fn classify_fixed_point_limit_cycle_divergent() {
        // FixedPoint: damped sinusoid settling to 0
        let settle: Vec<f64> = (0..90)
            .map(|i| (i as f64 * 0.4).sin() * (-(i as f64) * 0.15).exp())
            .collect();
        assert_eq!(classify_trajectory(&settle, 0.05), Regime::FixedPoint);

        // LimitCycle: pure sustained sinusoid (constant amplitude)
        let cycle: Vec<f64> = (0..90).map(|i| (i as f64 * 0.4).sin()).collect();
        assert_eq!(classify_trajectory(&cycle, 0.05), Regime::LimitCycle);

        // Divergent: growing oscillation
        let grow: Vec<f64> = (0..90)
            .map(|i| (i as f64 * 0.4).sin() * (i as f64 * 0.05).exp())
            .collect();
        assert_eq!(classify_trajectory(&grow, 0.05), Regime::Divergent);

        // Undetermined: too short
        assert_eq!(classify_trajectory(&[1.0, 2.0], 0.05), Regime::Undetermined);
    }
}
