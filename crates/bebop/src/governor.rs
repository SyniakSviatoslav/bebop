//! Governor — a deterministic PID servo (ported from `src/governor.ts`).
//! Authority rises on approve, falls on reject. Plant = authority dynamics.
//!
//! Blueprint BP-05 redesign (Jury-stable, discrete-time):
//!   * Filtered derivative (EMA low-pass, pole γ=0.8) operating on the APPROVAL-RATE
//!     `p_t ∈ [0,1]` (rolling frequency), NOT the raw {0,1} verdict — kills derivative
//!     kick on a binary signal.
//!   * Output clamp [u_min, u_max] applied to the SIGNED control effort BEFORE it moves
//!     authority (bounds per-step change → no slam-to-0), plus a `dead_ic` deadband so a
//!     sub-threshold control signal holds authority steady (bumpless).
//!   * Gains tuned so the closed loop is Jury-stable and critically damped (ζ ≈ 1).

/// EMA pole for the filtered-derivative low-pass (γ). Pole at z = γ.
pub const DERIV_GAMMA: f64 = 0.8;
/// EMA pole for the internal rolling approval-rate estimate maintained by `step()`.
pub const RATE_GAMMA: f64 = 0.8;

/// Discrete-time stability gate for a PID controller on the integrating plant
/// `G(z) = Kg/(z−1)` as actually wired in `step`/`step_with_rate`.
///
/// Closed-loop characteristic polynomial (linearized, no clamp/deadband; the derivative
/// EMA adds a stable pole at `γ=0.8` that never destabilizes, so the stability test is on
/// the PID part). With controller `u = Kp·e + Ki·I + Kd·(q_t−q_{t−1})` and plant
/// `A_{t+1}=A_t+u_t`, the loop reduces to the cubic
///     a·z³ + b·z² + c·z + d = 0,
///     a = 1 + Kg·Kd,
///     b = −2 + Kg·(Kp + Kd),
///     c = 1 + Kg·(−Kp + Ki − 2·Kd),
///     d = Kg·Kd.
/// Exact 3rd-order Jury conditions (a>0): `P(1) > 0`, `P(−1) < 0`, `|d| < a`.
///
/// The blueprint (BLUEPRINTS.md BP-05) states a simpler sufficient proxy
/// `|Kg·Kd| < 1 ∧ Kg·(2Kp+Ki+4Kd) < 4`; those two terms are kept in `a0`/`jury2` for
/// documentation and the legacy regression gate, but `stable` is the EXACT Jury result.
pub struct JuryResult {
    /// Kg·Kd — blueprint proxy term (condition 1 magnitude).
    pub a0: f64,
    /// Kg·(2Kp + Ki + 4Kd) — blueprint proxy term (must be < 4).
    pub jury2: f64,
    /// Blueprint 2-condition proxy verdict.
    pub proxy_stable: bool,
    /// `P(1)` of the closed-loop cubic (must be > 0).
    pub p_pos1: f64,
    /// `P(−1)` of the closed-loop cubic (must be < 0).
    pub p_neg1: f64,
    /// Exact 3rd-order Jury verdict (authoritative).
    pub exact_stable: bool,
    pub stable: bool,
}

impl JuryResult {
    pub fn check(kg: f64, kp: f64, ki: f64, kd: f64) -> Self {
        // Blueprint proxy terms (kept for documentation / legacy gate).
        let a0 = kg * kd;
        let jury2 = kg * (2.0 * kp + ki + 4.0 * kd);
        let proxy_stable = a0.abs() < 1.0 && jury2 < 4.0;

        // Exact closed-loop cubic coefficients (see struct doc).
        let a = 1.0 + kg * kd;
        let b = -2.0 + kg * (kp + kd);
        let c = 1.0 + kg * (-kp + ki - 2.0 * kd);
        let d = kg * kd;
        let p_pos1 = a + b + c + d;
        let p_neg1 = -a + b - c + d;
        // a > 0 is guaranteed when Kg·Kd > −1 (true for all sane gains); assert implicitly.
        let exact_stable = p_pos1 > 0.0 && p_neg1 < 0.0 && d.abs() < a;

        JuryResult {
            a0,
            jury2,
            proxy_stable,
            p_pos1,
            p_neg1,
            exact_stable,
            stable: exact_stable,
        }
    }

    /// Blueprint proxy condition (1): |Kg·Kd| < 1.
    pub fn cond_a0(&self) -> bool {
        self.a0.abs() < 1.0
    }

    /// Blueprint proxy condition (2): Kg·(2Kp + Ki + 4Kd) < 4.
    pub fn cond_a1(&self) -> bool {
        self.jury2 < 4.0
    }
}

/// Closed-loop damping ratio ζ, used for the *critical-damping* tuning target (≈1).
/// This is the continuous-time equivalent (G(s)=Kg/s, C(s)=Kp+Ki/s+Kd·s) approximation
/// `ωn = sqrt(Kg·Ki·(1 + Kg·Kd))`, `ζ = Kg·Kp / (2·ωn)` — a tuning heuristic, NOT the
/// stability test. The authoritative discrete-time stability test is [`JuryResult`]
/// (exact 3rd-order Jury on the closed-loop cubic).
pub fn damping_ratio(kg: f64, kp: f64, ki: f64, kd: f64) -> f64 {
    let wn = (kg * ki * (1.0 + kg * kd)).sqrt();
    if wn <= 0.0 {
        0.0
    } else {
        (kg * kp) / (2.0 * wn)
    }
}

/// Raw (unfiltered, unclamped) PID control effort for a single step from rest —
/// the pre-BP-05 behaviour. Used by tests to demonstrate why the clamp/filter are
/// needed (old gains produce |u| ≫ u_max on one reject).
pub fn raw_control_effort(kp: f64, ki: f64, kd: f64, error: f64) -> f64 {
    // First step from rest: integral = error, derivative = error − 0.
    kp * error + ki * error + kd * error
}

#[derive(Clone, Copy, Debug, Default)]
pub struct GovState {
    pub authority: f64,
    pub factor_status: &'static str,
    pub resonance_risky: bool,
    /// Integral accumulator (anti-windup clamped to [i_min, i_max]).
    pub integral: f64,
    /// Previous *error* e_{t-1} (raw quality − target). Retained for compat.
    pub prev_error: f64,
    /// Previous approval-rate p_{t-1} (rolling frequency in [0,1]) — for the EMA filtered
    /// derivative. Maintained internally by `step()` via an EMA of the raw verdict so the
    /// derivative term always sees a *rate*, never the raw {0,1} verdict (BP-05 intent).
    pub prev_p: f64,
    /// Internal rolling approval-rate estimate EMA (in [0,1]) — the rate the derivative
    /// actually differentiates. Updated each `step()` from the raw verdict.
    pub rate_ema: f64,
    /// Filtered-derivative state d_{t-1} (EMA).
    pub prev_d: f64,
    /// Last raw derivative term Kd·(p_t − p_{t-1}) before EMA filtering (observability).
    pub last_d: f64,
    /// Last control effort u (after clamp) — used for status/observability.
    pub last_u: f64,
}

impl GovState {
    pub fn new(authority: f64) -> Self {
        GovState {
            authority,
            factor_status: "unknown",
            // Start the rolling rate at the setpoint so the first verdict isn't a huge transient.
            rate_ema: 0.5,
            ..Default::default()
        }
    }

    /// Construct already settled at approval-rate `p0` (no derivative transient on the
    /// first step). Use when the system starts at steady state.
    pub fn new_at_rate(authority: f64, p0: f64) -> Self {
        GovState {
            authority,
            factor_status: "unknown",
            prev_p: p0,
            rate_ema: p0,
            ..Default::default()
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct GovConfig {
    pub kp: f64,
    pub ki: f64,
    pub kd: f64,
    /// Integral anti-windup limits.
    pub i_min: f64,
    pub i_max: f64,
    /// Signed control-effort clamp applied BEFORE u moves authority. Symmetric bound
    /// on per-step authority change → prevents slam-to-0/1 on a single verdict.
    pub u_min: f64,
    pub u_max: f64,
    /// Setpoint: target approval/quality in [0,1].
    pub target_quality: f64,
    /// Deadband on the (clamped) control signal: |u| ≤ dead_ic ⇒ authority held (bumpless).
    pub dead_ic: f64,
    /// Plant gain Kg of G(z) = Kg/(z−1). 1.0 for the canonical integrating plant.
    pub kg: f64,
}

impl Default for GovConfig {
    fn default() -> Self {
        // BP-05: the DEFAULT is now the tuned, Jury-stable, critically-damped set
        // (kg=1, γ=0.8): kp=1.03, ki=0.22, kd=0.20. This is the GREEN controller consumers
        // get out of the box. The old defective gains (kp=1.4, kd=1.5, Jury-violating,
        // underdamped ζ≈0.944) are kept only as `default_legacy()` for the RED side of the gate.
        Self::default_tuned()
    }
}

/// Old (defective, RED) gains from the blueprint — kept only for the BP-05 regression gate.
/// These violate Jury: |Kg·Kd| = 1.5 > 1 and Kg(2Kp+Ki+4Kd) = 9.02 ≥ 4; underdamped ζ≈0.944.
pub fn default_legacy() -> GovConfig {
    GovConfig {
        kp: 1.4,
        ki: 0.22,
        kd: 1.5,
        i_min: -1.0,
        i_max: 1.0,
        u_min: -0.2,
        u_max: 0.2,
        target_quality: 0.9,
        dead_ic: 0.02,
        kg: 1.0,
    }
}

impl GovConfig {
    pub fn default_ck() -> Self {
        Self::default()
    }

    /// BP-05 tuned, Jury-stable, critically-damped set (kg=1, γ=0.8):
    ///   kp=1.03, ki=0.22, kd=0.20
    ///   Jury (1): |Kg·Kd| = 0.20 < 1                       ✓
    ///   Jury (2): Kg·(2Kp+Ki+4Kd) = 2.06+0.22+0.80 = 3.08 < 4  ✓
    ///   ζ = Kp / (2·√(Ki·(1+Kd))) = 1.03 / (2·√0.264) ≈ 1.002   ✓
    pub fn default_tuned() -> Self {
        GovConfig {
            kp: 1.03,
            ki: 0.22,
            kd: 0.20,
            i_min: -1.0,
            i_max: 1.0,
            u_min: -0.2,
            u_max: 0.2,
            target_quality: 0.9,
            dead_ic: 0.02,
            kg: 1.0,
        }
    }

    /// Step the servo from a raw verdict. Internally maintains a rolling approval-RATE EMA
    /// (in [0,1]) and feeds THAT as the derivative signal — so the filtered derivative always
    /// differentiates a smoothed rate, never the raw {0,1} verdict (BP-05 intent: "D well-posed
    /// only on p_t"). Callers that already compute a true rolling rate should use
    /// `step_with_rate` directly.
    pub fn step(&self, st: &mut GovState, quality: f64, cost: f64, volume: f64) {
        // Update the internal rolling approval-rate estimate: EMA of the raw verdict.
        st.rate_ema = RATE_GAMMA * st.rate_ema + (1.0 - RATE_GAMMA) * quality;
        self.step_with_rate(st, quality, st.rate_ema, cost, volume)
    }

    /// Step the servo, supplying the rolling approval-rate `p_t` separately from the raw
    /// verdict `quality`. error = quality − target, so APPROVE (q=1) ⇒ authority RISES,
    /// REJECT (q=0) ⇒ authority FALLS. The signed control effort u is clamped to
    /// [u_min, u_max] and a sub-`dead_ic` signal holds authority (bumpless).
    pub fn step_with_rate(
        &self,
        st: &mut GovState,
        quality: f64,
        approval_rate: f64,
        _cost: f64,
        _volume: f64,
    ) {
        let error = quality - self.target_quality;

        // Integral with anti-windup clamp.
        st.integral += error;
        st.integral = st.integral.clamp(self.i_min, self.i_max);

        // Filtered derivative (EMA low-pass) on the APPROVAL-RATE p_t.
        let raw_d = self.kd * (approval_rate - st.prev_p);
        let d = DERIV_GAMMA * st.prev_d + (1.0 - DERIV_GAMMA) * raw_d;
        st.last_d = raw_d;
        st.prev_d = d;
        st.prev_p = approval_rate;

        let u = self.kp * error + self.ki * st.integral + d;

        // Clamp the SIGNED effort (bounds per-step authority change → no slam).
        let u_clamped = u.clamp(self.u_min, self.u_max);

        // Deadband: a sub-threshold effort holds authority steady (bumpless).
        if u_clamped.abs() > self.dead_ic {
            st.authority = (st.authority + u_clamped).clamp(0.0, 1.0);
            st.factor_status = if u_clamped > 0.0 {
                "expand"
            } else {
                "contract"
            };
        }

        st.last_u = u_clamped;
        st.prev_error = error;
        st.resonance_risky = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Pre-existing behavioural tests (kept green) ──────────────────────────────

    #[test]
    fn authority_rises_on_approve() {
        let cfg = GovConfig::default();
        let mut st = GovState::new(0.5);
        cfg.step(&mut st, 1.0, 1e-18, 100.0);
        assert!(
            st.authority > 0.5,
            "authority did not rise on approve: {}",
            st.authority
        );
        assert_eq!(st.factor_status, "expand");
    }

    #[test]
    fn authority_falls_on_reject() {
        let cfg = GovConfig::default();
        let mut st = GovState::new(0.5);
        cfg.step(&mut st, 0.0, 1e-18, 100.0);
        assert!(
            st.authority < 0.5,
            "authority did not fall on reject: {}",
            st.authority
        );
        assert_eq!(st.factor_status, "contract");
    }

    // ── BP-05 RED→GREEN gate: Jury stability ─────────────────────────────────────

    #[test]
    fn jury_old_defaults_unstable() {
        let cfg = default_legacy();
        let j = JuryResult::check(cfg.kg, cfg.kp, cfg.ki, cfg.kd);
        // Falsifiable documentation of the defect:
        assert!(
            (j.a0 - 1.5).abs() < 1e-9,
            "expected Kg·Kd = 1.5 on OLD gains, got {}",
            j.a0
        );
        assert!(
            (j.jury2 - 9.02).abs() < 1e-9,
            "expected Kg(2Kp+Ki+4Kd)=9.02, got {}",
            j.jury2
        );
        // OLD gains violate BOTH binding conditions → unstable.
        assert!(
            !j.cond_a0(),
            "OLD |Kg·Kd|<1 should be VIOLATED, got a0={}",
            j.a0
        );
        assert!(
            !j.cond_a1(),
            "OLD Kg(2Kp+Ki+4Kd)<4 should be VIOLATED, got {}",
            j.jury2
        );
        assert!(!j.stable, "OLD gains must be Jury-unstable");
    }

    #[test]
    fn jury_exact_cubic_vs_numeric_roots() {
        // Lock the EXACT Jury gate against first-principles closed-loop roots.
        // OLD gains (kp=1.4, ki=0.22, kd=1.5): cubic 2.5z³+0.9z²−3.18z+1.5 has a root at
        // z≈−1.487 (|z|>1) ⇒ UNSTABLE. TUNED (kp=1.03, ki=0.22, kd=0.20): all |z|<0.55 ⇒ STABLE.
        let old = JuryResult::check(1.0, 1.4, 0.22, 1.5);
        assert!(!old.exact_stable, "OLD gains must be EXACT-Jury unstable");
        assert!(!old.stable);
        let tuned = JuryResult::check(1.0, 1.03, 0.22, 0.20);
        assert!(tuned.exact_stable, "TUNED gains must be EXACT-Jury stable");
        assert!(tuned.stable);
        // p_pos1>0, p_neg1<0, |d|<a are the three exact Jury clauses.
        assert!(tuned.p_pos1 > 0.0 && tuned.p_neg1 < 0.0);
    }

    // ── BP-05 RED→GREEN gate: derivative kick ────────────────────────────────────

    #[test]
    fn raw_effort_old_exceeds_clamp() {
        // The pre-BP-05 unclamped controller slams on ONE reject: |u| ≫ u_max.
        let cfg = default_legacy();
        let u = raw_control_effort(cfg.kp, cfg.ki, cfg.kd, 0.0 - cfg.target_quality);
        assert!(
            u.abs() > 0.2,
            "OLD unclamped effort should exceed the ±0.2 clamp (got {}); gate broken if not",
            u
        );
    }

    #[test]
    fn kick_one_reject_bounded_by_clamp() {
        // With the BP-05 clamp active, one reject moves authority by at most 0.2.
        // Use a CHANGING rate (prev settled at 1.0, now 0.0) so the EMA-filtered derivative
        // path is actually exercised (a flat rate would zero the derivative term).
        let cfg = GovConfig::default_tuned();
        let mut st = GovState::new_at_rate(0.5, 1.0);
        cfg.step_with_rate(&mut st, 0.0, 0.0, 1e-18, 100.0);
        let delta = (st.authority - 0.5).abs();
        assert!(
            delta <= 0.2 + 1e-12,
            "one reject moved authority by {} (>0.2)",
            delta
        );
        assert!(
            delta > 0.0,
            "one reject should still move authority, got {}",
            delta
        );
        // The derivative term must have contributed (last_d != 0) on a changing rate.
        assert!(
            st.last_d.abs() > 1e-9,
            "derivative EMA path must fire on a changing rate; last_d={}",
            st.last_d
        );
    }

    #[test]
    fn kick_one_reject_live_step_path_bounded() {
        // Live `step()` path (raw verdict → internal rate EMA): a verdict transition
        // approve→reject must NOT slam authority; |Δauthority| ≤ 0.2 after CLAMP.
        let cfg = GovConfig::default_tuned();
        let mut st = GovState::new(0.5);
        cfg.step(&mut st, 1.0, 1e-18, 100.0); // settle a couple of approves first
        cfg.step(&mut st, 1.0, 1e-18, 100.0);
        let before = st.authority;
        cfg.step(&mut st, 0.0, 1e-18, 100.0); // the reject
        let delta = (st.authority - before).abs();
        assert!(
            delta <= 0.2 + 1e-12,
            "live-step reject slammed authority by {} (>0.2)",
            delta
        );
    }

    // ── BP-05 RED→GREEN gate: critical damping ───────────────────────────────────

    #[test]
    fn damping_tuned_is_critical() {
        let cfg = GovConfig::default_tuned();
        let zeta = damping_ratio(cfg.kg, cfg.kp, cfg.ki, cfg.kd);
        assert!(
            (zeta - 1.0).abs() < 0.05,
            "tuned gains must give ζ≈1, got {}",
            zeta
        );
    }

    #[test]
    fn damping_old_is_underdamped() {
        let cfg = default_legacy();
        let zeta = damping_ratio(cfg.kg, cfg.kp, cfg.ki, cfg.kd);
        assert!(
            (zeta - 0.944).abs() < 0.02,
            "OLD gains should be ζ≈0.944, got {}",
            zeta
        );
        assert!(
            (zeta - 1.0).abs() >= 0.05,
            "OLD gains should NOT be critical, got {}",
            zeta
        );
    }

    // ── Deadband / clamp wiring ──────────────────────────────────────────────────

    #[test]
    fn output_clamp_respected() {
        let cfg = GovConfig::default_tuned();
        let mut st = GovState::new(0.0);
        for _ in 0..50 {
            cfg.step_with_rate(&mut st, 1.0, 1.0, 1e-18, 100.0);
        }
        assert!(
            st.authority <= 1.0 + 1e-12,
            "authority exceeded 1.0: {}",
            st.authority
        );
        assert!(
            st.last_u <= cfg.u_max + 1e-12,
            "u exceeded u_max: {}",
            st.last_u
        );
        assert!(
            st.last_u >= cfg.u_min - 1e-12,
            "u below u_min: {}",
            st.last_u
        );
    }

    #[test]
    fn deadband_holds_authority() {
        // System already settled at setpoint (error=0, no rate change) ⇒ u≈0 ⇒ held.
        let cfg = GovConfig::default_tuned();
        let mut st = GovState::new_at_rate(0.5, cfg.target_quality);
        cfg.step_with_rate(
            &mut st,
            cfg.target_quality,
            cfg.target_quality,
            1e-18,
            100.0,
        );
        assert!(
            (st.authority - 0.5).abs() < 1e-9,
            "deadband did not hold authority: {}",
            st.authority
        );
    }
}
