//! Entropy-budget ledger — integer-bit entropy accounting (BP-06).
//!
//! **Model.** Entropy is a *consumed resource with a cap*, NOT a conserved
//! quantity. The money ledger (`ledger.rs`) enforces `Σ balance == 0`
//! (TigerBeetle conservation). Entropy does the opposite: it is a
//! monotonically-consumed budget with an upper bound. The invariant is
//!
//! ```text
//! 0 ≤ D_t ≤ H_max        (NOT Σ = 0)
//! ```
//!
//! where `D_t` is the accumulated "heat" (integer bits) — the entropy debt.
//! `D_t` is a random walk with drift `μ = E[ΔH_in] − E[ΔH_out]`. Infinite
//! liveness ⟺ strictly negative drift `⟨ΔH_out⟩ > ⟨ΔH_in⟩` (Foster–Lyapunov):
//! `μ ≥ 0` ⇒ overflow almost surely, which is exactly what the stationarity
//! monitor alarms on.
//!
//! **Reused machinery (from `ledger.rs`).** Entries are content-addressed by a
//! deterministic SHA-256 id `H(id‖ΔH_in‖ΔH_out)`, so replaying the same entry is
//! a NO-OP (idempotency). A malformed or budget-violating entry is rejected
//! (fail-closed), never silently applied.
//!
//! **Integer-only contract.** No `f64` ever touches `D_t`. The budget update
//! `step` is pure `i64` arithmetic. The stationarity monitor computes its mean
//! drift `μ̂` as an integer sum (`mean ≥ 0 ⟺ Σ ≥ 0` for a positive window
//! length), so the monitor is also float-free.
//!
//! **Compression length `L(x)` — deterministic proxy (FOLLOW-UP).** The
//! blueprint specifies `L(x) = gzip(x).len() * 8` (integer bits). `flate2`
//! (gzip) is NOT a dependency of this crate, and adding one offline is
//! undesirable, so we use the deterministic stand-in
//!
//! ```text
//! L(x) = len(x) * 8      (integer bits)
//! ```
//!
//! This is a strict *upper bound* on any real compressor's output length
//! (`gzip(x).len() ≤ len(x)` for compressible input), so the proxy makes the
//! budget conservative: swapping in real `flate2` `GzEncoder` later can only
//! ever *reduce* `ΔH_in`/`ΔH_out`. The integer-bit contract (`L` returns `i64`
//! bits) is unchanged, so the swap is a drop-in. Tracked as a follow-up.

use sha2::{Digest, Sha256};
use std::collections::HashSet;

/// Error returned when the entropy budget is violated (overflow) or a malformed
/// entry is offered (negative ΔH — which by construction must be ≥ 0).
///
/// `HeatOverflow` means "the budget cap `H_max` would be exceeded". On this
/// error the budget is left **unchanged** (reject-without-mutation) so the
/// invariant `0 ≤ D_t ≤ H_max` still holds in the rejected state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeatOverflow;

impl std::fmt::Display for HeatOverflow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("entropy budget overflow: D_t would exceed H_max")
    }
}

impl std::error::Error for HeatOverflow {}

/// A content-addressed entropy entry: `(id, ΔH_in, ΔH_out)`.
///
/// `id` is a caller-supplied logical id (e.g. an event/step id). Two entries
/// with the same `(id, ΔH_in, ΔH_out)` hash to the same SHA-256 entry id, so
/// replaying one is a no-op. Different `id`s are distinct entries even with
/// equal deltas (both are applied).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntropyEntry {
    pub id: String,
    pub dh_in: i64,  // ΔH_in  (must be ≥ 0)
    pub dh_out: i64, // ΔH_out (must be ≥ 0)
}

impl EntropyEntry {
    pub fn new(id: impl Into<String>, dh_in: i64, dh_out: i64) -> Self {
        Self {
            id: id.into(),
            dh_in,
            dh_out,
        }
    }

    /// Deterministic SHA-256 content address of this entry:
    /// `H(id ‖ ΔH_in ‖ ΔH_out)`, hex-encoded.
    pub fn entry_id(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.id.as_bytes());
        h.update(b"|");
        h.update(self.dh_in.to_le_bytes());
        h.update(b"|");
        h.update(self.dh_out.to_le_bytes());
        hex::encode(h.finalize())
    }
}

/// The entropy budget ledger. `D_t` is the integer-bit debt, `H_max` the cap.
///
/// Invariant (must always hold): `0 ≤ D_t ≤ H_max`.
#[derive(Debug, Clone)]
pub struct EntropyBudget {
    h_max: i64,
    debt: i64, // D_t, integer bits
    /// Set of entry ids already applied (idempotency guard).
    applied: HashSet<String>,
}

impl EntropyBudget {
    /// Create a fresh budget with cap `h_max`. `h_max` must be ≥ 0.
    pub fn new(h_max: i64) -> Self {
        assert!(h_max >= 0, "H_max must be non-negative");
        Self {
            h_max,
            debt: 0,
            applied: HashSet::new(),
        }
    }

    pub fn debt(&self) -> i64 {
        self.debt
    }
    pub fn h_max(&self) -> i64 {
        self.h_max
    }

    /// Invariant check: `0 ≤ D_t ≤ H_max`.
    pub fn invariant_holds(&self) -> bool {
        self.debt >= 0 && self.debt <= self.h_max
    }

    /// Raw budget update — matches the blueprint TARGET STATE signature.
    ///
    /// ```text
    /// D_t ← (D_t + ΔH_in − ΔH_out) clamp₀
    /// if D_t > H_max { reject }
    /// ```
    ///
    /// Negative deltas are malformed (ΔH is `max(0, …)` by construction) and
    /// are rejected. On overflow the budget is left **unchanged**
    /// (reject-without-mutation) so the `0 ≤ D_t ≤ H_max` invariant holds even
    /// in the rejected state.
    ///
    /// NOTE: this raw form does NOT do idempotency. For idempotent,
    /// content-addressed application use [`apply`](Self::apply) /
    /// [`apply_entry`](Self::apply_entry).
    pub fn step(&mut self, dh_in: i64, dh_out: i64) -> Result<(), HeatOverflow> {
        if dh_in < 0 || dh_out < 0 {
            return Err(HeatOverflow); // malformed entry (fail closed)
        }
        let candidate = (self.debt + dh_in - dh_out).max(0); // clamp₀: heat is not banked
        if candidate > self.h_max {
            return Err(HeatOverflow); // reject without mutating: invariant preserved
        }
        self.debt = candidate;
        Ok(())
    }

    /// Idempotent, content-addressed application of `(id, ΔH_in, ΔH_out)`.
    ///
    /// Replaying an already-applied entry (same SHA-256 id) is a clean no-op:
    /// the budget and `applied` set are unchanged and `Ok(())` is returned.
    /// This mirrors `ledger.rs` idempotent replay.
    pub fn apply(&mut self, id: &str, dh_in: i64, dh_out: i64) -> Result<(), HeatOverflow> {
        let e = EntropyEntry::new(id, dh_in, dh_out);
        self.apply_entry(&e)
    }

    /// Idempotent application of a pre-built [`EntropyEntry`].
    pub fn apply_entry(&mut self, e: &EntropyEntry) -> Result<(), HeatOverflow> {
        if e.dh_in < 0 || e.dh_out < 0 {
            return Err(HeatOverflow); // malformed entry (fail closed)
        }
        let id = e.entry_id();
        if self.applied.contains(&id) {
            return Ok(()); // idempotent replay = no-op
        }
        // Apply the budget update only after the id-check passes, so replay
        // never re-runs the arithmetic and the invariant is preserved.
        self.step(e.dh_in, e.dh_out)?;
        self.applied.insert(id);
        Ok(())
    }

    /// Has `(id, ΔH_in, ΔH_out)` already been applied?
    pub fn is_applied(&self, id: &str, dh_in: i64, dh_out: i64) -> bool {
        self.is_applied_entry(&EntropyEntry::new(id, dh_in, dh_out))
    }

    /// Has this exact [`EntropyEntry`] already been applied?
    pub fn is_applied_entry(&self, e: &EntropyEntry) -> bool {
        self.applied.contains(&e.entry_id())
    }
}

/// Deterministic compression-length proxy `L(x)` in integer BITS.
///
/// **PROXY (FOLLOW-UP):** real spec is `gzip(x).len() * 8`; `flate2` is not a
/// dep, so we use `len(x) * 8`. This is a strict upper bound on any real
/// compressor, making the budget conservative. Swap-in for real gzip is
/// drop-in (still returns `i64` bits).
pub fn compression_length_bits(x: &[u8]) -> i64 {
    (x.len() as i64) * 8
}

/// `ΔH_in = max(0, L(x_raw) − L(x_prev))`.
pub fn delta_h_in(x_raw: &[u8], x_prev: &[u8]) -> i64 {
    let l_raw = compression_length_bits(x_raw);
    let l_prev = compression_length_bits(x_prev);
    (l_raw - l_prev).max(0)
}

/// `ΔH_out = max(0, L(x_raw) − L(renorm(x_raw)))`.
pub fn delta_h_out(x_raw: &[u8], x_renorm: &[u8]) -> i64 {
    let l_raw = compression_length_bits(x_raw);
    let l_renorm = compression_length_bits(x_renorm);
    (l_raw - l_renorm).max(0)
}

/// Stationarity monitor (Foster–Lyapunov).
///
/// `increments` = per-step `ΔD_t = ΔH_in − ΔH_out` (the random-walk
/// increments). Returns `ALARM = true` iff the rolling mean drift
/// `μ̂ = mean(increments) ≥ 0`.
///
/// Integer-only: `mean ≥ 0 ⟺ Σ ≥ 0` (window length > 0), so no float touches
/// the math. `μ̂ ≥ 0` ⇒ overflow a.s. ⇒ alarm.
pub fn stationarity_monitor(increments: &[i64]) -> bool {
    if increments.is_empty() {
        return false; // nothing to observe yet — not an alarm
    }
    increments.iter().copied().sum::<i64>() >= 0
}

/// Convenience monitor over an observed `D_t` window.
///
/// Differences the window into increments `ΔD_t = D_{t+1} − D_t` and delegates
/// to [`stationarity_monitor`]. Needs ≥ 2 samples to form a difference.
///
/// CAVEAT: `D_t` is clamped at 0 (heat not banked), so during sustained
/// negative drift the observed `D_t` can flat-line at 0 and under-report the
/// true drift. Prefer feeding the *true* increments `ΔH_in − ΔH_out` to
/// [`stationarity_monitor`] when available.
pub fn stationarity_monitor_from_dt(d_t_window: &[i64]) -> bool {
    if d_t_window.len() < 2 {
        return false;
    }
    let increments: Vec<i64> = d_t_window.windows(2).map(|w| w[1] - w[0]).collect();
    stationarity_monitor(&increments)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------------
    // RED → ALARM: positive drift (μ>0) makes D_t grow monotonically to
    // HeatOverflow.
    // ---------------------------------------------------------------------
    #[test]
    fn red_positive_drift_overflows() {
        // H_max = 100, every step adds 10 bits of heat, never removes any.
        let mut b = EntropyBudget::new(100);
        let mut ok_count = 0usize;
        let mut err_count = 0usize;
        let mut last_debt = 0i64;
        for _ in 0..20 {
            match b.step(10, 0) {
                Ok(()) => {
                    ok_count += 1;
                    // D_t must grow monotonically while it is below the cap.
                    assert!(
                        b.debt() >= last_debt,
                        "D_t must be monotonically non-decreasing under μ>0"
                    );
                    last_debt = b.debt();
                }
                Err(HeatOverflow) => {
                    err_count += 1;
                    // Reject-without-mutation: invariant must STILL hold.
                    assert!(b.invariant_holds(), "invariant broken after overflow");
                }
            }
        }
        // 10 successful +10 steps (10..100), then every further step overflows.
        assert_eq!(ok_count, 10, "expected exactly 10 steps under the cap");
        assert!(err_count > 0, "positive drift must eventually overflow");
        // debt sits exactly at the cap; it never exceeds H_max.
        assert_eq!(b.debt(), 100);
        assert!(b.invariant_holds());
    }

    #[test]
    fn red_overflow_rejects_without_mutating() {
        // The violating step must NOT persist a debt > H_max.
        let mut b = EntropyBudget::new(50);
        assert!(b.step(50, 0).is_ok()); // debt = 50
        assert_eq!(b.debt(), 50);
        // Next +10 would make 60 > 50 → reject, debt stays 50.
        let res = b.step(10, 0);
        assert!(res.is_err());
        assert_eq!(b.debt(), 50, "overflow must not mutate debt");
        assert!(b.invariant_holds());
    }

    // ---------------------------------------------------------------------
    // GREEN: negative drift (μ<0) keeps D_t bounded; no overflow.
    // ---------------------------------------------------------------------
    #[test]
    fn green_negative_drift_stays_bounded() {
        // Start with debt 80, every step removes more than it adds.
        let mut b = EntropyBudget::new(100);
        b.step(0, 0).unwrap(); // bring to 0 baseline via raw step (no-op at 0)
        let start = b.debt();
        // Seed debt to 80 directly through a legitimate large-in step.
        b.step(80, 0).unwrap();
        assert_eq!(b.debt(), 80);
        for _ in 0..20 {
            // each step: +5 in, −15 out ⇒ drift −10, bounded.
            let r = b.step(5, 15);
            assert!(r.is_ok(), "negative drift must never overflow");
            assert!(b.invariant_holds());
            assert!(b.debt() <= 80);
            assert!(b.debt() >= start, "debt never negative (clamp₀)");
        }
        // It must have actually drained toward the floor, not stayed pinned.
        assert!(b.debt() < 80, "negative drift must reduce debt");
    }

    #[test]
    fn green_clamp_zero_no_banking() {
        // Even with large dh_out, debt never goes negative (heat not banked).
        let mut b = EntropyBudget::new(100);
        b.step(10, 0).unwrap();
        assert_eq!(b.debt(), 10);
        b.step(0, 999).unwrap();
        assert_eq!(b.debt(), 0);
        assert!(b.invariant_holds());
    }

    // ---------------------------------------------------------------------
    // Idempotent replay: re-applying the same (id, ΔH_in, ΔH_out) is a no-op.
    // ---------------------------------------------------------------------
    #[test]
    fn idempotent_replay_is_noop() {
        let mut b = EntropyBudget::new(1000);
        let e = EntropyEntry::new("evt-7", 40, 10);
        assert!(b.apply_entry(&e).is_ok());
        let after_first = b.debt();
        assert_eq!(after_first, 30, "0 + 40 − 10 = 30");
        assert!(b.is_applied_entry(&e));

        // Replay — must be a clean no-op, not a double application.
        assert!(b.apply_entry(&e).is_ok(), "replay returns Ok (no-op)");
        assert_eq!(
            b.debt(),
            after_first,
            "debt changed on replay — double spend!"
        );
        assert!(b.is_applied_entry(&e));

        // A different id with the SAME deltas is a distinct entry (applied).
        let e2 = EntropyEntry::new("evt-7-different", 40, 10);
        assert!(!b.is_applied_entry(&e2));
        assert!(b.apply_entry(&e2).is_ok());
        assert_eq!(b.debt(), 60, "distinct id applies its delta once");
    }

    #[test]
    fn idempotent_replay_via_apply() {
        let mut b = EntropyBudget::new(1000);
        assert!(b.apply("a", 20, 5).is_ok());
        assert_eq!(b.debt(), 15);
        assert!(b.is_applied("a", 20, 5));
        assert!(b.apply("a", 20, 5).is_ok()); // replay
        assert_eq!(b.debt(), 15, "replay must not re-apply");
    }

    // ---------------------------------------------------------------------
    // Stationarity monitor (Foster–Lyapunov): alarms when μ̂ ≥ 0.
    // ---------------------------------------------------------------------
    #[test]
    fn stationarity_alarms_on_nonneg_drift() {
        // μ > 0: increments all positive.
        assert!(stationarity_monitor(&[3, 2, 1]));
        // μ = 0: increments sum to 0 (alarm — borderline is NOT safe).
        assert!(stationarity_monitor(&[1, -1, 2, -2]));
        // μ < 0: increments sum negative → no alarm.
        assert!(!stationarity_monitor(&[-1, -2, -3]));
        // empty window → not an alarm (nothing observed).
        assert!(!stationarity_monitor(&[]));
    }

    #[test]
    fn stationarity_from_dt_window() {
        // Observed D_t growing: increments +3,+3 ⇒ μ̂>0 ⇒ alarm.
        assert!(stationarity_monitor_from_dt(&[4, 7, 10]));
        // Observed D_t shrinking: increments −1,−2,−3 ⇒ μ̂<0 ⇒ no alarm.
        assert!(!stationarity_monitor_from_dt(&[10, 9, 7, 4]));
        // Too few samples → not an alarm.
        assert!(!stationarity_monitor_from_dt(&[10]));
    }

    // ---------------------------------------------------------------------
    // Compression proxy L(x) contract (integer, deterministic).
    // ---------------------------------------------------------------------
    #[test]
    fn compression_length_proxy_is_deterministic_and_integer() {
        let x = b"some entropy-bearing payload";
        assert_eq!(compression_length_bits(x), (x.len() as i64) * 8);
        assert_eq!(compression_length_bits(x), compression_length_bits(x));
        // Bigger input ⇒ bigger (or equal) L ⇒ non-negative ΔH_in.
        let bigger = b"some entropy-bearing payload with extra words";
        assert!(delta_h_in(bigger, x) > 0);
        assert_eq!(
            delta_h_out(x, bigger),
            0,
            "renorm larger ⇒ no entropy removed"
        );
    }

    // ---------------------------------------------------------------------
    // Full entry-driven flow wired through L(x): ΔH computed from payloads,
    // applied idempotently to the ledger.
    // ---------------------------------------------------------------------
    #[test]
    fn entry_flow_through_compression_deltas() {
        let x_prev = b"";
        let x_raw = b"hello world this is raw";
        let x_renorm = b"hello world this is renorm"; // shorter ⇒ some ΔH_out

        let dh_in = delta_h_in(x_raw, x_prev);
        let dh_out = delta_h_out(x_raw, x_renorm);
        assert!(dh_in > 0);

        let mut b = EntropyBudget::new(10_000);
        let e = EntropyEntry::new("ingest-1", dh_in, dh_out);
        assert!(b.apply_entry(&e).is_ok());
        let d1 = b.debt();
        // Replay.
        assert!(b.apply_entry(&e).is_ok());
        assert_eq!(b.debt(), d1, "replay no-op through L(x)-derived entry");
        assert!(b.invariant_holds());
    }

    // ---------------------------------------------------------------------
    // Invariant is always preserved across the whole public surface.
    // ---------------------------------------------------------------------
    #[test]
    fn invariant_holds_everywhere() {
        let mut b = EntropyBudget::new(200);
        for i in 0..50 {
            // Mixed: sometimes μ>0, sometimes μ<0.
            let (din, dout) = if i % 3 == 0 { (10, 0) } else { (0, 7) };
            let _ = b.apply(&format!("s{i}"), din, dout);
            assert!(b.invariant_holds(), "invariant broken at step {i}");
        }
        assert!(b.invariant_holds());
    }
}
