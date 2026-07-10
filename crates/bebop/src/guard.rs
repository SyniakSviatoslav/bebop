//! GUARD — Input/Output guards + consensus kill-switch (audit 29158).
//!
//! "Surviving Day 2 through Year 5": the nightclub bouncer analogy — Input
//! guards filter what the system accepts; Output guards filter what it emits.
//! For a protocol, the kill-switch must be at the CONSENSUS level (nodes agree
//! to suspend a misbehaving peer), not buried in one binary. This module gives
//! both:
//!
//!   • `io_guard`  — Input/Output guard for the L5 layer: rejects proposals that
//!     would move the system outside its verified-safe envelope (the same
//!     fail-closed bound the stabilizer enforces). Prevents the L5 layer from
//!     "hallucinating" routes / parameters.
//!   • `KillSwitch` — a node-suspension registry. Any honest node can VOTE to
//!     suspend a peer; suspension triggers only when a supermajority (≥ 2/3)
//!     agrees — that is the consensus-level kill-switch, not a central off-button.
//!
//! Deterministic, std-only. RED+GREEN falsifiable below.

/// The safe envelope the L5/Output guard enforces. Any proposed delta whose
/// magnitude exceeds `max_delta` for its parameter is rejected (fail-closed).
#[derive(Clone, Debug)]
pub struct GuardPolicy {
    /// Max absolute parameter delta the L5 may apply in one tick (the wall).
    pub max_delta: f64,
    /// Hard floor on the field-stability signal V̇; if the field is unstable the
    /// guard freezes ALL L5 motion (matches stabilizer fail-closed).
    pub stable_v_dot: f64,
    /// Suspension trigger: fraction of voting nodes required to suspend a peer.
    pub suspend_threshold: f64,
}

impl Default for GuardPolicy {
    fn default() -> Self {
        GuardPolicy {
            max_delta: 1.0,
            stable_v_dot: 0.0,
            suspend_threshold: 2.0 / 3.0, // BFT-style supermajority
        }
    }
}

/// Input/Output guard verdict for one L5 proposed delta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardVerdict {
    /// Permitted (within envelope).
    Permit,
    /// Refused: delta exceeds the wall or the field is unstable.
    Refuse,
}

/// Guard a single L5-proposed delta. This is the Output bouncer: the L5 may
/// *propose*, the guard *decides*. Mirrors `stabilizer::saturate` but as an
/// explicit, testable policy — so "the L5 doesn't hallucinate" is provable.
pub fn io_guard(policy: &GuardPolicy, field_stable: bool, proposed_delta: f64) -> GuardVerdict {
    if !field_stable {
        return GuardVerdict::Refuse; // unstable field ⇒ freeze (fail-closed)
    }
    if proposed_delta.abs() > policy.max_delta {
        return GuardVerdict::Refuse; // outside safe envelope
    }
    GuardVerdict::Permit
}

/// Consensus-level kill-switch: nodes vote to suspend a misbehaving peer. The
/// peer is actually suspended only when ≥ `suspend_threshold` of *known* nodes
/// vote yes — no single node can kill another (no central off-button), and a
/// peer cannot suspend itself unilaterally (self-vote ignored for the count
/// unless it's the only node, which is a degenerate single-node net).
#[derive(Clone, Debug, Default)]
pub struct KillSwitch {
    /// Known (honest) node ids in the network.
    known_nodes: Vec<String>,
    /// votes[target] = set of nodes that voted to suspend `target`.
    votes: std::collections::HashMap<String, std::collections::HashSet<String>>,
    /// Currently suspended node ids.
    suspended: std::collections::HashSet<String>,
}

impl KillSwitch {
    pub fn new(known_nodes: Vec<String>) -> Self {
        KillSwitch {
            known_nodes,
            votes: std::collections::HashMap::new(),
            suspended: std::collections::HashSet::new(),
        }
    }

    /// Cast a vote to suspend `target`, from `voter`. Returns the new suspension
    /// state of `target` after the vote. A node may not vote to suspend itself.
    pub fn vote_suspend(&mut self, voter: &str, target: &str) -> bool {
        if voter == target {
            return self.suspended.contains(target); // self-vote ignored
        }
        if !self.known_nodes.contains(&voter.to_string())
            || !self.known_nodes.contains(&target.to_string())
        {
            return self.suspended.contains(target); // unknown node, ignore
        }
        self.votes
            .entry(target.to_string())
            .or_default()
            .insert(voter.to_string());
        self.recompute(target);
        self.suspended.contains(target)
    }

    fn recompute(&mut self, target: &str) {
        let threshold = self.known_nodes.len() as f64 * (2.0 / 3.0);
        let n = self.votes.get(target).map(|v| v.len()).unwrap_or(0) as f64;
        if n >= threshold && threshold > 0.0 {
            self.suspended.insert(target.to_string());
        }
    }

    pub fn is_suspended(&self, node: &str) -> bool {
        self.suspended.contains(node)
    }

    /// A suspended node's matcher outputs must be rejected by the network.
    pub fn accepts(&self, node: &str) -> bool {
        !self.is_suspended(node)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_guard_permits_in_envelope() {
        // GREEN: stable field + small delta ⇒ permit.
        let p = GuardPolicy::default();
        assert_eq!(io_guard(&p, true, 0.5), GuardVerdict::Permit);
    }

    #[test]
    fn io_guard_refuses_unstable_field() {
        // RED+GREEN: unstable field ⇒ freeze (L5 cannot move) even for tiny delta.
        let p = GuardPolicy::default();
        assert_eq!(io_guard(&p, false, 0.001), GuardVerdict::Refuse);
    }

    #[test]
    fn io_guard_refuses_out_of_envelope() {
        // RED+GREEN: a huge "hallucinated" delta is refused by the wall.
        let p = GuardPolicy::default();
        assert_eq!(io_guard(&p, true, 50.0), GuardVerdict::Refuse);
    }

    #[test]
    fn killswitch_needs_supermajority_not_single_node() {
        // RED+GREEN: one node cannot suspend another (no central off-button);
        // ≥2/3 of 4 known nodes (=3) must agree to suspend.
        let nodes = vec!["a", "b", "c", "d"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut ks = KillSwitch::new(nodes);
        assert!(!ks.vote_suspend("a", "b"), "single vote cannot suspend");
        assert!(!ks.vote_suspend("b", "b"), "self-vote ignored");
        ks.vote_suspend("a", "b");
        ks.vote_suspend("c", "b");
        // 2 of 4 = 0.5 < 2/3, still not suspended
        assert!(!ks.is_suspended("b"));
        ks.vote_suspend("d", "b"); // 3 of 4 = 0.75 ≥ 2/3 ⇒ suspended
        assert!(ks.is_suspended("b"), "supermajority suspends");
        assert!(!ks.accepts("b"), "suspended node rejected by network");
    }

    #[test]
    fn killswitch_unknown_node_cannot_vote() {
        // RED+GREEN: an unknown attacker cannot move the suspension set.
        let nodes = vec!["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        let mut ks = KillSwitch::new(nodes);
        ks.vote_suspend("attacker", "b");
        assert!(!ks.is_suspended("b"), "unknown voter ignored");
    }
}
