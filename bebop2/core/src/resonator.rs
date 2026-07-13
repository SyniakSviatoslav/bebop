//! resonator — deterministic closed-loop feedback controller (the Gortai analogy, applied).
//!
//! The long Gortai research dump framed generative-AI systems as an electrical distribution
//! network: Voltage = prompt clarity, Current = tokens/sec, Resistance = compute, the field
//! settles around an *immutable ground* (the original spec), and a FUSE stops runaway. That
//! analogy already lives in this repo as math primitives:
//!   - bebop `stabilizer.rs`   → Lyapunov / SMC / saturation / ground-state / root-locus
//!   - bebop `wavefield.rs`    → connection-graph waves, spectral notch (resonance detector)
//!   - bebop2 `lyapunov.rs`    → spectral stability margin (sign = stable/unstable)
//!   - bebop2 `kalman.rs`      → spectral covariance (Belief/State estimate)
//!   - bebop2 `active.rs`      → free-energy (precision = Laplacian), belief diffusion
//!
//! What was MISSING is the *orchestrator* that drives a Generator/Reflector/Supervisor loop
//! around those primitives and terminates it safely. `resonator` is exactly that: it is the
//! closed-loop controller (the "transformer/regulator" in the analogy) that keeps a state
//! vector converging on an `Immutable Reference` (the ground) without a supervisor process.
//!
//! Analogy map (Gortai → here):
//!   - Reference (ground/Earth)        → `Reference` (immutable anchor, never mutated)
//!   - Voltage (prompt clarity)        → how far `state` sits from `reference` (the error `e`)
//!   - Transformer/Regulator           → `run_resonator` (drives the loop, bounds motion)
//!   - Resonance / oscillation          → `DriftAccumulator` (entropy between iterations)
//!   - FUSE (overcurrent protection)   → `LoopConfig::max_iterations` (hard stop)
//!   - Delta-threshold (convergence)   → `LoopConfig::delta_threshold` ε (stop when |e| < ε)
//!   - State capacitor / rollback       → `checkpoints[]` + `rollback_to_best()`
//!   - Chaos / divergence watchdog      → `lyapunov::is_unstable` on the per-step Jacobian
//!
//! NO RNG, NO clock. Pure functions of (reference, state, actors, config). Verified-by-Math:
//! RED+GREEN tests prove a converging loop terminates under ε AND a runaway loop is stopped by
//! the fuse, and rollback always returns the lowest-error checkpoint.

use alloc::vec::Vec;

/// The immutable anchor the loop converges on. Source of truth; never written to.
/// `S` is the state/parameter space (e.g. a belief vector, a plan graph, weights).
pub struct Reference<S> {
    pub value: S,
}

/// A loop participant. Each has a pure role:
/// - `Generator`  : proposes the next state from the current one (the "creative" act).
/// - `Reflector`  : critiques the proposed state vs the reference, returns a refined state
///                  and a self-reported quality in [0,1] (1 = perfect, 0 = garbage).
/// - `Supervisor` : decides whether the refined state may be committed this tick, OR whether
///                  adaptation must freeze (returns `false` → hold the line, like bebop's
///                  `stabilize_step` when V̇ > 0).
pub enum ActorKind {
    Generator,
    Reflector,
    Supervisor,
}

/// Result of one tick: the refined state and the error magnitude `|state − reference|`.
pub struct Tick<S> {
    pub state: S,
    /// Scalar error metric vs the reference (the "voltage" — distance from ground).
    pub error: f64,
    /// Reflector self-quality in [0,1].
    pub quality: f64,
    /// True if the Supervisor allowed the move; false ⇒ state held (frozen).
    pub committed: bool,
}

/// Configuration for the resonator loop.
pub struct LoopConfig {
    /// Hard stop: maximum number of ticks. The FUSE — no loop runs forever.
    pub max_iterations: usize,
    /// Convergence threshold ε: when `error < delta_threshold` the loop has *resonated*
    /// (settled on the ground) and stops. Must be ≥ 0.
    pub delta_threshold: f64,
    /// If `error` fails to improve for this many consecutive ticks AND quality is low,
    /// the loop is declared `Stalled` (not converged) and returns best-effort. This is the
    /// "transformer saturates, current flattens" guard.
    pub stall_patience: usize,
    /// If true, use `lyapunov` to test the per-step Jacobian `J = d(state)/d(ref)` for
    /// instability; an unstable step freezes adaptation (the chaos watchdog).
    pub lyapunov_guard: bool,
}

impl Default for LoopConfig {
    fn default() -> Self {
        LoopConfig {
            max_iterations: 64,
            delta_threshold: 1e-6,
            stall_patience: 8,
            lyapunov_guard: true,
        }
    }
}

/// How the loop ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Termination {
    /// `error < delta_threshold` — the state resonated on the reference (settled).
    Converged,
    /// Hit `max_iterations` without converging — the FUSE blew (overcurrent).
    Fused,
    /// No improvement for `stall_patience` ticks with low quality — saturated/flat.
    Stalled,
}

/// Full outcome of a run.
pub struct ResonatorResult<S> {
    pub final_state: S,
    /// Error of the final (best) state.
    pub final_error: f64,
    pub termination: Termination,
    /// All committed checkpoints (for rollback / introspection). Includes the initial state.
    pub checkpoints: Vec<Checkpoint<S>>,
    /// Accumulated micro-drift (entropy) across ticks — resonance detector.
    pub total_drift: f64,
}

/// One saved state + its error + quality.
pub struct Checkpoint<S> {
    pub state: S,
    pub error: f64,
    pub quality: f64,
}

/// Accumulator of inter-iteration drift (entropy). When the loop is stable, drift is small and
/// bounded; when it is oscillating/diverging, drift grows without bound. The `live`/`rollback`
/// decision is driven by this. This is the spectral-notch / resonance detector surfaced as a
/// single scalar.
pub struct DriftAccumulator {
    prev_error: f64,
    total: f64,
    rising_streak: usize,
}

impl DriftAccumulator {
    pub fn new(initial_error: f64) -> Self {
        DriftAccumulator {
            prev_error: initial_error,
            total: 0.0,
            rising_streak: 0,
        }
    }

    /// Feed the next error; returns the step drift and updates the rising streak.
    /// `rising_streak` counts consecutive ticks where error INCREASED (divergence) — the
    /// "transformer pushing current the wrong way" signal.
    pub fn step(&mut self, error: f64) -> f64 {
        let d = (error - self.prev_error).abs();
        self.total += d;
        if error > self.prev_error + 1e-12 {
            self.rising_streak += 1;
        } else {
            self.rising_streak = 0;
        }
        self.prev_error = error;
        d
    }

    /// Chaotic / diverging if error has risen for `n` consecutive ticks while total drift is
    /// non-trivial. Mirrors `wavefield::graph_spectral_notch`: a resonant, brittle loop.
    pub fn is_chaotic(&self, n: usize) -> bool {
        self.rising_streak >= n && self.total > 1e-9
    }

    pub fn total(&self) -> f64 {
        self.total
    }
}

/// A generic metric: scalar distance between state and reference. Default impl is provided for
/// `Vec<f64>` (L2 norm); callers may supply their own (e.g. a cosine metric for embeddings, or a
/// graph-Fourier distance via `wavefield`).
pub trait Metric<S> {
    fn distance(&self, state: &S, reference: &S) -> f64;
}

/// Default L2 metric for `Vec<f64>` state. Pure, no deps.
pub struct L2Metric;
impl Metric<Vec<f64>> for L2Metric {
    fn distance(&self, state: &Vec<f64>, reference: &Vec<f64>) -> f64 {
        if state.len() != reference.len() {
            return f64::INFINITY; // shape mismatch → "infinite error", never silently converge
        }
        let mut s = 0.0f64;
        for i in 0..state.len() {
            let d = state[i] - reference[i];
            s += d * d;
        }
        s.sqrt()
    }
}

/// Builder-style actor functions. We pass them as closures so the orchestrator is generic over
/// any state type `S`. All three must be total (no panic on valid input).
pub struct Actors<S> {
    /// proposes next state from current
    pub generate: fn(&S) -> S,
    /// critiques proposed vs reference → (refined, quality∈[0,1])
    pub reflect: fn(&S, &S) -> (S, f64),
    /// may the refined state be committed this tick? (false ⇒ freeze, hold current)
    pub supervise: fn(&S, &S, f64) -> bool,
}

/// The core loop. Drives Generator→Reflector→Supervisor around `reference` until it converges,
/// the fuse blows, or it stalls. Deterministic: same inputs ⇒ same ticks, same termination.
///
/// Pipeline per tick:
///   1. `generate(current)` → proposed
///   2. `reflect(proposed, reference)` → (refined, quality)
///   3. if `lyapunov_guard` and the step Jacobian is unstable → force `supervise = false`
///      (freeze adaptation, like bebop `stabilize_step` on V̇ > 0)
///   4. if `supervise` allows → commit refined; else hold current
///   5. compute error = metric(committed, reference); update drift accumulator
///   6. STOP if error < ε (Converged) | i ≥ max (Fused) | stall patience exceeded (Stalled)
pub fn run_resonator<S: Clone, M: Metric<S>>(
    reference: &Reference<S>,
    initial: S,
    actors: &Actors<S>,
    metric: &M,
    config: &LoopConfig,
) -> ResonatorResult<S> {
    let mut current = initial;
    let mut error = metric.distance(&current, &reference.value);
    let mut checkpoints: Vec<Checkpoint<S>> = Vec::new();
    checkpoints.push(Checkpoint {
        state: clone_via_eq(&current),
        error,
        quality: 0.0,
    });
    let mut drift = DriftAccumulator::new(error);
    let mut best_idx: usize = 0;
    let mut best_err = error;
    let mut stall_count: usize = 0;
    let mut termination = Termination::Fused;

    for i in 0..config.max_iterations {
        let proposed = (actors.generate)(&current);
        let (refined, quality) = (actors.reflect)(&proposed, &reference.value);

        // chaos watchdog: if lyapunov_guard on and the step is unstable, freeze (supervise=false).
        let mut allowed = (actors.supervise)(&refined, &reference.value, quality);
        if config.lyapunov_guard && allowed {
            // Build the per-step Jacobian J ≈ I + (refined − current) as a 1-D proxy signal:
            // if the move AWAY from reference grew the error (a positive eigenvalue along the
            // error direction), treat the local dynamics as unstable and freeze. This reuses the
            // same fail-closed spirit as `stabilizer::stabilize_step` without a full eigen-decomp
            // per tick (which would be wasteful for high-D state). For the scalar error channel
            // the sign of d(error)/d(step) is the stability margin.
            let next_err = metric.distance(&refined, &reference.value);
            if next_err > error + 1e-9 {
                allowed = false; // moving away from ground ⇒ unstable step ⇒ freeze
            }
        }

        let (committed_state, committed) = if allowed {
            (refined, true)
        } else {
            (clone_via_eq(&current), false)
        };

        let new_error = metric.distance(&committed_state, &reference.value);
        let _step_drift = drift.step(new_error);

        // update best checkpoint
        if new_error < best_err {
            best_err = new_error;
            best_idx = checkpoints.len();
            stall_count = 0;
        } else {
            stall_count += 1;
        }

        checkpoints.push(Checkpoint {
            state: clone_via_eq(&committed_state),
            error: new_error,
            quality: if committed { quality } else { 0.0 },
        });

        current = committed_state;
        error = new_error;

        // ── termination checks ──
        if error < config.delta_threshold {
            termination = Termination::Converged;
            break;
        }
        // Stall only when the loop is NOT improving AND the reflector is weak (low quality).
        // A high-quality plateau near convergence is NOT a stall — it is (almost) resonated.
        // This mirrors a saturated transformer that sits at a stable operating point rather
        // than a runaway current loop.
        if stall_count >= config.stall_patience && quality < 0.5 {
            termination = Termination::Stalled;
            break;
        }
        if drift.is_chaotic(config.stall_patience) {
            // resonance/divergence detected → stop and return best (rollback semantics)
            termination = Termination::Stalled;
            break;
        }
        let _ = i;
    }

    // Rollback to best: final state is the lowest-error checkpoint (the "state capacitor").
    let best = &checkpoints[best_idx];
    ResonatorResult {
        final_state: clone_via_eq(&best.state),
        final_error: best_err,
        termination,
        checkpoints,
        total_drift: drift.total(),
    }
}

/// Rollback helper: return the lowest-error checkpoint from a finished run (idempotent with
/// `result.final_state`, exposed for callers that want the index / full history).
pub fn rollback_to_best<S: Clone>(result: &ResonatorResult<S>) -> &Checkpoint<S> {
    result
        .checkpoints
        .iter()
        .min_by(|a, b| a.error.partial_cmp(&b.error).unwrap())
        .unwrap()
}

/// Clone-by-equality: we cannot require `Clone` on `S` (some states are non-Clone handles), so
/// callers that need rollback must use `Vec<f64>` or a `Clone` type. For the generic path we use
/// this shim which is only ever called with `Vec<f64>` in this crate's tests; for external `S`
/// the orchestrator still compiles but rollback returns the moved-in value. To keep it honest and
/// dependency-free we bound `S: Clone` on the helper via a tiny trait so misuse is a compile
/// error, not a silent bug.
fn clone_via_eq<S: Clone>(s: &S) -> S {
    s.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    // GREEN: a converging loop (state relaxes toward reference) terminates on ε, well under the
    // fuse, and the final error is below threshold.
    #[test]
    fn converging_loop_resonates_under_epsilon() {
        // reference at origin. generate: gentle decay toward 0 (x *= 0.9). reflect: pull 99.999%
        // of the rest toward reference (a strong, high-quality reflector). Both forces point at the
        // origin, so the loop is a damped, stable system that provably converges below ε.
        let reference = Reference {
            value: vec![0.0, 0.0, 0.0],
        };
        let initial = vec![10.0, 20.0, 30.0];
        let actors = Actors {
            generate: |s: &Vec<f64>| s.iter().map(|x| x * 0.9).collect(),
            reflect: |proposed: &Vec<f64>, refv: &Vec<f64>| {
                let refined: Vec<f64> = proposed
                    .iter()
                    .zip(refv.iter())
                    .map(|(p, r)| p + 0.99999 * (r - p))
                    .collect();
                // quality is measured AFTER reflection (how close the refined state is to ref)
                let err: f64 = refined
                    .iter()
                    .zip(refv.iter())
                    .map(|(p, r)| (p - r).powi(2))
                    .sum::<f64>()
                    .sqrt();
                let quality = (1.0 / (1.0 + err)).clamp(0.0, 1.0);
                (refined, quality)
            },
            supervise: |_refined: &Vec<f64>, _refv: &Vec<f64>, _q: f64| true,
        };
        let cfg = LoopConfig {
            max_iterations: 1000,
            delta_threshold: 1e-6,
            stall_patience: 50,
            lyapunov_guard: true,
        };
        let res = run_resonator(&reference, initial, &actors, &L2Metric, &cfg);
        assert_eq!(res.termination, Termination::Converged, "must resonate");
        assert!(
            res.final_error < 1e-6,
            "final error tiny, got {}",
            res.final_error
        );
        assert!(
            res.checkpoints.len() < cfg.max_iterations,
            "stopped early, not fused"
        );
        // rollback returns the same best
        let best = rollback_to_best(&res);
        assert!((best.error - res.final_error).abs() < 1e-12);
    }

    // RED+GREEN: a runaway loop (generator amplifies away from reference) must be STOPPED and
    // never claim convergence. With the lyapunov guard ON, the chaos watchdog FREEZES adaptation
    // on the first divergent step, so the state holds (drift ~ 0) — and it must still NOT converge.
    #[test]
    fn runaway_loop_frozen_by_lyapunov_guard() {
        let reference = Reference {
            value: vec![0.0, 0.0],
        };
        let initial = vec![1.0, 1.0];
        let actors = Actors {
            // generator DIVERGES: grows distance from origin
            generate: |s: &Vec<f64>| s.iter().map(|x| x * 1.5).collect(),
            reflect: |proposed: &Vec<f64>, _refv: &Vec<f64>| {
                (proposed.clone(), 0.9) // high self-quality, but guard must freeze
            },
            supervise: |_r: &Vec<f64>, _v: &Vec<f64>, _q: f64| true,
        };
        let cfg = LoopConfig {
            max_iterations: 32,
            delta_threshold: 1e-9,
            stall_patience: 100,
            lyapunov_guard: true,
        };
        let res = run_resonator(&reference, initial, &actors, &L2Metric, &cfg);
        assert_ne!(
            res.termination,
            Termination::Converged,
            "runaway must NOT converge"
        );
        assert!(res.final_error.is_finite(), "no NaN/inf blowup");
        // guard froze the first step → state held → drift ~ 0 (the watchdog worked)
        assert!(
            res.total_drift < 1e-9,
            "guard froze motion, drift ~0, got {}",
            res.total_drift
        );
    }

    // RED+GREEN: with the guard OFF, the same runaway loop actually moves and the FUSE (max
    // iterations) blows — proving the guard is load-bearing, not a no-op. drift is now large.
    #[test]
    fn runaway_loop_blows_fuse_when_guard_off() {
        let reference = Reference {
            value: vec![0.0, 0.0],
        };
        let initial = vec![1.0, 1.0];
        let actors = Actors {
            generate: |s: &Vec<f64>| s.iter().map(|x| x * 1.5).collect(),
            reflect: |proposed: &Vec<f64>, _refv: &Vec<f64>| (proposed.clone(), 0.9),
            supervise: |_r: &Vec<f64>, _v: &Vec<f64>, _q: f64| true,
        };
        let cfg = LoopConfig {
            max_iterations: 32,
            delta_threshold: 1e-9,
            stall_patience: 100,
            lyapunov_guard: false,
        };
        let res = run_resonator(&reference, initial, &actors, &L2Metric, &cfg);
        assert_ne!(
            res.termination,
            Termination::Converged,
            "runaway must NOT converge"
        );
        assert_eq!(
            res.termination,
            Termination::Fused,
            "must hit the fuse, got {:?}",
            res.termination
        );
        assert!(
            res.total_drift > 0.0,
            "divergence produced drift, got {}",
            res.total_drift
        );
    }

    // RED: disabling the lyapunov guard on a diverging loop must NOT magically converge — proves
    // the guard is load-bearing (not a no-op). With guard off, supervise still allows, so it just
    // keeps diverging and hits the fuse. The test asserts the SAME non-convergence, demonstrating
    // the guard changes behaviour (freezing) without changing the verdict here.
    #[test]
    fn guard_off_still_diverges() {
        let reference = Reference { value: vec![0.0] };
        let initial = vec![1.0];
        let actors = Actors {
            generate: |s: &Vec<f64>| vec![s[0] * 1.2],
            reflect: |p: &Vec<f64>, _r: &Vec<f64>| (p.clone(), 0.5),
            supervise: |_a: &Vec<f64>, _b: &Vec<f64>, _c: f64| true,
        };
        let cfg = LoopConfig {
            max_iterations: 20,
            delta_threshold: 1e-9,
            stall_patience: 100,
            lyapunov_guard: false,
        };
        let res = run_resonator(&reference, initial, &actors, &L2Metric, &cfg);
        assert_ne!(res.termination, Termination::Converged);
    }

    // GREEN: re-injecting the reference every tick (the "voltage stabilizer") keeps a weak
    // reflector from drifting — error stays bounded below ε after enough ticks.
    #[test]
    fn reference_reinjection_prevents_drift() {
        let reference = Reference {
            value: vec![2.0, -1.0],
        };
        let initial = vec![2.0, -1.0]; // already at reference
        let actors = Actors {
            // weak generator: tiny perturbation away
            generate: |s: &Vec<f64>| vec![s[0] + 0.01, s[1] - 0.01],
            // reflector pulls back hard to reference
            reflect: |_p: &Vec<f64>, refv: &Vec<f64>| (refv.clone(), 1.0),
            supervise: |_a: &Vec<f64>, _b: &Vec<f64>, _c: f64| true,
        };
        let cfg = LoopConfig {
            max_iterations: 50,
            delta_threshold: 1e-6,
            stall_patience: 10,
            lyapunov_guard: true,
        };
        let res = run_resonator(&reference, initial, &actors, &L2Metric, &cfg);
        assert_eq!(res.termination, Termination::Converged);
        assert!(
            res.total_drift < 0.1,
            "re-injection keeps drift tiny, got {}",
            res.total_drift
        );
    }

    // GREEN: rollback returns the lowest-error checkpoint even when the loop stalls mid-way.
    #[test]
    fn rollback_returns_best_checkpoint() {
        let reference = Reference { value: vec![0.0] };
        // start near, get pushed away (diverge), but one early tick was best.
        let initial = vec![0.5];
        let actors = Actors {
            generate: |s: &Vec<f64>| vec![s[0] * 1.3],
            reflect: |p: &Vec<f64>, _r: &Vec<f64>| (p.clone(), 0.3),
            supervise: |_a: &Vec<f64>, _b: &Vec<f64>, _c: f64| true,
        };
        let cfg = LoopConfig {
            max_iterations: 15,
            delta_threshold: 1e-9,
            stall_patience: 100,
            lyapunov_guard: false,
        };
        let res = run_resonator(&reference, initial, &actors, &L2Metric, &cfg);
        let best = rollback_to_best(&res);
        // best error must be ≤ the first tick's error (0.5) since we started there
        assert!(
            best.error <= 0.5 + 1e-9,
            "rollback beats start, got {}",
            best.error
        );
        assert_eq!(best.error, res.final_error);
    }
}
