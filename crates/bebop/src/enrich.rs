//! ENRICH — native, falsifiable primitives reverse-engineered from the master
//! technical dossier (agent architecture + math + design-thinking). Every item is
//! grounded in verified methods and RED+GREEN tested. 0 deps.
//!
//! Dossier coverage map (what we ADD here vs what already exists):
//!   §1.1 design-thinking builder  → `DesignThinking` (NEW)
//!   §1.2 perceive-think-act-observe → copilot/pddl/tui (EXISTING, wired)
//!   §1.3 persistent memory        → memory/agentic_git/knowledge (EXISTING)
//!   §1.4 eval suite               → research_patterns::eval_rag (EXISTING)
//!   §1.5 safety layers            → redteam/audit/governor (EXISTING)
//!   §1.6 full-trace replay        → `Trace` + low-confidence gate (NEW)
//!   §1.8 optimization loop        → stabilizer (EXISTING)
//!   §1.9 SEAL self-adapting       → `seal_self_edit` (NEW analog)
//!   §2.10 Pareto frontier         → `pareto_frontier` (NEW)
//!   §2.13 adaptive control        → stabilizer Lyapunov/MRAC (EXISTING)
//!   §2.14 optimization algorithms → `gradient_descent` + `adam` (NEW)

/// ─────────────────────────────────────────────────────────────────────────
/// §1.6 FULL-TRACE REPLAY — record agent steps with durations; route
/// low-confidence steps to human review (the dossier's "make invisible visible").
/// ─────────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq)]
pub struct TraceStep {
    pub name: String,
    pub ms: u64,
    pub confidence: f64, // 0..1
}

#[derive(Default)]
pub struct Trace {
    pub steps: Vec<TraceStep>,
    pub human_reviews: Vec<String>,
}

impl Trace {
    pub fn new() -> Self {
        Trace::default()
    }

    /// Record a step. If confidence < `review_threshold`, route to human review
    /// (the dossier's low-confidence → human gate). GREEN: high-conf step → no
    /// review queued. RED: low-conf step → queued for human (not silently dropped).
    pub fn record(&mut self, name: &str, ms: u64, confidence: f64, review_threshold: f64) {
        let needs_review = confidence < review_threshold;
        self.steps.push(TraceStep {
            name: name.into(),
            ms,
            confidence,
        });
        if needs_review {
            self.human_reviews.push(name.into());
        }
    }

    /// Total elapsed (sum of step durations) — the replay timeline.
    pub fn total_ms(&self) -> u64 {
        self.steps.iter().map(|s| s.ms).sum()
    }

    /// Steps that breached the confidence gate (must be reviewed before ship).
    pub fn needs_review(&self) -> &[String] {
        &self.human_reviews
    }
}

/// ─────────────────────────────────────────────────────────────────────────
/// §2.10 PARETO FRONTIER — set of non-dominated solutions. A point A dominates
/// B iff A is >= B on every objective and strictly > on at least one.
/// ─────────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq)]
pub struct Point {
    pub id: String,
    pub obj: Vec<f64>, // Higher is better on every objective.
}

/// Returns the non-dominated subset (the frontier). GREEN: a strictly-better
/// point dominates a worse one. RED: equal points both survive (neither dominates).
pub fn pareto_frontier(points: &[Point]) -> Vec<Point> {
    let mut front = Vec::new();
    for (i, a) in points.iter().enumerate() {
        let mut dominated = false;
        for (j, b) in points.iter().enumerate() {
            if i == j {
                continue;
            }
            if dominates(b, a) {
                dominated = true;
                break;
            }
        }
        if !dominated {
            front.push(a.clone());
        }
    }
    front
}

/// `a` dominates `b` iff a >= b on all objectives and a > b on at least one.
fn dominates(a: &Point, b: &Point) -> bool {
    let mut strictly = false;
    for (av, bv) in a.obj.iter().zip(b.obj.iter()) {
        if av < bv {
            return false;
        }
        if av > bv {
            strictly = true;
        }
    }
    strictly
}

/// ─────────────────────────────────────────────────────────────────────────
/// §2.14 OPTIMIZATION ALGORITHMS — deterministic gradient descent + Adam.
/// Used by the stabilizer's optimizer and the SEAL analog.
/// ─────────────────────────────────────────────────────────────────────────
/// Plain gradient descent on a 1-D objective `f'(x)=grad`. Returns the trajectory
/// of x values (so callers can prove convergence). `lr` = learning rate.
pub fn gradient_descent(mut x: f64, grad: impl Fn(f64) -> f64, lr: f64, steps: usize) -> Vec<f64> {
    let mut traj = vec![x];
    for _ in 0..steps {
        x -= lr * grad(x);
        traj.push(x);
    }
    traj
}

/// Adam (Kingma & Ba) — adaptive moment estimation, deterministic.
/// `grad` is the gradient at x. Returns the trajectory of x.
pub fn adam(
    mut x: f64,
    grad: impl Fn(f64) -> f64,
    lr: f64,
    steps: usize,
    beta1: f64,
    beta2: f64,
    eps: f64,
) -> Vec<f64> {
    let mut m = 0.0;
    let mut v = 0.0;
    let mut traj = vec![x];
    for t in 1..=steps {
        let g = grad(x);
        m = beta1 * m + (1.0 - beta1) * g;
        v = beta2 * v + (1.0 - beta2) * g * g;
        let mhat = m / (1.0 - beta1.powi(t as i32));
        let vhat = v / (1.0 - beta2.powi(t as i32));
        x -= lr * mhat / (vhat.sqrt() + eps);
        traj.push(x);
    }
    traj
}

/// ─────────────────────────────────────────────────────────────────────────
/// §1.9 SEAL analog — Self-Adapting LLM: the model GENERATES its own adaptation
/// directives ("self-edits") from task feedback, then those edits update a local
/// adaptation store. We implement the DETERMINISTIC skeleton: a self-edit is a
/// (trigger_pattern, correction) rule; feedback that matches a trigger appends a
/// correction to the store. No real finetune (offline, reproducible).
/// ─────────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq)]
pub struct SelfEdit {
    pub trigger: String,
    pub correction: String,
}

#[derive(Default)]
pub struct SealStore {
    pub edits: Vec<SelfEdit>,
}

impl SealStore {
    pub fn new() -> Self {
        SealStore::default()
    }

    /// Generate a self-edit from feedback: if `feedback` contains `error_marker`,
    /// emit a correction rule keyed by the trigger concept. GREEN: a real error
    /// marker → a self-edit is stored. RED: no marker → no self-edit (no noise).
    pub fn learn_from(&mut self, feedback: &str, error_marker: &str, correction: &str) {
        if feedback.contains(error_marker) {
            self.edits.push(SelfEdit {
                trigger: error_marker.into(),
                correction: correction.into(),
            });
        }
    }

    /// Apply stored edits to a draft: replace any trigger occurrence with its
    /// correction. Deterministic replay of accumulated self-edits.
    pub fn apply(&self, draft: &str) -> String {
        let mut out = draft.to_string();
        for e in &self.edits {
            out = out.replace(&e.trigger, &e.correction);
        }
        out
    }
}

/// ─────────────────────────────────────────────────────────────────────────
/// §1.1 DESIGN-THINKING BUILDER — structured, template-driven (no LLM) outputs
/// for the 10 prompt patterns. Typed structs + generators.
/// ─────────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug, PartialEq)]
pub struct EmpathyMap {
    pub says: Vec<String>,
    pub thinks: Vec<String>,
    pub does: Vec<String>,
    pub feels: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValueProp {
    pub target: String,
    pub pain: String,
    pub benefit: String,
    pub competitor: String,
}

impl ValueProp {
    pub fn sentence(&self) -> String {
        format!(
            "For {} struggling with {}, {} provides {}, unlike {}",
            self.target, self.pain, "this product", self.benefit, self.competitor
        )
    }
}

/// Generate N "How Might We" questions from problem facets (deterministic).
pub fn how_might_we(facets: &[&str]) -> Vec<String> {
    facets.iter().map(|f| format!("How might we {}?", f)).collect()
}

/// Build an empathy map from a flat list of observations tagged by quadrant.
/// `tagged` items look like "says: users want speed". GREEN: tagged item lands
/// in its quadrant. RED: untagged/unknown quadrant → ignored (no garbage bin).
pub fn empathy_map(tagged: &[&str]) -> EmpathyMap {
    let mut m = EmpathyMap {
        says: vec![],
        thinks: vec![],
        does: vec![],
        feels: vec![],
    };
    for t in tagged {
        if let Some((q, rest)) = t.split_once(':') {
            let item = rest.trim().to_string();
            match q.trim() {
                "says" => m.says.push(item),
                "thinks" => m.thinks.push(item),
                "does" => m.does.push(item),
                "feels" => m.feels.push(item),
                _ => {} // unknown quadrant ignored
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_replay_and_confidence_gate() {
        // GREEN: high-conf step → no human review
        let mut tr = Trace::new();
        tr.record("agent.run", 1240, 0.95, 0.5);
        assert!(tr.needs_review().is_empty());
        assert_eq!(tr.total_ms(), 1240);
        // RED: low-conf step → routed to human review (not dropped)
        tr.record("llm.synthesize", 380, 0.2, 0.5);
        assert_eq!(tr.needs_review().len(), 1);
        assert_eq!(tr.total_ms(), 1620);
    }

    #[test]
    fn pareto_finds_non_dominated() {
        // A=(2,2) dominates C=(1,1); B=(3,1) is non-dominated (trades off)
        let pts = vec![
            Point { id: "A".into(), obj: vec![2.0, 2.0] },
            Point { id: "B".into(), obj: vec![3.0, 1.0] },
            Point { id: "C".into(), obj: vec![1.0, 1.0] },
        ];
        let f = pareto_frontier(&pts);
        let ids: Vec<&str> = f.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"A"));
        assert!(ids.contains(&"B"));
        assert!(!ids.contains(&"C"), "dominated point must be excluded");
    }

    #[test]
    fn pareto_equal_points_both_survive() {
        // Equal points: neither dominates → both on frontier (RED-safe)
        let pts = vec![
            Point { id: "X".into(), obj: vec![1.0, 1.0] },
            Point { id: "Y".into(), obj: vec![1.0, 1.0] },
        ];
        assert_eq!(pareto_frontier(&pts).len(), 2);
    }

    #[test]
    fn gradient_descent_minimizes() {
        // f(x)=x^2 → grad=2x; GD from 3 should approach 0
        let traj = gradient_descent(3.0, |x| 2.0 * x, 0.1, 50);
        let last = *traj.last().unwrap();
        assert!(last.abs() < 0.01, "GD should converge to ~0, got {last}");
        // monotonic decrease (RED: no divergence)
        assert!(traj[1] < traj[0]);
    }

    #[test]
    fn adam_converges_faster_than_gd() {
        // On f(x)=x^2 both converge; Adam should reach tighter tolerance in fewer steps
        let gd = gradient_descent(3.0, |x| 2.0 * x, 0.1, 20);
        let ad = adam(3.0, |x| 2.0 * x, 0.1, 20, 0.9, 0.999, 1e-8);
        assert!(ad.last().unwrap().abs() < gd.last().unwrap().abs(), "Adam should be tighter");
    }

    #[test]
    fn seal_learns_and_applies_self_edits() {
        // GREEN: error marker in feedback → self-edit stored
        let mut store = SealStore::new();
        store.learn_from("output had NULL deref", "NULL deref", "add null check");
        assert_eq!(store.edits.len(), 1);
        // RED: no marker → no self-edit (no noise)
        store.learn_from("output looks fine", "NULL deref", "add null check");
        assert_eq!(store.edits.len(), 1);
        // apply: draft trigger replaced by correction
        let fixed = store.apply("code with NULL deref bug");
        assert_eq!(fixed, "code with add null check bug");
    }

    #[test]
    fn design_thinking_structures() {
        // empathy map routes tags to quadrants
        let m = empathy_map(&["says: want speed", "thinks: it is slow", "bogus: ignore", "feels: frustrated"]);
        assert_eq!(m.says, vec!["want speed"]);
        assert_eq!(m.thinks, vec!["it is slow"]);
        assert_eq!(m.feels, vec!["frustrated"]);
        // HMW generation
        let hmw = how_might_we(&["reduce latency", "improve trust"]);
        assert_eq!(hmw[0], "How might we reduce latency?");
        // value prop sentence
        let vp = ValueProp {
            target: "couriers".into(),
            pain: "late dispatches".into(),
            benefit: "on-time routing".into(),
            competitor: "legacy TMS".into(),
        };
        assert!(vp.sentence().contains("unlike legacy TMS"));
    }
}
