//! Module registry — deterministic, content-addressed capability registry.
//!
//! Replaces the TS-retired `module registry` behavior as real, tested Rust.
//! Each module is addressed by the SHA-256 of its (name || version || spec),
//! so a module's identity is bound to its content — tamper a spec and the
//! address no longer matches. NO rng, NO wall-clock.

use sha2::{Digest, Sha256};
use std::collections::HashMap;

fn sha256(s: &str) -> String {
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    let d = h.finalize();
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// A registered module: a capability with a content-addressed id.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct Module {
    pub name: String,
    pub version: String,
    pub spec: String,    // the capability description / contract text
    pub address: String, // = H(name || version || spec)
}

impl Module {
    /// Compute and return the canonical content address for a (name, version, spec).
    pub fn address_of(name: &str, version: &str, spec: &str) -> String {
        sha256(&format!("{}|{}|{}", name, version, spec))
    }

    /// Build a module, computing its address.
    pub fn new(name: &str, version: &str, spec: &str) -> Self {
        let address = Self::address_of(name, version, spec);
        Module {
            name: name.to_string(),
            version: version.to_string(),
            spec: spec.to_string(),
            address,
        }
    }

    /// Verify this module's address matches its content (tamper check).
    pub fn self_consistent(&self) -> bool {
        self.address == Self::address_of(&self.name, &self.version, &self.spec)
    }
}

/// The registry: name → Module, plus a content-address index.
#[derive(Default)]
pub struct Registry {
    by_name: HashMap<String, Module>,
    by_address: HashMap<String, String>, // address -> name
}

impl Registry {
    pub fn new() -> Self {
        Registry::default()
    }

    /// Register a module. Returns Err if the name already exists (no silent overwrite).
    pub fn register(&mut self, m: Module) -> anyhow::Result<()> {
        if !m.self_consistent() {
            anyhow::bail!(
                "module {} has inconsistent address (tampered spec?)",
                m.name
            );
        }
        if self.by_name.contains_key(&m.name) {
            anyhow::bail!("module {} already registered", m.name);
        }
        self.by_address.insert(m.address.clone(), m.name.clone());
        self.by_name.insert(m.name.clone(), m);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&Module> {
        self.by_name.get(name)
    }

    /// Resolve by content address (the tamper-proof lookup).
    pub fn resolve(&self, address: &str) -> Option<&Module> {
        self.by_address
            .get(address)
            .and_then(|name| self.by_name.get(name))
    }

    pub fn len(&self) -> usize {
        self.by_name.len()
    }
    pub fn is_empty(&self) -> bool {
        self.by_name.is_empty()
    }

    /// Detect any module whose stored address no longer matches its content.
    pub fn find_tampered(&self) -> Vec<String> {
        self.by_name
            .values()
            .filter(|m| !m.self_consistent())
            .map(|m| m.name.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_is_content_bound() {
        // GREEN: same content → same address; different spec → different address.
        let a = Module::new("vault", "1.0", "encrypt at rest");
        let b = Module::new("vault", "1.0", "encrypt at rest");
        let c = Module::new("vault", "1.0", "TAMPERED spec");
        assert_eq!(a.address, b.address);
        assert_ne!(a.address, c.address);
        assert!(a.self_consistent());
    }

    #[test]
    fn register_and_resolve() {
        let mut r = Registry::new();
        let m = Module::new("field", "2.0", "graph-PDE cost surface");
        r.register(m.clone()).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r.get("field").unwrap().address, m.address);
        assert_eq!(r.resolve(&m.address).unwrap().name, "field");
    }

    #[test]
    fn duplicate_name_rejected() {
        // RED: re-registering a name must fail (no silent overwrite).
        let mut r = Registry::new();
        r.register(Module::new("x", "1", "spec")).unwrap();
        assert!(r.register(Module::new("x", "2", "other")).is_err());
    }

    #[test]
    fn tampered_module_detected() {
        // RED: a module whose address doesn't match its content is caught.
        let mut r = Registry::new();
        let mut m = Module::new("auth", "1.0", "deny on red");
        r.register(m.clone()).unwrap();
        // tamper the stored spec (bypass register)
        let stored = r.by_name.get_mut("auth").unwrap();
        stored.spec = "ALLOW ALL".into();
        assert!(!stored.self_consistent());
        assert_eq!(r.find_tampered(), vec!["auth".to_string()]);
    }
}
