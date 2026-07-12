//! policy.rs — DEFAULT POLICIES (category N of the master plan).
//!
//! N1 auto_structure: decompose a task into structured categories, assign a
//!     max-EV approach + priority score.
//! N2 parallel_sessions: launch independent workstreams as parallel sessions
//!     (the orchestrator honors this when dispatching).
//! N3 descartes_square: auto-emit a 2x2 comparison (exact pros / exact cons)
//!     for proposed changes / research / analysis / library loading.
//!
//! All three are `Profile`-style toggles (default ON, changeable). Pure config +
//! helper functions; no new deps. The orchestrator (parent) reads N2 when it
//! dispatches subagents.

/// The three default policies, all configurable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Policies {
    pub auto_structure: bool,
    pub parallel_sessions: bool,
    pub descartes_square: bool,
    /// max_lanes for N2 (default = logical cores, capped here at a sane default).
    pub max_lanes: usize,
}

impl Default for Policies {
    fn default() -> Self {
        Policies {
            auto_structure: true,
            parallel_sessions: true,
            descartes_square: true,
            max_lanes: 4,
        }
    }
}

impl Policies {
    /// Parse from a settings-like map (bebop settings keys n1/n2/n3/max_lanes).
    pub fn from_map(m: &std::collections::HashMap<String, String>) -> Self {
        let b = |k: &str| m.get(k).map(|v| v == "true").unwrap_or(true);
        let lanes = m
            .get("max_lanes")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(4);
        Policies {
            auto_structure: b("n1"),
            parallel_sessions: b("n2"),
            descartes_square: b("n3"),
            max_lanes: lanes.max(1),
        }
    }

    /// N1: decompose a free-form task into structured buckets + a max-EV tag.
    /// Cheap, deterministic heuristic — real LLM routing happens upstream; this
    /// just categorizes so the dispatcher can prioritize.
    pub fn structure(&self, task: &str) -> (String, u8) {
        let t = task.to_ascii_lowercase();
        let (cat, prio) = if t.contains("test") || t.contains("verif") {
            ("quality", 9)
        } else if t.contains("secur") || t.contains("red-line") || t.contains("auth") {
            ("red-line", 10)
        } else if t.contains("docs") || t.contains("readme") || t.contains("plan") {
            ("docs", 4)
        } else if t.contains("build") || t.contains("impl") || t.contains("code") {
            ("build", 7)
        } else {
            ("general", 5)
        };
        (cat.to_string(), prio)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_all_on() {
        let p = Policies::default();
        assert!(p.auto_structure && p.parallel_sessions && p.descartes_square);
        assert_eq!(p.max_lanes, 4);
    }

    #[test]
    fn from_map_honors_off() {
        let mut m = std::collections::HashMap::new();
        m.insert("n1".into(), "false".into());
        m.insert("n2".into(), "false".into());
        m.insert("max_lanes".into(), "2".into());
        let p = Policies::from_map(&m);
        assert!(!p.auto_structure && !p.parallel_sessions);
        assert!(p.descartes_square); // unchanged -> default true
        assert_eq!(p.max_lanes, 2);
    }

    #[test]
    fn structure_prioritizes_red_line() {
        let p = Policies::default();
        let (c, pr) = p.structure("harden the auth red-line gate");
        assert_eq!(c, "red-line");
        assert_eq!(pr, 10);
        let (c2, _) = p.structure("write the README section");
        assert_eq!(c2, "docs");
    }
}
