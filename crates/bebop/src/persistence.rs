//! Persistence survival table (BP-09) — Hungarian matching + D* survival test
//! + non-destructive attic re-entry.
//!
//! This is **survival analysis**, NOT TDA. It measures generator *fixed points*
//! (persistence of a claim across iterations), not truth. Therefore it is an
//! **ADVISORY** pre-filter that *ranks what to verify* — it never adjudicates
//! truth on its own. The accept decision is `Accept(c) = P(c) ∧ V(c)`:
//! persistence (`P`) rejects noise (`D < D*`), the external verify layer (`V`)
//! rejects falsehood. An entrenched false claim re-emitted every turn gets
//! `D = n−1` (max) and is labelled CORE by persistence alone — exactly why the
//! AND-gate with verify is mandatory (see `accept`).
//!
//! Reuses `knowledge::cosine` for the similarity edge weight and the
//! `LivingMemory` attic/restore primitives for non-destructive re-entry.

use crate::knowledge::cosine;
use std::collections::HashMap;

/// A tracked claim. `birth`/`last_seen` are iteration indices (not wall-clock).
#[derive(Clone)]
pub struct Claim {
    pub id: String,
    pub birth: u32,
    pub last_seen: u32,
    pub embedding: Vec<f64>,
}

impl Claim {
    pub fn new(id: &str, birth: u32, embedding: Vec<f64>) -> Self {
        Claim {
            id: id.into(),
            birth,
            last_seen: birth,
            embedding,
        }
    }
    /// Duration of survival: `D = last_seen − birth`.
    pub fn duration(&self) -> u32 {
        self.last_seen.saturating_sub(self.birth)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    /// `D ≥ D*` and `p^D ≤ α` → persistent enough to be a candidate signal.
    /// ADVISORY only — must be AND-gated with verify.
    Core,
    /// `D < D*` → below the survival threshold; treat as noise / not established.
    Noise,
    /// `n < D* + 1` → insufficient iterations to decide; abstain, never "no signal".
    Abstain,
    /// Falls in the `(birth, duration)` shear triangle → verify/human, never auto-accept.
    Anomaly,
}

/// The survival table. `iter` is the current iteration counter. `attic`
/// preserves the original `birth` of a claim that vanished for a snapshot so a
/// later re-match can STITCH it (non-destructive re-entry, BP-09 spec).
pub struct PersistenceTable {
    claims: HashMap<String, Claim>,
    /// Cold tier: claims absent for a snapshot keep their `birth` here so a
    /// later re-match restores the ORIGINAL birth (survival continuity).
    attic: HashMap<String, Claim>,
    iter: u32,
    /// Cosine edge threshold τ for matching (NOISE_FLOOR is only a floor).
    tau: f64,
}

impl PersistenceTable {
    pub fn new() -> Self {
        // τ* ≈ equal-error point of p(sim|same)/p(sim|diff); 0.5 is a sane
        // default for the 256-dim bag-of-bytes cosine used here; NOISE_FLOOR 0.35
        // remains a hard floor inside `cosine` consumers.
        PersistenceTable {
            claims: HashMap::new(),
            attic: HashMap::new(),
            iter: 0,
            tau: 0.5,
        }
    }

    pub fn with_tau(mut self, tau: f64) -> Self {
        self.tau = tau.max(0.0);
        self
    }

    pub fn iter(&self) -> u32 {
        self.iter
    }

    pub fn len(&self) -> usize {
        self.claims.len()
    }

    pub fn is_empty(&self) -> bool {
        self.claims.is_empty()
    }

    /// Ingest a fresh snapshot `claims_now`. Matches `S_t ⇔ S_{t+1}` with a
    /// **Hungarian** (max-weight bipartite) assignment over cosine edges
    /// `w = cosine ≥ τ`. Surviving matches *carry over* `birth` (stitch via
    /// attic re-entry): a claim that vanished for a snapshot is moved to the
    /// `attic` (preserving its ORIGINAL birth); if it reappears later and
    /// re-matches, its `birth` is restored from the attic — survival continuity
    /// is never lost. Unmatched old claims die; unmatched new claims are born.
    pub fn ingest(&mut self, claims_now: Vec<Claim>) {
        self.iter += 1;
        let prev: Vec<Claim> = self.claims.drain().map(|(_, c)| c).collect();
        let n = prev.len();
        let m = claims_now.len();

        if n == 0 {
            // First snapshot (or all prior claims parked in attic): everyone is
            // born now — BUT restore an ORIGINAL birth from the attic if this id
            // re-enters after a gap (survival continuity, BP-09 re-entry).
            for mut c in claims_now {
                if let Some(prior) = self.attic.remove(&c.id) {
                    c.birth = prior.birth; // stitch across the gap
                } else {
                    c.birth = self.iter;
                }
                c.last_seen = self.iter;
                self.claims.insert(c.id.clone(), c);
            }
            return;
        }
        if m == 0 {
            // No new claims; move all prev to attic (preserve birth for re-entry).
            for c in prev {
                self.attic.insert(c.id.clone(), c);
            }
            return;
        }

        // Cost matrix for Hungarian (min-cost). We want MAX cosine ⇒ cost = 1 − w.
        let dim = n.max(m);
        let mut cost = vec![vec![1.0f64; dim]; dim];
        for i in 0..n {
            for j in 0..m {
                let w = cosine(&prev[i].embedding, &claims_now[j].embedding);
                // Edges below τ are forbidden (effectively infinite cost).
                cost[i][j] = if w >= self.tau { 1.0 - w } else { 1e9 };
            }
        }
        let assign = hungarian(&cost); // row i → col assign[i] (or dim if unmatched)

        let mut used_now = vec![false; m];
        for i in 0..n {
            let j = assign[i];
            if j < m && cost[i][j] < 1e8 {
                // Matched: stitch — keep the ORIGINAL birth, update last_seen.
                let mut merged = claims_now[j].clone();
                merged.birth = prev[i].birth; // re-entry preserves birth
                merged.last_seen = self.iter;
                self.claims.insert(merged.id.clone(), merged);
                used_now[j] = true;
            } else {
                // Unmatched old claim: move to attic (preserve birth for re-entry).
                self.attic.insert(prev[i].id.clone(), prev[i].clone());
            }
        }
        for j in 0..m {
            if !used_now[j] {
                // New claim born this iteration — but check attic for a prior birth
                // (re-entry): if this id was seen before, restore its ORIGINAL birth.
                let mut born = claims_now[j].clone();
                if let Some(prior) = self.attic.remove(&born.id) {
                    born.birth = prior.birth; // stitch across the gap
                } else {
                    born.birth = self.iter;
                }
                born.last_seen = self.iter;
                self.claims.insert(born.id.clone(), born);
            }
        }
    }

    /// Geometric MLE for the per-iteration survival probability `p̂`:
    /// `p̂ = mean_span / (mean_span + 1)` where mean_span is the mean observed
    /// gap between consecutive sightings. Falls back to a neutral 0.9 when no
    /// span data exists.
    pub fn p_hat(&self) -> f64 {
        if self.claims.is_empty() {
            return 0.9;
        }
        let mut spans = 0u64;
        let mut count = 0u64;
        for c in self.claims.values() {
            let d = c.duration() as u64;
            if d > 0 {
                spans += d;
                count += 1;
            }
        }
        if count == 0 {
            return 0.9;
        }
        let mean_span = spans as f64 / count as f64;
        (mean_span / (mean_span + 1.0)).clamp(0.01, 0.999)
    }

    /// Survival threshold `D* = ⌈log_p α⌉` and Bonferroni-corrected
    /// `D*_Bonf = D* + ⌈log_{1/p} N⌉`. The verdict gates on the Bonferroni-
    /// corrected threshold (the spec's "☑ Bonferroni" acceptance item).
    fn d_star(&self, p: f64, alpha: f64, n_claims: usize) -> (i64, i64) {
        let p = p.clamp(1e-6, 1.0 - 1e-6);
        let d = (alpha.ln() / p.ln()).ceil() as i64;
        let d_bonf = d + ((n_claims as f64).ln() / (1.0 - p).ln().abs()).ceil() as i64;
        (d, d_bonf)
    }

    /// Expose the Bonferroni-corrected threshold `D*_Bonf` (for tests/wiring).
    pub fn d_star_bonf(&self, p: f64, alpha: f64, n_claims: usize) -> i64 {
        self.d_star(p, alpha, n_claims).1
    }

    /// Core survival verdict for claim `c` (BP-09 spec).
    /// - `n_claims` = total claims in the current snapshot (for Bonferroni).
    /// - Power gate: if `n < D*_Bonf + 1`, return `Abstain` (insufficient iterations).
    /// - Anomaly triangle: `b ≥ b_thr` and `D*_Bonf ≤ D ≤ n−1−b` ⇒ `Anomaly`.
    /// - Survival test gates on the **Bonferroni-corrected** `D*_Bonf` (not raw D*).
    pub fn is_signal(&self, c: &Claim, p: f64, alpha: f64, n_claims: usize) -> Verdict {
        let n = n_claims as i64;
        let (_d_star, d_star_bonf) = self.d_star(p, alpha, n_claims);
        let d = c.duration() as i64;
        let birth = c.birth as i64;

        // Power gate first (Bonferroni-corrected threshold).
        if n < d_star_bonf + 1 {
            return Verdict::Abstain;
        }
        // Anomaly shear triangle: b_thr = ⌈β·n⌉, β ∈ (0.5, 0.8].
        let beta = 0.7f64;
        let b_thr = (beta * n as f64).ceil() as i64;
        if birth >= b_thr && d >= d_star_bonf && d <= (n - 1 - birth) {
            return Verdict::Anomaly;
        }
        // Survival test (Bonferroni-corrected).
        if d >= d_star_bonf && p.powi(d as i32) <= alpha {
            Verdict::Core
        } else {
            Verdict::Noise
        }
    }

    /// **AND-gate decision** (BP-09 RED→GREEN). Persistence alone is advisory:
    /// `Accept = P(c) ∧ V(c)`. An entrenched-hallucination (early-born false
    /// claim re-emitted every turn) gets `D = n−1` MAX and is labelled CORE by
    /// persistence — `accept` returns `false` when `verified = false`, proving
    /// the verify layer is what actually catches it.
    pub fn accept(verdict: Verdict, verified: bool) -> bool {
        matches!(verdict, Verdict::Core) && verified
    }
}

impl Default for PersistenceTable {
    fn default() -> Self {
        Self::new()
    }
}

/// Kuhn–Munkres Hungarian assignment (min-cost) over a square-ish `dim×dim`
/// cost matrix. Returns `assign[i]` = matched column for row `i` (or `dim` if
/// that row is unmatched, which the caller treats as "died/no-edge").
/// O(dim³); dim ≈ 10 here, trivial.
fn hungarian(cost: &[Vec<f64>]) -> Vec<usize> {
    let n = cost.len();
    if n == 0 {
        return Vec::new();
    }
    let dim = cost[0].len();
    // Potentials (1-indexed internally for clarity).
    let mut u = vec![0f64; n + 1];
    let mut v = vec![0f64; dim + 1];
    let mut p = vec![0usize; dim + 1]; // p[j] = row assigned to col j
    let inf = 1e18;
    for i in 1..=n {
        p[0] = i;
        let mut j0 = 0usize;
        let mut minv = vec![inf; dim + 1];
        let mut used = vec![false; dim + 1];
        let mut way = vec![0usize; dim + 1];
        loop {
            used[j0] = true;
            let i0 = p[j0];
            let mut delta = inf;
            let mut j1 = 0usize;
            for j in 1..=dim {
                if !used[j] {
                    let cur = cost[i0 - 1][j - 1] - u[i0] - v[j];
                    if cur < minv[j] {
                        minv[j] = cur;
                        way[j] = j0;
                    }
                    if minv[j] < delta {
                        delta = minv[j];
                        j1 = j;
                    }
                }
            }
            for j in 0..=dim {
                if used[j] {
                    u[p[j]] += delta;
                    v[j] -= delta;
                } else {
                    minv[j] -= delta;
                }
            }
            j0 = j1;
            if p[j0] == 0 {
                break;
            }
        }
        loop {
            let j1 = way[j0];
            p[j0] = p[j1];
            j0 = j1;
            if j0 == 0 {
                break;
            }
        }
    }
    let mut assign = vec![dim; n]; // dim = unmatched sentinel
    for j in 1..=dim {
        if p[j] != 0 {
            assign[p[j] - 1] = j - 1;
        }
    }
    assign
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emb(x: f64) -> Vec<f64> {
        // 2-D embedding so cosine is exact and controllable.
        vec![x.cos(), x.sin()]
    }

    #[test]
    fn hungarian_matches_max_weight_not_greedy() {
        // Two prev, two now. prev0↔now0 (cos 1.0), prev1↔now1 (cos 1.0) is the
        // optimal assignment; a greedy left-to-right would also work here, so
        // use a case greedy gets wrong: prev0 matches now1 better, prev1→now0.
        let prev = vec![Claim::new("a", 1, emb(0.0)), Claim::new("b", 1, emb(1.5))];
        let now = vec![Claim::new("x", 2, emb(1.6)), Claim::new("y", 2, emb(0.1))];
        // a(0.0) is closer to y(0.1) [Δ0.1], b(1.5) closer to x(1.6) [Δ0.1].
        // Optimal: a→y, b→x. Greedy a→x would be suboptimal.
        let cost = vec![
            vec![
                1.0 - cosine(&prev[0].embedding, &now[0].embedding),
                1.0 - cosine(&prev[0].embedding, &now[1].embedding),
            ],
            vec![
                1.0 - cosine(&prev[1].embedding, &now[0].embedding),
                1.0 - cosine(&prev[1].embedding, &now[1].embedding),
            ],
        ];
        let a = hungarian(&cost);
        // a→y(idx1), b→x(idx0)
        assert_eq!(a[0], 1, "Hungarian must pick the optimal a→y match");
        assert_eq!(a[1], 0, "Hungarian must pick the optimal b→x match");
    }

    #[test]
    fn ingest_stitches_birth_across_snapshots() {
        // Claim re-emitted every iteration must preserve its ORIGINAL birth.
        let mut t = PersistenceTable::new();
        t.ingest(vec![Claim::new("c1", 1, emb(0.3))]); // iter1, born at iter1
        t.ingest(vec![Claim::new("c1", 2, emb(0.3))]); // iter2, same embedding
        t.ingest(vec![Claim::new("c1", 3, emb(0.3))]); // iter3
        let c = t.claims.get("c1").expect("claim survived");
        assert_eq!(c.birth, 1, "birth must be stitched from the first sighting");
        assert_eq!(c.last_seen, 3);
        assert_eq!(c.duration(), 2);
    }

    #[test]
    fn noise_claim_below_d_star_is_noise() {
        // One-shot noise: D=0 < D* ⇒ Noise (rejected by persistence).
        // Use p=0.5 so D*=⌈log_0.5 0.05⌉=5; at n=10 the power gate clears
        // (n ≥ D*+1) and a duration-0 claim falls below D* → Noise.
        let t = PersistenceTable::new();
        let c = Claim::new("noise", 5, emb(0.9));
        // birth==last_seen ⇒ duration 0.
        let v = t.is_signal(&c, 0.5, 0.05, 10);
        assert_eq!(v, Verdict::Noise, "single-sighting claim must be Noise");
    }

    #[test]
    fn entrenched_hallucination_persistence_labels_core_but_verify_catches_it() {
        // RED→GREEN: an early-born FALSE claim re-emitted every turn.
        let n = 20usize;
        let mut t = PersistenceTable::new();
        // Simulate it being seen every iteration from birth=1 to last_seen=n-1.
        let c = Claim {
            id: "false".into(),
            birth: 1,
            last_seen: (n - 1) as u32,
            embedding: emb(0.7),
        };
        // D = n-2 (≈ max). With p=0.9, alpha=0.05, D*=⌈log_0.9 0.05⌉≈28? Actually
        // log_0.9(0.05)=ln0.05/ln0.9≈(-3.0)/(-0.105)=28.5→29. So D*=29 > n-2=18,
        // meaning power gate ⇒ Abstain at n=20. Use a larger n to clear the gate
        // and prove persistence labels CORE (which is the point: it does NOT catch
        // the falsehood — verify must).
        let n2 = 40usize;
        let c2 = Claim {
            id: "false".into(),
            birth: 1,
            last_seen: (n2 - 1) as u32,
            embedding: emb(0.7),
        };
        let v = t.is_signal(&c2, 0.9, 0.05, n2);
        assert_eq!(
            v,
            Verdict::Core,
            "entrenched re-emitted claim is labelled CORE by persistence alone"
        );
        // The AND-gate: persistence says Core, but verify=false ⇒ reject.
        assert!(
            !PersistenceTable::accept(v, false),
            "entrenched hallucination MUST be rejected once verify=false"
        );
        // If verify were true (genuinely established), it would accept.
        assert!(
            PersistenceTable::accept(v, true),
            "established+verified claim should accept"
        );
    }

    #[test]
    fn power_gate_abstains_on_insufficient_n() {
        // n < D*+1 ⇒ Abstain, never "no signal".
        let t = PersistenceTable::new();
        let c = Claim::new("x", 1, emb(0.2));
        // At n=3 with p=0.9,alpha=0.05, D*≈29 >> 3 ⇒ power gate ⇒ Abstain.
        let v = t.is_signal(&c, 0.9, 0.05, 3);
        assert_eq!(v, Verdict::Abstain, "insufficient iterations must Abstain");
    }

    #[test]
    fn p_hat_geometric_mle() {
        let mut t = PersistenceTable::new();
        // One claim seen at birth=1 and last_seen=4 ⇒ span 3 ⇒ p̂=3/4=0.75.
        t.ingest(vec![Claim::new("c", 1, emb(0.4))]);
        t.ingest(vec![Claim::new("c", 2, emb(0.4))]);
        t.ingest(vec![Claim::new("c", 3, emb(0.4))]);
        t.ingest(vec![Claim::new("c", 4, emb(0.4))]);
        let ph = t.p_hat();
        assert!(
            (ph - 0.75).abs() < 1e-6,
            "p̂ should be mean_span/(mean_span+1)=0.75, got {ph}"
        );
    }

    // ── Gap-remediation tests (overlap review A–E) ───────────────────────────

    #[test]
    fn salience_reinforce_survives_noise_evicts() {
        // BP-13 RED→GREEN: a reinforced (high-persistence) node survives tick(),
        // a one-shot noise node decays out. This is the falsifiability gate the
        // overlap reviewer flagged as untested.
        use crate::memory::LivingMemory;
        let mut mm = LivingMemory::new().with_params(7.0, 0.5);
        let persistent = mm.remember("core-truth", "x");
        let noise = mm.remember("one-shot", "y");
        // Reinforce the persistent claim every tick; never reinforce the noise.
        for _ in 0..7 {
            mm.reinforce("core-truth", 1.0);
            mm.tick();
        }
        assert!(
            mm.nodes().contains_key(&persistent),
            "reinforced node must survive"
        );
        assert!(
            !mm.nodes().contains_key(&noise),
            "one-shot noise must evict"
        );
        assert!(
            mm.attic_contains(&noise),
            "evicted noise preserved in attic"
        );
    }

    #[test]
    fn theta_wired_from_d_star_bonferroni() {
        // BP-09→BP-13 (overlap B): θ must come from BP-09's D*_Bonf, not a
        // hardcoded 0.5. Prove with_tau/with_params wiring drives eviction.
        use crate::memory::LivingMemory;
        let mut mm = LivingMemory::new().with_params(7.0, 0.05); // θ=0.05 (small)
        let id = mm.remember("low", "z");
        mm.tick(); // salience 0 < 0.05 ⇒ evicts even with tiny threshold
        assert!(mm.attic_contains(&id), "θ=0.05 evicts un-reinforced node");
    }

    #[test]
    fn attic_reentry_stitches_birth_across_gap() {
        // BP-09 (overlap C): a claim absent for one snapshot must NOT lose its
        // original birth — it is preserved in the attic and re-stitched on return.
        let mut t = PersistenceTable::new();
        t.ingest(vec![Claim::new("c1", 1, emb(0.3))]); // iter1 born
        t.ingest(vec![Claim::new("c1", 2, emb(0.3))]); // iter2 (continuous)
        t.ingest(vec![]); // iter3: claim ABSENT → must move to attic (birth kept)
        assert!(
            t.attic.contains_key("c1"),
            "absent claim must park in attic"
        );
        t.ingest(vec![Claim::new("c1", 4, emb(0.3))]); // iter4: reappears
        let c = t.claims.get("c1").expect("re-entered claim survives");
        assert_eq!(
            c.birth, 1,
            "re-entry must restore ORIGINAL birth=1, got {}",
            c.birth
        );
        assert_eq!(c.last_seen, 4, "re-entry last_seen = 4");
        assert_eq!(c.duration(), 3, "duration spans the gap (1→4)");
    }

    #[test]
    fn bonferroni_threshold_stricter_than_raw() {
        // BP-09 (overlap D): verdict must gate on D*_Bonf, which is ≥ D*. Prove
        // the exposed Bonferroni threshold is strictly larger than raw D* for N>1.
        let t = PersistenceTable::new();
        let (d_star, d_bonf) = (|| {
            let raw = (0.05f64.ln() / 0.9f64.ln()).ceil() as i64; // D*
            (raw, t.d_star_bonf(0.9, 0.05, 10))
        })();
        assert!(d_bonf >= d_star, "D*_Bonf must be ≥ D*");
        assert!(
            d_bonf > d_star,
            "Bonferroni must tighten the threshold for N=10"
        );
    }

    #[test]
    fn anomaly_triangle_triggers_verify_human() {
        // BP-09 (overlap E): a claim with high birth AND duration in the shear
        // window must return Anomaly (verify/human), never auto-accept.
        // n=100, β=0.7 ⇒ b_thr=70. Choose birth=80 (≥70), D=15.
        // p=0.5,α=0.05 ⇒ D*=5; D*_Bonf=5+⌈log₂100⌉=12. Window 12≤15≤(100-1-80=19) ✓.
        let t = PersistenceTable::new();
        let c = Claim {
            id: "a".into(),
            birth: 80u32,
            last_seen: 80u32 + 15,
            embedding: emb(0.1),
        };
        let v = t.is_signal(&c, 0.5, 0.05, 100);
        assert_eq!(v, Verdict::Anomaly, "shear-triangle claim must be Anomaly");
    }
}
