//! Wiring — the 3-layer runtime that ties the field sim, the L5 stabilizer,
//! living memory, and project/action gating into ONE deterministic loop.
//!
//! The three layers previously lived as disconnected modules:
//!   - `field`      → graph-PDE red-line physics veto (fail-closed)
//!   - `stabilizer` → L5 Neuro-Symbolic Gate (advisor proposes, kernel decides)
//!   - `memory`     → the ONE associative store (VSA + graph + recursion)
//!   - `research_patterns` → project/action gating (ActionContract, TargetScope, AuditLog)
//!
//! `wire` is the seam that runs an incoming action through all of them and
//! returns a single structured verdict. No RNG, no Date, no network — fully
//! reproducible. Every step is independently RED+GREEN tested in `stabilizer`,
//! `field`, and `research_patterns`; here we prove they COMPOSE correctly.
//!
//! Decision rule (fail-closed by construction):
//!   proceed = field == Permit
//!           AND action cleared the forbidden-zone wall (or no contract given)
//!           AND target in scope (or no target given)
//! Any one refusal ⇒ proceed = false, and the refusal reason is recorded.

use crate::audit::AuditLog; // BP-12: strong SHA256 hash-chained log (was weak research_patterns::AuditLog)
use crate::field::{field_gate_verdict, FieldVerdict};
use crate::memory::LivingMemory;
use crate::research_patterns::TargetScope;
use crate::stabilizer::{consensual_aggregate, permit_action, stabilize_step, ActionContract};

/// A proposed L5 motion (the optimizer's delta) for one tick.
#[derive(Clone, Debug, Default)]
pub struct L5Proposal {
    pub v_prev: f64,
    pub v_cur: f64,
    pub dt: f64,
    pub proposed_delta: f64,
    pub limit: f64,
    /// Optional ensemble: parallel L5 agents' proposals (consensual defense).
    pub ensemble: Vec<f64>,
    pub entropy_threshold: f64,
}

/// Outcome of the full 3-layer wire.
#[derive(Clone, Debug)]
pub struct WireOutcome {
    /// Field-sim verdict (fail-closed; Unhealthy also refuses).
    pub field: FieldVerdict,
    /// Bounded delta the deterministic core actually applies this tick (L5).
    pub l5_applied: f64,
    /// Whether the L5 ensemble agreed (None ⇒ ignored L5 on disagreement).
    pub l5_ensemble_applied: Option<f64>,
    /// Whether the action cleared the forbidden-zone geometric wall.
    pub action_permitted: bool,
    /// Whether the target was inside the authorized scope (true if no target).
    pub target_authorized: bool,
    /// Final decision: may the action proceed?
    pub proceed: bool,
    /// Human-readable refusal reason(s), empty when proceed=true.
    pub reason: String,
    /// Number of memory nodes recorded during this wire (living-memory layer).
    pub memory_nodes: usize,
    /// Audit-entry count after this wire.
    pub audit_entries: usize,
}

/// Run the full 3-layer wire for one action.
///
/// `task`          — the action text (drives the field-sim node mapping).
/// `l5`            — the L5 optimizer's proposed motion this tick.
/// `contract`      — optional ActionContract (forbidden-zone wall); None ⇒ no wall.
/// `baseline`/`k`  — potential-well shape for the contract saturation.
/// `scope`         — optional authorized target scope (TargetScope gate).
/// `target`        — optional (ip, host) the action would touch.
/// `mm` / `audit`  — the living-memory + audit surfaces (caller owns them so the
///                   loop can persist across wires — memory is STATEFUL).
pub fn wire(
    task: &str,
    l5: &L5Proposal,
    contract: Option<&ActionContract>,
    baseline: &[f64],
    k: &[f64],
    scope: Option<&TargetScope>,
    target: Option<(u32, &str)>,
    mm: &mut LivingMemory,
    audit: &mut AuditLog,
) -> WireOutcome {
    let _span =
        tracing::info_span!("wire", task = %task, has_contract = contract.is_some()).entered();
    // ── LAYER 1: FIELD SIM (red-line physics veto, fail-closed) ──────────────
    let field = field_gate_verdict(task);

    // ── LAYER 2: L5 STABILIZER (advisor proposes, kernel decides) ───────────
    let l5_applied = stabilize_step(l5.v_prev, l5.v_cur, l5.dt, l5.proposed_delta, l5.limit, 0.0);
    let l5_ensemble_applied = if l5.ensemble.is_empty() {
        None
    } else {
        consensual_aggregate(&l5.ensemble, l5.limit, l5.entropy_threshold)
    };

    // ── LAYER 4a: PROJECT/ACTION GATE (geometric forbidden-zone wall) ────────
    let action_permitted = match contract {
        Some(c) => permit_action(c, baseline, k, l5.limit).is_some(),
        None => true,
    };

    // ── LAYER 4b: TARGET SCOPE (own-project-only authorization) ─────────────
    let target_authorized = match (scope, target) {
        (Some(s), Some((ip, host))) => s.is_authorized(ip, host),
        _ => true, // no scope declared ⇒ nothing to authorize against
    };

    // ── DECISION (fail-closed): every layer must pass ───────────────────────
    let mut reasons = Vec::new();
    if field.refused() {
        reasons.push(format!("field:{field:?}"));
    }
    if !action_permitted {
        reasons.push("action_in_forbidden_zone".to_string());
    }
    if !target_authorized {
        reasons.push("target_out_of_scope".to_string());
    }
    let mut proceed = reasons.is_empty();
    let mut reason = if proceed {
        String::new()
    } else {
        reasons.join("; ")
    };

    // ── LAYER 3: LIVING MEMORY (record the wire so recall informs future) ────
    // Content-addressed, deterministic. The task hash + verdict is the key; the
    // payload carries the bounded L5 delta + decision. Recall can later surface
    // "this kind of task was previously vetoed" to upstream gating.
    let mem_concept = format!("wire:{task}");
    let mem_payload = format!(
        "field={:?} l5_applied={:.4} action_ok={} target_ok={} proceed={}",
        field, l5_applied, action_permitted, target_authorized, proceed
    );
    mm.remember(&mem_concept, &mem_payload);

    // ── LAYER 5: AUDIT (tamper-evident, hash-chained ledger of the decision) ─
    let seq = audit.len() as u64 + 1;
    audit.append(
        seq, // monotonic tick (== entry index) satisfies append's tick>=last invariant
        "wire",
        task, // no needless borrow
        &format!(
            "field={:?} action_ok={} target_ok={} proceed={}",
            field, action_permitted, target_authorized, proceed
        ),
    );

    // Fail-closed: a tampered audit chain is a red-line surface. If verify()
    // reports a broken link, refuse the decision and surface the break index.
    if let Some(broken) = audit.verify() {
        proceed = false;
        reason =
            format!("AUDIT TAMPER detected at entry {broken} — decision refused (fail-closed)");
    }

    WireOutcome {
        field,
        l5_applied,
        l5_ensemble_applied,
        action_permitted,
        target_authorized,
        proceed,
        reason,
        memory_nodes: mm.size(),
        audit_entries: audit.len(),
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

    #[test]
    fn benign_action_proceeds_and_records_memory() {
        // GREEN: a normal implementation task, stable L5, no wall, no target.
        let mut mm = LivingMemory::new();
        let mut audit = AuditLog::new();
        let out = wire(
            "implement the parser",
            &l5_stable(),
            None,
            &[],
            &[],
            None,
            None,
            &mut mm,
            &mut audit,
        );
        assert!(out.proceed, "benign action must proceed: {}", out.reason);
        assert_eq!(out.field, FieldVerdict::Permit);
        assert!(out.l5_applied > 0.0 && out.l5_applied <= 0.5);
        assert_eq!(out.memory_nodes, 1, "wire recorded into living memory");
        assert_eq!(out.audit_entries, 1, "wire wrote an audit entry");
        // The memory node is recallable by concept:
        let stored = mm.nodes().values().next().unwrap();
        assert_eq!(stored.concept, "wire:implement the parser");
    }

    #[test]
    fn redline_task_refused_by_field_fail_closed() {
        // RED: even with a STABLE L5 and no forbidden zone, a red-line task is
        // refused by the field sim (fail-closed). Proves the layers compose:
        // field veto dominates regardless of L5 optimism.
        let mut mm = LivingMemory::new();
        let mut audit = AuditLog::new();
        let out = wire(
            "rotate the deploy secrets",
            &l5_stable(),
            None,
            &[],
            &[],
            None,
            None,
            &mut mm,
            &mut audit,
        );
        assert!(!out.proceed, "red-line task must NOT proceed");
        assert_eq!(out.field, FieldVerdict::Override);
        assert!(
            out.reason.contains("field"),
            "reason cites field: {}",
            out.reason
        );
    }

    #[test]
    fn forbidden_zone_action_refused() {
        // RED: an action whose effect lands in the forbidden wall is refused,
        // even though the field sim would permit the task. The L5 geometric
        // wall (project gating) is load-bearing and independent of field.
        let contract = ActionContract {
            name: "touch-secret",
            effect: vec![0.0], // lands on the forbidden center
            forbidden_center: 0.0,
            forbidden_radius: 0.5,
            forbidden_height: 10.0,
        };
        let mut mm = LivingMemory::new();
        let mut audit = AuditLog::new();
        let out = wire(
            "write the docs", // field would permit this
            &l5_stable(),
            Some(&contract),
            &[1.0],
            &[1.0],
            None,
            None,
            &mut mm,
            &mut audit,
        );
        assert!(!out.proceed, "forbidden-zone action must be refused");
        assert!(!out.action_permitted);
        assert!(out.reason.contains("forbidden"), "{}", out.reason);
    }

    #[test]
    fn out_of_scope_target_refused() {
        // RED: a target outside the declared scope is refused (own-project-only).
        let mut scope = TargetScope::new();
        scope.allow_cidr("10.0.0.0/8");
        let mut mm = LivingMemory::new();
        let mut audit = AuditLog::new();
        // 8.8.8.8 is outside 10.0.0.0/8 → unauthorized.
        let out = wire(
            "scan the host",
            &l5_stable(),
            None,
            &[],
            &[],
            Some(&scope),
            Some((0x08080808, "google.com")),
            &mut mm,
            &mut audit,
        );
        assert!(!out.proceed, "out-of-scope target must be refused");
        assert!(!out.target_authorized);
        assert!(out.reason.contains("scope"), "{}", out.reason);
    }

    #[test]
    fn l5_ensemble_disagreement_ignored() {
        // GREEN/RED: when parallel L5 agents disagree (high entropy), the core
        // ignores L5 (None) and falls to the deterministic field + ground state.
        let mut l5 = l5_stable();
        l5.ensemble = vec![0.4, -0.4, 0.45];
        let mut mm = LivingMemory::new();
        let mut audit = AuditLog::new();
        let out = wire(
            "implement the parser",
            &l5,
            None,
            &[],
            &[],
            None,
            None,
            &mut mm,
            &mut audit,
        );
        assert!(out.proceed);
        assert!(
            out.l5_ensemble_applied.is_none(),
            "disagreement ⇒ ignore L5"
        );
    }

    #[test]
    fn memory_persists_across_wires() {
        // GREEN: living memory is STATEFUL — multiple wires accumulate, and the
        // recall layer can later surface veto history. Proves the 3 layers are
        // genuinely wired (memory is not reset per call).
        let mut mm = LivingMemory::new();
        let mut audit = AuditLog::new();
        for i in 0..3 {
            wire(
                &format!("task {i}"),
                &l5_stable(),
                None,
                &[],
                &[],
                None,
                None,
                &mut mm,
                &mut audit,
            );
        }
        assert_eq!(mm.size(), 3, "memory accumulated across wires");
        assert_eq!(audit.len(), 3);
    }

    #[test]
    fn audit_is_tamper_evident_red_to_green() {
        // BP-12 RED→GREEN: the wired AuditLog must be hash-chained. A clean
        // chain verifies; mutating a past payload breaks the chain at that index.
        let mut mm = LivingMemory::new();
        let mut audit = AuditLog::new();
        // Two clean wires → intact chain.
        wire(
            "a",
            &l5_stable(),
            None,
            &[],
            &[],
            None,
            None,
            &mut mm,
            &mut audit,
        );
        wire(
            "b",
            &l5_stable(),
            None,
            &[],
            &[],
            None,
            None,
            &mut mm,
            &mut audit,
        );
        assert!(audit.verify().is_none(), "intact chain must verify clean");

        // RED: tamper a past entry's payload (bypass the chain via entries_mut).
        {
            let rogue = &mut audit.entries_mut()[0];
            rogue.payload = "MUTATED".to_string();
        }
        let broken = audit.verify();
        assert!(
            broken.is_some_and(|i| i == 0),
            "tamper MUST be detected at index 0, got {broken:?}"
        );
    }

    #[test]
    fn wire_refuses_on_tampered_chain_fail_closed() {
        // Overlap fix #2: verify() must be fail-closed at RUNTIME, not just test.
        // A tampered audit chain fed into wire() MUST refuse the decision.
        let mut mm = LivingMemory::new();
        let mut audit = AuditLog::new();
        wire(
            "a",
            &l5_stable(),
            None,
            &[],
            &[],
            None,
            None,
            &mut mm,
            &mut audit,
        );
        // Tamper entry 0 after the fact (simulates persisted-log corruption).
        {
            let rogue = &mut audit.entries_mut()[0];
            rogue.payload = "MUTATED".to_string();
        }
        // Next wire() call must detect the break and refuse fail-closed.
        let out = wire(
            "b",
            &l5_stable(),
            None,
            &[],
            &[],
            None,
            None,
            &mut mm,
            &mut audit,
        );
        assert!(!out.proceed, "tampered chain MUST refuse (fail-closed)");
        assert!(
            out.reason.contains("TAMP"),
            "refusal reason must cite tamper, got: {}",
            out.reason
        );
    }
}
