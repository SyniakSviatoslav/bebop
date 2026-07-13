//! changes.rs — Hermes-style change/action record (KEY CHANGES visibility).
//!
//! The agent loop appends a `ChangeRecord` for every mutating action (file write,
//! command run, config set, git push). `render_changes` emits a compact, scannable,
//! Hermes-like log: one line per change with a verb + target. No LLM, no IO.

/// Kind of mutation the agent performed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    Create,
    Edit,
    Delete,
    Run,
    Config,
    Git,
}

impl ChangeKind {
    /// Hermes-like glyph prefix.
    pub fn glyph(self) -> &'static str {
        match self {
            ChangeKind::Create => "✎",  // wrote
            ChangeKind::Edit => "◆",    // edited
            ChangeKind::Delete => "�", // deleted
            ChangeKind::Run => "↳",     // ran
            ChangeKind::Config => "⚙",  // config
            ChangeKind::Git => "⎇",     // git
        }
    }
    pub fn verb(self) -> &'static str {
        match self {
            ChangeKind::Create => "wrote",
            ChangeKind::Edit => "edited",
            ChangeKind::Delete => "deleted",
            ChangeKind::Run => "ran",
            ChangeKind::Config => "set",
            ChangeKind::Git => "git",
        }
    }
}

/// A single mutating action, recorded for visibility.
#[derive(Clone, Debug)]
pub struct ChangeRecord {
    pub kind: ChangeKind,
    pub target: String,
    pub summary: String,
    /// Set by `destructive::classify` once the policy is applied.
    pub destructive: bool,
    /// Severity label (None / "destructive" / "critical") from the policy.
    pub severity: Option<String>,
}

impl ChangeRecord {
    pub fn new(kind: ChangeKind, target: &str, summary: &str) -> Self {
        ChangeRecord {
            kind,
            target: target.to_string(),
            summary: summary.to_string(),
            destructive: false,
            severity: None,
        }
    }
}

/// Render a Hermes-style scannable change log. Every record → one line:
/// `<glyph> <verb> <target> — <summary>`; destructive/critical get a ⚠ prefix.
pub fn render_changes(records: &[ChangeRecord]) -> String {
    if records.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for r in records {
        let warn = if r.severity.is_some() { "⚠ " } else { "" };
        let sev = match &r.severity {
            Some(s) => format!(" [{s}]"),
            None => String::new(),
        };
        out.push_str(&format!(
            "{} {} {} {} — {}{}\n",
            warn,
            r.kind.glyph(),
            r.kind.verb(),
            r.target,
            r.summary,
            sev
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_renders_empty() {
        // RED: no records → no output.
        assert_eq!(render_changes(&[]), "");
    }

    #[test]
    fn renders_verb_and_target() {
        // GREEN: edit + create render with glyph + verb + target.
        let recs = vec![
            ChangeRecord::new(ChangeKind::Edit, "crates/bebop/src/foo.rs", "added axis"),
            ChangeRecord::new(ChangeKind::Create, "docs/plan.md", "wrote plan"),
        ];
        let out = render_changes(&recs);
        assert!(out.contains("edited crates/bebop/src/foo.rs"));
        assert!(out.contains("wrote docs/plan.md"));
        assert!(out.contains("◆")); // edit glyph
        assert!(out.contains("✎")); // create glyph
    }

    #[test]
    fn destructive_gets_warning_prefix() {
        // GREEN: a critical record surfaces a ⚠ + [critical] label.
        let mut r = ChangeRecord::new(ChangeKind::Git, "force-push", "origin");
        r.destructive = true;
        r.severity = Some("critical".to_string());
        let out = render_changes(&[r]);
        assert!(out.contains("⚠"));
        assert!(out.contains("[critical]"));
    }
}
