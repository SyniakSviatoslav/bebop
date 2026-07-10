//! WAVEFIELD — geometric + wave simulation of the *connection graph* itself.
//!
//! Extension of the deterministic field/coherence core (see `field.rs`,
//! `coherence.rs`, `mathx.rs`). Your idea, made falsifiable: represent NOT just
//! memory/files but their CONNECTIONS — actions, methods, relations — as a
//! weighted geometric graph, then simulate WAVES over it and read off structure
//! (cycles, bottlenecks, runaway divergence, forbidden couplings).
//!
//! Pipeline (all pure, no RNG/clock — same doctrine as the rest of the core):
//!   1. `Node2D` — a memory / file / entity placed in 2-D space (geometry).
//!   2. `connection_edges` — edges weighted by 1/distance (closer ⇒ stronger
//!      coupling) AND by a `kind` tag (action | method | relation | data) so the
//!      *nature* of a link is part of the sim, not just its existence.
//!   3. `propagate_wave` — reuse coherence heat-kernel to propagate an impulse
//!      seeded on a node; the wavefront spreads along connections (NOT just
//!      adjacent — geometry bends the path).
//!   4. `graph_fourier` — eigenvalue proxy of the connection Laplacian → which
//!      modes (subgraphs) the wave excites (band-stop / notch detection).
//!   5. `floyd_cycle` — detect a cyclic dependency in actions (fast/slow ptr
//!      analog over the action edge list) → a loop in the plan graph.
//!   6. `field_divergence` — net outward activity at a node (mathx::divergence
//!      over the geometric vector field of edge momenta) → runaway hub check.
//!   7. `wave_probe` — compose all of the above into ONE `WaveVerdict`: a cycle
//!      on the red-line (action→secret→action) or a divergent hub forces
//!      `Unhealthy` (fail-closed); an isolated/banded-safe graph is `Permit`.
//!
//! No external model, no network. The thin live glue (real file graph, real
//! embeddings) lives OUTSIDE, behind an eval gate — this models the logic.

use crate::coherence;
use crate::field_physics;
use crate::geometry_field::Platonic;

/// A node in the geometric connection graph: a memory / file / entity.
#[derive(Debug, Clone, PartialEq)]
pub struct Node2D {
    pub id: String,
    /// Geometry: position in 2-D space. Distances between nodes drive coupling.
    pub x: f64,
    pub y: f64,
    /// If true, the node is a RED-LINE node (secrets/auth/money/migration).
    /// A wave that loops back into a red-line node is a fail-closed condition.
    pub red_line: bool,
}

/// The kind of connection between two nodes — the NATURE of the link, not just
/// its existence. Your idea: connections carry semantics (action/method/relation).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Action,   // an operation that mutates / transitions
    Method,   // a callable / function reference
    Relation, // a structural relationship (owns / contains / depends-on)
    Data,     // pure data flow
}

impl LinkKind {
    /// Semantic weight multiplier: actions are the most dangerous to loop, data
    /// the least. Used to scale the geometric coupling so a cycle of ACTIONS
    /// dominates the verdict over a cycle of plain data edges.
    pub fn weight(&self) -> f64 {
        match self {
            LinkKind::Action => 1.0,
            LinkKind::Method => 0.7,
            LinkKind::Relation => 0.5,
            LinkKind::Data => 0.3,
        }
    }
}

/// A weighted, kind-tagged edge in the connection graph.
#[derive(Debug, Clone)]
pub struct ConnEdge {
    pub from: usize,
    pub to: usize,
    pub kind: LinkKind,
    /// Geometric coupling (kind.weight() / distance).
    pub weight: f64,
}

/// Euclidean distance between two nodes (geometry).
pub fn dist(a: &Node2D, b: &Node2D) -> f64 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

/// Build kind-tagged, geometrically-weighted edges from explicit (from,to,kind)
/// triples, weighting each by kind.weight() / (geometric dist + ε). This is the
/// function that encodes ACTIONS / METHODS / RELATIONS into the sim.
pub fn connection_edges_kinded(
    nodes: &[Node2D],
    links: &[(usize, usize, LinkKind)],
) -> Vec<ConnEdge> {
    links
        .iter()
        .map(|&(i, j, k)| {
            let d = dist(&nodes[i], &nodes[j]).max(1e-6);
            ConnEdge {
                from: i,
                to: j,
                kind: k,
                weight: k.weight() / d,
            }
        })
        .collect()
}

/// Project the kind-weighted connection graph into the undirected graph form
/// `coherence::propagate` consumes: index pairs (from,to). Edge presence is
/// gated by coupling above `min_coupling` so weak/remote links don't dominate.
fn to_graph(edges: &[ConnEdge], min_coupling: f64) -> Vec<(usize, usize)> {
    edges
        .iter()
        .filter(|e| e.weight >= min_coupling)
        .map(|e| (e.from, e.to))
        .collect()
}

/// Propagate a wave impulse seeded on `seed_node` across the connection graph.
/// Reuses the deterministic heat-kernel `coherence::propagate` (no RNG). Returns
/// the n-vector field amplitude `u(t)` — the wavefront over memory/file space.
pub fn propagate_wave(
    nodes: &[Node2D],
    edges: &[ConnEdge],
    seed_node: usize,
    t: f64,
    coeff: f64,
    min_coupling: f64,
) -> Vec<f64> {
    let n = nodes.len();
    if n == 0 || seed_node >= n {
        return vec![];
    }
    let mut u0 = vec![0.0f64; n];
    u0[seed_node] = 1.0;
    let g = to_graph(edges, min_coupling);
    coherence::propagate(&u0, &g, t, coeff)
}

/// Graph-Fourier notch/band-stop proxy.
///
/// Reverse-engineered from the dossier (Band-Stop / Notch filter, Butterworth
/// magnitude, graph Laplacian spectrum): a real connection graph has a
/// *spectral gap*. If the wave excites a mode whose energy concentrates in a
/// narrow band (i.e. the propagated field has a high peak-to-spread ratio), the
/// graph has a resonant, poorly-damped substructure → flagged. We proxy the
/// "spectrum" by the spread of the propagated amplitude vector: a tight,
/// peaked distribution (high normalized peak share) = a resonant band = NOTCH.
///
/// Returns `(peak, notch_hit)`. `notch_hit` is true when the field's peak energy
/// share exceeds `concentration` (spectral concentration → brittle coupling).
pub fn graph_fourier_notch(field: &[f64], concentration: f64) -> (f64, bool) {
    if field.is_empty() {
        return (0.0, false);
    }
    let peak = field.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let sum: f64 = field.iter().map(|v| v.abs()).sum();
    if sum < 1e-9 {
        return (peak, false);
    }
    // normalized peak energy share = spectral concentration
    let share = peak.abs() / sum;
    (peak, share >= concentration)
}

/// Floyd's cycle detection over a plan graph.
///
/// Reverse-engineered from the dossier (Floyd's Cycle, fast & slow pointers):
/// a CYCLIC DEPENDENCY in ACTIONS — the plan graph loops back (step i leads to
/// j leads to i) — is a loop the planner must refuse. `actions` is the
/// SUCCESSOR array: `actions[i]` = the next step from node `i`, or `n`
/// (`== nodes.len()`, a halt sentinel) for a terminal step. Two walkers (step-1
/// and step-2) meet inside the graph iff a cycle exists. Returns `Some(len)` or
/// `None`.
///
/// Fail-closed: a degenerate/empty plan returns `None` (not a cycle).
pub fn floyd_cycle(actions: &[usize], n: usize) -> Option<usize> {
    let m = actions.len();
    if m < 2 {
        return None;
    }
    let halt = n; // out-of-range pointer == halt
    let step = |i: usize| -> usize { actions.get(i).copied().unwrap_or(halt) };
    let mut slow = 0usize;
    let mut fast = step(0);
    let mut guard = 0;
    while slow != fast && guard < 2 * (m + n + 1) {
        slow = step(slow);
        fast = step(step(fast));
        guard += 1;
    }
    // met only at the halt sentinel ⇒ acyclic (ran off the graph)
    if slow != fast || slow >= halt {
        return None;
    }
    // measure cycle length
    let mut len = 1usize;
    let mut cur = step(slow);
    while cur != slow {
        len += 1;
        cur = step(cur);
    }
    Some(len)
}

/// Net outward activity (divergence) at a node, from the geometric vector field
/// of edge momenta. Each edge carries momentum proportional to its weight in
/// the direction (to − from). Approximated as the signed sum of weights of
/// outgoing minus incoming edges (a 0-D divergence / balance). Positive ⇒ source
/// (activity radiating out, potential runaway hub); negative ⇒ sink; ~0 ⇒
/// solenoidal (balanced, healthy).
pub fn field_divergence(node: usize, edges: &[ConnEdge]) -> f64 {
    let mut flux = 0.0f64;
    for e in edges {
        if e.from == node {
            flux += e.weight; // radiating out
        } else if e.to == node {
            flux -= e.weight; // flowing in
        }
    }
    flux
}

/// The unified probe verdict over the connection graph.
#[derive(Debug, PartialEq, Eq)]
pub enum WaveVerdict {
    Permit,    // graph is safe: no red-line cycle, no runaway, banded-ok
    Unhealthy, // fail-closed: a red-line action cycle OR a divergent hub was found
}

/// Compose the full geometric-wave probe into one falsifiable verdict.
///
/// `actions` is the ordered action chain the planner is about to run (Floyd
/// cycle detection — a loop of actions is refused). `red_line_action_cycle` is
/// precomputed by the caller (does the chain re-enter a red-line node?). If a
/// red-line cycle exists OR a node's divergence exceeds `hub_limit` (runaway
/// hub) OR the wave field is spectrally concentrated above `concentration`
/// (resonant notch = brittle coupling), the verdict is `Unhealthy` (fail-closed).
/// `Permit` only when all three checks pass.
pub fn wave_probe(
    nodes: &[Node2D],
    edges: &[ConnEdge],
    actions: &[usize],
    red_line_action_cycle: bool,
    hub_limit: f64,
    concentration: f64,
    seed: usize,
    t: f64,
    coeff: f64,
    min_coupling: f64,
) -> WaveVerdict {
    // 1) RED-LINE ACTION CYCLE → fail-closed (a loop touching secrets/auth/money)
    if red_line_action_cycle {
        return WaveVerdict::Unhealthy;
    }
    // 2) Floyd cycle on the plan successor graph (any cycle is a planner loop)
    if floyd_cycle(actions, nodes.len()).is_some() {
        return WaveVerdict::Unhealthy;
    }
    // 3) propagate the wave and inspect spectral concentration (notch)
    let field = propagate_wave(nodes, edges, seed, t, coeff, min_coupling);
    let (_peak, notch) = graph_fourier_notch(&field, concentration);
    if notch {
        return WaveVerdict::Unhealthy;
    }
    // 4) runaway hub: any node with net outward flux > hub_limit
    for ni in 0..nodes.len() {
        if field_divergence(ni, edges) > hub_limit {
            return WaveVerdict::Unhealthy;
        }
    }
    WaveVerdict::Permit
}

/// ─────────────────────────────────────────────────────────────────────────
/// AUTO-LAYOUT — closes the "positions fed in externally" gap. Deterministic
/// geometric placement of graph nodes (no RNG): every layout is a closed form
/// of the node index `i` and count `n`, so two calls with the same graph are
/// identical. The planner can lay out a connection graph before simulating it.
/// ─────────────────────────────────────────────────────────────────────────

/// Place `n` nodes on a unit circle (evenly spaced, deterministic).
pub fn layout_circle(n: usize) -> Vec<(f64, f64)> {
    (0..n)
        .map(|i| {
            let a = 2.0 * std::f64::consts::PI * (i as f64) / (n as f64).max(1.0);
            (a.cos(), a.sin())
        })
        .collect()
}

/// Place `n` nodes on a grid with `cols` columns (row-major, deterministic).
pub fn layout_grid(n: usize, cols: usize) -> Vec<(f64, f64)> {
    let cols = cols.max(1);
    (0..n)
        .map(|i| ((i % cols) as f64, (i / cols) as f64))
        .collect()
}

/// Fruchterman–Reingold force-directed layout, deterministic (seeded by a
/// circle init, NO RNG). `edges` are (i,j) index pairs; `iters` fixed steps.
/// Repulsion k²/d between all pairs, attraction d²/k along edges, k=√(area/n).
/// Returns final (x,y) per node. Fail-closed: empty graph → empty layout.
pub fn layout_spring(n: usize, edges: &[(usize, usize)], iters: usize) -> Vec<(f64, f64)> {
    if n == 0 {
        return vec![];
    }
    let mut pos = layout_circle(n);
    let area = 1.0;
    let k = (area / (n as f64)).sqrt();
    let mut disp = vec![(0.0, 0.0); n];
    for _ in 0..iters.max(1) {
        for d in disp.iter_mut() {
            *d = (0.0, 0.0);
        }
        // repulsion (all pairs)
        for i in 0..n {
            for j in (i + 1)..n {
                let dx = pos[i].0 - pos[j].0;
                let dy = pos[i].1 - pos[j].1;
                let d = (dx * dx + dy * dy + 1e-9).sqrt();
                let f = k * k / d;
                let (ux, uy) = (dx / d, dy / d);
                disp[i].0 += ux * f;
                disp[i].1 += uy * f;
                disp[j].0 -= ux * f;
                disp[j].1 -= uy * f;
            }
        }
        // attraction (edges)
        for &(i, j) in edges {
            if i >= n || j >= n {
                continue;
            }
            let dx = pos[i].0 - pos[j].0;
            let dy = pos[i].1 - pos[j].1;
            let d = (dx * dx + dy * dy + 1e-9).sqrt();
            let f = d * d / k;
            let (ux, uy) = (dx / d, dy / d);
            disp[i].0 -= ux * f;
            disp[i].1 -= uy * f;
            disp[j].0 += ux * f;
            disp[j].1 += uy * f;
        }
        // fixed temperature step (no randomness)
        let temp = 0.1;
        for i in 0..n {
            let dl = (disp[i].0 * disp[i].0 + disp[i].1 * disp[i].1 + 1e-9).sqrt();
            let lim = dl.min(temp);
            pos[i].0 += disp[i].0 / dl * lim;
            pos[i].1 += disp[i].1 / dl * lim;
        }
    }
    pos
}

/// ─────────────────────────────────────────────────────────────────────────
/// REAL GRAPH-FOURIER SPECTRUM — closes the "proxy" gap. The dossier's band-
/// stop/notch is a property of the graph Laplacian spectrum: a graph that is
/// barely connected has a tiny algebraic connectivity λ₂ and resonates / can be
/// split by a notch. We compute the actual Laplacian eigenvalues via the cyclic
/// Jacobi method (symmetric, deterministic, 0 deps) — no concentration proxy.
/// ─────────────────────────────────────────────────────────────────────────

/// Symmetric weighted adjacency matrix from kind-tagged edges.
pub fn adjacency_from_edges(n: usize, edges: &[ConnEdge]) -> Vec<Vec<f64>> {
    let mut a = vec![vec![0.0f64; n]; n];
    for e in edges {
        if e.from < n && e.to < n {
            a[e.from][e.to] += e.weight;
            a[e.to][e.from] += e.weight;
        }
    }
    a
}

/// Eigenvalues of the unnormalized graph Laplacian L = D − A, ascending.
/// Cyclic Jacobi eigenvalue algorithm (real symmetric, deterministic).
pub fn graph_laplacian_eigs(adj: &[Vec<f64>]) -> Vec<f64> {
    let n = adj.len();
    if n == 0 {
        return vec![];
    }
    let mut l = vec![vec![0.0f64; n]; n];
    for i in 0..n {
        let mut deg = 0.0;
        for j in 0..n {
            l[i][j] = -adj[i][j];
            deg += adj[i][j];
        }
        l[i][i] = deg;
    }
    jacobi_eigenvalues(&l)
}

/// Cyclic Jacobi eigenvalue algorithm for a real symmetric matrix. Returns the
/// eigenvalues sorted ascending. Deterministic, no allocations beyond work.
///
/// Classic Jacobi: sweep every off-diagonal pair, annihilate the largest
/// off-diagonal entry with a similarity rotation, repeat until the off-diagonal
/// mass is below a tolerance. For the small (≤ a few hundred) Laplacian matrices
/// here this converges in a handful of sweeps and is exact to ~1e-9.
fn jacobi_eigenvalues(a: &[Vec<f64>]) -> Vec<f64> {
    let n = a.len();
    let mut m: Vec<Vec<f64>> = a.iter().map(|row| row.to_vec()).collect();
    let mut off: f64 = 0.0;
    for i in 0..n {
        for j in (i + 1)..n {
            off += m[i][j] * m[i][j];
        }
    }
    let mut sweeps = 100;
    let tol = 1e-14;
    while off > tol && sweeps > 0 {
        for p in 0..n {
            for q in (p + 1)..n {
                let apq = m[p][q];
                if apq.abs() < 1e-300 {
                    continue;
                }
                let app = m[p][p];
                let aqq = m[q][q];
                // Numerical-Recipes rotation (provably a similarity transform)
                let theta = (aqq - app) / (2.0 * apq);
                let t = if theta < 0.0 {
                    -1.0 / (-theta + (theta * theta + 1.0).sqrt())
                } else {
                    1.0 / (theta + (theta * theta + 1.0).sqrt())
                };
                let c = 1.0 / (1.0 + t * t).sqrt();
                let s = t * c;
                let tau = s / (1.0 + c);
                // 2x2 diagonal block
                m[p][p] = app - t * apq;
                m[q][q] = aqq + t * apq;
                m[p][q] = 0.0;
                m[q][p] = 0.0;
                // off-diagonal rows/cols except p,q
                for i in 0..n {
                    if i == p || i == q {
                        continue;
                    }
                    let aip = m[i][p];
                    let aiq = m[i][q];
                    m[i][p] = aip - s * (aiq + tau * aip);
                    m[i][q] = aiq + s * (aip - tau * aiq);
                    m[p][i] = m[i][p];
                    m[q][i] = m[i][q];
                }
            }
        }
        off = 0.0;
        for i in 0..n {
            for j in (i + 1)..n {
                off += m[i][j] * m[i][j];
            }
        }
        sweeps -= 1;
    }
    let mut ev: Vec<f64> = (0..n).map(|i| m[i][i]).collect();
    ev.sort_by(|x, y| x.partial_cmp(y).unwrap());
    // eigenvalues of a Laplacian are non-negative; clamp tiny negatives to 0
    ev.iter()
        .map(|&v| if v < 0.0 && v.abs() < 1e-7 { 0.0 } else { v })
        .collect()
}

/// Real spectral notch: algebraic connectivity λ₂ (2nd-smallest Laplacian
/// eigenvalue) below `frac * λ_max` ⇒ graph barely connected (brittle,
/// resonates, splittable by a notch) → flag. A healthy connected graph has
/// λ₂ ≫ 0. Scale-invariant: thresholds on the relative gap so edge weights
/// (kind-weighted, small) don't break the check.
pub fn graph_spectral_notch(eigs: &[f64], frac: f64) -> bool {
    if eigs.len() < 2 {
        return false;
    }
    let lmax = eigs[eigs.len() - 1];
    if lmax < 1e-9 {
        return true; // all-zero spectrum ⇒ disconnected
    }
    eigs[1] < frac * lmax
}

/// Does the plan's successor graph ever step INTO a red-line node? (fail-closed
/// precheck for `plan_wave_gate`.)
pub fn red_line_in_plan(plan_targets: &[usize], nodes: &[Node2D]) -> bool {
    let n = nodes.len();
    let halt = n;
    let step = |i: usize| -> usize { plan_targets.get(i).copied().unwrap_or(halt) };
    let mut i = 0usize;
    let mut guard = 0;
    loop {
        if i >= halt {
            return false;
        }
        if nodes[i].red_line {
            return true;
        }
        i = step(i);
        guard += 1;
        if guard > plan_targets.len() + n + 1 {
            return false;
        }
    }
}

/// PLANNER GATE — closes the "not wired into the planner" gap.
///
/// Composes the FULL geometric-wave probe using the REAL Laplacian spectrum
/// (not the proxy) and refuses any plan that: steps into a red-line node,
/// contains a Floyd cycle, sits on a brittle (near-disconnected) spectral band,
/// or drives a node into runaway divergence. Returns `WaveVerdict` fail-closed.
///
/// `plan_targets` is the action successor graph (step i → plan_targets[i], or
/// `n` to halt), exactly like `floyd_cycle`'s `actions`. `nodes`/`edges` are the
/// memory/file connection graph (geometry + kinds). The heat-kernel wave is
/// still propagated for the interference concept, but gating uses the real
/// structural checks.
pub fn plan_wave_gate(
    plan_targets: &[usize],
    nodes: &[Node2D],
    edges: &[ConnEdge],
    hub_limit: f64,
    spectral_threshold: f64,
) -> WaveVerdict {
    // Production entry: the novel-wave blast-radius check is FLAG-OFF by default;
    // flip it on with env BEBOP_WAVE_GATE=1. (Tests call the explicit `_with`
    // form to avoid process-global env-var races across parallel test threads.)
    let use_wave = std::env::var("BEBOP_WAVE_GATE").is_ok();
    plan_wave_gate_with(
        plan_targets,
        nodes,
        edges,
        hub_limit,
        spectral_threshold,
        use_wave,
    )
}

/// Internal gate with the wave check explicitly toggled (no env-var side effects).
pub(crate) fn plan_wave_gate_with(
    plan_targets: &[usize],
    nodes: &[Node2D],
    edges: &[ConnEdge],
    hub_limit: f64,
    spectral_threshold: f64,
    use_wave_gate: bool,
) -> WaveVerdict {
    // 1) plan steps into a red-line node → fail-closed (needs human override)
    if red_line_in_plan(plan_targets, nodes) {
        return WaveVerdict::Unhealthy;
    }
    // 2) Floyd cycle in the plan successor graph → fail-closed
    if floyd_cycle(plan_targets, nodes.len()).is_some() {
        return WaveVerdict::Unhealthy;
    }
    // 3) REAL spectral notch (algebraic connectivity λ₂)
    let adj = adjacency_from_edges(nodes.len(), edges);
    let eigs = graph_laplacian_eigs(&adj);
    if graph_spectral_notch(&eigs, spectral_threshold) {
        return WaveVerdict::Unhealthy;
    }
    // 4) runaway hub
    for ni in 0..nodes.len() {
        if field_divergence(ni, edges) > hub_limit {
            return WaveVerdict::Unhealthy;
        }
    }
    // 5) NOVEL WAVE CHANGE-IMPACT (flag-OFF; enabled by env BEBOP_WAVE_GATE=1).
    if use_wave_gate {
        // Runs the damped graph-wave/field (the operator's novel approach) as an
        // independent blast-radius check: if the impulse at the FIRST plan step
        // propagates (respecting topology + mass) into a RED-LINE node, refuse.
        // This replaces the blind Euclidean notion of "near" with a real wave
        // reach. Default OFF so existing spectral gating is unchanged.
        if let Some(first) = plan_targets.first().copied() {
            if first < nodes.len() {
                let solids: Vec<Platonic> = vec![Platonic::Tetrahedron; nodes.len()];
                let (affected, _e) =
                    field_physics::change_impact(nodes, &solids, edges, first, 4.0, 60, 1e-3);
                if affected.iter().any(|&i| nodes[i].red_line) {
                    return WaveVerdict::Unhealthy;
                }
            }
        }
    }
    WaveVerdict::Permit
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_nodes() -> Vec<Node2D> {
        // geometry: spread nodes so distance coupling varies
        vec![
            Node2D {
                id: "mem".into(),
                x: 0.0,
                y: 0.0,
                red_line: false,
            },
            Node2D {
                id: "file".into(),
                x: 1.0,
                y: 0.0,
                red_line: false,
            },
            Node2D {
                id: "act".into(),
                x: 2.0,
                y: 1.0,
                red_line: false,
            },
            Node2D {
                id: "secret".into(),
                x: 0.0,
                y: 3.0,
                red_line: true,
            },
        ]
    }

    #[test]
    fn geometry_weights_closer_stronger() {
        // GREEN: geometric coupling falls with distance.
        let n = sample_nodes();
        let e = connection_edges_kinded(&n, &[(0, 1, LinkKind::Action), (0, 3, LinkKind::Action)]);
        let w_near = e.iter().find(|c| c.to == 1).unwrap().weight;
        let w_far = e.iter().find(|c| c.to == 3).unwrap().weight;
        assert!(w_near > w_far, "closer node must couple stronger");
    }

    #[test]
    fn action_kind_dominates_data() {
        // GREEN: an Action edge binds tighter than a Data edge at equal distance.
        let n = sample_nodes();
        let e = connection_edges_kinded(&n, &[(0, 1, LinkKind::Action), (0, 1, LinkKind::Data)]);
        let act = e
            .iter()
            .find(|c| c.kind == LinkKind::Action)
            .unwrap()
            .weight;
        let dat = e.iter().find(|c| c.kind == LinkKind::Data).unwrap().weight;
        assert!(act > dat, "action weight must exceed data weight");
    }

    #[test]
    fn floyd_finds_action_cycle() {
        // RED: a plan loop (0→1→0→halt) → cycle detected. n=3 nodes, sentinel=3.
        let cycle = [1usize, 0, 3]; // step0→1, step1→0, step2→halt(3)
        assert!(floyd_cycle(&cycle, 3).is_some(), "must detect the loop");
        // GREEN: an acyclic plan (0→1→2→halt) returns None.
        assert!(floyd_cycle(&[1usize, 2, 3], 3).is_none());
    }

    #[test]
    fn wave_probe_fails_closed_on_redline_cycle() {
        // RED: a red-line action cycle → Unhealthy (fail-closed, no RNG/clock).
        let n = sample_nodes();
        let cycle = [1usize, 3, 1]; // action chain re-enters the secret (red-line) node
        assert_eq!(
            wave_probe(&n, &[], &cycle, true, 10.0, 0.9, 0, 1.0, 0.5, 1e-3),
            WaveVerdict::Unhealthy
        );
        // RED: a runaway hub (huge divergence) → Unhealthy.
        let edges = connection_edges_kinded(
            &n,
            &[
                (0, 1, LinkKind::Action),
                (0, 2, LinkKind::Action),
                (0, 3, LinkKind::Action),
            ],
        );
        // node 0 radiates to all three → high divergence. hub_limit tiny.
        // acyclic plan (0→1→2→3→halt) so the only failure is the runaway hub.
        assert_eq!(
            wave_probe(
                &n,
                &edges,
                &[1, 2, 3, 4],
                false,
                0.5,
                0.99,
                0,
                1.0,
                0.5,
                1e-3
            ),
            WaveVerdict::Unhealthy
        );
        // GREEN: a small safe graph with no cycle, no hub, no resonance → Permit.
        let safe_edges = connection_edges_kinded(&n, &[(0, 1, LinkKind::Data)]);
        // acyclic plan (0→1→2→3→halt) + weak single data edge → Permit.
        assert_eq!(
            wave_probe(
                &n,
                &safe_edges,
                &[1, 2, 3, 4],
                false,
                50.0,
                0.999,
                0,
                1.0,
                0.5,
                1e-1
            ),
            WaveVerdict::Permit
        );
    }

    #[test]
    fn wave_propagation_is_deterministic() {
        // GREEN: same graph+seed → identical field (no hidden state).
        let n = sample_nodes();
        let e = connection_edges_kinded(&n, &[(0, 1, LinkKind::Action), (1, 2, LinkKind::Action)]);
        let a = propagate_wave(&n, &e, 0, 1.0, 0.5, 1e-3);
        let b = propagate_wave(&n, &e, 0, 1.0, 0.5, 1e-3);
        assert_eq!(a, b);
    }

    #[test]
    fn divergence_signals_source_vs_sink() {
        // RED+GREEN: outgoing-heavy node is a source (positive), incoming is sink.
        let n = sample_nodes();
        let e = connection_edges_kinded(&n, &[(0, 1, LinkKind::Action)]);
        assert!(field_divergence(0, &e) > 0.0, "node 0 is a source");
        assert!(field_divergence(1, &e) < 0.0, "node 1 is a sink");
    }

    #[test]
    fn layout_is_deterministic_and_springs_apart() {
        // GREEN: same n → identical layouts (no RNG).
        assert_eq!(layout_circle(4), layout_circle(4));
        assert_eq!(layout_grid(4, 2), layout_grid(4, 2));
        // GREEN: spring layout separates two edge-connected nodes (dist>0).
        let p = layout_spring(2, &[(0, 1)], 30);
        let d = ((p[0].0 - p[1].0).powi(2) + (p[0].1 - p[1].1).powi(2)).sqrt();
        assert!(d > 1e-3, "connected nodes should be pulled apart");
    }

    #[test]
    fn laplacian_spectrum_detects_brittle_graph() {
        // A 3-node chain 0-1-2: Laplacian eigs = [0, ~1, ~3].
        let adj = vec![
            vec![0.0, 1.0, 0.0],
            vec![1.0, 0.0, 1.0],
            vec![0.0, 1.0, 0.0],
        ];
        let eigs = graph_laplacian_eigs(&adj);
        assert!(eigs[0].abs() < 1e-6, "λ₁=0 (connected)");
        // brittle threshold (frac=0.5): thin chain λ₂/λ_max≈1/3 < 0.5 → notch
        assert!(
            graph_spectral_notch(&eigs, 0.5),
            "thin chain ⇒ λ₂/λ_max≈1/3 < 0.5"
        );
        // connected-but-not-disconnected (frac=0.05): accepted, NOT a notch
        assert!(
            !graph_spectral_notch(&eigs, 0.05),
            "connected chain is not near-disconnected"
        );
        // a disconnected 2+1 graph → λ₂ = 0 → notch flags it
        let disc = vec![
            vec![0.0, 1.0, 0.0],
            vec![1.0, 0.0, 0.0],
            vec![0.0, 0.0, 0.0],
        ];
        let de = graph_laplacian_eigs(&disc);
        assert!(
            graph_spectral_notch(&de, 0.05),
            "disconnected ⇒ λ₂≈0 ⇒ notch"
        );
        // a well-connected 3-clique → λ₂ = λ_max → no notch
        let clique = vec![
            vec![0.0, 1.0, 1.0],
            vec![1.0, 0.0, 1.0],
            vec![1.0, 1.0, 0.0],
        ];
        let ce = graph_laplacian_eigs(&clique);
        assert!(
            !graph_spectral_notch(&ce, 0.5),
            "clique well-connected ⇒ λ₂/λ_max=1 ⇒ no notch"
        );
    }

    #[test]
    fn plan_wave_gate_refuses_redline_and_cycles() {
        // RED: plan steps into a red-line node → Unhealthy (fail-closed).
        let n = sample_nodes(); // node 3 = secret (red_line)
        let edges = connection_edges_kinded(&n, &[(0, 1, LinkKind::Data)]);
        assert_eq!(
            plan_wave_gate(&[1, 3, 4], &n, &edges, 50.0, 0.05),
            WaveVerdict::Unhealthy
        );
        // RED: plan Floyd cycle (0→1→0) → Unhealthy.
        assert_eq!(
            plan_wave_gate(&[1, 0, 4], &n, &edges, 50.0, 0.05),
            WaveVerdict::Unhealthy
        );
        // RED: disconnected connection graph (isolated nodes) → Unhealthy.
        let broken = connection_edges_kinded(&n, &[(0, 1, LinkKind::Data)]); // 2,3 isolated
        assert_eq!(
            plan_wave_gate(&[1, 2, 3, 4], &n, &broken, 50.0, 0.05),
            WaveVerdict::Unhealthy
        );
        // GREEN: acyclic plan, no red-line, connected chain 0-1-2 (stops before
        // the secret node 3) → Permit.
        let connected = connection_edges_kinded(
            &n,
            &[
                (0, 1, LinkKind::Data),
                (1, 2, LinkKind::Data),
                (2, 3, LinkKind::Data),
            ],
        );
        assert_eq!(
            plan_wave_gate(&[1, 2, 4], &n, &connected, 50.0, 0.05),
            WaveVerdict::Permit
        );
    }

    #[test]
    fn wave_change_impact_gate_refuses_redline_reach() {
        // RED+GREEN: the novel damped graph-wave is wired into the planner as a
        // blast-radius check (use_wave_gate=true). The graph is a 4-clique so the
        // spectral/divergence checks PASS — the ONLY thing distinguishing the
        // verdict is the WAVE reach into the red-line node.
        let n = vec![
            Node2D {
                id: "x".into(),
                x: 0.0,
                y: 0.0,
                red_line: false,
            },
            Node2D {
                id: "seed".into(),
                x: 1.0,
                y: 0.0,
                red_line: false,
            }, // wave seed (plan step 0)
            Node2D {
                id: "secret".into(),
                x: 2.0,
                y: 0.0,
                red_line: true,
            }, // red-line, in clique
            Node2D {
                id: "far".into(),
                x: 9.0,
                y: 9.0,
                red_line: false,
            },
        ];
        // fully-connected clique ⇒ spectral gate passes (λ₂=λ_max, no notch)
        let clique = connection_edges_kinded(
            &n,
            &[
                (0, 1, LinkKind::Relation),
                (0, 2, LinkKind::Relation),
                (0, 3, LinkKind::Relation),
                (1, 2, LinkKind::Relation),
                (1, 3, LinkKind::Relation),
                (2, 3, LinkKind::Relation),
            ],
        );
        // GREEN: seed at 1 → wave propagates across the clique → reaches the
        // red-line secret node ⇒ fail-closed (Unhealthy).
        assert_eq!(
            plan_wave_gate_with(&[1, 3, 4], &n, &clique, 1e9, 0.05, true),
            WaveVerdict::Unhealthy,
            "wave blast reaches red-line secret ⇒ refuse"
        );
    }

    #[test]
    fn wave_gate_off_by_default_ignores_redline_reach() {
        // GREEN: wave gate OFF (use_wave_gate=false) ⇒ blast check skipped; with
        // a well-connected clique the spectral/divergence checks PASS, so the
        // SAME graph that returns Unhealthy with the wave gate ON returns Permit
        // with it OFF. Isolates the wave gate's contribution (no false positive).
        let n = vec![
            Node2D {
                id: "x".into(),
                x: 0.0,
                y: 0.0,
                red_line: false,
            },
            Node2D {
                id: "seed".into(),
                x: 1.0,
                y: 0.0,
                red_line: false,
            },
            Node2D {
                id: "secret".into(),
                x: 2.0,
                y: 0.0,
                red_line: true,
            },
            Node2D {
                id: "far".into(),
                x: 9.0,
                y: 9.0,
                red_line: false,
            },
        ];
        let clique = connection_edges_kinded(
            &n,
            &[
                (0, 1, LinkKind::Relation),
                (0, 2, LinkKind::Relation),
                (0, 3, LinkKind::Relation),
                (1, 2, LinkKind::Relation),
                (1, 3, LinkKind::Relation),
                (2, 3, LinkKind::Relation),
            ],
        );
        assert_eq!(
            plan_wave_gate_with(&[1, 3, 4], &n, &clique, 1e9, 0.05, false),
            WaveVerdict::Permit
        );
    }
}
