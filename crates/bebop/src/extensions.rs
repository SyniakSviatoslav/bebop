//! extensions.rs — USER-DEFINED rules / hooks / loops / gates / prompts (category F).
//!
//! Loaded from `~/.bebop/extensions/{rules,hooks,loops,gates,prompts}.toml`.
//! Fail-closed: a malformed file or bad entry is skipped + logged, never panics,
//! never breaks the agent. Static = literal string; dynamic = expression over live
//! Telemetry/Trace (evaluated opaquely by the consumer — here we only store + validate).
//! No new deps; reuses `toml` + `serde` already in the crate.

use serde::Deserialize;
use std::path::Path;

/// The five extension kinds, mapped to their TOML file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionKind {
    Rules,
    Hooks,
    Loops,
    Gates,
    Prompts,
}

impl ExtensionKind {
    pub fn file_name(self) -> &'static str {
        match self {
            ExtensionKind::Rules => "rules.toml",
            ExtensionKind::Hooks => "hooks.toml",
            ExtensionKind::Loops => "loops.toml",
            ExtensionKind::Gates => "gates.toml",
            ExtensionKind::Prompts => "prompts.toml",
        }
    }
}

/// One extension entry. `dynamic` marks an expression over live state.
#[derive(Clone, Debug, Deserialize)]
pub struct Extension {
    pub name: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub dynamic: bool,
    /// Free-form metadata the user may attach (category, enabled, args...).
    #[serde(default)]
    pub enabled: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
struct Manifest {
    #[serde(default)]
    extensions: Vec<Extension>,
}

/// Resolve `~/.bebop/extensions` (or $BEBOP_HOME/extensions).
pub fn extensions_dir() -> std::path::PathBuf {
    let base = std::env::var("BEBOP_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_else(|| ".".to_string());
    Path::new(&base).join(".bebop").join("extensions")
}

/// Load + validate one kind from a specific directory. Fail-closed: any parse
/// error returns the good entries seen so far (an empty vec if missing/unreadable).
pub fn load_from(kind: ExtensionKind, dir: &Path) -> Vec<Extension> {
    let path = dir.join(kind.file_name());
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(_) => return Vec::new(), // missing file = no extensions, not an error
    };
    match toml::from_str::<Manifest>(&text) {
        Ok(m) => m
            .extensions
            .into_iter()
            .filter(|e| !e.name.trim().is_empty() && !e.body.trim().is_empty())
            .collect(),
        Err(e) => {
            eprintln!(
                "  ⚠ extension load failed ({}): skipped — {}",
                path.display(),
                e
            );
            Vec::new()
        }
    }
}

/// Load + validate one kind from the default `~/.bebop/extensions`.
pub fn load(kind: ExtensionKind) -> Vec<Extension> {
    load_from(kind, &extensions_dir())
}

/// Load all five kinds from a directory, returning (kind, entries).
pub fn load_all_from(dir: &Path) -> Vec<(ExtensionKind, Vec<Extension>)> {
    [
        ExtensionKind::Rules,
        ExtensionKind::Hooks,
        ExtensionKind::Loops,
        ExtensionKind::Gates,
        ExtensionKind::Prompts,
    ]
    .into_iter()
    .map(|k| (k, load_from(k, dir)))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_ext() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("bebop_ext_test_{}", std::process::id()));
        let ext = d.join("extensions");
        std::fs::create_dir_all(&ext).unwrap();
        ext
    }

    #[test]
    fn missing_file_is_empty_not_panic() {
        let d = std::env::temp_dir().join("bebop_ext_none_xyz");
        assert!(load_from(ExtensionKind::Rules, &d).is_empty());
        assert!(load_all_from(&d).iter().all(|(_, v)| v.is_empty()));
    }

    #[test]
    fn bad_toml_is_skipped_fail_closed() {
        let ext = tmp_ext();
        let mut f = std::fs::File::create(ext.join("rules.toml")).unwrap();
        f.write_all(b"this is = not = valid toml @@@").unwrap();
        // Bad file -> empty, no panic.
        assert!(load_from(ExtensionKind::Rules, &ext).is_empty());
    }

    #[test]
    fn valid_toml_parses_and_filters_blanks() {
        let ext = tmp_ext();
        let mut f = std::fs::File::create(ext.join("hooks.toml")).unwrap();
        f.write_all(
            br#"
[[extensions]]
name = "on-red-line"
body = "block and ask"
dynamic = false

[[extensions]]
name = ""
body = "blank name skipped"

[[extensions]]
name = "dynamic-reporter"
body = "trace.cost > 100"
dynamic = true
"#,
        )
        .unwrap();
        let hooks = load_from(ExtensionKind::Hooks, &ext);
        // blank name filtered out -> 2 remain
        assert_eq!(hooks.len(), 2);
        assert!(hooks.iter().any(|e| e.name == "on-red-line" && !e.dynamic));
        assert!(hooks
            .iter()
            .any(|e| e.name == "dynamic-reporter" && e.dynamic));
    }
}
