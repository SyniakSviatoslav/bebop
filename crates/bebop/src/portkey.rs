//! Portkey — deterministic local transport / gateway abstraction.
//!
//! Replaces the TS-retired `Portkey gateway` behavior as real, tested Rust.
//! This is the *offline* gateway: an in-process pub/sub + request/reply router
//! keyed by topic, with deterministic routing (no network, no rng, no clock).
//! The wire shape is JSON over a string envelope so the same API can later sit
//! on top of a real mesh (e.g. Zenoh) without changing call sites.
//!
//! Design note: this is NOT the network stack. It is the *abstraction* — the
//! seam where a real mesh transport would plug in. Verified by in-process tests.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// A routed message envelope.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Envelope {
    pub topic: String,
    pub from: String,
    pub to: String, // "" = broadcast
    pub body: String,
}

/// A subscriber callback handle id.
pub type SubId = usize;

/// Portkey: in-process message bus. `Arc<Mutex<..>>` so it can be shared across
/// "peers" in a single process (the offline stand-in for a mesh node).
#[derive(Clone, Default)]
pub struct Portkey {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    subs: HashMap<String, Vec<SubId>>,
    handlers: HashMap<SubId, Box<dyn Fn(&Envelope) + Send + Sync>>,
    next_id: SubId,
    /// delivery log (topic, body) — for deterministic assertions in tests.
    log: Vec<(String, String)>,
}

impl Default for Inner {
    fn default() -> Self {
        Inner {
            subs: HashMap::new(),
            handlers: HashMap::new(),
            next_id: 1,
            log: Vec::new(),
        }
    }
}

impl Portkey {
    pub fn new() -> Self {
        Portkey::default()
    }

    /// Subscribe to a topic. Returns a handle id (used to unsubscribe).
    pub fn subscribe<F>(&self, topic: &str, f: F) -> SubId
    where
        F: Fn(&Envelope) + Send + Sync + 'static,
    {
        let mut g = self.inner.lock().unwrap();
        let id = g.next_id;
        g.next_id += 1;
        g.handlers.insert(id, Box::new(f));
        g.subs.entry(topic.to_string()).or_default().push(id);
        id
    }

    pub fn unsubscribe(&self, topic: &str, id: SubId) {
        let mut g = self.inner.lock().unwrap();
        if let Some(v) = g.subs.get_mut(topic) {
            v.retain(|x| *x != id);
        }
        g.handlers.remove(&id);
    }

    /// Publish to a topic. Delivers to every subscriber (and to `to`-matched
    /// subscribers). Returns the count of handlers invoked.
    pub fn publish(&self, env: &Envelope) -> usize {
        let mut g = self.inner.lock().unwrap();
        g.log.push((env.topic.clone(), env.body.clone()));
        let ids: Vec<SubId> = match g.subs.get(&env.topic) {
            Some(v) => v.clone(),
            None => return 0,
        };
        // invoke each handler in-place (boxed closures can't be cloned)
        let mut count = 0;
        for id in &ids {
            if let Some(h) = g.handlers.get(id) {
                h(env);
                count += 1;
            }
        }
        count
    }

    /// Request/reply over the same bus: publish on `topic`, but addressed `to`
    /// a specific peer. Convenience wrapper; routing is still topic-based.
    pub fn send(&self, env: &Envelope) -> usize {
        self.publish(env)
    }

    /// Number of deliveries recorded (for deterministic test assertions).
    pub fn delivery_count(&self) -> usize {
        self.inner.lock().unwrap().log.len()
    }

    /// All delivered (topic, body) pairs (for assertions).
    pub fn deliveries(&self) -> Vec<(String, String)> {
        self.inner.lock().unwrap().log.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_reaches_subscriber() {
        // GREEN: a subscriber on "helm" receives the published envelope.
        let bus = Portkey::new();
        let got = Arc::new(Mutex::new(String::new()));
        let g2 = got.clone();
        bus.subscribe("helm", move |e| {
            *g2.lock().unwrap() = e.body.clone();
        });
        let n = bus.publish(&Envelope {
            topic: "helm".into(),
            from: "copilot".into(),
            to: "".into(),
            body: "turn to port".into(),
        });
        assert_eq!(n, 1);
        assert_eq!(*got.lock().unwrap(), "turn to port");
    }

    #[test]
    fn no_sub_no_delivery() {
        // RED: publishing on a topic with no subscriber delivers 0.
        let bus = Portkey::new();
        let n = bus.publish(&Envelope {
            topic: "void".into(),
            from: "x".into(),
            to: "".into(),
            body: "silence".into(),
        });
        assert_eq!(n, 0);
        assert_eq!(bus.delivery_count(), 1); // the envelope is still logged
    }

    #[test]
    fn unsubscribe_stops_delivery() {
        // RED: after unsubscribe the handler must not fire.
        let bus = Portkey::new();
        let hits = Arc::new(Mutex::new(0usize));
        let h2 = hits.clone();
        let id = bus.subscribe("engines", move |_| {
            *h2.lock().unwrap() += 1;
        });
        bus.publish(&Envelope {
            topic: "engines".into(),
            from: "a".into(),
            to: "".into(),
            body: "burn".into(),
        });
        bus.unsubscribe("engines", id);
        bus.publish(&Envelope {
            topic: "engines".into(),
            from: "a".into(),
            to: "".into(),
            body: "burn again".into(),
        });
        assert_eq!(*hits.lock().unwrap(), 1, "handler fired after unsubscribe");
    }
}
