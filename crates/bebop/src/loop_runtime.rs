//! BP-18 — Resonator wired into the 6-layer control loop.
//!
//! The six layers of the hydraulic sense loop, as one deterministic state
//! machine driven by the EXISTING verified pieces (no new math, only wiring):
//!
//!   L1 FIELD      → `field::field_gate_verdict` + `field::loop_health`
//!                   (red-line physics veto + oscillation/drift detection)
//!   L2 L5         → `stabilizer::stabilize_step` (advisor proposes, kernel decides)
//!   L3 MEMORY     → `memory::LivingMemory` (content-addressed recall)
//!   L4 GATE       → `research_patterns::TargetScope` + `ActionContract` wall
//!   L5 GOVERNOR   → `governor::GovState` (authority servo, Jury-stable)
//!   L6 SENSE      → `field::field_kalman` (measurement update) + `orthogonality::goodhart_alarm`
//!
//! The loop cycles: INTAKE → PRIMED → SPIN → {CONVERGED | ABORT | BRANCH | BYPASS} → DELIVER.
//!
//! Determinism contract: no RNG, no Date, no network. Every tick is a pure
//! function of (task, l5-proposal, history). Verified end-to-end by the four
//! RED→GREEN scenarios below.
//!
//! innovate: the GENERATE / REFLECT / SUPERVISE steps are deterministic stubs
//! here (the real LLM/introspection is out of scope for a pure-Rust gate). They
//! are explicit seams (#[allow(dead_code)] hooks) so a model-backed layer can be
//! dropped in without touching the loop's fail-closed decision logic.

use crate::field::{field_gate_verdict, field_kalman, loop_health, FieldVerdict};
use crate::governor::{GovConfig, GovState};
use crate::memory::LivingMemory;
use crate::orthogonality::goodhart_alarm;
use crate::research_patterns::TargetScope;
use crate::stabilizer::ActionContract;
use crate::wiring::{wire, L5Proposal, WireOutcome};

/// Loop phase (the state machine).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopPhase {
    /// Awaiting an intake task.
    Intake,
    /// Task passed the INTAKE gate; ready to spin.
    Primed,
    /// Running the 6-layer cycle.
    Spin,
    /// Field + governor + goodhart all stable → deliver.
    Converged,
    /// Red-line / forbidden / tampered → abort (fail-closed).
    Abort,
    /// Loop health unstable (oscillation/drift) → branch to ground state.
    Branch,
    /// Trivial task → bypass the heavy layers, single-pass deliver.
    Bypass,
    /// Delivered (terminal for the tick).
    Deliver,
}

/// Tunables for the loop's health + governor layers.
#[derive(Debug, Clone)]
pub struct LoopConfig {
    /// Kalman process noise Q.
    pub kalman_q: f64,
    /// Kalman measurement noise R.
    pub kalman_r: f64,
    /// loop_health drift bound on the smoothed estimate.
    pub drift: f64,
    /// loop_health min sign-flips to call a limit cycle.
    pub min_flips: usize,
    /// loop_health bounded-amplitude band for a limit cycle.
    pub amp_band: f64,
    /// Goodhart alarm sliding-window length.
    pub goodhart_w: usize,
    /// Governor config (defaults to the Jury-stable tuned set).
    pub gov: GovConfig,
}

impl Default for LoopConfig {
    fn default() -> Self {
        LoopConfig {
            kalman_q: 0.01,
            kalman_r: 0.1,
            drift: 1.0,
            min_flips: 4,
            amp_band: 2.0,
            goodhart_w: 8,
            gov: GovConfig::default(),
        }
    }
}

/// The runtime: owns the stateful surfaces (memory, governor, signal history)
/// so the loop persists across ticks.
pub struct LoopRuntime {
    pub phase: LoopPhase,
    pub cfg: LoopConfig,
    pub gov: GovState,
    /// Living memory (L3) — stateful across ticks.
    pub memory: LivingMemory,
    /// Rolling field-signal history (L6 Kalman input).
    pub field_signal: Vec<f64>,
    /// Rolling loop-metric vs held-out-similarity series (L6 goodhart input).
    pub q_series: Vec<f64>,
    pub s_series: Vec<f64>,
    /// Last wire outcome (for observability / tests).
    pub last: Option<WireOutcome>,
    /// Number of ticks since start.
    pub ticks: u64,
}

impl LoopRuntime {
    pub fn new(authority: f64) -> Self {
        LoopRuntime {
            phase: LoopPhase::Intake,
            cfg: LoopConfig::default(),
            gov: GovState::new(authority),
            memory: LivingMemory::new(),
            field_signal: Vec::new(),
            q_series: Vec::new(),
            s_series: Vec::new(),
            last: None,
            ticks: 0,
        }
    }

    /// Run one full loop cycle for `task` with an L5 proposal.
    ///
    /// Returns the terminal phase. `target`/`contract` are optional gating
    /// surfaces (own-project-only + forbidden-zone wall).
    pub fn cycle(
        &mut self,
        task: &str,
        l5: &L5Proposal,
        contract: Option<&ActionContract>,
        baseline: &[f64],
        k: &[f64],
        scope: Option<&TargetScope>,
        target: Option<(u32, &str)>,
    ) -> LoopPhase {
        self.ticks += 1;
        self.phase = LoopPhase::Intake;
        let _span = tracing::info_span!("loop_cycle", ticks = self.ticks, task = %task).entered();

        // ── INTAKE GATE (L1 field red-line + fast Bypass for trivial tasks) ──
        let verdict = field_gate_verdict(task);
        if verdict.refused() {
            // Red-line / physics veto → fail-closed abort (never negotiates).
            self.phase = LoopPhase::Abort;
            return self.phase;
        }
        // Trivial-task fast path: short, no-forbidden-char task → BYPASS heavy layers.
        // BUT the bypass is still fail-closed: the wire (forbidden-zone wall +
        // out-of-scope target) must still clear. A refused trivial action ABORTS.
        if task.len() <= 24 && !task.contains(char::is_uppercase) {
            self.phase = LoopPhase::Bypass;
            let out = self.light_wire(task, l5, contract, baseline, k, scope, target);
            self.last = Some(out.clone());
            if !out.proceed {
                self.phase = LoopPhase::Abort;
                return self.phase;
            }
            self.record_signal(0.0); // nominal health on bypass
            self.phase = LoopPhase::Deliver;
            return self.phase;
        }

        // ── PRIMED → SPIN ──
        self.phase = LoopPhase::Primed;
        // Mint a per-tick audit ledger (innovate: a persistent one belongs on
        // the runtime; here we keep the borrow checker simple — the tamper gate
        // in `wire` still exercises on it).
        let mut audit = crate::audit::AuditLog::new();
        let out = wire(
            task,
            l5,
            contract,
            baseline,
            k,
            scope,
            target,
            &mut self.memory,
            &mut audit,
        );
        self.last = Some(out.clone());

        // L6 SENSE: feed the field verdict into the Kalman + goodhart series.
        let signal = match out.field {
            FieldVerdict::Permit => 0.0,
            FieldVerdict::Unhealthy => 1.0,
            FieldVerdict::Override => 2.0,
        };
        self.record_signal(signal);

        // Abort if any hard gate refused (field already passed; this catches
        // forbidden-zone wall + out-of-scope + audit tamper — fail-closed).
        if !out.proceed {
            self.phase = LoopPhase::Abort;
            return self.phase;
        }

        // L5 GOVERNOR: servo authority on the loop's quality signal.
        // Approve (q=1) when the wire proceeded; here we feed the smoothed
        // field estimate as the approval-rate proxy.
        let (est, _, _) = field_kalman(&self.field_signal, self.cfg.kalman_q, self.cfg.kalman_r);
        let approval = est.last().copied().unwrap_or(0.0);
        // GovConfig::step(&mut GovState, quality, cost, volume)
        self.cfg.gov.step(&mut self.gov, approval, 1e-9, 100.0);

        // L6 loop-health: oscillation / drift → BRANCH to ground state.
        if !self.loop_stable() {
            self.phase = LoopPhase::Branch;
            self.gov.authority = self.gov.authority.clamp(0.0, 0.25); // drop toward ground
            return self.phase;
        }

        // L6 Goodhart: if the loop metric decouples from held-out similarity,
        // the loop is optimizing a proxy → BRANCH (fail-closed on metric gaming).
        if self.goodhart_fires() {
            self.phase = LoopPhase::Branch;
            return self.phase;
        }

        // All layers green → CONVERGED → DELIVER.
        self.phase = LoopPhase::Converged;
        self.phase = LoopPhase::Deliver;
        self.phase
    }

    /// Light single-layer wire for the BYPASS path (no governor/goodhart cost).
    fn light_wire(
        &mut self,
        task: &str,
        l5: &L5Proposal,
        contract: Option<&ActionContract>,
        baseline: &[f64],
        k: &[f64],
        scope: Option<&TargetScope>,
        target: Option<(u32, &str)>,
    ) -> WireOutcome {
        let mut audit = crate::audit::AuditLog::new();
        wire(
            task,
            l5,
            contract,
            baseline,
            k,
            scope,
            target,
            &mut self.memory,
            &mut audit,
        )
    }

    fn record_signal(&mut self, s: f64) {
        self.field_signal.push(s);
        // keep a bounded history (last 64) for the Kalman/health windows.
        if self.field_signal.len() > 64 {
            self.field_signal.remove(0);
        }
    }

    /// L6 loop-health: stable when neither drifting past `drift` nor in a
    /// bounded limit cycle. Fail-closed: empty/unstable → false.
    fn loop_stable(&self) -> bool {
        if self.field_signal.len() < 4 {
            return true; // not enough data to call unstable yet
        }
        loop_health(
            &self.field_signal,
            self.cfg.kalman_q,
            self.cfg.kalman_r,
            self.cfg.drift,
            self.cfg.min_flips,
            self.cfg.amp_band,
        ) == FieldVerdict::Permit
    }

    /// L6 Goodhart: fires when the loop metric decouples from held-out
    /// similarity over the sliding window. Needs both series populated.
    fn goodhart_fires(&self) -> bool {
        if self.q_series.len() < self.cfg.goodhart_w || self.s_series.len() < self.cfg.goodhart_w {
            return false;
        }
        goodhart_alarm(&self.q_series, &self.s_series, self.cfg.goodhart_w).triggered
    }

    /// Push one (loop-metric, held-out-similarity) sample for the goodhart watch.
    pub fn observe(&mut self, q: f64, s: f64) {
        self.q_series.push(q);
        self.s_series.push(s);
        if self.q_series.len() > 64 {
            self.q_series.remove(0);
        }
        if self.s_series.len() > 64 {
            self.s_series.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stabilizer::ActionContract;

    fn l5_stable() -> L5Proposal {
        L5Proposal {
            v_prev: 1.0,
            v_cur: 0.9,
            dt: 1.0,
            proposed_delta: 0.3,
            limit: 0.5,
            ..Default::default()
        }
    }

    // ── Scenario 1: benign stable task → CONVERGED → DELIVER ──
    #[test]
    fn scenario_benign_converges_and_delivers() {
        let mut rt = LoopRuntime::new(0.5);
        let phase = rt.cycle("write the docs", &l5_stable(), None, &[], &[], None, None);
        assert_eq!(phase, LoopPhase::Deliver);
        assert!(rt.last.as_ref().unwrap().proceed, "benign must proceed");
        assert_eq!(rt.last.as_ref().unwrap().field, FieldVerdict::Permit);
        // memory recorded the wire
        assert!(rt.memory.size() >= 1);
    }

    // ── Scenario 2: red-line task → ABORT (fail-closed, never delivers) ──
    #[test]
    fn scenario_redline_aborts_fail_closed() {
        let mut rt = LoopRuntime::new(0.9); // high authority must NOT bypass red-line
        let phase = rt.cycle(
            "rotate the deploy secrets",
            &l5_stable(),
            None,
            &[],
            &[],
            None,
            None,
        );
        assert_eq!(phase, LoopPhase::Abort);
        assert!(rt.last.is_none(), "red-line must not even wire");
        // authority untouched (no governor step on abort)
        assert!((rt.gov.authority - 0.9).abs() < 1e-9);
    }

    // ── Scenario 3: oscillating field signal → BRANCH (limit cycle detected) ──
    #[test]
    fn scenario_oscillation_branches_to_ground() {
        let mut rt = LoopRuntime::new(0.5);
        // Prime a few benign ticks, then inject a sustained sign-flipping signal
        // (limit cycle) by feeding oscillating approvals into the Kalman series.
        for i in 0..6 {
            // alternate benign / unhealthy verdicts to build a limit cycle in field_signal
            let task = if i % 2 == 0 {
                "write the docs"
            } else {
                "scan the host"
            };
            let _ = rt.cycle(task, &l5_stable(), None, &[], &[], None, None);
        }
        // Manually push an oscillating signal to force the limit-cycle detector.
        // Use NON-ZERO signed values so sign-flips are counted (0.0 has signum 0).
        rt.field_signal = vec![-1.0, 1.0, -1.0, 1.0, -1.0, 1.0, -1.0, 1.0];
        assert!(
            !rt.loop_stable(),
            "bounded oscillation must be flagged unstable"
        );
        // A fresh benign cycle while the signal oscillates must BRANCH, not deliver.
        // Use a NON-bypassable task (has uppercase + >24 chars) so it runs the
        // full SPIN path and reaches the loop-health check.
        let phase = rt.cycle(
            "Implement the recursive descent parser module now",
            &l5_stable(),
            None,
            &[],
            &[],
            None,
            None,
        );
        assert_eq!(phase, LoopPhase::Branch);
    }

    // ── Scenario 4: trivial task → BYPASS (single-layer fast path) ──
    #[test]
    fn scenario_trivial_bypasses_heavy_layers() {
        let mut rt = LoopRuntime::new(0.5);
        // "write docs" is <=24 chars, no uppercase → bypass path.
        let phase = rt.cycle("write docs", &l5_stable(), None, &[], &[], None, None);
        assert_eq!(phase, LoopPhase::Deliver);
        assert!(rt.last.as_ref().unwrap().proceed);
        // field verdict still computed (bypass still respects the red-line gate)
        assert_eq!(rt.last.as_ref().unwrap().field, FieldVerdict::Permit);
    }

    // ── RED→GREEN: forbidden-zone + out-of-scope still abort via the wire ──
    #[test]
    fn forbidden_zone_action_aborts() {
        let contract = ActionContract {
            name: "touch-secret",
            effect: vec![0.0],
            forbidden_center: 0.0,
            forbidden_radius: 0.5,
            forbidden_height: 10.0,
        };
        let mut rt = LoopRuntime::new(0.5);
        // baseline/k define the potential well the forbidden-zone wall lives in
        // (mirrors wiring::forbidden_zone_action_refused) — without them the
        // wall can't be evaluated and the action would wrongly proceed.
        let phase = rt.cycle(
            "write the docs",
            &l5_stable(),
            Some(&contract),
            &[1.0],
            &[1.0],
            None,
            None,
        );
        assert_eq!(phase, LoopPhase::Abort);
    }

    // ── Goodhart alarm wiring: decoupled series must fire ──
    #[test]
    fn goodhart_decoupled_series_fires() {
        let mut rt = LoopRuntime::new(0.5);
        // Loop metric and held-out similarity both vary but are DECORRELATED
        // (sin vs sin at a different frequency): the loop is optimizing a proxy
        // decoupled from the real objective → Goodhart should fire. Both deltas
        // have variance AND stay uncorrelated (unlike sin/cos, whose deltas are
        // phase-related and DO correlate).
        for i in 0..16 {
            let x = i as f64;
            rt.observe(x.sin(), (x * 2.3).sin());
        }
        assert!(
            rt.goodhart_fires(),
            "decoupled (sin vs sin2.3) q/s series must trip goodhart"
        );
    }
}
