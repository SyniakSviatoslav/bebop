//! BP-19 — Instrument panel L2 (агрегація приладів).
//!
//! Aggregates the eight live instruments from BP-02/06/07/09/10 + Kalman (BP-21)
//! into one `InstrumentPanel` snapshot, and evaluates the four alarm bands from
//! the corrected plan §13:
//!   - k-meter `ρ̂ ∈ [0.5, 0.8]`  → `k_band`    (DMD spectral-radius instability band)
//!   - orthogonometer `r_Δ < 0`   → `ortho_fire` (style/content decoupled)
//!   - entropy `D_t > H_max`      → `entropy_overflow` (heat budget breached)
//!   - persistence late-anomaly   → `persistence_anomaly` (survival verdict degenerate)
//!
//! This module does NOT re-derive any instrument math (out-of-scope per spec);
//! it only *reads* the already-verified primitives and aggregates.

use crate::entropy_ledger::EntropyBudget;
use crate::field::field_kalman;
use crate::orthogonality::{style_content_alignment, Critic};
use crate::persistence::PersistenceTable;
use bebop2_core::algebra::geodesic_distance;
use bebop2_core::dmd::OnlineDMD;

/// The eight instruments on the helm panel.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InstrumentPanel {
    /// BP-02 arccos metric: geodesic distance between two embeddings.
    pub sim_arccos: f64,
    /// BP-07 DMD k-meter: spectral-radius estimate `ρ̂`.
    pub k_dmd: f64,
    /// BP-09 persistence: number of live claims `n` (delta_n).
    pub delta_n: i64,
    /// BP-09 persistence: Bonferroni-corrected survival threshold `D*_Bonf`.
    pub persistence_d_star: i64,
    /// BP-06 entropy budget: current debt `D_t` (integer bits).
    pub entropy_debt_dt: i64,
    /// BP-10 orthogonometer: Pearson `r_Δ` of style vs content axes.
    pub orthogonometer_r_delta: f64,
    /// BP-07 affine regression slope `a` of the length-ratio series (§2.2).
    pub len_ratio_affine_a: f64,
    /// BP-21 Kalman: newest smoothed estimate of the observed signal.
    pub kalman_smoothed: f64,
}

/// The four alarm bits evaluated by the panel each iteration.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PanelAlarms {
    pub k_band: bool,
    pub ortho_fire: bool,
    pub entropy_overflow: bool,
    pub persistence_anomaly: bool,
}

/// DMD spectral-radius instability band (plan §13).
const K_BAND_LO: f64 = 0.5;
const K_BAND_HI: f64 = 0.8;

impl InstrumentPanel {
    /// Least-squares slope `a` of `y` vs `x` (the affine `a` from §2.2).
    /// Pure aggregation helper — the length-ratio *series* is supplied by the
    /// caller; this does not re-derive the DMD math.
    fn affine_slope(xs: &[f64], ys: &[f64]) -> f64 {
        let n = xs.len().min(ys.len());
        if n < 2 {
            return 0.0;
        }
        let (x, y) = (&xs[..n], &ys[..n]);
        let mx = x.iter().sum::<f64>() / n as f64;
        let my = y.iter().sum::<f64>() / n as f64;
        let mut sxy = 0.0;
        let mut sxx = 0.0;
        for i in 0..n {
            sxy += (x[i] - mx) * (y[i] - my);
            sxx += (x[i] - mx) * (x[i] - mx);
        }
        if sxx == 0.0 {
            0.0
        } else {
            sxy / sxx
        }
    }

    /// Read the eight instruments from the live sources.
    ///
    /// `len_ratio_xs` / `len_ratio_ys` drive the affine `a` (§2.2). `series` is
    /// the observed signal for the Kalman smoother. `q` / `r` are the Kalman
    /// process / measurement noise.
    pub fn read(
        embedding_a: &[f64],
        embedding_b: &[f64],
        dmd: &OnlineDMD,
        persistence: &PersistenceTable,
        entropy: &EntropyBudget,
        style_axis: &[f64],
        content_axis: &[f64],
        len_ratio_xs: &[f64],
        len_ratio_ys: &[f64],
        series: &[f64],
        q: f64,
        r: f64,
    ) -> Self {
        let sim_arccos = geodesic_distance(embedding_a, embedding_b);
        let k_dmd = dmd.spectral_radius();
        let delta_n = persistence.iter() as i64;
        let persistence_d_star = persistence.d_star_bonf(0.5, 0.05, persistence.len());
        let entropy_debt_dt = entropy.debt();
        let orthogonometer_r_delta = style_content_alignment(style_axis, content_axis);
        let len_ratio_affine_a = Self::affine_slope(len_ratio_xs, len_ratio_ys);
        let kalman_smoothed = if series.is_empty() {
            0.0
        } else {
            let (smoothed, _g, _res) = field_kalman(series, q, r);
            *smoothed.last().unwrap_or(&0.0)
        };
        InstrumentPanel {
            sim_arccos,
            k_dmd,
            delta_n,
            persistence_d_star,
            entropy_debt_dt,
            orthogonometer_r_delta,
            len_ratio_affine_a,
            kalman_smoothed,
        }
    }

    /// Evaluate the four alarm bands (plan §13).
    pub fn alarms(&self, entropy_h_max: i64, ortho_threshold: f64) -> PanelAlarms {
        let k_band = self.k_dmd >= K_BAND_LO && self.k_dmd <= K_BAND_HI;
        let ortho_fire = self.orthogonometer_r_delta < 0.0
            && self.orthogonometer_r_delta.abs() >= ortho_threshold;
        let entropy_overflow = self.entropy_debt_dt > entropy_h_max;
        // Late-anomaly: survival threshold degenerate — D*_Bonf claims survival
        // with effectively no buffer (early anomaly masked by late low power).
        let persistence_anomaly = self.persistence_d_star <= 0 && self.delta_n > 0;
        PanelAlarms {
            k_band,
            ortho_fire,
            entropy_overflow,
            persistence_anomaly,
        }
    }

    /// Convenience boolean: any alarm lit.
    pub fn any_alarm(&self, entropy_h_max: i64, ortho_threshold: f64) -> bool {
        let a = self.alarms(entropy_h_max, ortho_threshold);
        a.k_band || a.ortho_fire || a.entropy_overflow || a.persistence_anomaly
    }
}

/// Lightweight OTLP-style metric export (no external exporter dependency).
/// Each instrument becomes a named gauge sample. The real OTLP sink is wired
/// later; this is the metric-core contract (name + f64).
#[derive(Clone, Debug, PartialEq)]
pub struct Metric {
    pub name: &'static str,
    pub value: f64,
}

impl InstrumentPanel {
    /// Serialize the panel to metric samples (OTLP-gauge shaped).
    pub fn to_metrics(&self) -> Vec<Metric> {
        vec![
            Metric {
                name: "sim_arccos",
                value: self.sim_arccos,
            },
            Metric {
                name: "k_dmd",
                value: self.k_dmd,
            },
            Metric {
                name: "delta_n",
                value: self.delta_n as f64,
            },
            Metric {
                name: "persistence_d_star",
                value: self.persistence_d_star as f64,
            },
            Metric {
                name: "entropy_debt_dt",
                value: self.entropy_debt_dt as f64,
            },
            Metric {
                name: "orthogonometer_r_delta",
                value: self.orthogonometer_r_delta,
            },
            Metric {
                name: "len_ratio_affine_a",
                value: self.len_ratio_affine_a,
            },
            Metric {
                name: "kalman_smoothed",
                value: self.kalman_smoothed,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orthogonality::Critic;

    // --- each instrument reads from its source ---
    #[test]
    fn panel_reads_all_eight_instruments() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        let arccos = geodesic_distance(&a, &b); // perpendicular → π/2
        let dmd = OnlineDMD::new_from_snapshots(&[1.0, 2.0, 3.0, 4.0], 2, 1, 1.0, 0.98);
        let mut persistence = PersistenceTable::new();
        let mut entropy = EntropyBudget::new(100);
        // no claims yet → delta_n=0, d_star neutral
        let style = vec![1.0, 0.0];
        let content = vec![0.0, 1.0]; // orthogonal → r_Δ ≈ 0
        let xs = vec![1.0, 2.0, 3.0];
        let ys = vec![2.0, 4.0, 6.0]; // slope a = 2
        let series = vec![1.0, 1.2, 0.9, 1.1, 1.0];

        let panel = InstrumentPanel::read(
            &a,
            &b,
            &dmd,
            &persistence,
            &entropy,
            &style,
            &content,
            &xs,
            &ys,
            &series,
            1e-3,
            1e-2,
        );
        assert!((panel.sim_arccos - arccos).abs() < 1e-9, "arccos wired");
        assert!(panel.k_dmd.is_finite(), "k-meter finite");
        assert_eq!(panel.delta_n, 0);
        assert!(panel.entropy_debt_dt <= entropy.h_max());
        assert!(
            panel.orthogonometer_r_delta.abs() < 1e-6,
            "orthogonal → r≈0"
        );
        assert!((panel.len_ratio_affine_a - 2.0).abs() < 1e-9, "affine a=2");
        assert!(panel.kalman_smoothed.is_finite(), "kalman smoothed");
        let _ = &mut persistence; // keep mut warning away
    }

    // --- RED->GREEN: inject each failure → its alarm fires ---

    #[test]
    fn alarm_k_band_fires_in_instability_band() {
        let mut p = InstrumentPanel::default();
        p.k_dmd = 0.65; // inside [0.5, 0.8]
        let a = p.alarms(100, 0.3);
        assert!(a.k_band, "ρ̂ in band must alarm");
        p.k_dmd = 0.2;
        assert!(!p.alarms(100, 0.3).k_band, "ρ̂ below band must NOT alarm");
    }

    #[test]
    fn alarm_ortho_fires_on_negative_r_delta() {
        let mut p = InstrumentPanel::default();
        p.orthogonometer_r_delta = -0.7; // decoupled style/content
        let a = p.alarms(100, 0.3);
        assert!(a.ortho_fire, "negative r_Δ (|r|≥τ) must fire");
        p.orthogonometer_r_delta = 0.7; // coupled
        assert!(!p.alarms(100, 0.3).ortho_fire, "positive r_Δ must NOT fire");
    }

    #[test]
    fn alarm_entropy_overflow_on_dt_exceeds_hmax() {
        let mut p = InstrumentPanel::default();
        p.entropy_debt_dt = 150;
        let a = p.alarms(100, 0.3);
        assert!(a.entropy_overflow, "D_t > H_max must alarm");
        p.entropy_debt_dt = 50;
        assert!(
            !p.alarms(100, 0.3).entropy_overflow,
            "D_t ≤ H_max must NOT alarm"
        );
    }

    #[test]
    fn alarm_persistence_anomaly_on_degenerate_dstar() {
        let mut p = InstrumentPanel::default();
        p.persistence_d_star = 0;
        p.delta_n = 5;
        let a = p.alarms(100, 0.3);
        assert!(
            a.persistence_anomaly,
            "degenerate D*_Bonf with claims must alarm"
        );
        p.persistence_d_star = 3;
        assert!(
            !p.alarms(100, 0.3).persistence_anomaly,
            "healthy D* must NOT alarm"
        );
    }

    #[test]
    fn metric_export_has_eight_samples() {
        let panel = InstrumentPanel::default();
        assert_eq!(panel.to_metrics().len(), 8, "one metric per instrument");
    }
}
