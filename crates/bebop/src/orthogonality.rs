//! BP-10 — Orthogonometer + Goodhart detector.
//!
//! A. Paraphrase-invariance (∂q/∂v = 0):
//!    e_⊥ = e_style − proj_∇u(e_style); for a binary critic q over N meaning-preserving
//!    paraphrases, flip_rate = (1/N) Σ 1[q(x'_i) ≠ q(x)]; a critic is orthogonal ⟺
//!    flip_rate ≈ 0; enforce |ρ_gc| ≤ τ_orth ≈ 0.1.
//!
//! B. Goodhart detector corr(Δq, Δs), where s = a decorrelated / held-out similarity
//!    (NOT the loop metric). r_Δ = Pearson(Δq_t, Δs_t) on a sliding window W;
//!    Fisher-z lower bound r_lower = tanh(artanh(r̂) − z/√(W−3));
//!    ALARM when r_lower < r_min (≈0.3–0.5) OR cumulative CUSUM C_t > h.
//!
//! The module *measures* — it never calls an LLM. Inputs are f64 embedding vectors plus a
//! caller-supplied scoring closure `q: &dyn Fn(&[f64]) -> f64)`.

use crate::algebra;

/// Paraphrase-invariance threshold: flip_rate and |ρ_gc| must stay ≤ this to be orthogonal.
pub const TAU_ORTH: f64 = 0.1;
/// Goodhart detector: minimum acceptable Fisher-z lower-bound correlation.
pub const R_MIN: f64 = 0.4;
/// Fisher-z confidence half-width (≈ 1.96 for a 95% two-sided interval).
pub const Z: f64 = 1.96;
/// CUSUM alarm threshold.
pub const H: f64 = 2.0;

/// A critic under test.
///
/// * `style_axis`  — the embedding direction the critic may (undesirably) key off.
/// * `content_axis`— the meaning direction a well-formed rubric keys off.
///
/// The actual scoring is supplied externally as `q: &dyn Fn(&[f64]) -> f64)`.
#[derive(Clone, Debug)]
pub struct Critic {
    pub style_axis: Vec<f64>,
    pub content_axis: Vec<f64>,
}

impl Critic {
    pub fn new(style_axis: Vec<f64>, content_axis: Vec<f64>) -> Self {
        Self {
            style_axis,
            content_axis,
        }
    }
}

/// Orthogonalize the style direction against the content (meaning) direction:
/// `e_⊥ = e_style − proj_∇u(e_style)`. Reuses [`algebra::project`].
pub fn orthogonalize_style(style_axis: &[f64], content_axis: &[f64]) -> Vec<f64> {
    let n = style_axis.len();
    debug_assert_eq!(n, content_axis.len());
    let mut basis = vec![0.0f64; n];
    basis[..n].copy_from_slice(content_axis);
    let mut coeffs = [0.0f64; 1];
    algebra::project(style_axis, &basis, 1, n, &mut coeffs);
    let proj: Vec<f64> = content_axis.iter().map(|c| coeffs[0] * c).collect();
    style_axis.iter().zip(&proj).map(|(s, p)| s - p).collect()
}

/// Cosine alignment between the style axis and the content axis (0 ⇒ orthogonal axes).
/// Reuses [`algebra::cosine`].
pub fn style_content_alignment(style_axis: &[f64], content_axis: &[f64]) -> f64 {
    algebra::cosine(style_axis, content_axis)
}

/// Pearson correlation between the style-projection delta and the score delta across
/// paraphrases: `ρ_gc = corr(⟨x'_i − x, v⟩, q(x'_i) − q(x))`.
/// `|ρ_gc| ≤ τ_orth` ⇒ the critic is orthogonal (style-invariant).
pub fn style_correlation(
    critic: &Critic,
    x: &[f64],
    paraphrases: &[Vec<f64>],
    q: &dyn Fn(&[f64]) -> f64,
) -> f64 {
    if paraphrases.is_empty() {
        return 0.0;
    }
    let qx = q(x);
    let sd: Vec<f64> = paraphrases
        .iter()
        .map(|p| algebra::dot(p, &critic.style_axis) - algebra::dot(x, &critic.style_axis))
        .collect();
    let qd: Vec<f64> = paraphrases.iter().map(|p| q(p) - qx).collect();
    pearson(&sd, &qd)
}

/// BP-10 orthogonometer.
///
/// Returns the paraphrase flip_rate ∈ [0, 1]:
/// `flip_rate = (1/N) Σ 1[q(x'_i) ≠ q(x)]` for the binary critic q
/// (class = `q(e) >= 0`). A style-invariant (orthogonal) critic has flip_rate ≈ 0;
/// a style-sensitive critic has flip_rate > τ_orth.
pub fn orthogonality(
    _critic: &Critic,
    x: &[f64],
    paraphrases: &[Vec<f64>],
    q: &dyn Fn(&[f64]) -> f64,
) -> f64 {
    if paraphrases.is_empty() {
        return 0.0;
    }
    let b0 = if q(x) >= 0.0 { 1i32 } else { 0i32 };
    let mut flips = 0usize;
    for p in paraphrases {
        let b = if q(p) >= 0.0 { 1i32 } else { 0i32 };
        if b != b0 {
            flips += 1;
        }
    }
    flips as f64 / paraphrases.len() as f64
}

/// Goodhart alarm result for one evaluation.
#[derive(Clone, Debug, PartialEq)]
pub struct Alarm {
    /// True when the Goodhart detector fires.
    pub triggered: bool,
    /// Raw Pearson r_Δ on the final sliding window.
    pub r_hat: f64,
    /// Fisher-z lower bound of r_Δ.
    pub r_lower: f64,
    /// Final CUSUM accumulation.
    pub cusum: f64,
}

/// Goodhart detector (default thresholds).
///
/// `q_series` is the loop/optimized metric over time; `s_series` is a *decorrelated,
/// held-out* similarity (NOT the loop metric). `w` is the sliding-window length.
/// Fires when the Fisher-z lower bound `r_lower < R_MIN` or `CUSUM > H`.
pub fn goodhart_alarm(q_series: &[f64], s_series: &[f64], w: usize) -> Alarm {
    goodhart_alarm_cfg(q_series, s_series, w, R_MIN, Z, H)
}

/// Goodhart detector with explicit thresholds.
pub fn goodhart_alarm_cfg(
    q_series: &[f64],
    s_series: &[f64],
    w: usize,
    r_min: f64,
    z: f64,
    h: f64,
) -> Alarm {
    let n = q_series.len().min(s_series.len());
    if n < 2 || w < 3 {
        return Alarm {
            triggered: false,
            r_hat: 0.0,
            r_lower: 0.0,
            cusum: 0.0,
        };
    }
    let win = w.min(n);
    let mut cusum = 0.0f64;
    let mut last_r = 0.0f64;
    // Slide windows of size `win` ending at t = win..=n; maintain CUSUM and the final r_Δ.
    for t in win..=n {
        let qw = &q_series[t - win..t];
        let sw = &s_series[t - win..t];
        let dq: Vec<f64> = qw.windows(2).map(|s| s[1] - s[0]).collect();
        let ds: Vec<f64> = sw.windows(2).map(|s| s[1] - s[0]).collect();
        let r = pearson(&dq, &ds);
        cusum = (cusum + (r_min - r)).max(0.0);
        last_r = r;
    }

    // Fisher-z lower bound on the final-window r̂.
    let rc = last_r.clamp(-0.999_999, 0.999_999);
    let art = 0.5 * ((1.0 + rc).ln() - (1.0 - rc).ln());
    let denom = ((win as f64) - 3.0).max(1e-9).sqrt();
    let se = if denom > 0.0 { z / denom } else { 0.0 };
    let y = (2.0 * (art - se)).exp();
    let r_lower = (y - 1.0) / (y + 1.0);

    let triggered = r_lower < r_min || cusum > h;
    Alarm {
        triggered,
        r_hat: last_r,
        r_lower,
        cusum,
    }
}

/// Pearson correlation of two equal-length slices. Returns 0.0 when either has ~zero variance.
fn pearson(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len();
    if n < 2 {
        return 0.0;
    }
    let ma = a.iter().sum::<f64>() / n as f64;
    let mb = b.iter().sum::<f64>() / n as f64;
    let mut cov = 0.0f64;
    let mut va = 0.0f64;
    let mut vb = 0.0f64;
    for i in 0..n {
        let da = a[i] - ma;
        let db = b[i] - mb;
        cov += da * db;
        va += da * da;
        vb += db * db;
    }
    let denom = (va * vb).sqrt();
    if denom <= 1e-12 {
        0.0
    } else {
        (cov / denom).clamp(-1.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Orthogonality RED: a critic sensitive to a style axis → high flip_rate ──────
    #[test]
    fn orthogonality_red_style_sensitive_critic() {
        // style axis v = [0,1,0]; content axis u = [1,0,0] (orthogonal by construction).
        let critic = Critic::new(vec![0.0, 1.0, 0.0], vec![1.0, 0.0, 0.0]);
        // q keys off the STYLE axis: q(e) = e[1].
        let q = |e: &[f64]| e[1];
        let x = vec![2.0, 0.1, 0.0]; // small positive style ⇒ class 1
        let paraphrases = vec![
            vec![2.0, -0.5, 0.0],
            vec![2.0, -1.0, 0.0],
            vec![2.0, -0.3, 0.0],
            vec![2.0, -0.7, 0.0],
        ];
        let fr = orthogonality(&critic, &x, &paraphrases, &q);
        assert!(
            fr > TAU_ORTH,
            "style-sensitive critic must flip: flip_rate={fr}"
        );
        let rho = style_correlation(&critic, &x, &paraphrases, &q);
        assert!(
            rho.abs() > TAU_ORTH,
            "ρ_gc should exceed τ_orth: {rho}"
        );
    }

    // ── Orthogonality GREEN: reference-term rubric → flip_rate ≈ 0 ─────────────────
    #[test]
    fn orthogonality_green_reference_rubric() {
        let critic = Critic::new(vec![0.0, 1.0, 0.0], vec![1.0, 0.0, 0.0]);
        // q depends ONLY on the CONTENT axis (reference-term rubric): q(e) = e[0].
        let q = |e: &[f64]| e[0];
        let x = vec![2.0, 0.1, 0.0];
        let paraphrases = vec![
            vec![2.0, -0.5, 0.0],
            vec![2.0, -1.0, 0.0],
            vec![2.0, -0.3, 0.0],
            vec![2.0, -0.7, 0.0],
        ];
        let fr = orthogonality(&critic, &x, &paraphrases, &q);
        assert!(
            fr < TAU_ORTH,
            "reference rubric must be style-invariant: flip_rate={fr}"
        );
        let rho = style_correlation(&critic, &x, &paraphrases, &q);
        assert!(
            rho.abs() <= TAU_ORTH,
            "ρ_gc must be ≤ τ_orth: {rho}"
        );
    }

    // ── e_⊥ reuse of algebra::project ──────────────────────────────────────────────
    #[test]
    fn orthogonalize_style_uses_algebra_project() {
        let style = vec![1.0, 1.0, 0.0];
        let content = vec![1.0, 0.0, 0.0];
        let e_perp = orthogonalize_style(&style, &content);
        // proj of style onto content = [1,0,0]; residual = [0,1,0].
        assert!(e_perp[0].abs() < 1e-9, "residual has no content component");
        assert!((e_perp[1] - 1.0).abs() < 1e-9, "residual keeps style component");
        assert!(
            style_content_alignment(&e_perp, &content).abs() < 1e-9,
            "residual ⊥ content"
        );
    }

    // ── Goodhart RED: q and s perfectly anti-correlated ⇒ alarm fires ──────────────
    #[test]
    fn goodhart_red_anti_correlated_series() {
        // Perfect negative correlation built from *varying* increments. (The literal
        // q_t=t, s_t=−t has constant first-differences ⇒ correlation is undefined, so we
        // use the equivalent varying-increment series that achieves r_Δ = −1.)
        let inc = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let mut q = Vec::new();
        let mut s = Vec::new();
        let mut cq = 0.0f64;
        let mut cs = 0.0f64;
        for v in inc {
            cq += v;
            cs -= v;
            q.push(cq);
            s.push(cs);
        }
        let alarm = goodhart_alarm(&q, &s, 10);
        assert!(
            alarm.triggered,
            "anti-correlated q,s must raise Goodhart alarm (r_lower={}, cusum={})",
            alarm.r_lower, alarm.cusum
        );
    }

    // ── Goodhart GREEN: q ≈ s + small noise ⇒ no alarm, r_Δ > 0.9 ──────────────────
    #[test]
    fn goodhart_green_correlated_with_noise() {
        let inc = [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        let noise = [0.02, -0.01, 0.03, -0.02, 0.01, -0.03, 0.02, -0.01, 0.03, -0.02];
        let mut s = Vec::new();
        let mut q = Vec::new();
        let mut cs = 0.0f64;
        let mut cq = 0.0f64;
        for (i, v) in inc.iter().enumerate() {
            cs += v;
            cq += v + noise[i];
            s.push(cs);
            q.push(cq);
        }
        let alarm = goodhart_alarm(&q, &s, 10);
        assert!(
            !alarm.triggered,
            "q≈s+noise must NOT alarm (r_lower={}, r_hat={})",
            alarm.r_lower, alarm.r_hat
        );
        assert!(alarm.r_hat > 0.9, "r_hat should exceed 0.9: {}", alarm.r_hat);
    }
}
