//! Zenoh — deterministic mesh transport (local broker stand-in).
//!
//! Replaces the Research-slot "Zenoh mesh" as real, tested Rust. This is the
//! *offline* mesh: a process-local pub/sub broker that mirrors the `Portkey`
//! envelope interface, so the two are swappable behind the same call pattern.
//! A real Zenoh (`zenoh` crate) would implement the same `Mesh` trait over the
//! network; here we prove the routing/dispatch logic deterministically with no
//! network, no rng, no clock.
//!
//! This is the seam, not the wire protocol. Verified by in-process tests.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Re-use Portkey's envelope shape so transports are interchangeable.
pub use crate::portkey::Envelope;

/// A mesh transport: same contract as Portkey, different topology (mesh vs bus).
#[derive(Clone, Default)]
pub struct Mesh {
    inner: Arc<Mutex<MeshInner>>,
}

struct MeshInner {
    /// topic -> list of (node, handler)
    subs: HashMap<String, Vec<(String, usize)>>,
    handlers: HashMap<usize, Box<dyn Fn(&Envelope) + Send + Sync>>,
    next_id: usize,
    /// per-node delivery log for deterministic assertions
    log: Vec<(String, String, String)>, // (node, topic, body)
}

impl Default for MeshInner {
    fn default() -> Self {
        MeshInner {
            subs: HashMap::new(),
            handlers: HashMap::new(),
            next_id: 1,
            log: Vec::new(),
        }
    }
}

impl Mesh {
    pub fn new() -> Self {
        Mesh::default()
    }

    /// Subscribe `node` to a topic. Returns a handle id.
    pub fn join(
        &self,
        node: &str,
        topic: &str,
        f: impl Fn(&Envelope) + Send + Sync + 'static,
    ) -> usize {
        let mut g = self.inner.lock().unwrap();
        let id = g.next_id;
        g.next_id += 1;
        g.handlers.insert(id, Box::new(f));
        g.subs
            .entry(topic.to_string())
            .or_default()
            .push((node.to_string(), id));
        id
    }

    pub fn leave(&self, topic: &str, id: usize) {
        let mut g = self.inner.lock().unwrap();
        if let Some(v) = g.subs.get_mut(topic) {
            v.retain(|(_, x)| *x != id);
        }
        g.handlers.remove(&id);
    }

    /// Publish to a topic across the mesh. Every subscribed node receives a copy
    /// (that's the mesh fan-out). Returns the number of node-deliveries.
    pub fn publish(&self, env: &Envelope) -> usize {
        let mut g = self.inner.lock().unwrap();
        let targets: Vec<(String, usize)> = match g.subs.get(&env.topic) {
            Some(v) => v.clone(),
            None => return 0,
        };
        let mut count = 0;
        for (node, id) in &targets {
            if let Some(h) = g.handlers.get(id) {
                h(env);
                g.log
                    .push((node.clone(), env.topic.clone(), env.body.clone()));
                count += 1;
            }
        }
        count
    }

    /// Total node-deliveries recorded.
    pub fn delivery_count(&self) -> usize {
        self.inner.lock().unwrap().log.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_fanout_to_all_nodes() {
        // GREEN: two nodes on "telemetry" both receive the publication.
        let m = Mesh::new();
        let a = Arc::new(Mutex::new(0usize));
        let b = Arc::new(Mutex::new(0usize));
        let a2 = a.clone();
        let b2 = b.clone();
        m.join("nodeA", "telemetry", move |_| *a2.lock().unwrap() += 1);
        m.join("nodeB", "telemetry", move |_| *b2.lock().unwrap() += 1);
        let n = m.publish(&Envelope {
            topic: "telemetry".into(),
            from: "sensor".into(),
            to: "".into(),
            body: "tick".into(),
        });
        assert_eq!(n, 2);
        assert_eq!(*a.lock().unwrap(), 1);
        assert_eq!(*b.lock().unwrap(), 1);
    }

    #[test]
    fn leave_stops_node_receiving() {
        // RED: after a node leaves, it no longer receives mesh fan-out.
        let m = Mesh::new();
        let hits = Arc::new(Mutex::new(0usize));
        let h2 = hits.clone();
        let id = m.join("nodeC", "alerts", move |_| *h2.lock().unwrap() += 1);
        m.publish(&Envelope {
            topic: "alerts".into(),
            from: "x".into(),
            to: "".into(),
            body: "1".into(),
        });
        m.leave("alerts", id);
        m.publish(&Envelope {
            topic: "alerts".into(),
            from: "x".into(),
            to: "".into(),
            body: "2".into(),
        });
        assert_eq!(*hits.lock().unwrap(), 1, "node received after leaving mesh");
    }
}
