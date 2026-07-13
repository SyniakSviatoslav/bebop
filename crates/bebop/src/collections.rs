//! collections.rs — LIBRARY COLLECTIONS (category J of the master plan).
//!
//! GitHub lib collections: name/gist/version/memory/downloads/langs; share /
//! install / rename / snapshot / backup / 3-2-1. Every function that touches
//! disk has a `*_in_dir` test-friendly twin so tests never race on a global
//! `BEBOP_HOME` env var.
//!
//! ponytail: vuln-scan is a deterministic name-flag stand-in here (no network);
//! swap for a real advisory DB + git-clone behind the same signature.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A single library entry in a collection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Library {
    pub name: String,
    pub url: String,
    /// Vulnerability / dual-use flag (nmap, metasploit, etc.). Install refuses
    /// without `force`.
    #[serde(default)]
    pub dual_use: bool,
    #[serde(default)]
    pub langs: Vec<String>,
}

impl Library {
    /// GREEN: a tool whose name flags known dual-use / vuln-scan surfaces.
    pub fn is_dual_use(&self) -> bool {
        is_dual_use(&self.name)
    }
}

/// A named collection of libraries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Collection {
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub libs: Vec<Library>,
}

fn default_true() -> bool {
    true
}

impl Collection {
    /// GREEN: adding a lib to a default collection succeeds and round-trips.
    pub fn add_lib(&mut self, lib: Library) {
        self.libs.retain(|l| l.name != lib.name);
        self.libs.push(lib);
    }
    /// GREEN: removing a present lib returns true; absent returns false.
    pub fn remove_lib(&mut self, name: &str) -> bool {
        let before = self.libs.len();
        self.libs.retain(|l| l.name != name);
        self.libs.len() != before
    }
}

/// The base collection always available to the operator.
pub fn default_collection() -> Collection {
    Collection {
        name: "default".into(),
        enabled: true,
        libs: vec![
            Library {
                name: "serde".into(),
                url: "https://github.com/serde-rs/serde".into(),
                dual_use: false,
                langs: vec!["rust".into()],
            },
            Library {
                name: "tokio".into(),
                url: "https://github.com/tokio-rs/tokio".into(),
                dual_use: false,
                langs: vec!["rust".into()],
            },
            Library {
                name: "ratatui".into(),
                url: "https://github.com/ratatui/ratatui".into(),
                dual_use: false,
                langs: vec!["rust".into()],
            },
        ],
    }
}

/// Dual-use / vuln-scan name flags (deterministic stand-in; no network).
pub fn is_dual_use(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.contains("nmap")
        || n.contains("metasploit")
        || n.contains("sqlmap")
        || n.contains("burpsuite")
        || n.contains("wireshark")
        || n.contains("john")
        || n.contains("hydra")
        || n.contains("exploit")
}

fn collections_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    PathBuf::from(home).join(".bebop").join("collections")
}

fn path_for(name: &str, dir: &Path) -> PathBuf {
    dir.join(format!("{name}.toml"))
}

/// Load a collection from an explicit directory (test-friendly, no global env).
pub fn load_from_dir(name: &str, dir: &Path) -> Collection {
    let p = path_for(name, dir);
    match std::fs::read_to_string(&p) {
        Ok(t) => toml::from_str(&t).unwrap_or_else(|_| default_collection()),
        Err(_) if name == "default" => default_collection(),
        Err(_) => Collection {
            name: name.to_string(),
            enabled: true,
            libs: vec![],
        },
    }
}

/// Persist a collection to an explicit directory.
pub fn save_to_dir(c: &Collection, dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let body = toml::to_string(c).unwrap_or_default();
    std::fs::write(path_for(&c.name, dir), body)
}

/// List collection names present in an explicit directory.
pub fn list_in_dir(dir: &Path) -> Vec<String> {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let p = e.path();
                if p.extension().map(|x| x == "toml").unwrap_or(false) {
                    p.file_stem()
                        .and_then(|s| s.to_str())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect(),
        Err(_) => vec![],
    }
}

/// List collection names present on disk.
pub fn list() -> Vec<String> {
    list_in_dir(&collections_dir())
}

/// Load a collection (falls back to the default collection if "default" +
/// missing — so the operator always has a base set).
pub fn load(name: &str) -> Collection {
    load_from_dir(name, &collections_dir())
}

/// Persist a collection to disk.
pub fn save(c: &Collection) -> std::io::Result<()> {
    save_to_dir(c, &collections_dir())
}

/// Add a library to a collection in an explicit dir (test-friendly).
pub fn add_in_dir(name: &str, lib: Library, dir: &Path) -> std::io::Result<Collection> {
    let mut c = load_from_dir(name, dir);
    c.add_lib(lib);
    save_to_dir(&c, dir)?;
    Ok(c)
}

/// Add a library to a collection (creates the collection if missing).
pub fn add(name: &str, lib: Library) -> std::io::Result<Collection> {
    add_in_dir(name, lib, &collections_dir())
}

/// Remove a library by name in an explicit dir; true if it was present.
pub fn remove_in_dir(name: &str, lib: &str, dir: &Path) -> std::io::Result<bool> {
    let mut c = load_from_dir(name, dir);
    let removed = c.remove_lib(lib);
    save_to_dir(&c, dir)?;
    Ok(removed)
}

/// Remove a library by name; returns true if it was present.
pub fn remove(name: &str, lib: &str) -> std::io::Result<bool> {
    remove_in_dir(name, lib, &collections_dir())
}

/// Install (enable) a collection in an explicit dir; dual-use libs need force.
pub fn install_in_dir(name: &str, force: bool, dir: &Path) -> Collection {
    let mut c = load_from_dir(name, dir);
    c.enabled = !c.libs.iter().any(|l| l.dual_use) || force;
    let _ = save_to_dir(&c, dir);
    c
}

/// Install (enable) a collection; dual-use libs need `--force`.
pub fn install(name: &str, force: bool) -> Collection {
    install_in_dir(name, force, &collections_dir())
}

/// Rename a collection file (default stays default — refuse rename of default).
pub fn rename_in_dir(from: &str, to: &str, dir: &Path) -> std::io::Result<bool> {
    if from == "default" {
        return Ok(false);
    }
    let src = path_for(from, dir);
    let dst = path_for(to, dir);
    if src.exists() {
        std::fs::rename(&src, &dst)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Rename a collection (refuses rename of the `default` collection).
pub fn rename(from: &str, to: &str) -> std::io::Result<bool> {
    rename_in_dir(from, to, &collections_dir())
}

/// Snapshot a collection into `<name>.snapshot.toml` in an explicit dir.
pub fn snapshot_in_dir(name: &str, dir: &Path) -> std::io::Result<PathBuf> {
    let c = load_from_dir(name, dir);
    let body = toml::to_string(&c).unwrap_or_default();
    let snap = dir.join(format!("{name}.snapshot.toml"));
    std::fs::write(&snap, body)?;
    Ok(snap)
}

/// Snapshot a collection into `<name>.snapshot.toml` (backup copy).
pub fn snapshot(name: &str) -> std::io::Result<PathBuf> {
    snapshot_in_dir(name, &collections_dir())
}

/// A share = a gistable TOML of the collection (returns the serialized form).
pub fn share(name: &str) -> String {
    let c = load(name);
    toml::to_string(&c).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};
    static COLL_SEQ: AtomicUsize = AtomicUsize::new(0);
    fn tmp_coll() -> PathBuf {
        let n = COLL_SEQ.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("bebop_coll_{}_{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn default_collection_has_three_libs() {
        let c = default_collection();
        assert_eq!(c.libs.len(), 3);
        assert!(c.enabled);
    }

    #[test]
    fn add_and_remove_roundtrip() {
        let dir = tmp_coll();
        let lib = Library {
            name: "reqwest".into(),
            url: "https://github.com/seanmonstar/reqwest".into(),
            dual_use: false,
            langs: vec!["rust".into()],
        };
        let c = add_in_dir("default", lib, &dir).unwrap();
        assert!(c.libs.iter().any(|l| l.name == "reqwest"));
        let removed = remove_in_dir("default", "reqwest", &dir).unwrap();
        assert!(removed);
        let c2 = load_from_dir("default", &dir);
        assert!(!c2.libs.iter().any(|l| l.name == "reqwest"));
    }

    #[test]
    fn rename_refuses_default() {
        let dir = tmp_coll();
        let lib = Library {
            name: "a".into(),
            url: "x".into(),
            dual_use: false,
            langs: vec![],
        };
        let _ = add_in_dir("mine", lib, &dir);
        assert!(rename_in_dir("default", "renamed", &dir).unwrap() == false);
        assert!(rename_in_dir("mine", "yours", &dir).unwrap() == true);
        let names = list_in_dir(&dir);
        assert!(names.contains(&"yours".to_string()));
    }

    #[test]
    fn snapshot_writes_file() {
        let dir = tmp_coll();
        let lib = Library {
            name: "z".into(),
            url: "x".into(),
            dual_use: false,
            langs: vec![],
        };
        let _ = add_in_dir("default", lib, &dir);
        let snap = snapshot_in_dir("default", &dir).unwrap();
        assert!(snap.exists());
    }

    #[test]
    fn share_returns_toml() {
        let dir = tmp_coll();
        let lib = Library {
            name: "q".into(),
            url: "x".into(),
            dual_use: false,
            langs: vec![],
        };
        let _ = add_in_dir("default", lib, &dir);
        let s = share_in_dir("default", &dir);
        assert!(s.contains("name = \"q\""));
    }

    #[test]
    fn dual_use_install_blocked_without_force() {
        let dir = tmp_coll();
        let lib = Library {
            name: "nmap".into(),
            url: "x".into(),
            dual_use: true,
            langs: vec![],
        };
        let _ = add_in_dir("default", lib, &dir);
        let c = install_in_dir("default", false, &dir);
        assert!(
            !c.enabled,
            "dual-use lib must block install without --force"
        );
        let c2 = install_in_dir("default", true, &dir);
        assert!(c2.enabled, "--force overrides the vuln gate");
    }

    /// Share a collection as a gist-ready TOML from an explicit dir.
    pub fn share_in_dir(name: &str, dir: &Path) -> String {
        let c = load_from_dir(name, dir);
        toml::to_string(&c).unwrap_or_default()
    }
}
