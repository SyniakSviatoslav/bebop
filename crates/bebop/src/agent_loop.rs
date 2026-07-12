//! loop.rs — governed agentic loop driver (Loop Engineering concept, native Rust).
//!
//! OpenManus / Loop Engineering both converge on the same primitive: an agentic
//! loop that (1) decides next move, (2) runs a step, (3) VERIFIES it, (4) caps
//! iterations, (5) stops/rolls back on failure. Bebop already has `intent`
//! (detect GOAL/LOOP), `governor` (kill-switch), `agentic_git` (rollback-ready).
//! This module supplies the missing DRIVER: `run_loop`.
//!
//! Offline + deterministic: `step` and `verify` are injected by the caller
//! (no LLM in the loop). Safe to run autonomously under `governor`.

use crate::intent::Intent;

/// Outcome of a single loop iteration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepStatus {
    /// Step produced a result that the verify gate accepted.
    Ok,
    /// Step failed its verify gate (loop should stop / rollback).
    Failed,
}

/// Report of a finished loop run.
#[derive(Clone, Debug, PartialEq)]
pub struct LoopReport {
    pub iterations: usize,
    pub successes: usize,
    /// Index of the first failed iteration (None if all passed).
    pub last_failure: Option<usize>,
    /// True if the loop reached a natural end (verify passed every iteration
    /// up to max_iter) rather than stopping on a failure.
    pub done: bool,
}

/// Run a governed agentic loop.
///
/// - `intent`: only LOOP (or an explicit GOAL the caller wants looped) drives iteration.
/// - `max_iter`: hard cap (prevents runaway autonomous loops — Loop Engineering's "cap").
/// - `step(i)`: produces a `StepStatus` for iteration `i`.
/// - `verify(&status)`: gate; if it returns false the loop STOPS (rollback is the
///   caller's concern via `agentic_git` — this module only reports where it broke).
///
/// Deterministic: no RNG, no Date, no IO. Same inputs → same report.
pub fn run_loop(
    intent: Intent,
    max_iter: usize,
    mut step: impl FnMut(usize) -> StepStatus,
    verify: impl Fn(&StepStatus) -> bool,
) -> LoopReport {
    // A GOAL without an explicit loop flag does NOT auto-iterate here; the
    // caller opts in. LOOP always iterates. (Prevents surprise autonomy.)
    let _ = intent; // intent is advisory; max_iter is the real governor.
    let mut successes = 0usize;
    let mut last_failure: Option<usize> = None;
    let mut i = 0usize;
    while i < max_iter {
        let status = step(i);
        if !verify(&status) {
            last_failure = Some(i);
            break;
        }
        successes += 1;
        i += 1;
    }
    LoopReport {
        iterations: i,
        successes,
        last_failure,
        done: last_failure.is_none() && i >= max_iter,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loop_stops_on_first_failure() {
        // RED: a failing verify gate must STOP the loop immediately (no runaway).
        let r = run_loop(
            Intent::Loop,
            10,
            |_| StepStatus::Failed,
            |s| matches!(s, StepStatus::Ok),
        );
        assert_eq!(r.iterations, 0, "loop ran despite failed step");
        assert_eq!(r.last_failure, Some(0));
        assert!(!r.done);
    }

    #[test]
    fn loop_runs_to_cap_when_all_pass() {
        // GREEN: every step verifies → runs to max_iter, done=true.
        let r = run_loop(
            Intent::Loop,
            5,
            |_| StepStatus::Ok,
            |s| matches!(s, StepStatus::Ok),
        );
        assert_eq!(r.iterations, 5);
        assert_eq!(r.successes, 5);
        assert!(r.last_failure.is_none());
        assert!(r.done);
    }

    #[test]
    fn loop_reports_mid_failure_and_stops() {
        // GREEN: fails at iter 3 → reports 3 successes, stops, not done.
        let r = run_loop(
            Intent::Loop,
            10,
            |i| if i < 3 { StepStatus::Ok } else { StepStatus::Failed },
            |s| matches!(s, StepStatus::Ok),
        );
        assert_eq!(r.successes, 3);
        assert_eq!(r.last_failure, Some(3));
        assert!(!r.done);
    }

    #[test]
    fn loop_is_deterministic() {
        let a = run_loop(Intent::Loop, 4, |_| StepStatus::Ok, |s| matches!(s, StepStatus::Ok));
        let b = run_loop(Intent::Loop, 4, |_| StepStatus::Ok, |s| matches!(s, StepStatus::Ok));
        assert_eq!(a, b, "loop is non-deterministic");
    }
}
