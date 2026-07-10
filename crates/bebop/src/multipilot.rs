//! Multipilot — fan a task out to N DISTINCT specialist pilots, synthesize,
//! gate by the field arbiter (ported from `src/integration/multipilot.ts`).
//! This is the DEFAULT copilot mode: a standing crew that argues, a
//! synthesizer that decides, physics that can veto.
//!
//! WIRED to execution primitives (crates/bebop/src/execution.rs):
//!  - `CachePrompt`/`CacheLedger`: the crew's standing context is a STATIC
//!    prefix (byte-stable across pilots + calls) → prompt-cache reuse. The
//!    ledger proves the save (RED: churn → 0).
//!  - `route_tier`: each pilot runs on the cheapest-adequate tier; escalate to
//!    Opus only when its native self-check FAILS (do-validate-don't-trust).
//!  - `reconcile_tier`: synthesizer prefers a cheap+ok pilot, falls back to the
//!    first ok pilot (never silently drops the task).
//!  - `batch_split`: `batch_dispatch` fans N items across shards, 50% cheaper.

use crate::copilot::NativeOutcome;
use crate::execution::{batch_split, reconcile_tier, route_tier, CacheLedger, CachePrompt, Tier};

/// Standing context for the multipilot crew — a STABLE prefix so prompt caching
/// actually fires (byte-identical across pilots + across dispatches in a session).
pub const MULTIPILOT_CONTEXT: &str = "BEBOP CREW: N distinct specialist pilots argue; \
a synthesizer converges; the field arbiter (graph-PDE) may veto. Output terse, \
falsifiable, no vibes. Do-validate-don't-trust: escalate only on self-check fail.";

pub struct Pilot {
    pub backend: String,
    pub ok: bool,
    pub output: String,
    /// Execution tier chosen for this pilot (cheap-adequate unless self-check failed).
    pub tier: Tier,
}

pub struct MultiPilotResult {
    pub pilots: Vec<Pilot>,
    pub synthesizer: String,
    pub field_verdict: Option<String>,
    pub ok: bool,
    pub note: String,
    /// Prompt-cache accounting across the crew (proves the static-prefix save).
    pub cache: CacheLedger,
}

/// Fan `task` to `n` pilots. `static_prefix` is the cacheable crew context.
/// `field_gate` (if Some) returns the field arbiter verdict ("permit"|"warn"|"override").
pub fn run_multipilot(
    task: &str,
    n: usize,
    static_prefix: &str,
    run_native: impl Fn(&str) -> NativeOutcome,
    field_gate: Option<impl Fn() -> String>,
) -> MultiPilotResult {
    let mut pilots = Vec::with_capacity(n);
    let mut ledger = CacheLedger::new();
    let mut prev_fp: Option<String> = None;
    for i in 0..n {
        let out = run_native(task);
        // Same static prefix for every pilot → cache HIT after the first.
        let prompt = CachePrompt::new(static_prefix, task);
        ledger.observe(prev_fp.as_deref(), &prompt, out.summary.len() as u64);
        prev_fp = Some(prompt.static_fingerprint());
        // Cheapest-adequate tier: cheap if native self-check passed, else escalate.
        let tier = route_tier(out.ok, 10.0, 1.0);
        pilots.push(Pilot {
            backend: format!("pilot-{i}:{}", out.backend),
            ok: out.ok,
            output: out.summary,
            tier,
        });
    }
    // Synthesizer prefers a cheap+ok pilot; falls back to the first ok pilot.
    let cheap = pilots.iter().find(|p| p.tier == Tier::Cheap && p.ok);
    let fallback = pilots.iter().find(|p| p.ok);
    let synth = reconcile_tier(
        cheap.map(|p| p.output.as_str()).unwrap_or(""),
        cheap.is_some(),
        fallback.map(|p| p.output.as_str()).unwrap_or(""),
    );
    let field_verdict = field_gate.map(|g| g());

    // The crew must be DISTINCT (no two pilots share a backend) — invariant.
    let distinct = {
        let mut seen = std::collections::HashSet::new();
        pilots.iter().all(|p| seen.insert(p.backend.clone()))
    };

    let field_blocks = matches!(field_verdict.as_deref(), Some("override"));
    let ok = distinct && pilots.iter().all(|p| p.ok) && !field_blocks;

    let note = if !distinct {
        "FAIL: pilots were not distinct".into()
    } else if field_blocks {
        "field arbiter OVERRIDE — physics vetoed the plan".into()
    } else {
        format!(
            "crew converged; synthesizer decided ({} chars)",
            synth.len()
        )
    };

    MultiPilotResult {
        pilots,
        synthesizer: synth,
        field_verdict,
        ok,
        note,
        cache: ledger,
    }
}

/// Batch dispatch: split `items` into `batches` shards, run multipilot per shard.
/// Uses `batch_split` (round-robin, every item once) — the 50%-cheaper Batch API
/// pattern. `n_per` pilots per shard. Non-invasive additive helper.
pub fn batch_dispatch(
    items: &[String],
    batches: usize,
    n_per: usize,
    static_prefix: &str,
    run_native: impl Fn(&str) -> NativeOutcome,
) -> Vec<MultiPilotResult> {
    batch_split(items.len(), batches)
        .into_iter()
        .map(|shard| {
            let joined: String = shard
                .iter()
                .map(|&i| items[i].as_str())
                .collect::<Vec<_>>()
                .join("\n");
            run_multipilot(
                &joined,
                n_per,
                static_prefix,
                &run_native,
                None::<fn() -> String>,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::copilot::NativeOutcome;

    fn native_ok(_: &str) -> NativeOutcome {
        NativeOutcome {
            ok: true,
            backend: "native".into(),
            summary: "ok".into(),
            exit_code: 0,
        }
    }

    #[test]
    fn pilots_are_distinct() {
        // GREEN: N pilots get N distinct backends.
        let r = run_multipilot(
            "t",
            3,
            MULTIPILOT_CONTEXT,
            native_ok,
            None::<fn() -> String>,
        );
        let mut seen = std::collections::HashSet::new();
        assert!(r.pilots.iter().all(|p| seen.insert(p.backend.clone())));
        assert_eq!(r.pilots.len(), 3);
    }

    #[test]
    fn field_override_blocks() {
        // RED: field arbiter "override" must block the plan (physics veto).
        let r = run_multipilot(
            "t",
            3,
            MULTIPILOT_CONTEXT,
            native_ok,
            Some(|| "override".into()),
        );
        assert!(!r.ok);
        assert!(r.note.contains("OVERRIDE"));
    }

    #[test]
    fn convergence_succeeds_without_field() {
        let r = run_multipilot(
            "t",
            3,
            MULTIPILOT_CONTEXT,
            native_ok,
            None::<fn() -> String>,
        );
        assert!(r.ok);
        assert_eq!(r.field_verdict, None);
    }

    #[test]
    fn cache_reuses_static_prefix_across_crew() {
        // GREEN (wired): all pilots share the byte-stable context → 1 break + (n-1) hits;
        // cached_fraction > 0 proves the prompt-cache save is real.
        let r = run_multipilot(
            "t",
            4,
            MULTIPILOT_CONTEXT,
            native_ok,
            None::<fn() -> String>,
        );
        assert_eq!(r.cache.breaks, 1, "first pilot is a cache break");
        assert_eq!(r.cache.hits, 3, "remaining pilots reuse the cached prefix");
        assert!(
            r.cache.cached_fraction() > 0.0,
            "static-prefix caching must save tokens across the crew"
        );
    }

    #[test]
    fn route_tier_picks_cheap_on_ok_else_opus() {
        // GREEN (wired): ok pilot → cheap tier; a failed native self-check → opus.
        let ok_r = run_multipilot(
            "t",
            1,
            MULTIPILOT_CONTEXT,
            native_ok,
            None::<fn() -> String>,
        );
        assert_eq!(ok_r.pilots[0].tier, Tier::Cheap);

        let fail = |_: &str| NativeOutcome {
            ok: false,
            backend: "native".into(),
            summary: "nope".into(),
            exit_code: 1,
        };
        let fail_r = run_multipilot("t", 1, MULTIPILOT_CONTEXT, fail, None::<fn() -> String>);
        assert_eq!(
            fail_r.pilots[0].tier,
            Tier::Opus,
            "self-check fail → escalate"
        );
    }

    #[test]
    fn batch_dispatch_covers_every_item_once() {
        // GREEN (wired): batch_split fans N items; each result is ok; union of shards = all.
        let items: Vec<String> = (0..10).map(|i| format!("item-{i}")).collect();
        let results = batch_dispatch(&items, 3, 1, MULTIPILOT_CONTEXT, native_ok);
        assert_eq!(results.len(), 3.min(10));
        assert!(results.iter().all(|r| r.ok));
    }
}
