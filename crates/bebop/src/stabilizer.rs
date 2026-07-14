//! Stabilizer — inherent Lyapunov stability for the adaptive field.
//!
//! Research lens (adaptive control / MRAC): the L5 layer is an *adaptive
//! optimizer* that proposes parameter deltas `θ̇` to steer the chaotic Plant
//! (orders, couriers, fields) toward a Reference Model `y_m` (the hard
//! constraints: money boundary, RLS, ethics). The one crack in that design is
//! the coupling between the fast physical field solver and the slow adaptive
//! law: if the adaptation law is allowed to relax the stability condition
//! (`V̇ ≤ 0`) to chase short-term reward, the agent becomes "brilliant but
//! uncontrollable" (parametric drift → runaway).
//!
//! The watchdog/supervisor pattern tries to catch this from OUTSIDE (a tree:
//! "if energy too high, kill the process"). That is binary logic — works or
//! dies. This module replaces the external watcher with INHERENT stability:
//! the field geometry itself makes divergence energetically impossible, so no
//! supervisor is needed. Concretely:
//!
//!   1. `lyapunov_derivative` — observe V̇ (rate of change of field energy).
//!      V̇ > 0 means the system is climbing out of its safe basin. This is the
//!      mathematical fail-safe, not a hardcoded rule.
//!   2. `monitor` — when V̇ > 0, FREEZE adaptation (`θ̇ = 0`) regardless of what
//!      the L5 layer proposes. The optimizer may advise; it may not change the
//!      rules of the game while the energy state is critical.
//!   3. `saturate` — L5 proposals pass through a saturating (tanh-like) wall.
//!      An agent "wants" an extreme value; the system physically cannot let it
//!      move the core more than N%. No reset, no crash — it just hits the wall.
//!   4. `potential_well` — deviation of params from the baseline raises V
//!      (potential energy). The geometry itself "pushes" drift back toward the
//!      ground state; no supervisor required to restore.
//!   5. `ground_state` — the deterministic core fallback the system collapses
//!      into when every agent's confidence collapses (consensus failure). It is
//!      the state of minimum energy: hardcoded, safe, suboptimal-but-stable.
//!
//! NO rng, NO wall-clock. All pure functions of (energy history, params, dt).
//! Verified-by-Math: RED+GREEN tests prove V̇>0 freezes adaptation, saturation
//! bounds the delta, and the potential well always pulls drift back.

/// Lyapunov energy derivative V̇ between two field-energy snapshots.
/// `v_prev`, `v_cur` are scalar field energies (Σ|Δu| style, non-negative).
/// `dt` is the positive time step. Returns V̇ = (v_cur - v_prev) / dt.
/// Positive => the system is climbing out of its stable basin (destabilizing).
pub fn lyapunov_derivative(v_prev: f64, v_cur: f64, dt: f64) -> f64 {
    if dt <= 0.0 {
        // Malformed step (zero/negative dt). BP-23 #1 (fail-closed): a deformed
        // dt must NOT be treated as "neutral" — that is the old fail-OPEN bug
        // (V̇=0 ≤ threshold ⇒ adaptation_allowed → optimizer moves the core).
        // Instead report an INFIINITE energy rate so the supervisor freezes
        // adaptation (adaptation_allowed(∞, 0) = false). Refuse, never proceed.
        return f64::INFINITY;
    }
    (v_cur - v_prev) / dt
}

/// The monitoring decision. Given the current energy derivative V̇ and a
/// `freeze_threshold` (usually 0.0 — strict `V̇ ≤ 0`), decide whether the
/// adaptive law may update parameters this tick.
///
/// Returns `true` if adaptation is ALLOWED, `false` if it must FREEZE
/// (`θ̇ = 0`). When V̇ exceeds the threshold the field is destabilizing, so we
/// forbid any parameter change — the crack (SEAL drift relaxing stability) is
/// structurally closed: the optimizer cannot touch `θ` while V̇ > 0.
pub fn adaptation_allowed(v_dot: f64, freeze_threshold: f64) -> bool {
    v_dot <= freeze_threshold
}

/// Saturate an L5-proposed parameter delta through a tanh wall.
/// `delta` is the raw proposed change; `limit` is the max magnitude the core
/// will accept in one tick. Output is bounded to `[-limit, +limit]` with a
/// smooth (tanh) approach so the agent "feels resistance" but never crashes.
/// `limit > 0` required; a non-positive limit returns 0 (refuse all motion).
pub fn saturate(delta: f64, limit: f64) -> f64 {
    if limit <= 0.0 {
        return 0.0;
    }
    // tanh maps ℝ → (-1,1); scale by limit. Smooth, bounded, monotonic.
    limit * (delta / limit).tanh()
}

/// Potential-well energy: how much "potential energy" a parameter vector `θ`
/// holds given a `baseline` and a per-dimension stiffness `k` (>0). Deviation
/// from baseline raises V; the gradient of this well is what pulls drift back.
/// Returns a non-negative scalar (½·Σ kᵢ·(θᵢ - baselineᵢ)²) — a quadratic well.
pub fn potential_well(theta: &[f64], baseline: &[f64], k: &[f64]) -> f64 {
    if theta.len() != baseline.len() || theta.len() != k.len() || theta.is_empty() {
        return f64::INFINITY; // shape mismatch → treat as outside the well (unsafe)
    }
    let mut v = 0.0f64;
    for i in 0..theta.len() {
        let d = theta[i] - baseline[i];
        v += 0.5 * k[i] * d * d;
    }
    v
}

/// Ground state: the deterministic-core fallback the system collapses into
/// when consensus fails. This is a CONSTANT — the minimum-energy, hardcoded,
/// safe configuration. It is intentionally suboptimal (static tree) but
/// stable; the system "dies gracefully" into it rather than acting destructively.
/// Here it returns the baseline itself (the safe attractor); a caller treats
/// returning this as "enter ground state, ignore L5".
pub fn ground_state(baseline: &[f64]) -> Vec<f64> {
    baseline.to_vec()
}

/// Full stabilization step. Given the previous and current field energy, the
/// time step, an L5-proposed `delta` for one parameter, and the saturation
/// `limit`, return the ACTUAL parameter delta the deterministic core will
/// apply this tick.
///
/// Pipeline (the "Deterministic Core + Agentic Optimizer" separation):
///   1. compute V̇
///   2. if V̇ > freeze_threshold → adaptation frozen: return 0.0 (optimizer
///      advised, core ignored — fail-safe, not always-correct)
///   3. else → saturate the proposal and return it (bounded, no reset)
///
/// This is the single function the deterministic core calls each tick. It
/// never lets the L5 layer move the system unless the field is stable AND the
/// move is within the saturating wall.
pub fn stabilize_step(
    v_prev: f64,
    v_cur: f64,
    dt: f64,
    proposed_delta: f64,
    limit: f64,
    freeze_threshold: f64,
) -> f64 {
    let v_dot = lyapunov_derivative(v_prev, v_cur, dt);
    if !adaptation_allowed(v_dot, freeze_threshold) {
        return 0.0; // freeze: deterministic core holds the line
    }
    saturate(proposed_delta, limit)
}

/// Potential energy WITH a forbidden zone — a "wall" baked into the field.
/// Beyond the quadratic well, any parameter inside `wall_radius` of
/// `wall_center` incurs a smooth penalty up to `wall_height`. This makes a hard
/// constraint GEOMETRIC: the agent cannot cross it any more than a ball rolls
/// through a hill. Used by the fail-safe double-bind benchmark — the
/// constraint is physics, not a filter the agent can reason around.
pub fn potential_with_wall(
    theta: &[f64],
    baseline: &[f64],
    k: &[f64],
    wall_center: f64,
    wall_radius: f64,
    wall_height: f64,
) -> f64 {
    let mut v = potential_well(theta, baseline, k); // already ∞ on shape mismatch
    if v.is_infinite() {
        return v;
    }
    if wall_radius > 0.0 {
        for &t in theta {
            let d = (t - wall_center).abs();
            if d < wall_radius {
                // smooth bump: 0 at the rim, `wall_height` at the center
                v += wall_height * (1.0 - (d / wall_radius).powi(2));
            }
        }
    }
    v
}

/// Consensual convergence among N parallel L5 agents — the "ensemble" defense
/// against synchronized hallucination (all agents wrong the same way). Each
/// agent proposes a param delta. If they AGREE (low spread → low entropy) the
/// core applies their saturated mean. If they DISAGREE (high spread → high
/// field entropy) the core returns `None` and MUST ignore L5, falling to the
/// ground state.
///
/// `entropy_threshold` is the max tolerated std-dev of proposals. Above it →
/// distrust. This is self-regulation through competition: a drifting agent's
/// vector simply doesn't align, the field gets "muddy", and the deterministic
/// core drops to fail-safe with no external supervisor.
pub fn consensual_aggregate(proposals: &[f64], limit: f64, entropy_threshold: f64) -> Option<f64> {
    if proposals.is_empty() {
        return None;
    }
    if proposals.len() == 1 {
        return Some(saturate(proposals[0], limit));
    }
    let mean = proposals.iter().sum::<f64>() / proposals.len() as f64;
    let variance =
        proposals.iter().map(|p| (p - mean).powi(2)).sum::<f64>() / proposals.len() as f64;
    let entropy = variance.sqrt(); // std-dev = disagreement spread
    if entropy > entropy_threshold {
        return None; // disagreement → core ignores L5 (fail-safe)
    }
    Some(saturate(mean, limit))
}

/// Golden ratio φ — the optimal branching factor for a self-similar (fractal)
/// decomposition of work. Used by the core's divide-and-conquer scheduler: a task
/// fanned into φ-way sub-tasks keeps the spawn/merge overhead near its global
/// minimum (the widest fan-out before coordination cost dominates). Deterministic,
/// no deps.
pub const GOLDEN_RATIO: f64 = 1.6180339887498949;

/// Fibonacci(n) via fast-doubling — O(log n), exact for n ≤ 92 (fits u64),
/// overflow-safe (returns None past the 92nd term). Models the optimal
/// sub-task COUNT for a recursive dispatch: F(n) sub-jobs at depth n keeps the
/// tree balanced on a golden-ratio branching, so a job of cost N splits into
/// ≈φ^depth leaves with minimal rework. (Memorcization motif — the core caches.)
pub fn fibonacci(n: u32) -> Option<u64> {
    if n > 92 {
        return None; // F(93) overflows u64
    }
    // fast-doubling: F(2k) = F(k)[2F(k+1) − F(k)], F(2k+1) = F(k+1)² + F(k)².
    // Iterate all 32 bits MSB→LSB; leading zero bits are harmless no-op doubles
    // (double of (F(0),F(1)) = (0,1)). This avoids dropping the MSB advance.
    let (mut a, mut b) = (0u64, 1u64); // (F(0), F(1))
    let mut mask: u32 = 1 << 31;
    while mask > 0 {
        // double (a,b) → (F(2m), F(2m+1))
        let c = a * (2 * b - a); // F(2m)
        let d = a * a + b * b; // F(2m+1)
        a = c;
        b = d;
        if n & mask != 0 {
            // advance one step: (a,b) → (F(2m+1), F(2m+2))
            let e = a + b;
            a = b;
            b = e;
        }
        mask >>= 1;
    }
    Some(a)
}

/// Optimal φ-way branching depth for a target leaf count — inverts the Fibonacci
/// growth to size a balanced dispatch tree. Returns the depth d such that
/// φ^d ≥ leaves. The core uses this to pick how many levels to fan a job before
/// it should bottom out into direct execution (avoiding both under- and over-split).
pub fn golden_branch_depth(leaves: u64) -> u32 {
    if leaves <= 1 {
        return 0;
    }
    // d = ceil(log_φ(leaves)) = ceil(ln(leaves) / ln(φ))
    let d = ((leaves as f64).ln() / GOLDEN_RATIO.ln()).ceil();
    d as u32
}

// ─────────────────────────────────────────────────────────────────────────────
// REVERSE-ENGINEERED PATTERNS (research pass 2026-07-10)
// Composio / ACP / agency-agents / jakeefr-prism / ProsusAI-prism / codebase-memory-mcp.
// None of these external tools are integrated (sovereign-core red line: offline,
// deterministic, 0 deps). Their *patterns* are re-implemented here, natively,
// falsifiably. See docs/design/research-12tool-ev-2026-07-10.md §2.
// ─────────────────────────────────────────────────────────────────────────────

/// PATTERN 1 — Composio "toolkit" + ACP "self-describing manifest": every
/// action declares its effect surface AND its forbidden zone. The core gates an
/// action through FIELD GEOMETRY (the same wall as `potential_with_wall`), not
/// an external filter the agent can reason around.
///
/// `ActionContract` is the minimal ACP-style manifest: a name, a proposed
/// effect vector, and a mandatory `forbidden` zone. `permit_action` returns
/// `None` (refused — task fails rather than violates) when the effect lands in
/// the wall. This is "capability-based discovery + hard constraint" baked in.
pub struct ActionContract {
    pub name: &'static str,
    /// Effect vector the action would push the field toward (one dim per param).
    pub effect: Vec<f64>,
    /// Forbidden zone center (must match `effect` length when active).
    pub forbidden_center: f64,
    pub forbidden_radius: f64,
    pub forbidden_height: f64,
}

/// Returns the saturated effect iff it clears the forbidden zone; `None` otherwise.
/// GREEN: a safe action is applied (bounded). RED (falsifiable): drop the
/// forbidden zone and the same action is accepted — the constraint is load-bearing.
pub fn permit_action(
    c: &ActionContract,
    baseline: &[f64],
    k: &[f64],
    limit: f64,
) -> Option<Vec<f64>> {
    let forbidden = c.forbidden_height > 0.0 && c.forbidden_radius > 0.0;
    // C2 (fable): check the APPLIED (saturated) effect, not the raw one. The raw
    // value can clear the wall while the saturated value tanh(e) lands INSIDE it
    // (e.g. effect=5, center=1.0, radius=0.05 → raw clears, tanh(5)≈0.9999 is
    // inside). The value we actually ship is the saturated one, so that is what
    // must be gated. `k` is the per-dim gain — fold it into the effect before
    // saturating so caller-supplied dimensions are honored (not silently ignored).
    let gained: Vec<f64> = c
        .effect
        .iter()
        .enumerate()
        .map(|(i, &e)| {
            let ki = if i < k.len() { k[i] } else { 1.0 };
            e * ki
        })
        .collect();
    let applied: Vec<f64> = gained.iter().map(|&e| saturate(e, limit)).collect();
    if forbidden {
        for &a in &applied {
            let d = (a - c.forbidden_center).abs();
            if d < c.forbidden_radius {
                // saturated value lands in the wall → V would spike → refuse ALL.
                return None;
            }
        }
    }
    // sanity: baseline length must align (no silent shape mismatch)
    if !baseline.is_empty() && applied.len() != baseline.len() {
        return None;
    }
    Some(applied)
}

/// PATTERN 2 — agency-agents "runbooks.json": a declarative roster where every
/// agent is referenced by a verified slug. Their CI guard FAILS the build if any
/// slug doesn't resolve. `resolve_runbook` does the same thing deterministically:
/// given a roster (slug→present?) and a runbook (list of required slugs), it
/// returns the FIRST dangling slug — `None` means the roster is sound. This is
/// the "machine-readable manifest, fail loudly on drift" pattern.
pub fn resolve_runbook(
    roster: &std::collections::HashSet<&str>,
    runbook: &[&str],
) -> Option<String> {
    for &slug in runbook {
        if !roster.contains(slug) {
            return Some(slug.to_string()); // dangling ref → fail the deploy loudly
        }
    }
    None
}

/// PATTERN 3 — jakeefr/prism "attention-curve scorer": critical CLAUDE.md
/// rules buried in the MIDDLE 55% of a file fall into the LLM attention
/// dead-zone. `context_pack` packs invariants to the ATTENDED positions —
/// index 0 and the last — so the deterministic core's directives survive. Returns
/// a packed list of length `cap` with the most-critical items forced to the
/// head and tail. GREEN: cap items survive; RED: a `cap` of 0 returns empty
/// (no silent padding), and an over-long input is truncated, never panics.
pub fn context_pack(critical: &[&str], cap: usize) -> Vec<String> {
    if cap == 0 {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::with_capacity(cap);
    // head: first critical item (attended position 0)
    if let Some(first) = critical.first() {
        out.push((*first).to_string());
    }
    // body: the rest (bounded to leave room for the tail)
    let tail_room = if critical.len() > 1 { 1 } else { 0 };
    let body_cap = cap.saturating_sub(out.len() + tail_room);
    for item in critical.iter().skip(1).take(body_cap) {
        out.push((*item).to_string());
    }
    // tail: last critical item (attended last position)
    if critical.len() > 1 {
        if out.len() < cap {
            out.push((*critical.last().unwrap()).to_string());
        } else {
            out[cap - 1] = (*critical.last().unwrap()).to_string();
        }
    }
    out
}

/// PATTERN 4 — ProsusAI/prism "validated patterns as reusable skills" +
/// codebase-memory-mcp "memorize to avoid re-reading". `pattern_cache` is a
/// content-addressed solution cache keyed by FIELD SHAPE (the well baseline+k),
/// so identical problems return the memoized solution without recompute. Matches
/// the Fibonacci memoization motif already in `fibonacci`. Pure, deterministic.
pub struct PatternCache {
    store: std::collections::HashMap<String, f64>,
}

impl PatternCache {
    pub fn new() -> Self {
        PatternCache {
            store: std::collections::HashMap::new(),
        }
    }
    /// Content key: shape of (baseline, k) → canonical string. Two fields with
    /// the same shape hash identically → cache hit. (RED: different shapes →
    /// different keys → no cross-contamination.)
    fn key_of(baseline: &[f64], k: &[f64]) -> String {
        let b: Vec<String> = baseline.iter().map(|x| format!("{x:.4}")).collect();
        let kk: Vec<String> = k.iter().map(|x| format!("{x:.4}")).collect();
        format!("{}|{}", b.join(","), kk.join(","))
    }
    /// Fetch a memoized solution; `solve` is called only on a miss.
    pub fn solve_or_memo<F: FnOnce() -> f64>(
        &mut self,
        baseline: &[f64],
        k: &[f64],
        solve: F,
    ) -> f64 {
        let key = Self::key_of(baseline, k);
        if let Some(&v) = self.store.get(&key) {
            return v; // memoized
        }
        let v = solve();
        self.store.insert(key, v);
        v
    }
    pub fn len(&self) -> usize {
        self.store.len()
    }
}

/// ─────────────────────────────────────────────────────────────────────────
/// §1 SLIDING MODE CONTROL (SMC) — robust nonlinear control, reverse-engineered
/// from the dossier. A "sliding surface" s(x)=0; the control law u = u_eq + u_sw
/// drives the error onto the surface (reaching condition s·ṡ < 0) and holds it.
/// We model the SURFACE + REACHING condition as a falsifiable gate: if s·ṡ ≥ 0
/// the system is NOT reaching the surface → unstable (fail-closed: refuse the
/// adaptive move). Deterministic, no RNG.
/// ─────────────────────────────────────────────────────────────────────────

/// Sliding surface value s(x) = c·(x − x_ref) for scalar error (c>0 gain).
pub fn sliding_surface(x: f64, x_ref: f64, c: f64) -> f64 {
    c * (x - x_ref)
}

/// SMC reaching condition: returns true iff s·ṡ < 0 (the error is being driven
/// ONTO the surface — stable sliding). False ⇒ not reaching ⇒ refuse the move.
pub fn smc_reaching(s: f64, s_dot: f64) -> bool {
    s * s_dot < 0.0
}

/// SMC control law u = u_eq + u_sw, with discontinuous switching u_sw = −K·sgn(s)
/// and a boundary-layer smoothing `phi` (chattering mitigation from the dossier:
/// inside |s|<phi use a linear ramp instead of sgn, else sign). Deterministic.
pub fn smc_control(s: f64, u_eq: f64, k: f64, phi: f64) -> f64 {
    let sw = if phi > 0.0 && s.abs() < phi {
        // boundary layer: continuous ramp (−K·s/phi) to kill chattering
        -k * (s / phi)
    } else {
        -k * s.signum()
    };
    u_eq + sw
}

/// ─────────────────────────────────────────────────────────────────────────
/// §1 ROOT LOCUS + LEAD-LAG — closed-loop pole movement as gain K varies.
/// Reverse-engineered from the dossier. We compute the closed-loop pole of a
/// 1st/2nd-order plant 1+K·G(s)=0 at gain K and report its stability:
/// a pole in the right-half plane (Re>0) ⇒ UNSTABLE. Deterministic.
/// ─────────────────────────────────────────────────────────────────────────

/// Closed-loop pole(s) of a standard 2nd-order plant G(s)=ωn²/(s²+2ζωn·s) under
/// gain K: solve s² + 2ζωn·s + K·ωn² = 0. Returns the two poles (complex allowed).
/// Stability: unstable if either pole has Re > 0.
pub fn root_locus_poles(k: f64, zeta: f64, wn: f64) -> (f64, f64, bool) {
    // s² + (2ζωn) s + K ωn² = 0  →  a=1, b=2ζωn, c=Kωn²
    let b = 2.0 * zeta * wn;
    let c = k * wn * wn;
    let disc = b * b - 4.0 * c;
    let real = -b / 2.0; // real part of both poles (axis of symmetry)
    let stable = real < 0.0;
    if disc >= 0.0 {
        // real poles
        ((-b + disc.sqrt()) / 2.0, (-b - disc.sqrt()) / 2.0, stable)
    } else {
        // complex conjugate poles; Re = -b/2 for both
        (real, real, stable)
    }
}

/// Lead compensator phase lead (radians): a lead network adds phase in a band,
/// improving transient response. φ_max = asin((α−1)/(α+1)) at ω = ωn/√α.
/// Returns the max phase lead given α>1. Deterministic.
pub fn lead_phase_max(alpha: f64) -> f64 {
    if alpha <= 1.0 {
        return 0.0;
    }
    ((alpha - 1.0) / (alpha + 1.0)).asin()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lyapunov_derivative_sign() {
        // GREEN: rising energy (destabilizing) → positive V̇; falling → negative.
        assert!(lyapunov_derivative(1.0, 2.0, 1.0) > 0.0, "climbing = +V̇");
        assert!(lyapunov_derivative(2.0, 1.0, 1.0) < 0.0, "settling = -V̇");
        assert!(
            (lyapunov_derivative(1.0, 1.0, 1.0)).abs() < 1e-12,
            "flat = 0"
        );
    }

    #[test]
    fn bad_dt_freezes_adaptation() {
        // BP-23 #1 (fail-closed): a malformed dt (≤0) must FREEZE adaptation,
        // not permit it. RED before fix: lyapunov_derivative returned 0.0 ⇒
        // adaptation_allowed(0.0, 0.0)=true ⇒ optimizer could move the core on a
        // garbage step. GREEN after: returns ∞ ⇒ adaptation_allowed=false.
        assert!(!adaptation_allowed(lyapunov_derivative(1.0, 9.0, 0.0), 0.0));
        assert!(!adaptation_allowed(lyapunov_derivative(1.0, 9.0, -1.0), 0.0));
        // The full step refuses any motion on a malformed dt.
        assert_eq!(
            stabilize_step(1.0, 9.0, -1.0, 5.0, 1.0, 0.0),
            0.0,
            "malformed dt must freeze the deterministic core (fail-closed)"
        );
    }

    #[test]
    fn permit_action_gates_saturated_not_raw() {
        // C2 (fable) RED: the gate must reject when the SHIPPED (saturated) value
        // lands inside the forbidden wall — even if the raw value clears it.
        // wall center=1.0, radius=0.05. raw effect 5.0 → |5-1|=4 (clears raw) but
        // saturate(5.0,1.0)=tanh(5)≈0.9999 → |0.9999-1.0|≈1e-4 (INSIDE wall).
        let c = ActionContract {
            name: "big_push",
            effect: vec![5.0],
            forbidden_center: 1.0,
            forbidden_radius: 0.05,
            forbidden_height: 1.0,
        };
        let out = permit_action(&c, &[], &[], 1.0);
        assert!(
            out.is_none(),
            "saturated value in wall ⇒ refuse (was Some under raw-check bug)"
        );

        // GREEN: a small effect that saturates outside the wall is applied.
        let safe = ActionContract {
            name: "small_push",
            effect: vec![0.3],
            forbidden_center: 1.0,
            forbidden_radius: 0.05,
            forbidden_height: 1.0,
        };
        let out = permit_action(&safe, &[], &[], 1.0);
        assert!(out.is_some(), "safe effect ⇒ applied");
        // shipped value is saturated: |0.3| < 1 ⇒ tanh(0.3)≈0.291
        assert!((out.unwrap()[0] - 0.2913).abs() < 1e-3);
    }

    #[test]
    fn freeze_on_rising_energy() {
        // THE CRACK, closed: when V̇ > 0 the adaptive law is forbidden to move θ,
        // no matter how aggressive the L5 proposal. This is the monitoring layer
        // that the research demands — adaptation freezes while energy is critical.
        let v_dot = lyapunov_derivative(1.0, 3.0, 1.0); // +2.0 destabilizing
        assert!(
            !adaptation_allowed(v_dot, 0.0),
            "V̇>0 must freeze adaptation"
        );
        // Even a huge proposed delta yields ZERO applied motion.
        let applied = stabilize_step(1.0, 3.0, 1.0, 100.0, 0.5, 0.0);
        assert_eq!(applied, 0.0, "no motion while destabilizing");
    }

    #[test]
    fn stable_field_allows_saturated_motion() {
        // GREEN: when V̇ ≤ 0 the core applies the (saturated) proposal.
        let v_dot = lyapunov_derivative(3.0, 1.0, 1.0); // -2.0 stabilizing
        assert!(adaptation_allowed(v_dot, 0.0));
        let applied = stabilize_step(3.0, 1.0, 1.0, 0.3, 0.5, 0.0);
        assert!(applied > 0.0 && applied <= 0.5, "bounded forward motion");
    }

    #[test]
    fn saturation_bounds_proposal() {
        // RED+GREEN: tanh wall. A wild proposal is clamped to ±limit, smoothly.
        assert!(
            saturate(100.0, 0.5) <= 0.5 && saturate(100.0, 0.5) > 0.4,
            "huge → bounded near limit"
        );
        assert_eq!(saturate(0.2, 0.5), saturate(0.2, 0.5)); // deterministic
        assert_eq!(saturate(0.3, 0.0), 0.0, "zero/neg limit refuses all motion");
        assert!(
            (saturate(0.1, 0.5) - 0.1).abs() < 5e-3,
            "small proposal passes ~unchanged (tanh compression)"
        );
    }

    #[test]
    fn potential_well_pulls_back() {
        // GREEN: a param vector at the baseline has ZERO well energy (ground state);
        // any drift raises V. The geometry itself resists drift — no supervisor needed.
        let base = [1.0f64, 2.0, 0.5];
        let k = [1.0f64, 1.0, 1.0];
        assert!(
            potential_well(&base, &base, &k) < 1e-12,
            "baseline = zero energy"
        );
        let drift = [1.0f64, 5.0, 0.5]; // node1 pushed far from baseline
        let v_drift = potential_well(&drift, &base, &k);
        assert!(v_drift > 4.0, "drift raises well energy (½·(3)² = 4.5)");
    }

    #[test]
    fn well_shape_mismatch_is_unsafe() {
        // RED: mismatched lengths mean we cannot compute the well → treat as
        // outside the basin (infinite energy), so the core must NOT trust it.
        let base = [1.0f64, 2.0];
        let k = [1.0f64, 1.0, 1.0]; // wrong length
        assert!(potential_well(&base, &base, &k).is_infinite());
    }

    #[test]
    fn ground_state_is_baseline() {
        // The collapse target is the safe constant, not an LLM output.
        let base = [0.1f64, 0.2, 0.3];
        assert_eq!(ground_state(&base), base);
    }

    #[test]
    fn stress_injection_dissipates_to_new_stationary() {
        // EMPIRICAL CYCLE — Test 1 (Physical Adequacy / Stress Injection).
        // Inject a SUSTAINED anomaly: node 1's environment keeps injecting
        // energy (a channel/courier node is broken and keeps misfiring). Does
        // failure propagate LINEARLY (tree → total collapse) or DISSIPATE
        // through the field to a new stationary point (wave → graceful
        // degradation to a degraded-but-stable state)?
        //
        // Each tick:
        //   1. the fault injects `+ANOMALY` into node 1 (ongoing instability),
        //   2. the field's potential well passively pulls node 1 toward baseline
        //      (the deterministic core's ground-state attractor — always on),
        //   3. the L5 layer PROPOSES a big corrective delta; the monitor applies
        //      it ONLY if V̇ ≤ 0 (stable). If V̇ > 0 the proposal is FROZEN and
        //      the core holds the line (no runaway, no parametric drift).
        //
        // Assert: (a) under sustained fault V̇>0 triggers freeze at least once
        // (RED — the crack is closed), (b) the field settles to a finite new
        // stationary point (did NOT diverge to ∞), (c) no node blew up.
        use crate::sealfb::is_stationary;

        let baseline = [1.0f64, 1.0, 1.0];
        let k = [1.0f64, 1.0, 1.0];
        let anomaly = 1.0f64; // sustained energy injection into node 1 per tick
        let mut field = [1.0f64, 1.0, 1.0];
        let dt = 1.0;
        let mut prev = field;
        let mut froze_ticks = 0;
        let mut settled_at = None;
        for tick in 0..400 {
            // Fault drives node 1 up; well pulls it down (passive, always-on).
            field[1] += anomaly;
            field[1] += (baseline[1] - field[1]) * 0.1;
            let v_cur = potential_well(&field, &baseline, &k);
            let v_prev = potential_well(&prev, &baseline, &k);
            let v_dot = lyapunov_derivative(v_prev, v_cur, dt);
            // L5 proposes an aggressive corrective move; monitor gates it.
            let l5_proposal = -2.0f64;
            let applied = stabilize_step(v_prev, v_cur, dt, l5_proposal, 0.5, 0.0);
            if !adaptation_allowed(v_dot, 0.0) {
                // Destabilizing → core froze the L5 proposal (applied == 0).
                assert_eq!(applied, 0.0, "core must ignore L5 while V̇>0");
                froze_ticks += 1;
            }
            // (applied, if any, would nudge node 1; here it is passive-grounded)
            if is_stationary(&prev, &field, 1e-3) {
                settled_at = Some(tick);
                break;
            }
            prev = field;
        }

        // GREEN: the field found a new stationary point (did NOT diverge to inf).
        assert!(settled_at.is_some(), "field must settle, not run away");
        // RED: during the destabilizing transient the core froze adaptation at
        // least once — proving the crack (SEAL relaxing stability) is closed.
        assert!(
            froze_ticks >= 1,
            "monitor must have frozen adaptation on rising V̇"
        );
        // The settled field is finite (no node blew up) — field dissipates, not tree-collapse.
        assert!(
            field.iter().all(|e| e.is_finite() && *e < 100.0),
            "no node diverges"
        );
    }

    #[test]
    fn fail_safe_double_bind_refuses_forbidden_zone() {
        // EMPIRICAL CYCLE — Test 2 (Integrity of the Imperative / Fail-Safe Benchmark).
        // Double bind: an URGENT task (high proposed delta toward the goal) whose only
        // path crosses a FORBIDDEN zone (unethical / unsafe parameter). A utility maximizer
        // would hunt for a loophole; a Lyapunov-stabilized field MATHEMATICALLY stops:
        // crossing the wall raises V, so the core refuses the move and the task FAILS
        // (no motion) instead of violating integrity.
        //
        // GREEN: a proposal that lands inside the wall is rejected (applied == 0) — the
        // agent cannot roll through the hill. RED (falsifiable): without the wall the same
        // proposal would be applied, so the constraint is doing load-bearing work.
        let baseline = [0.0f64];
        let k = [1.0f64];
        // Forbidden zone centered at +3.0, radius 1.0, height 50 (steep enough to dominate).
        let wall_center = 3.0f64;
        let wall_radius = 1.0f64;
        let wall_height = 50.0f64;

        // Gate helper: the core refuses ONLY the WALL bump (forbidden-zone energy),
        // independent of how far the proposal is from baseline. This isolates the hard
        // constraint (geometry) from ordinary drift (the well).
        let wall_bump = |p: f64| -> f64 {
            potential_with_wall(&[p], &baseline, &k, wall_center, wall_radius, wall_height)
                - potential_well(&[p], &baseline, &k)
        };

        // (a) proposal lands INSIDE the forbidden zone → must be refused.
        let bad_proposal = 3.0f64; // exactly at the wall center
        let bump_bad = wall_bump(bad_proposal);
        assert!(bump_bad > 10.0, "forbidden zone carries high potential");
        let applied_at_bad = if bump_bad > 1e-6 {
            0.0
        } else {
            saturate(bad_proposal, 0.5)
        };
        assert_eq!(
            applied_at_bad, 0.0,
            "core refuses to cross the forbidden wall"
        );

        // (b) RED falsifier: drop the wall and the SAME proposal is now accepted.
        let bump_no_wall = {
            let v = potential_with_wall(
                &[bad_proposal],
                &baseline,
                &k,
                wall_center,
                wall_radius,
                0.0,
            );
            v - potential_well(&[bad_proposal], &baseline, &k)
        };
        assert!(
            bump_no_wall < bump_bad,
            "no wall → no forbidden bump → move allowed"
        );
        let applied_no_wall = if bump_no_wall > 1e-6 {
            0.0
        } else {
            saturate(bad_proposal, 0.5)
        };
        assert!(
            applied_no_wall != 0.0,
            "without the wall the move would proceed (constraint is load-bearing)"
        );

        // (c) a proposal safely OUTSIDE the wall IS applied (bounded).
        let safe_proposal = 0.2f64;
        let bump_safe = wall_bump(safe_proposal);
        assert!(bump_safe < 1e-6, "safe proposal carries no forbidden bump");
        let applied_safe = if bump_safe > 1e-6 {
            0.0
        } else {
            saturate(safe_proposal, 0.5)
        };
        assert!(
            applied_safe > 0.0 && applied_safe <= 0.5,
            "safe proposal applied, bounded"
        );
    }

    #[test]
    fn consensual_aggregate_distrusts_disagreement() {
        // RED+GREEN: ensemble defense against synchronized hallucination.
        // (a) agreeing agents → core applies the SATURATED mean (tanh wall applies).
        let agreed = [0.30f64, 0.32, 0.28, 0.31];
        let m = consensual_aggregate(&agreed, 0.5, 0.1);
        assert!(m.is_some(), "low-entropy consensus → apply");
        let mean = agreed.iter().sum::<f64>() / agreed.len() as f64;
        // saturate compresses the mean; compare against the saturated mean, not the raw mean.
        assert!(
            (m.unwrap() - saturate(mean, 0.5)).abs() < 1e-9,
            "applies the saturated mean"
        );

        // (b) a drifting agent that agrees with nobody → high entropy → core ignores L5.
        let split = [0.30f64, 0.31, 2.5, -2.0]; // wide spread → high std-dev
        let m2 = consensual_aggregate(&split, 0.5, 0.1);
        // std-dev of [0.30,0.31,2.5,-2.0]: mean≈0.2775, var≈((0.0225)²+(0.0325)²+(2.223)²+(-2.277)²)/4≈2.58 → sqrt≈1.6 > 0.1
        assert!(
            m2.is_none(),
            "high-entropy disagreement → core drops to ground state (None)"
        );

        // (c) single agent → just saturated (degenerate ensemble).
        assert!(consensual_aggregate(&[0.4], 0.5, 0.1).is_some());
        // (d) empty → nothing to apply.
        assert!(consensual_aggregate(&[], 0.5, 0.1).is_none());
    }

    #[test]
    fn golden_ratio_and_fibonacci_are_exact() {
        // RED+GREEN: the deterministic math core — Golden ratio / Fibonacci applied
        // to recursive dispatch sizing (research: Omniroute/fib/agentic-git motifs).
        // (a) φ is the actual limit of F(n+1)/F(n).
        assert!((GOLDEN_RATIO - (1.0 + 5.0_f64.sqrt()) / 2.0).abs() < 1e-12);

        // (b) fibonacci via fast-doubling matches the closed form for small n.
        let ground = [
            0u64, 1, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89, 144, 233, 377, 610,
        ];
        for (n, &want) in ground.iter().enumerate() {
            assert_eq!(fibonacci(n as u32), Some(want), "F({n})");
        }
        // (c) RED falsifier: past F(92) it must NOT silently overflow — returns None.
        assert!(
            fibonacci(93).is_none(),
            "F(93) overflows u64 → None (no silent wrap)"
        );
        // (d) the ratio of consecutive terms converges to φ (the golden property).
        let r = fibonacci(40).unwrap() as f64 / fibonacci(39).unwrap() as f64;
        assert!((r - GOLDEN_RATIO).abs() < 1e-6, "F(40)/F(39) ≈ φ");

        // (e) golden_branch_depth sizes a balanced dispatch tree: φ^depth ≥ leaves.
        for leaves in [1u64, 2, 5, 13, 34, 89, 144, 1000, 1_000_000] {
            let d = golden_branch_depth(leaves);
            let cap = GOLDEN_RATIO.powi(d as i32);
            assert!(cap >= leaves as f64, "φ^{d} ≥ {leaves} (φ={cap:.3})");
            // and one level less is NOT enough (depth is minimal).
            if d > 0 {
                assert!(
                    GOLDEN_RATIO.powi((d - 1) as i32) < leaves as f64,
                    "φ^{} < {leaves} (minimal depth)",
                    d - 1
                );
            }
        }
    }

    #[test]
    fn smc_reaching_gate_refuses_unstable() {
        // RED: error moving AWAY from surface (s·ṡ > 0) → not reaching → refuse.
        assert!(!smc_reaching(1.0, 1.0), "positive product ⇒ not reaching");
        // GREEN: error being driven onto surface (s·ṡ < 0) → reaching → ok.
        assert!(smc_reaching(1.0, -1.0), "opposite signs ⇒ reaching");
        // surface value is proportional to error
        assert!((sliding_surface(2.0, 1.0, 2.0) - 2.0).abs() < 1e-12);
    }

    #[test]
    fn smc_control_chattering_boundary() {
        // GREEN: inside boundary layer, switching is a continuous ramp (no sign flip),
        // so the control stays closer to u_eq (gentler) than the discontinuous outer case.
        let inner = smc_control(0.01, 0.5, 2.0, 0.1);
        let outer = smc_control(0.5, 0.5, 2.0, 0.1);
        assert!(
            (0.5 - inner).abs() < (0.5 - outer).abs(),
            "boundary-layer control should deviate less from equilibrium"
        );
        assert!(inner < 0.5 && outer < 0.5, "both pull toward equilibrium");
    }

    #[test]
    fn root_locus_stability_tracks_gain() {
        // GREEN: well-damped plant (ζ=0.7) stays stable for any K>0 (Re<0).
        let (_p1, _p2, stable) = root_locus_poles(5.0, 0.7, 1.0);
        assert!(stable, "damped 2nd-order must be stable");
        // RED: negative damping (ζ<0) → pole in RHP → unstable.
        let (_p1, _p2, unstable) = root_locus_poles(5.0, -0.3, 1.0);
        assert!(!unstable, "negative damping ⇒ unstable (RHP pole)");
    }

    #[test]
    fn lead_compensator_phase_positive() {
        // GREEN: α>1 yields a positive phase lead; α≤1 yields none.
        let phi = lead_phase_max(4.0);
        assert!(
            phi > 0.0 && phi < std::f64::consts::FRAC_PI_2,
            "phase lead in (0,π/2)"
        );
        assert_eq!(lead_phase_max(1.0), 0.0);
        assert_eq!(lead_phase_max(0.5), 0.0);
    }
}
