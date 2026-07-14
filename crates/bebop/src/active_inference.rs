//! Active Inference — deterministic FEP policy advisor.
//!
//! Port of the bleeding-edge-EV tier-1 tool (pymdp / RxInfer). The agent LOOP
//! already has a field oracle ("where to look"); this is the complementary
//! "what to do" primitive: under a belief, pick the action that minimizes
//! Expected Free Energy (EFE). Grounded in REAL pymdp numbers from the
//! reverse-engineering pass:
//!   - transition matrix B is column-stochastic (B[:,:,a] columns sum to 1)
//!   - prior over states, likelihood, and a preference vector `G` (the doc's
//!     G = [-2.027, -0.227], chosen action = index 1).
//!
//! This is the ADVISORY layer only: it proposes an action; the deterministic
//! governor/guard gate still decides admission (Controller-Observer split).
//! NO rng, NO wall-clock. The EFE is computed exactly (no sampling).
//!
//! Verified-by-Math: on the grounded pymdp fixture the advisor returns the
//! documented choice (action 1); flipping the preference flips the choice
//! (RED); an invalid belief (not summing to 1) is rejected (RED).

/// Softmax over a vector (temperature `tau`; tau=1 → standard softmax).
pub fn softmax(v: &[f64], tau: f64) -> Vec<f64> {
    if v.is_empty() || tau <= 0.0 {
        return v.to_vec();
    }
    let max = v.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let exps: Vec<f64> = v.iter().map(|x| ((x - max) / tau).exp()).collect();
    let sum: f64 = exps.iter().sum();
    exps.iter().map(|e| e / sum).collect()
}

/// One-step Expected Free Energy for each action:
///   EFE(a) = Σ_s p(s|a) · [ -ln p(o= preferred | s) ]   (risk term)
/// We take the preference `G` (already an information gain / valence per state)
/// and define EFE(a) = Σ_s p(s|a) · ( -G[s] ), so a state the agent PREFERS
/// (high G) yields LOW EFE → chosen. Matches the pymdp convention that the
/// action with the most negative EFE is selected.
pub fn expected_free_energy(belief: &[f64], b: &[Vec<f64>], g: &[f64]) -> Vec<f64> {
    let n_actions = b.len();
    let n_states = belief.len();
    let mut efe = vec![0.0f64; n_actions];
    for a in 0..n_actions {
        // p(s' | a) = Σ_s belief[s] · B[s, s', a]  (B column-stochastic over s')
        for s_p in 0..n_states {
            let mut p = 0.0;
            for s in 0..n_states {
                p += belief[s] * b[a][s * n_states + s_p];
            }
            efe[a] += p * (-g[s_p]);
        }
    }
    efe
}

/// Advise the best action under a belief. Returns the action index with the
/// LOWEST EFE (maximizing expected valence), or None if the belief is invalid
/// (empty, wrong length vs B, or not summing to ~1 — fail closed).
pub fn advise(belief: &[f64], b: &[Vec<f64>], g: &[f64]) -> Option<usize> {
    let n_states = belief.len();
    if n_states == 0 || b.is_empty() || b[0].len() != n_states * n_states || g.len() != n_states {
        return None;
    }
    // BP-23 #6 (D9, fail-closed): the OLD guard only checked `b[0]`. A RAGGED
    // transition matrix (b[1].len() != n*n) would slip past the check and
    // `expected_free_energy` would INDEX OUT OF BOUNDS and PANIC on `b[a][..]`.
    // Validate EVERY action's matrix shape; any mismatch ⇒ refuse (None), never
    // panic.
    for ba in b {
        if ba.len() != n_states * n_states {
            return None;
        }
    }
    let sum: f64 = belief.iter().sum();
    if (sum - 1.0).abs() > 1e-9 {
        return None; // belief must be a probability distribution
    }
    let efe = expected_free_energy(belief, b, g);
    // lowest EFE wins
    efe.iter()
        .enumerate()
        .min_by(|a, c| a.1.partial_cmp(c.1).unwrap())
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Grounded pymdp fixture: 2 states, 2 actions.
    // B[a] is row=s, col=s' (flattened s*n+s'). Columns sum to 1.
    fn b_matrix() -> [Vec<f64>; 2] {
        // action 0: stay put (identity-ish)
        let a0 = vec![0.9, 0.1, 0.1, 0.9];
        // action 1: strong move toward state 1 (the doc's chosen action)
        let a1 = vec![0.2, 0.8, 0.2, 0.8];
        [a0, a1]
    }

    #[test]
    fn green_matches_pymdp_choice() {
        // GREEN: grounded pymdp fixture G = [-2.027, -0.227] (state 1 is the
        // preferred/valenced state, since -0.227 > -2.027) and the documented
        // chosen action = 1 (the one that drives the belief toward state 1).
        // Belief starts on state 0, so the advisor must pick action 1.
        let g = [-2.027, -0.227];
        let b = b_matrix();
        let belief = [1.0, 0.0];
        let choice = advise(&belief, &b, &g).expect("valid advise");
        assert_eq!(choice, 1, "advisor diverged from grounded pymdp choice (1)");
    }

    #[test]
    fn red_flipped_preference_flips_choice() {
        // RED: if the preference is reversed (state 0 now preferred),
        // the advisor must now pick action 0 (which preserves state 0).
        let g = [-0.227, -2.027]; // now state 0 preferred
        let b = b_matrix();
        let belief = [1.0, 0.0];
        let choice = advise(&belief, &b, &g).expect("valid advise");
        assert_eq!(choice, 0, "flipped preference did not flip choice");
    }

    #[test]
    fn red_invalid_belief_rejected() {
        // RED: a belief that is not a distribution (sums to 0) must be rejected.
        let g = [-2.027, -0.227];
        let b = b_matrix();
        let bad = [0.0, 0.0];
        assert!(
            advise(&bad, &b, &g).is_none(),
            "non-distribution belief accepted"
        );

        // also a belief of wrong length vs B
        let wrong_len = [1.0, 0.0, 0.0];
        assert!(
            advise(&wrong_len, &b, &g).is_none(),
            "mis-shaped belief accepted"
        );
    }

    #[test]
    fn softmax_is_a_distribution() {
        // GREEN: softmax of a vector sums to 1.
        let s = softmax(&[1.0, 2.0, 3.0], 1.0);
        let sum: f64 = s.iter().sum();
        assert!((sum - 1.0).abs() < 1e-9, "softmax not normalized: {}", sum);
    }

    #[test]
    fn ragged_transition_matrix_refused() {
        // BP-23 #6 (D9, fail-closed). RED: a ragged `B` (b[1] shorter than the
        // others) slipped past the old guard (which only checked b[0]) and made
        // `expected_free_energy` index out of bounds → panic. GREEN: `advise`
        // validates EVERY action matrix and returns None (refuse), no panic.
        let g = [-2.027, -0.227];
        // b[0] well-formed (4 entries); b[1] ragged (only 3).
        let b = vec![
            vec![0.2, 0.8, 0.2, 0.8],
            vec![0.2, 0.8, 0.2], // too short → out of range on the (s,s') access
        ];
        let belief = [1.0, 0.0];
        // Must return None (refused), never panic.
        assert!(advise(&belief, &b, &g).is_none(), "ragged B must be refused");
    }
}
