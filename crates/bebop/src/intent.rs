//! intent.rs — auto-detect GOAL vs LOOP from a prompt (category P of the master plan).
//! Minimal but real: heuristics over the prompt text. Extended by Wave 3.
//! Operator rule: GOAL -> autopilot-to-done (final confirm only); LOOP ->
//! propose (create-new or pick-existing) + user sets cycle count.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Intent {
    /// One target — run all tasks/checks/fixes until truly done+verified+pushed+merged.
    Goal,
    /// Repetitive pattern — propose a loop with a cycle count.
    Loop,
    /// Ambiguous — treat as a goal but ask for confirmation of intent.
    Ambiguous,
}

impl Default for Intent {
    fn default() -> Self {
        Intent::Ambiguous
    }
}

/// Detect intent from a raw prompt. Heuristic, cheap, deterministic.
/// - Loop markers: "every", "loop", "each", "repeatedly", "whenever", "每隔",
///   "цикл", "луп", "серія", "постійно", "N times".
/// - Otherwise: Goal (the operator default — maximal automation).
pub fn detect(prompt: &str) -> Intent {
    let p = prompt.to_ascii_lowercase();
    const LOOP_MARKERS: &[&str] = &[
        "every",
        "loop",
        "each ",
        "each time",
        "repeatedly",
        "whenever",
        "每隔",
        "循环",
        "цикл",
        "луп",
        "серія",
        "серій",
        "постійно",
        "times",
        " n ",
        "n раз",
        "щодня",
        "per run",
    ];
    if LOOP_MARKERS.iter().any(|m| p.contains(m)) {
        Intent::Loop
    } else {
        Intent::Goal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goal_by_default_no_loop_markers() {
        assert_eq!(detect("implement the debrief panel"), Intent::Goal);
    }

    #[test]
    fn loop_when_marker_present() {
        assert_eq!(detect("run this check every 5 minutes"), Intent::Loop);
        assert_eq!(detect("запускай луп по всіх файлах"), Intent::Loop);
    }

    #[test]
    fn ambiguous_is_default_variant() {
        assert_eq!(Intent::default(), Intent::Ambiguous);
    }
}
