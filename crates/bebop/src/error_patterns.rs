//! error_patterns.rs — AUTO-LEARNING from errors (operator ask:
//! "автонавчання на помилках, error patterns повинні шукатись по закінченню
//! сесії, лупа, дебага і показуватись чітко у самері").
//!
//! The agent scans collected output/errors at the END of a session / loop /
//! debug run, classifies recurring ERROR PATTERNS, accumulates them in a local
//! JSON store (so learning persists across runs — no LLM needed), and renders a
//! clear SUMMARY block. Patterns are detected by cheap substring/regex markers
//! (offline, deterministic). Pure std + serde.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A detected error pattern — a recurring failure signature.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorPattern {
    /// Stable id (the marker that triggered it, normalized).
    pub id: String,
    /// Human-readable label shown in the summary.
    pub label: String,
    /// How many times this pattern was seen (across all runs, persisted).
    pub count: usize,
    /// Last seen context snippet (truncated).
    pub last_context: String,
    /// Where it was last seen (session / loop / debug).
    pub last_scope: String,
}

/// Scope in which errors were scanned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanScope {
    Session,
    Loop,
    Debug,
}

/// A known error marker: substring + label. Extend freely.
fn markers() -> Vec<(&'static str, &'static str)> {
    vec![
        ("panic", "Rust panic (unrecoverable)"),
        ("thread '", "Thread panic / unwind"),
        ("error[E", "Rust compile error (E-code)"),
        ("error:", "Generic error line"),
        ("cannot find", "Unresolved name / missing import"),
        ("borrow", "Borrow-checker violation"),
        ("mismatched types", "Type mismatch"),
        ("timeout", "Timeout / hung operation"),
        ("denied", "Permission denied"),
        ("not found", "Missing file / resource"),
        ("assertion failed", "Assertion failure"),
        ("FAILED", "Test failure"),
        ("warning: unused", "Dead code / unused (smell)"),
        ("connection refused", "Network/connection refused"),
        ("segfault", "Segfault (memory corruption)"),
    ]
}

/// Scan `text` for error patterns. Returns the list of (id, label, context) hits.
/// Deterministic: lowercased substring scan.
pub fn scan(text: &str, scope: ScanScope) -> Vec<(String, String, String)> {
    let hay = text.to_ascii_lowercase();
    let scope_s = match scope {
        ScanScope::Session => "session",
        ScanScope::Loop => "loop",
        ScanScope::Debug => "debug",
    };
    let mut hits: Vec<(String, String, String)> = Vec::new();
    for (marker, label) in markers() {
        let m = marker.to_ascii_lowercase();
        if let Some(pos) = hay.find(&m) {
            // capture a short context window around the hit
            let start = pos.saturating_sub(20);
            let end = (pos + marker.len() + 40).min(hay.len());
            let ctx = text
                .get(start..end)
                .unwrap_or("")
                .replace('\n', " ")
                .trim()
                .to_string();
            hits.push((marker.to_string(), label.to_string(), ctx));
            // scope only affects the stored last_scope; record it once
            let _ = scope_s;
        }
    }
    hits
}

/// Load the persisted pattern store from `path` (empty if missing/invalid).
pub fn load_store(path: &Path) -> Vec<ErrorPattern> {
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Persist the pattern store to `path` (atomic-ish write).
pub fn save_store(path: &Path, store: &[ErrorPattern]) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(s) = serde_json::to_string_pretty(store) {
        let _ = std::fs::write(path, s);
    }
}

/// Merge fresh scan hits into the persisted store, bumping counts + last context.
pub fn learn(store: &mut Vec<ErrorPattern>, hits: &[(String, String, String)], scope: ScanScope) {
    let scope_s = match scope {
        ScanScope::Session => "session",
        ScanScope::Loop => "loop",
        ScanScope::Debug => "debug",
    };
    let mut by_id: HashMap<String, usize> = HashMap::new();
    for (i, p) in store.iter().enumerate() {
        by_id.insert(p.id.clone(), i);
    }
    for (id, label, ctx) in hits {
        if let Some(&idx) = by_id.get(id) {
            store[idx].count += 1;
            store[idx].last_context = ctx.clone();
            store[idx].last_scope = scope_s.to_string();
        } else {
            store.push(ErrorPattern {
                id: id.clone(),
                label: label.clone(),
                count: 1,
                last_context: ctx.clone(),
                last_scope: scope_s.to_string(),
            });
            by_id.insert(id.clone(), store.len() - 1);
        }
    }
}

/// Default store path under the bebop data dir.
pub fn store_path() -> PathBuf {
    let base = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(base).join(".bebop/error_patterns.json")
}

/// Render a clear summary block for the CLI/summary. Empty store → "no patterns".
pub fn render_summary(store: &[ErrorPattern]) -> String {
    if store.is_empty() {
        return "⚠ ERROR PATTERNS: none learned yet.".to_string();
    }
    let mut lines = vec!["⚠ ERROR PATTERNS (learned, persisted):".to_string()];
    // show most-frequent first
    let mut sorted: Vec<&ErrorPattern> = store.iter().collect();
    sorted.sort_by(|a, b| b.count.cmp(&a.count));
    for p in sorted {
        lines.push(format!(
            "  • [x{}] {} — last({}): {}",
            p.count, p.label, p.last_scope, p.last_context
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_finds_compile_error() {
        // GREEN: a Rust E-code is detected in session text.
        let hits = scan(
            "src/main.rs:3:1 error[E0599]: no method named `foo`",
            ScanScope::Session,
        );
        assert!(hits.iter().any(|(id, _, _)| id == "error[E"));
    }

    #[test]
    fn scan_finds_test_failure() {
        let hits = scan(
            "test result: FAILED; thread 'main' panicked",
            ScanScope::Loop,
        );
        assert!(hits
            .iter()
            .any(|(_, l, _)| l.contains("Test failure") || l.contains("Thread panic")));
    }

    #[test]
    fn learn_accumulates_and_counts() {
        // GREEN: two scans of the same pattern bump count to 2.
        let mut store: Vec<ErrorPattern> = Vec::new();
        let h1 = scan("error[E0599]: no field", ScanScope::Session);
        let h2 = scan("error[E0599]: again", ScanScope::Debug);
        learn(&mut store, &h1, ScanScope::Session);
        learn(&mut store, &h2, ScanScope::Debug);
        assert_eq!(store.len(), 1);
        assert_eq!(store[0].count, 2);
        assert_eq!(store[0].last_scope, "debug");
    }

    #[test]
    fn render_empty_is_honest() {
        assert!(render_summary(&[]).contains("none learned"));
    }

    #[test]
    fn store_roundtrip_via_json() {
        // persistence: learn → save → load → same count (offline, no LLM).
        let dir = std::env::temp_dir().join("bebop_test_errstore");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("error_patterns.json");
        let _ = std::fs::remove_file(&path);
        let mut store: Vec<ErrorPattern> = Vec::new();
        let h = scan("panic at line 1", ScanScope::Loop);
        learn(&mut store, &h, ScanScope::Loop);
        save_store(&path, &store);
        let loaded = load_store(&path);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].count, 1);
        assert_eq!(loaded[0].last_scope, "loop");
        let _ = std::fs::remove_file(&path);
    }
}
