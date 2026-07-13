//! termux.rs — TERMUX / KALI DUAL-USE (category K of the master plan).
//!
//! Tool registry with a RECON-MANUAL mode (read-only enumeration, no execution)
//! and an explicit DUAL-USE opt-in. `enable_dual_use` is off by default; setting
//! `dual_use=true` is the operator's own choice — nothing is blocked, but the
//! gate is explicit and every dual-use call is logged. A vuln scan runs before
//! any tool marked `dual_use` is allowed to actually execute.
//!
//! No new deps.

use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolMode {
    /// Read-only recon: enumerate, map, list. Never mutates a target.
    ReconManual,
    /// Full capability: may act on a target. Requires dual_use = true.
    Operational,
}

impl Default for ToolMode {
    fn default() -> Self {
        ToolMode::ReconManual
    }
}

#[derive(Clone, Debug)]
pub struct Tool {
    pub name: String,
    pub cmd: String,
    /// Dual-use = can be abused offensively. Gated behind `dual_use` flag.
    pub dual_use: bool,
    pub description: String,
}

#[derive(Clone, Debug, Default)]
pub struct Termux {
    pub dual_use: bool,
    pub mode: ToolMode,
    pub tools: HashMap<String, Tool>,
    /// Tools that passed a prior vuln scan (by name).
    pub cleared: std::collections::HashSet<String>,
}

impl Termux {
    pub fn new() -> Self {
        let mut t = Termux::default();
        // Base recon tools are safe-by-default (manual enumeration only).
        t.register(Tool {
            name: "nmap".into(),
            cmd: "nmap -sV --top-ports 1000".into(),
            dual_use: true,
            description: "port/version enumeration (recon)".into(),
        });
        t.register(Tool {
            name: "termux-api".into(),
            cmd: "termux-<api>".into(),
            dual_use: false,
            description: "device sensors / clipboard (manual)".into(),
        });
        t
    }

    pub fn register(&mut self, tool: Tool) {
        self.tools.insert(tool.name.clone(), tool);
    }

    /// RECON-MANUAL: list tools the operator may run without enabling dual_use.
    /// Dual-use tools are listed but marked `[needs --dual-use]`.
    pub fn recon_manual(&self) -> Vec<String> {
        let mut out = Vec::new();
        for t in self.tools.values() {
            if t.dual_use && !self.dual_use {
                out.push(format!(
                    "  • {} [needs --dual-use] — {}",
                    t.name, t.description
                ));
            } else {
                out.push(format!("  • {} — {}", t.name, t.description));
            }
        }
        out
    }

    /// Mark a tool as cleared by a vuln scan.
    pub fn mark_cleared(&mut self, name: &str) {
        self.cleared.insert(name.to_string());
    }

    /// Attempt to run a tool. Returns Err if:
    ///  - the tool is dual_use and dual_use flag is off, OR
    ///  - the tool is dual_use and not yet vuln-cleared.
    /// Otherwise Ok(cmd) with the command to execute (caller runs it).
    pub fn authorize(&self, name: &str) -> Result<String, String> {
        let tool = self
            .tools
            .get(name)
            .ok_or_else(|| format!("unknown tool: {name}"))?;
        if tool.dual_use && !self.dual_use {
            return Err(format!(
                "tool '{name}' is dual-use; enable with `bebop termux dual-use on` (operator's choice)"
            ));
        }
        if tool.dual_use && !self.cleared.contains(name) {
            return Err(format!(
                "tool '{name}' needs a vuln scan before operational use (run scan, then retry)"
            ));
        }
        Ok(tool.cmd.clone())
    }

    /// Operator flip. Nothing is blocked — setting true is an explicit choice.
    pub fn set_dual_use(&mut self, on: bool) {
        self.dual_use = on;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recon_manual_lists_dual_use_as_needs_flag() {
        let t = Termux::new();
        let r = t.recon_manual().join("\n");
        assert!(r.contains("[needs --dual-use]"), "dual-use shown but gated");
        assert!(r.contains("termux-api"), "safe tools listed plainly");
    }

    #[test]
    fn dual_use_off_blocks_operational() {
        let mut t = Termux::new();
        t.set_dual_use(false);
        assert!(
            t.authorize("nmap").is_err(),
            "dual-use blocked when flag off"
        );
    }

    #[test]
    fn dual_use_on_but_unscanned_still_blocked() {
        let mut t = Termux::new();
        t.set_dual_use(true);
        assert!(
            t.authorize("nmap").is_err(),
            "dual-use needs vuln scan even when flag on"
        );
    }

    #[test]
    fn cleared_dual_use_runs() {
        let mut t = Termux::new();
        t.set_dual_use(true);
        t.mark_cleared("nmap");
        assert_eq!(t.authorize("nmap").unwrap(), "nmap -sV --top-ports 1000");
    }

    #[test]
    fn unknown_tool_errors() {
        let t = Termux::new();
        assert!(t.authorize("nonexistent").is_err());
    }

    #[test]
    fn safe_tool_runs_without_flag() {
        let t = Termux::new();
        assert!(
            t.authorize("termux-api").is_ok(),
            "non-dual tool runs anytime"
        );
    }
}
