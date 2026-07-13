//! Living memory — the ONE associative store (VSA + graph + recursion).
//! Ported from `src/memory.ts`. Deterministic: a forgetting clock
//! (`tick`) decays + evicts like human memory. No RNG/Date in output paths.
//!
//! Eviction is NON-DESTRUCTIVE: nodes that age out of the live `nodes` map are
//! MOVED to an `attic` (cold tier) rather than dropped. This preserves the raw
//! state and gives a restore path, so forgetting is reversible.

use std::collections::HashMap;

#[derive(Clone)]
pub struct MemoryNode {
    pub id: String,
    pub concept: String,
    pub payload: String,
    pub layer: Layer,
    /// SimpleMem-style multi-view metadata (offline: no external embedder).
    /// entities/topic/salience let retrieval + consolidation reason beyond bag-of-bytes.
    pub entities: Vec<String>,
    pub topic: String,
    pub salience: f64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    Working,
    Short,
    Long,
}

pub struct LivingMemory {
    nodes: HashMap<String, MemoryNode>,
    /// Cold tier: evicted nodes are MOVED here, never dropped, so they stay
    /// recoverable via `restore` / `get_from_attic`.
    attic: HashMap<String, MemoryNode>,
    clock: u64,
}

impl LivingMemory {
    pub fn new() -> Self {
        LivingMemory {
            nodes: HashMap::new(),
            attic: HashMap::new(),
            clock: 0,
        }
    }

    pub fn remember(&mut self, concept: &str, payload: &str) -> String {
        // Deterministic id: hash of concept (no RNG).
        let id = format!("{:08x}", simple_hash(concept.as_bytes()));
        self.nodes.insert(
            id.clone(),
            MemoryNode {
                id: id.clone(),
                concept: concept.into(),
                payload: payload.into(),
                layer: Layer::Short,
                entities: Vec::new(),
                topic: String::new(),
                salience: 0.0,
            },
        );
        id
    }

    /// Remember with SimpleMem-style multi-view metadata (entities/topic/salience).
    /// Offline: metadata is caller-supplied, no external embedder involved.
    pub fn remember_meta(
        &mut self,
        concept: &str,
        payload: &str,
        entities: Vec<String>,
        topic: &str,
        salience: f64,
    ) -> String {
        let id = format!("{:08x}", simple_hash(concept.as_bytes()));
        self.nodes.insert(
            id.clone(),
            MemoryNode {
                id: id.clone(),
                concept: concept.into(),
                payload: payload.into(),
                layer: Layer::Short,
                entities,
                topic: topic.into(),
                salience,
            },
        );
        id
    }

    pub fn size(&self) -> usize {
        self.nodes.len()
    }

    /// Number of nodes currently preserved in the cold-tier attic.
    pub fn attic_size(&self) -> usize {
        self.attic.len()
    }

    /// Read-only access to the stored nodes (used by the knowledge retriever).
    pub fn nodes(&self) -> &std::collections::HashMap<String, MemoryNode> {
        &self.nodes
    }

    /// Mutable access to the stored nodes (used by consolidation to promote
    /// abstract parents into the `Long` layer).
    pub fn nodes_mut(&mut self) -> &mut std::collections::HashMap<String, MemoryNode> {
        &mut self.nodes
    }

    /// Read-only access to the cold-tier attic (evicted-but-preserved nodes).
    pub fn attic(&self) -> &std::collections::HashMap<String, MemoryNode> {
        &self.attic
    }

    /// True if `id` is currently preserved in the attic (evicted, not live).
    pub fn attic_contains(&self, id: &str) -> bool {
        self.attic.contains_key(id)
    }

    /// Borrow a node straight from the attic without restoring it to live.
    pub fn get_from_attic(&self, id: &str) -> Option<&MemoryNode> {
        self.attic.get(id)
    }

    /// Restore a previously-evicted node from the attic back into the live map.
    /// Returns `true` if a node with `id` was in the attic and restored.
    pub fn restore(&mut self, id: &str) -> bool {
        if let Some(node) = self.attic.remove(id) {
            self.nodes.insert(node.id.clone(), node);
            true
        } else {
            false
        }
    }

    /// Advance the forgetting clock: every tick ages nodes; old ones evict.
    /// Evicted nodes are MOVED to the `attic` (cold tier) — never dropped — so
    /// the raw state is preserved and recoverable via `restore`.
    pub fn tick(&mut self) {
        self.clock += 1;
        // Evict nodes whose concept hash mod 7 == clock mod 7 (deterministic "forgetting").
        let target = (self.clock % 7) as u8;
        let mut evicted: Vec<(String, MemoryNode)> = Vec::new();
        self.nodes.retain(|id, n| {
            if (simple_hash(n.concept.as_bytes()) as u8) % 7 == target {
                // Move to attic instead of dropping.
                evicted.push((id.clone(), n.clone()));
                false
            } else {
                true
            }
        });
        for (id, node) in evicted {
            self.attic.insert(id, node);
        }
    }

    pub fn layer_size(&self, l: Layer) -> usize {
        self.nodes.values().filter(|n| n.layer == l).count()
    }
}

/// Tiny FNV-1a hash — deterministic, no deps.
pub fn simple_hash(b: &[u8]) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    for &x in b {
        h ^= x as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_then_size() {
        let mut m = LivingMemory::new();
        let id = m.remember("copilot", "native doer/checker");
        assert!(!id.is_empty());
        assert_eq!(m.size(), 1);
    }

    #[test]
    fn tick_forgets_deterministically() {
        // GREEN/RED: ticking removes SOME nodes but the SAME sequence of ticks
        // from the SAME memory yields the SAME size (reproducible forgetting).
        let mut a = LivingMemory::new();
        let mut b = LivingMemory::new();
        for i in 0..20 {
            a.remember(&format!("c{i}"), "x");
            b.remember(&format!("c{i}"), "x");
        }
        for _ in 0..5 {
            a.tick();
            b.tick();
        }
        assert_eq!(a.size(), b.size(), "forgetting is non-deterministic");
        assert!(a.size() < 20, "tick forgot nothing");
    }

    #[test]
    fn tick_moves_evicted_node_to_attic() {
        // RED on the OLD `nodes.retain(...)` (destructive) code: the evicted
        // node is permanently gone and `attic`/`attic_contains` do not exist.
        // GREEN after the move-to-attic fix: the node is gone from `nodes` but
        // preserved in `attic` and recoverable via `restore`.
        let mut m = LivingMemory::new();
        let id = m.remember("copilot", "native doer/checker");
        // Over 7 ticks `clock % 7` cycles through every bucket 0..=6, so this
        // node's eviction bucket is guaranteed to be hit at least once.
        for _ in 0..7 {
            m.tick();
        }
        // Evicted -> no longer live.
        assert!(
            !m.nodes().contains_key(&id),
            "node still live after its eviction tick"
        );
        // NON-DESTRUCTIVE: preserved in the cold tier, not dropped.
        assert!(
            m.attic_contains(&id),
            "evicted node was permanently deleted; it must be preserved in the attic"
        );
        // Restore path brings it back into the live map unchanged.
        assert!(
            m.restore(&id),
            "restore() failed to recover node from attic"
        );
        assert!(
            m.nodes().contains_key(&id),
            "node not restored into live map"
        );
        assert_eq!(m.size(), 1);
        assert_eq!(m.attic_size(), 0, "restored node should leave the attic");
        // Raw payload preserved across eviction + restore.
        assert_eq!(m.nodes().get(&id).unwrap().payload, "native doer/checker");
    }
}
