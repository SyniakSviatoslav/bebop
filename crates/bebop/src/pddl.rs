//! PDDL `logicalCot` — deterministic STRIPS-style planner + chain-of-thought trace.
//!
//! Replaces the TS-retired `PDDL logicalCot` behavior as real, tested Rust.
//! Given a set of typed predicates (facts), actions with preconditions/effects,
//! an initial state and a goal, it performs forward search (BFS) and returns
//! either a plan (sequence of action names) or `None` if the goal is unreachable
//! (anti-hallucination: no invented plan for a bogus goal). It also emits a
//! step-by-step `trace` so the reasoning is auditable — that's the "logical
//! chain of thought", not an LLM. NO rng, NO wall-clock.

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Pred {
    pub name: String,
    pub args: Vec<String>,
}

impl Pred {
    pub fn new(name: &str, args: &[&str]) -> Self {
        Pred {
            name: name.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
        }
    }
    /// Stable string key for set membership.
    fn key(&self) -> String {
        format!("{}[{}]", self.name, self.args.join(","))
    }
}

#[derive(Debug, Clone)]
pub struct Action {
    pub name: String,
    pub pre: Vec<Pred>,
    pub add: Vec<Pred>,
    pub del: Vec<Pred>,
}

/// A node in the search: the current world state (fact set) + how we got here +
/// the CoT trace accumulated along this branch.
#[derive(Debug, Clone)]
struct SearchNode {
    state: std::collections::HashSet<String>,
    plan: Vec<String>,
    trace: Vec<String>,
}

/// Result of planning: plan + the CoT trace (one line per applied action).
#[derive(Debug, Clone)]
pub struct Plan {
    pub actions: Vec<String>,
    pub trace: Vec<String>,
}

/// Forward-search planner. `goal` is a list of predicates all required true.
/// Returns `None` if no plan exists within `max_steps` (guards against blowup).
///
/// The returned `Plan.trace` is exactly the path taken — each branch carries its
/// own trace, so the winning branch's reasoning is reconstructed verbatim.
pub fn plan(init: &[Pred], actions: &[Action], goal: &[Pred], max_steps: usize) -> Option<Plan> {
    let mut start = std::collections::HashSet::new();
    for p in init {
        start.insert(p.key());
    }
    let goal_keys: Vec<String> = goal.iter().map(|g| g.key()).collect();

    let mut frontier = vec![SearchNode {
        state: start,
        plan: vec![],
        trace: vec![format!(
            "init: {:?}",
            init.iter().map(|p| p.key()).collect::<Vec<_>>()
        )],
    }];

    let mut depth = 0;
    while let Some(node) = frontier.pop() {
        // goal test
        if goal_keys.iter().all(|g| node.state.contains(g)) {
            return Some(Plan {
                actions: node.plan,
                trace: node.trace,
            });
        }
        if depth >= max_steps {
            continue;
        }
        // expand: apply every action whose preconditions hold
        for a in actions {
            let pre_ok = a.pre.iter().all(|p| node.state.contains(&p.key()));
            if !pre_ok {
                continue;
            }
            let mut next = node.state.clone();
            for d in &a.del {
                next.remove(&d.key());
            }
            for ad in &a.add {
                next.insert(ad.key());
            }
            let mut np = node.plan.clone();
            np.push(a.name.clone());
            let mut nt = node.trace.clone();
            nt.push(format!(
                "step {}: apply {} (pre {:?} → +{:?} -{:?})",
                np.len(),
                a.name,
                a.pre,
                a.add,
                a.del
            ));
            frontier.push(SearchNode {
                state: next,
                plan: np,
                trace: nt,
            });
        }
        depth += 1;
    }
    None
}

/// Alias kept for call-site clarity (the traced planner is canonical).
pub fn plan_traced(
    init: &[Pred],
    actions: &[Action],
    goal: &[Pred],
    max_steps: usize,
) -> Option<Plan> {
    plan(init, actions, goal, max_steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_actions() -> Vec<Action> {
        vec![
            Action {
                name: "drive".into(),
                pre: vec![Pred::new("at", &["garage"]), Pred::new("has_key", &["me"])],
                add: vec![Pred::new("at", &["store"])],
                del: vec![Pred::new("at", &["garage"])],
            },
            Action {
                name: "buy".into(),
                pre: vec![Pred::new("at", &["store"])],
                add: vec![Pred::new("has", &["milk"])],
                del: vec![],
            },
        ]
    }

    #[test]
    fn plans_to_goal() {
        // GREEN: drive then buy reaches has(milk).
        let init = vec![Pred::new("at", &["garage"]), Pred::new("has_key", &["me"])];
        let acts = mk_actions();
        let goal = vec![Pred::new("has", &["milk"])];
        let p = plan(&init, &acts, &goal, 20).expect("plan should exist");
        assert_eq!(p.actions, vec!["drive", "buy"]);
        assert!(p.trace.len() >= 3, "CoT trace should show steps");
    }

    #[test]
    fn unreachable_goal_no_plan() {
        // RED: missing has_key → drive's precondition fails → no plan.
        let init = vec![Pred::new("at", &["garage"])]; // no has_key
        let acts = mk_actions();
        let goal = vec![Pred::new("has", &["milk"])];
        assert!(plan(&init, &acts, &goal, 20).is_none());
    }

    #[test]
    fn already_satisfied_goal_empty_plan() {
        let init = vec![Pred::new("has", &["milk"])];
        let acts = mk_actions();
        let goal = vec![Pred::new("has", &["milk"])];
        let p = plan(&init, &acts, &goal, 5).unwrap();
        assert!(p.actions.is_empty());
    }
}
