//! drift.rs — systems-thinking / architecture DRIFT detector (global rule).
//!
//! Operator global rule: best practices from systems thinking (feedback loops,
//! system boundaries, delays, emergence) and software architecture (SOLID, clean
//! boundaries, minimal deps, KISS/DRY) are CONFIGURABLE settings (default ON).
//! DEFAULT BEHAVIOR: if systems-thinking or overall-architecture DRIFT is detected,
//! flag it in the CLI (non-blocking warning, like the Hermes change log).
//!
//! Drift = a concrete violation of a pinned best-practice. This module is the
//! deterministic, offline classifier. The CLI surface (`bebop drift`) and the
//! agent loop call `detect_drift` over a proposed/observed change.

/// A single pinned best-practice that, if violated, is "drift".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Practice {
    /// New global dependency introduced without need (minimal-deps rule).
    NewGlobalDep,
    /// A module reaches across architectural layers (boundary bleed).
    LayerBleed,
    /// A single module grows into a god-object (cohesion violated).
    GodModule,
    /// A removed/weakened boundary (e.g. red-line gate dropped).
    BoundaryRemoved,
    /// Feedback loop / delay ignored in a systems change (systems-thinking).
    LoopIgnored,
}

impl Practice {
    pub fn label(self) -> &'static str {
        match self {
            Practice::NewGlobalDep => "new-global-dep",
            Practice::LayerBleed => "layer-bleed",
            Practice::GodModule => "god-module",
            Practice::BoundaryRemoved => "boundary-removed",
            Practice::LoopIgnored => "loop-ignored",
        }
    }

    /// Per-occurrence "drift weight" (0..=1) — how structurally significant a
    /// violation of this practice is. Consumed by the threshold guard in
    /// `detect_drift`: a violation whose weight is *below* the policy threshold
    /// is treated as benign and NOT reported. Defaults keep all five practices
    /// above the default 0.5 threshold, so out-of-the-box behavior is unchanged.
    pub fn weight(self) -> f64 {
        match self {
            Practice::NewGlobalDep => 1.0,
            Practice::LayerBleed => 0.8,
            Practice::GodModule => 0.7,
            Practice::BoundaryRemoved => 1.0,
            Practice::LoopIgnored => 0.6,
        }
    }
}

/// A detected drift event.
#[derive(Clone, Debug)]
pub struct Drift {
    pub practice: Practice,
    pub detail: String,
}

/// Policy: which practices are currently watched (user-tunable). Default: all on.
#[derive(Clone, Debug)]
pub struct DriftPolicy {
    pub watch: Vec<Practice>,
    /// Minimum practice weight required for a violation to be *reported*.
    /// A drift whose `Practice::weight()` is below this threshold is treated as
    /// benign and suppressed (the threshold guard). Defaults to 0.5, which keeps
    /// every shipped practice (weights ≥ 0.6) reported — so default behavior is
    /// unchanged. Raising it lets callers ignore low-significance noise.
    pub threshold: f64,
}

impl Default for DriftPolicy {
    fn default() -> Self {
        DriftPolicy {
            watch: vec![
                Practice::NewGlobalDep,
                Practice::LayerBleed,
                Practice::GodModule,
                Practice::BoundaryRemoved,
                Practice::LoopIgnored,
            ],
            threshold: 0.5,
        }
    }
}

impl DriftPolicy {
    /// Toggle a practice on/off (so the user can relax the rule).
    pub fn set(&mut self, p: Practice, on: bool) {
        if on && !self.watch.contains(&p) {
            self.watch.push(p);
        } else if !on {
            self.watch.retain(|x| *x != p);
        }
    }
}

/// Classify a change description (target + summary) against the policy.
/// Returns all drifts found (a change can violate several practices).
/// Deterministic: substring match over the lowercased haystack.
pub fn detect_drift(policy: &DriftPolicy, target: &str, summary: &str) -> Vec<Drift> {
    let hay = format!("{} {}", target, summary).to_ascii_lowercase();
    let mut out = Vec::new();
    let mut check = |p: Practice, pat: &str, detail: &str| {
        // Threshold guard: only flag a watched practice whose drift weight meets
        // the policy threshold. Without this, a `threshold` of 0.0 (or a `* 0.0`
        // weight) would make the guard dead — every substring match flagged
        // regardless of significance. `p.weight() >= threshold` is a *meaningful*
        // test: low-weight practices (e.g. LoopIgnored=0.6) are suppressed once
        // `threshold` is raised above them, while high-weight ones still fire.
        if policy.watch.contains(&p) && hay.contains(pat) && p.weight() >= policy.threshold {
            out.push(Drift {
                practice: p,
                detail: detail.to_string(),
            });
        }
    };
    check(
        Practice::NewGlobalDep,
        "add dependency",
        "introduces a new global dependency",
    );
    check(
        Practice::LayerBleed,
        "cross-layer",
        "reaches across architectural layers",
    );
    check(
        Practice::GodModule,
        "god module",
        "module is becoming a god-object",
    );
    check(
        Practice::BoundaryRemoved,
        "remove boundary",
        "a boundary/red-line gate was removed",
    );
    check(
        Practice::LoopIgnored,
        "ignore loop",
        "feedback loop / delay ignored in systems change",
    );
    out
}

/// Render drifts as a CLI warning block (⚠ per event). Empty if none.
pub fn render_drift(drifts: &[Drift]) -> String {
    if drifts.is_empty() {
        return String::new();
    }
    let mut s = String::from("⚠ systems/architecture drift detected:\n");
    for d in drifts {
        s.push_str(&format!("  - [{}] {}\n", d.practice.label(), d.detail));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_drift_when_clean() {
        // RED: a clean change must produce no drift.
        let d = detect_drift(&DriftPolicy::default(), "edit foo.rs", "added a helper");
        assert!(d.is_empty());
        assert_eq!(render_drift(&d), "");
    }

    #[test]
    fn detects_new_global_dep() {
        // GREEN: "add dependency" → NewGlobalDep drift.
        let d = detect_drift(
            &DriftPolicy::default(),
            "cargo.toml",
            "add dependency serde",
        );
        assert!(d.iter().any(|x| x.practice == Practice::NewGlobalDep));
        assert!(render_drift(&d).contains("new-global-dep"));
    }

    #[test]
    fn detects_boundary_removed() {
        // GREEN: "remove boundary" → BoundaryRemoved (red-line dropped).
        let d = detect_drift(&DriftPolicy::default(), "auth.rs", "remove boundary check");
        assert!(d.iter().any(|x| x.practice == Practice::BoundaryRemoved));
    }

    #[test]
    fn policy_is_user_tunable() {
        // GREEN: user can disable NewGlobalDep watching.
        let mut pol = DriftPolicy::default();
        pol.set(Practice::NewGlobalDep, false);
        let d = detect_drift(&pol, "cargo.toml", "add dependency serde");
        assert!(!d.iter().any(|x| x.practice == Practice::NewGlobalDep));
    }

    #[test]
    fn threshold_guard_suppresses_low_weight_below_threshold() {
        // RED→GREEN (fix #3): raising the policy threshold above a practice's
        // weight must suppress that practice's drift, while a still-higher-weight
        // practice remains flagged. LoopIgnored weight = 0.6, BoundaryRemoved = 1.0.
        let mut pol = DriftPolicy::default();
        pol.threshold = 0.9;
        let d = detect_drift(&pol, "auth.rs", "ignore loop and remove boundary check");
        // LoopIgnored (0.6 < 0.9) suppressed.
        assert!(
            !d.iter().any(|x| x.practice == Practice::LoopIgnored),
            "low-weight practice must be suppressed above threshold"
        );
        // BoundaryRemoved (1.0 >= 0.9) still flagged.
        assert!(
            d.iter().any(|x| x.practice == Practice::BoundaryRemoved),
            "high-weight practice must still fire above threshold"
        );
    }

    #[test]
    fn threshold_default_keeps_all_practices_flagged() {
        // The default threshold (0.5) is below every shipped practice weight
        // (min 0.6), so default behavior is unchanged: a matching change flags.
        let d = detect_drift(&DriftPolicy::default(), "auth.rs", "remove boundary check");
        assert!(d.iter().any(|x| x.practice == Practice::BoundaryRemoved));
    }
}
