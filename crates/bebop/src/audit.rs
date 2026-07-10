//! Audit log — tamper-evident, hash-chained event record.
//!
//! Replaces the TS-retired `audit` behavior as real, tested Rust. Each entry is
//! chained: `entry.hash = H(prev_hash || seq || ts_ticks || payload)`. Any
//! mutation of a past payload breaks every subsequent chain hash → fails closed
//! on verification. NO wall-clock (timestamps are monotonic tick counters,
//! deterministic), NO rng.

use sha2::{Digest, Sha256};

/// A single audit entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Entry {
    pub seq: u64,
    /// Monotonic tick (not a wall-clock; deterministic).
    pub tick: u64,
    pub actor: String,
    pub action: String,
    pub payload: String,
    /// SHA-256 of (prev_hash || seq || tick || actor || action || payload).
    pub hash: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct AuditLog {
    entries: Vec<Entry>,
    prev_hash: String,
}

fn sha256(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    let d = h.finalize();
    d.iter().map(|b| format!("{b:02x}")).collect()
}

impl AuditLog {
    /// New empty log. The genesis prev_hash is a fixed deterministic sentinel.
    pub fn new() -> Self {
        AuditLog {
            entries: Vec::new(),
            prev_hash: "GENESIS".to_string(),
        }
    }

    /// Append an entry, chaining off the previous hash. `tick` must be >= last.
    pub fn append(&mut self, tick: u64, actor: &str, action: &str, payload: &str) -> &Entry {
        let seq = self.entries.len() as u64;
        let material = format!(
            "{}|{}|{}|{}|{}|{}",
            self.prev_hash, seq, tick, actor, action, payload
        );
        let hash = sha256(&material);
        let e = Entry {
            seq,
            tick,
            actor: actor.to_string(),
            action: action.to_string(),
            payload: payload.to_string(),
            hash: hash.clone(),
        };
        self.prev_hash = hash;
        self.entries.push(e);
        &self.entries[seq as usize]
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Verify the whole chain. Returns the index of the FIRST broken entry, or
    /// None if the chain is intact. Tamper → Some(broken_index) (RED case).
    pub fn verify(&self) -> Option<usize> {
        let mut prev = "GENESIS".to_string();
        for (i, e) in self.entries.iter().enumerate() {
            let material = format!(
                "{}|{}|{}|{}|{}|{}",
                prev, e.seq, e.tick, e.actor, e.action, e.payload
            );
            let expect = sha256(&material);
            if expect != e.hash {
                return Some(i);
            }
            prev = e.hash.clone();
        }
        None
    }

    /// Serialize the whole log to JSON (for on-disk persistence).
    pub fn to_json(&self) -> String {
        serde_json::to_string(&self.entries).expect("audit serialize")
    }

    /// Load from JSON. The chain is NOT auto-verified — call `verify()` after.
    pub fn from_json(json: &str) -> anyhow::Result<Self> {
        let entries: Vec<Entry> = serde_json::from_str(json)?;
        let prev = entries
            .last()
            .map(|e| e.hash.clone())
            .unwrap_or_else(|| "GENESIS".to_string());
        Ok(AuditLog {
            entries,
            prev_hash: prev,
        })
    }
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_chains_and_verifies() {
        // GREEN: a fresh log of 3 entries verifies clean.
        let mut log = AuditLog::new();
        log.append(1, "operator", "deploy", "staging");
        log.append(2, "operator", "promote", "prod");
        log.append(3, "agent", "dispatch", "fix red ship");
        assert_eq!(log.len(), 3);
        assert!(log.verify().is_none(), "intact chain reported broken");
    }

    #[test]
    fn tamper_breaks_chain() {
        // RED: mutating a past payload must break verification at that index.
        let mut log = AuditLog::new();
        log.append(1, "operator", "deploy", "staging");
        log.append(2, "operator", "promote", "prod");
        let mut log2 = log.clone();
        // tamper with entry 0's payload (bypass the chain by editing the struct)
        log2.entries[0].payload = "EVIL".into();
        let broken = log2.verify();
        assert_eq!(broken, Some(0), "tamper at 0 not detected");
        // the original is still intact
        assert!(log.verify().is_none());
    }

    #[test]
    fn json_roundtrip_preserves_chain() {
        let mut log = AuditLog::new();
        log.append(5, "a", "x", "1");
        log.append(6, "b", "y", "2");
        let j = log.to_json();
        let back = AuditLog::from_json(&j).unwrap();
        assert_eq!(back.len(), 2);
        assert!(back.verify().is_none());
    }
}
