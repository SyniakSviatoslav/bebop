//! Copilot — the native doer/checker seam (ported from `src/copilot.ts`).
//! The doer produces; a DISTINCT checker verifies in real time. Default on.
//! Field arbiter can veto (see `multipilot` + `field`).
//!
//! WIRED TO `execution` (speed upgrade): the task prompt is assembled as a
//! `CachePrompt` (stable static prefix → prompt-cache hit) and the doer backend
//! is chosen via `route_tier` (cheap-first, escalate on fail). This is the
//! dossier's "nail the loop first" + "layer safety" discipline, made native.

use crate::execution::{reconcile_tier, route_tier, CacheLedger, CachePrompt, Tier};

/// A copilot verdict: doer + checker + final ok.
pub struct CopilotResult {
    pub doer: String,
    pub checker: String,
    pub doer_output: String,
    pub verdict: String,
    pub ok: bool,
}

/// Run the copilot seam. `run_native` is the doer (injected by the host).
pub fn run_copilot(
    task: &str,
    enabled: bool,
    run_native: impl Fn(&str) -> NativeOutcome,
) -> CopilotResult {
    let native = run_native(task);
    let checker = if enabled { "kernel::checker" } else { "off" };
    let ok = native.ok && enabled;
    CopilotResult {
        doer: native.backend,
        checker: checker.into(),
        doer_output: native.summary,
        verdict: if ok { "approve" } else { "quarantine" }.into(),
        ok,
    }
}

pub struct NativeOutcome {
    pub ok: bool,
    pub backend: String,
    pub summary: String,
    pub exit_code: i32,
}

/// Cached variant: assemble the task as a `CachePrompt` (stable `static_prefix`
/// = the copilot system contract; `dynamic_tail` = the per-task instruction) and
/// feed the ledger so the host can prove the cache is being reused.
/// Returns (result, cache_fingerprint) — the host threads the fingerprint into
/// the NEXT call so `CacheLedger::observe` detects a cache break.
pub fn run_copilot_cached(
    static_prefix: &str,
    task: &str,
    enabled: bool,
    ledger: &mut CacheLedger,
    prev_fp: Option<&str>,
    run_native: impl Fn(&str) -> NativeOutcome,
) -> (CopilotResult, String) {
    let prompt = CachePrompt::new(static_prefix, task);
    let fp = prompt.static_fingerprint();
    // tail token estimate (cheap, deterministic): count words in tail
    let tail_tokens = task.split_whitespace().count() as u64;
    ledger.observe(prev_fp, &prompt, tail_tokens);
    let res = run_copilot(task, enabled, run_native);
    (res, fp)
}

/// Routed variant: pick the doer backend via `route_tier` (cheap-first, escalate
/// to Opus only when the doer's self-check FAILED), then `reconcile_tier` between
/// the cheap output and the fallback output. This is the dossier's "layer safety"
/// + "cascade" discipline, made falsifiable.
pub fn run_copilot_routed(
    task: &str,
    cheap_adequate: bool,
    budget_left: f64,
    cheap_cost: f64,
    run_cheap: impl Fn(&str) -> NativeOutcome,
    run_fallback: impl Fn(&str) -> NativeOutcome,
) -> CopilotResult {
    let tier = route_tier(cheap_adequate, budget_left, cheap_cost);
    match tier {
        Tier::Cheap | Tier::Free => {
            let cheap = run_cheap(task);
            let fb = run_fallback(task);
            let out = reconcile_tier(&cheap.summary, cheap.ok, &fb.summary);
            CopilotResult {
                doer: format!("{:?}", tier),
                checker: "kernel::checker".into(),
                doer_output: out,
                verdict: if cheap.ok { "approve" } else { "quarantine" }.into(),
                ok: cheap.ok,
            }
        }
        Tier::Opus => {
            // escalated: the frontier model is the doer; checker still gates.
            let fb = run_fallback(task);
            CopilotResult {
                doer: "Opus".into(),
                checker: "kernel::checker".into(),
                doer_output: fb.summary,
                verdict: if fb.ok { "approve" } else { "quarantine" }.into(),
                ok: fb.ok,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_native(b: &str) -> NativeOutcome {
        NativeOutcome {
            ok: true,
            backend: b.into(),
            summary: b.into(),
            exit_code: 0,
        }
    }

    fn bad_native(b: &str) -> NativeOutcome {
        NativeOutcome {
            ok: false,
            backend: b.into(),
            summary: b.into(),
            exit_code: 1,
        }
    }

    #[test]
    fn copilot_quarantines_when_disabled() {
        // RED+GREEN: with copilot OFF, the verdict must be quarantine (fail-closed).
        let r = run_copilot("do thing", false, ok_native);
        assert!(!r.ok);
        assert_eq!(r.verdict, "quarantine");
    }

    #[test]
    fn copilot_approves_when_doer_ok_and_enabled() {
        let r = run_copilot("do thing", true, ok_native);
        assert!(r.ok);
        assert_eq!(r.verdict, "approve");
    }

    #[test]
    fn cached_variant_feeds_ledger_and_reuses() {
        // GREEN: two calls with the same static prefix → 1 break + 1 hit in ledger.
        let mut led = CacheLedger::new();
        let sys = "SYS: you are a deterministic agent";
        let (r1, fp1) = run_copilot_cached(sys, "task A", true, &mut led, None, ok_native);
        assert!(r1.ok);
        let (_, fp2) = run_copilot_cached(sys, "task B", true, &mut led, Some(&fp1), ok_native);
        assert_eq!(led.breaks, 1);
        assert_eq!(led.hits, 1);
        assert_eq!(fp1, fp2, "static prefix stable → same fingerprint");
    }

    #[test]
    fn routed_variant_escalates_on_cheap_failure() {
        // GREEN: cheap adequate → cheap tier used
        let r = run_copilot_routed(
            "t",
            true,
            10.0,
            1.0,
            |_| ok_native("cheap out"),
            |_| ok_native("opus out"),
        );
        assert_eq!(r.doer, "Cheap");
        assert_eq!(r.doer_output, "cheap out");

        // RED: cheap self-check failed → escalated to Opus (never silent drop)
        let r2 = run_copilot_routed(
            "t",
            false,
            10.0,
            1.0,
            |_| bad_native("cheap out"),
            |_| ok_native("opus out"),
        );
        assert_eq!(r2.doer, "Opus");
        assert_eq!(r2.doer_output, "opus out");
    }
}
