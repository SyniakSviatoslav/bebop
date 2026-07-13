//! AGENTIC GIT — GCC pattern (arXiv:2508.00031 "Manage the Context of Agents
//! by Agentic Git"), reverse-engineered native + 0 deps.
//!
//! Treat an agent's memory like a git repo: every action snapshots the live
//! `LivingMemory` into a content-addressed, append-only COMMIT. `CONTEXT(hash)`
//! reconstructs the EXACT memory state at any point → a deterministic,
//! tamper-evident audit trail of the agent's own actions. `MERGE` joins two
//! histories. No RNG/clock in hashes → reproducible.
//!
//! This is the MAX-EV finding from the agentic-git-history theme: the 7 listed
//! repos (Aisdkagents, cult-ui, aliimam, styles-refero, skiper-ui, yt-dlb,
//! mgchev/skills-best-practices) are component/DESIGN.md tooling, NOT
//! agentic-git tools — so we implement the pattern ourselves, on top of the
//! existing `LivingMemory` + `vault` + `AuditLog` core, rather than integrate
//! any of them.

use crate::memory::{simple_hash, LivingMemory, MemoryNode};
use std::collections::HashMap;

/// A NON-LOSSY memory snapshot: every live node AND every attic (cold-tier)
/// node, with ALL fields preserved (concept, payload, layer, entities, topic,
/// salience) — not just concept→payload. Restoring it via `replay` reconstructs
/// the EXACT memory state (layer/salience/attic included), which the persistence
/// filter (BP-09) and salience decay (BP-13) depend on.
#[derive(Clone, Debug, PartialEq)]
pub struct FullState {
    /// live nodes, keyed by id
    pub live: HashMap<String, MemoryNode>,
    /// cold-tier (evicted-but-preserved) nodes, keyed by id
    pub attic: HashMap<String, MemoryNode>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Commit {
    pub hash: String,
    pub parents: Vec<String>,
    pub message: String,
    /// content hash of the serialized memory state at commit time
    pub state_hash: String,
    pub seq: u64,
}

/// A content-addressed chain of memory snapshots.
pub struct AgenticGit {
    head: Option<String>,
    commits: HashMap<String, Commit>,
    /// full memory snapshot, keyed by commit hash
    states: HashMap<String, FullState>,
    seq: u64,
}

impl AgenticGit {
    pub fn new() -> Self {
        AgenticGit {
            head: None,
            commits: HashMap::new(),
            states: HashMap::new(),
            seq: 0,
        }
    }

    /// COMMIT: snapshot `mem` as a child of the current head.
    pub fn commit(&mut self, mem: &LivingMemory, message: &str) -> String {
        let parent = self.head.clone();
        self.commit_with(message, parent, mem)
    }

    /// MERGE: a commit with two parents (self.head + other.head), snapshotting `mem`.
    pub fn merge(&mut self, other: &AgenticGit, mem: &LivingMemory, message: &str) -> String {
        let mut parents = Vec::new();
        if let Some(h) = &self.head {
            parents.push(h.clone());
        }
        if let Some(h) = &other.head {
            parents.push(h.clone());
        }
        self.commit_with(
            message,
            if parents.is_empty() {
                None
            } else {
                Some(parents.join("+"))
            },
            mem,
        )
    }

    fn commit_with(
        &mut self,
        message: &str,
        parent_key: Option<String>,
        mem: &LivingMemory,
    ) -> String {
        let parents: Vec<String> = match &parent_key {
            None => vec![],
            Some(k) => k.split('+').map(|s| s.to_string()).collect(),
        };
        let state = snapshot(mem);
        let serialized = serialize(&state);
        let state_hash = format!("{:08x}", simple_hash(serialized.as_bytes()));
        let seq = self.seq;
        let hash_input = format!("{:?}|{}|{}|{}", parents, state_hash, message, seq);
        let hash = format!("{:08x}", simple_hash(hash_input.as_bytes()));
        self.commits.insert(
            hash.clone(),
            Commit {
                hash: hash.clone(),
                parents: parents.clone(),
                message: message.into(),
                state_hash,
                seq,
            },
        );
        self.states.insert(hash.clone(), state);
        self.head = Some(hash.clone());
        self.seq += 1;
        hash
    }

    /// CONTEXT(hash): reconstruct the exact memory state at a commit (GREEN:
    /// returns the full snapshot; RED: unknown hash → None).
    pub fn context(&self, hash: &str) -> Option<FullState> {
        self.states.get(hash).cloned()
    }

    /// LOG: commits from root → head (chronological). Honors merge parents by
    /// walking the primary (first) parent chain; full DAG reachable via `all`.
    pub fn log(&self) -> Vec<Commit> {
        let mut out = Vec::new();
        let mut cur = self.head.clone();
        while let Some(h) = cur {
            let c = match self.commits.get(&h) {
                Some(c) => c.clone(),
                None => break,
            };
            out.push(c.clone());
            cur = c.parents.first().cloned();
        }
        out.reverse();
        out
    }

    /// All commits (full DAG), ordered by seq.
    pub fn all(&self) -> Vec<Commit> {
        let mut v: Vec<Commit> = self.commits.values().cloned().collect();
        v.sort_by_key(|c| c.seq);
        v
    }

    pub fn head(&self) -> Option<&str> {
        self.head.as_deref()
    }

    /// Tamper-evident integrity check: every stored state must still hash to its
    /// commit's `state_hash`, and every commit hash must reproduce. Returns false
    /// if ANY state was mutated after the fact (the load-bearing audit property).
    pub fn verify_integrity(&self) -> bool {
        for c in self.commits.values() {
            let state = match self.states.get(&c.hash) {
                Some(s) => s,
                None => return false,
            };
            let recomputed_state = format!("{:08x}", simple_hash(serialize(state).as_bytes()));
            if recomputed_state != c.state_hash {
                return false;
            }
            let hash_input = format!("{:?}|{}|{}|{}", c.parents, c.state_hash, c.message, c.seq);
            let recomputed_hash = format!("{:08x}", simple_hash(hash_input.as_bytes()));
            if recomputed_hash != c.hash {
                return false;
            }
        }
        true
    }
}

impl Default for AgenticGit {
    fn default() -> Self {
        Self::new()
    }
}

/// Deterministically serialize a full memory snapshot (sorted by id → stable
/// hash). Captures EVERY field of every node (live + attic) so the
/// content-addressed `state_hash` changes iff the exact memory state changes.
// LESSON: a memory snapshot that drops fields (layer/salience/attic) is a
// *lossy* audit trail — the persistence filter (BP-09) and salience decay
// (BP-13) depend on exactly those fields, so replay must restore them 1:1.
fn snapshot(mem: &LivingMemory) -> FullState {
    let mut live = HashMap::new();
    for (id, n) in mem.nodes() {
        live.insert(id.clone(), n.clone());
    }
    let mut attic = HashMap::new();
    for (id, n) in mem.attic() {
        attic.insert(id.clone(), n.clone());
    }
    FullState { live, attic }
}

fn serialize(state: &FullState) -> String {
    let mut lines: Vec<String> = Vec::new();
    for (id, n) in state.live.iter().chain(state.attic.iter()) {
        // Stable, field-explicit encoding. Layer is encoded by discriminant.
        let layer = match n.layer {
            crate::memory::Layer::Working => "W",
            crate::memory::Layer::Short => "S",
            crate::memory::Layer::Long => "L",
        };
        lines.push(format!(
            "{}|{}|{}|{}|{}|{}|{:.6}|{}",
            id,
            layer,
            n.concept,
            n.payload,
            n.topic,
            n.entities.join(","),
            n.salience,
            if state.attic.contains_key(id) {
                "ATTIC"
            } else {
                "LIVE"
            },
        ));
    }
    lines.sort();
    lines.join("\n")
}

/// Rebuild a `LivingMemory` from a reconstructed full snapshot (replay utility).
/// Restores EVERY field (layer/salience/entities/topic) and the attic — unlike
/// the old `remember()` path which reset everything to defaults. The replay is
/// therefore lossless: commit → replay yields the EXACT same state.
pub fn replay(state: &FullState) -> LivingMemory {
    let mut m = LivingMemory::new();
    for n in state.live.values() {
        m.restore_node(n.clone());
    }
    for n in state.attic.values() {
        m.restore_attic(n.clone());
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_with(items: &[(&str, &str)]) -> LivingMemory {
        let mut m = LivingMemory::new();
        for (c, p) in items {
            m.remember(c, p);
        }
        m
    }

    #[test]
    fn commit_is_content_addressed_and_deterministic() {
        // GREEN: same parent+state+msg+seq → identical hash (reproducible).
        let m = mem_with(&[("auth", "login boundary")]);
        let mut g1 = AgenticGit::new();
        let h1 = g1.commit(&m, "add auth");
        let mut g2 = AgenticGit::new();
        let h2 = g2.commit(&m, "add auth");
        assert_eq!(h1, h2, "content-addressed hash must be deterministic");
        // RED: different message → different hash (no collisions masquerading as equal)
        let mut g3 = AgenticGit::new();
        let h3 = g3.commit(&m, "add auth (edited)");
        assert_ne!(h1, h3, "different message must change the hash");
    }

    #[test]
    fn context_reconstructs_exact_state() {
        // GREEN: CONTEXT(hash) returns the precise memory at that commit.
        let mut m = mem_with(&[("auth", "login boundary")]);
        let mut g = AgenticGit::new();
        let h = g.commit(&m, "seed");
        m.remember("session", "token lifetime");
        let h2 = g.commit(&m, "add session");

        let at_h = g.context(&h).expect("seed context present");
        assert_eq!(at_h.live.len(), 1);
        assert!(
            at_h.live.values().any(|n| n.concept == "auth"),
            "seed snapshot has auth"
        );
        assert!(
            !at_h.live.values().any(|n| n.concept == "session"),
            "seed snapshot predates session"
        );

        let at_h2 = g.context(&h2).expect("session context present");
        assert_eq!(at_h2.live.len(), 2);
        // RED: unknown hash → None (no fabricated history)
        assert!(g.context("deadbeef").is_none());
    }

    #[test]
    fn log_walks_chronologically() {
        let mut m = mem_with(&[("a", "1")]);
        let mut g = AgenticGit::new();
        g.commit(&m, "c0");
        m.remember("b", "2");
        g.commit(&m, "c1");
        let log = g.log();
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].message, "c0");
        assert_eq!(log[1].message, "c1");
        assert_eq!(g.head(), Some(log[1].hash.as_str()));
    }

    #[test]
    fn integrity_detects_tamper() {
        // GREEN: pristine chain verifies
        let m = mem_with(&[("auth", "login boundary")]);
        let mut g = AgenticGit::new();
        g.commit(&m, "seed");
        assert!(g.verify_integrity(), "clean chain must verify");

        // RED: mutate a stored state → integrity must FAIL (tamper-evident)
        let head = g.head().unwrap().to_string();
        if let Some(s) = g.states.get_mut(&head) {
            if let Some(node) = s.live.values_mut().next() {
                node.payload = "PWNED".into();
            }
        }
        assert!(!g.verify_integrity(), "mutated state must break integrity");
    }

    #[test]
    fn merge_joins_two_histories() {
        let mut ma = mem_with(&[("a", "1")]);
        let mut ga = AgenticGit::new();
        ga.commit(&ma, "a0");
        ma.remember("a2", "x");
        ga.commit(&ma, "a1");

        let mut mb = mem_with(&[("b", "2")]);
        let mut gb = AgenticGit::new();
        gb.commit(&mb, "b0");

        // merged chain snapshots union state, with two parents
        let mut union = mem_with(&[("a", "1"), ("a2", "x"), ("b", "2")]);
        let mh = ga.merge(&gb, &union, "merge");
        let mc = ga.commits.get(&mh).unwrap();
        assert_eq!(mc.parents.len(), 2, "merge commit has two parents");
        assert_eq!(
            ga.context(&mh).unwrap().live.len(),
            3,
            "merged state = union"
        );
        assert!(ga.verify_integrity());
    }

    #[test]
    fn replay_is_lossless_full_state() {
        // BP-16 RED→GREEN: a node with rich metadata (salience, layer,
        // entities, topic) AND an attic (evicted) node must survive a
        // commit → replay roundtrip EXACTLY. The OLD code dropped every field
        // (reset to Short/layer-less) and ignored the attic.
        use crate::memory::{Layer, LivingMemory};

        let mut m = LivingMemory::new();
        let id = m.remember_meta(
            "auth",
            "login boundary",
            vec!["oauth".into(), "session".into()],
            "security",
            0.9,
        );
        // Promote to Long layer (consolidation path).
        m.nodes_mut().get_mut(&id).unwrap().layer = Layer::Long;
        // Evict it into the attic via decay (theta default 0.5; salience 0.9
        // survives, so instead force an eviction by lowering salience first).
        m.reinforce("auth", -1.0); // salience -> -0.1 (below theta) → evicted on tick
        m.tick();
        assert!(
            m.attic_contains(&id),
            "auth node should be in attic after decay"
        );

        let mut g = AgenticGit::new();
        let h = g.commit(&m, "snapshot with attic");

        // REPLAY: reconstruct EXACT state.
        let r = replay(&g.context(&h).expect("snapshot present"));
        // Attic node preserved (non-lossy cold tier).
        assert!(
            r.attic_contains(&id),
            "attic node must survive commit→replay"
        );
        let node = r.get_from_attic(&id).expect("attic node present");
        // All metadata preserved (was lost before BP-16).
        assert_eq!(node.payload, "login boundary");
        assert_eq!(node.entities, vec!["oauth", "session"]);
        assert_eq!(node.topic, "security");
        assert!(
            (node.salience - (-0.1)).abs() < 0.05,
            "salience preserved (decayed, not reset to 0)"
        );
        assert_eq!(node.layer, Layer::Long, "layer preserved");

        // Roundtrip determinism: serialize(commit) == serialize(replay).
        let orig = serialize(&snapshot(&m));
        let repl = serialize(&snapshot(&r));
        assert_eq!(
            orig, repl,
            "commit→replay must be byte-identical (lossless)"
        );
    }
}
