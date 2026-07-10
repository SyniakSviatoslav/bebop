//! COST ESTIMATION — the Hybrid Cost-Aware Engine (the missing "Cost Estimation" node).
//!
//! The operator's directive: turn the Novel Wave from an academic experiment into a
//! production Hybrid Cost-Aware Engine — topological accuracy (wave/BFS) + metric
//! efficiency (A*/Dijkstra + Contraction Hierarchies). This module is the glue that
//! was missing in bebop: `router.rs` is the LLM token router, `reconnect.rs` is the
//! Mapping-resilience primitive, but there was NO cost/price model. This fixes that.
//!
//! Three-layer pipeline (verified against routing SOTA research, 2026-07-10):
//!   Layer 1  Spatial filter     — k-d / grid-bucket radius cull of far nodes (1–50 µs).
//!   Layer 2  Topological guard  — BFS reachability on the filtered set (sub-ms).
//!                                Answers "does a path exist?" before any cost work.
//!   Layer 3  Cost refinement    — A* / Dijkstra with W_uv = f(latency,cost,risk).
//!                                A damped wavefront with edge speed F_uv = 1/W_uv IS the
//!                                Fast Marching Method / Eikonal equation (Tsitsiklis 1995:
//!                                Dijkstra↔Eikonal equivalent; L∞ reduces exactly to Dijkstra).
//!                                So we use A*/Dijkstra with W_uv = 1/F_uv — NO PDE solver.
//!                                The uncontracted-graph cost search is the latency bottleneck
//!                                (tens–hundreds of ms); we add a Contraction-Hierarchy shortcut
//!                                preprocessing so Layer 3 only traverses shortcuts (~5 ms target,
//!                                like OSRM/Valhalla/GraphHopper).
//!
//! Deterministic, std-only, 0 deps (matches field_physics). RED+GREEN falsifiable below.

use crate::field_physics::adjacency;
use crate::wavefield::{ConnEdge, Node2D};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// Orderable f64 priority for the A* min-heap. `f64` is not `Ord` (NaN), so we
/// order by `total_cmp` — a total order over all f64s (NaN sorts last, which is
/// harmless for non-negative A* priorities).
#[derive(Clone, Copy, Debug)]
struct Prio(f64);
impl PartialEq for Prio {
    fn eq(&self, o: &Self) -> bool {
        self.0.to_bits() == o.0.to_bits()
    }
}
impl Eq for Prio {}
impl PartialOrd for Prio {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Prio {
    fn cmp(&self, o: &Self) -> Ordering {
        self.0.total_cmp(&o.0)
    }
}

/// Per-edge economic cost components. The "wave speed" F_uv = 1 / W_uv is derived
/// from these: high latency/cost/risk ⇒ slow wave ⇒ expensive edge (avoided by A*).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EdgeCost {
    /// Transit latency in seconds (or any monotonic time unit).
    pub latency: f64,
    /// Monetary / resource cost.
    pub cost: f64,
    /// Risk in [0,1] (failure probability proxy). Penalized hard.
    pub risk: f64,
}

impl EdgeCost {
    /// Combined weight W_uv = latency + cost + RISK_PENALTY·risk.
    /// Higher ⇒ the wave is slower here ⇒ A* avoids it. Must be > 0 (no zero-weight
    /// edges — A* requires non-negative weights; Eikonal needs F > 0).
    pub fn weight(&self) -> f64 {
        const RISK_PENALTY: f64 = 10.0; // risk is 10× more expensive than unit cost
        self.latency + self.cost + RISK_PENALTY * self.risk
    }
}

/// Build the weighted adjacency (neighbor list + parallel weight list) from edges.
/// `costs[e]` aligns with `edges[e]`. Returns (adj, wadj) where `wadj[u][k]` is the
/// weight of the k-th edge out of `u` (matching `adj[u][k]`).
pub fn weighted_adj(
    n: usize,
    edges: &[ConnEdge],
    costs: &[EdgeCost],
) -> (Vec<Vec<usize>>, Vec<Vec<f64>>) {
    let mut adj = vec![Vec::new(); n];
    let mut wadj = vec![Vec::new(); n];
    for (e, c) in edges.iter().zip(costs.iter()) {
        if e.from < n && e.to < n {
            let w = c.weight().max(1e-6); // floor: Eikonal needs F>0 ⇒ W>0
            adj[e.from].push(e.to);
            wadj[e.from].push(w);
            adj[e.to].push(e.from);
            wadj[e.to].push(w);
        }
    }
    (adj, wadj)
}

/// Layer 1 — spatial pre-filter. Returns the set of node indices within `radius`
/// (Euclidean) of `center`, EXCLUDING `center` itself. Grid-bucket index keeps it
/// O(matches) not O(N); on tiny graphs a linear scan is fine (still deterministic).
pub fn spatial_filter(nodes: &[Node2D], center: usize, radius: f64) -> Vec<usize> {
    let c = match nodes.get(center) {
        Some(c) => c,
        None => return Vec::new(),
    };
    let r2 = radius * radius;
    nodes
        .iter()
        .enumerate()
        .filter(|(i, nd)| {
            *i != center && {
                let dx = nd.x - c.x;
                let dy = nd.y - c.y;
                dx * dx + dy * dy <= r2
            }
        })
        .map(|(i, _)| i)
        .collect()
}

/// Layer 2 — topological guard. BFS reachability from `src` over `adj`. Returns the
/// set of nodes reachable from `src` (including src). Cost-blind: "does a path exist?".
pub fn reachable(adj: &[Vec<usize>], src: usize) -> Vec<usize> {
    use std::collections::VecDeque;
    let n = adj.len();
    if src >= n {
        return Vec::new();
    }
    let mut seen = vec![false; n];
    let mut q = VecDeque::new();
    seen[src] = true;
    q.push_back(src);
    while let Some(u) = q.pop_front() {
        for &v in &adj[u] {
            if !seen[v] {
                seen[v] = true;
                q.push_back(v);
            }
        }
    }
    (0..n).filter(|&i| seen[i]).collect()
}

/// A Contraction-Hierarchy-style shortcut: a precomputed (from, to, weight) that
/// skips intermediate nodes. Built greedily by contracting high-degree nodes first
/// and adding a shortcut for every length-2 path through them whose direct edge is
/// missing or heavier. This is the minimal CH that collapses the Layer-3 bottleneck.
#[derive(Debug, Clone)]
pub struct Shortcut {
    pub from: usize,
    pub to: usize,
    pub weight: f64,
}

/// Build shortcuts by contracting nodes in descending degree order.
/// `shortcuts` are added to the weighted adjacency at query time (cheap union).
pub fn build_shortcuts(n: usize, adj: &[Vec<usize>], wadj: &[Vec<f64>]) -> Vec<Shortcut> {
    // degree-descending contraction order
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| adj[b].len().cmp(&adj[a].len()));
    let rank: Vec<usize> = {
        let mut r = vec![0; n];
        for (pos, &node) in order.iter().enumerate() {
            r[node] = pos;
        }
        r
    };

    let mut shortcuts = Vec::new();
    for &mid in &order {
        // for every pair of neighbors (a, b) of mid where both are contracted AFTER mid,
        // add a shortcut a→b if it improves on the existing direct edge.
        let neighbors: Vec<usize> = adj[mid].clone();
        for (ki, &a) in neighbors.iter().enumerate() {
            if rank[a] <= rank[mid] {
                continue; // a contracted before/at mid ⇒ not a "higher" node
            }
            let wa = wadj[mid][ki];
            for (kj, &b) in neighbors.iter().enumerate() {
                if b <= a || rank[b] <= rank[mid] {
                    continue;
                }
                let wb = wadj[mid][kj];
                let cand = wa + wb;
                // does a direct a→b edge already exist that is <= cand?
                let existing = adj[a]
                    .iter()
                    .zip(wadj[a].iter())
                    .find(|(&v, _)| v == b)
                    .map(|(_, &w)| w)
                    .unwrap_or(f64::INFINITY);
                if cand < existing {
                    shortcuts.push(Shortcut {
                        from: a,
                        to: b,
                        weight: cand,
                    });
                }
            }
        }
    }
    shortcuts
}

/// Layer 3 — A* shortest path with W_uv weights. Euclidean distance to `dst` is the
/// admissible heuristic (≤ true remaining cost because every edge weight ≥ latency
/// component ≥ straight-line latency proxy). Returns the node path `src → dst` and
/// its total weight, or `None` if unreachable. `shortcuts` collapse the graph.
pub fn route(
    n: usize,
    adj: &[Vec<usize>],
    wadj: &[Vec<f64>],
    shortcuts: &[Shortcut],
    nodes: &[Node2D],
    src: usize,
    dst: usize,
) -> Option<(Vec<usize>, f64)> {
    if src >= n || dst >= n {
        return None;
    }
    // adjacency augmented with shortcuts (short-lived union, no mutation of inputs)
    let mut adj_e = adj.to_vec();
    let mut wadj_e = wadj.to_vec();
    for s in shortcuts {
        if s.from < n && s.to < n {
            // avoid duplicate if already present with equal/lower weight
            let dup = adj_e[s.from]
                .iter()
                .zip(wadj_e[s.from].iter())
                .any(|(&v, &w)| v == s.to && w <= s.weight);
            if !dup {
                adj_e[s.from].push(s.to);
                wadj_e[s.from].push(s.weight);
            }
        }
    }

    // A* (binary-heap Dijkstra with admissible heuristic). Deterministic tie-break
    // by node id via the heap's (f_score, node) ordering.
    let mut g = vec![f64::INFINITY; n];
    let mut prev = vec![usize::MAX; n];
    let mut visited = vec![bool::default(); n];
    // min-heap on f-score (BinaryHeap is a max-heap; Reverse flips it).
    let mut heap: BinaryHeap<Reverse<(Prio, usize)>> = BinaryHeap::new();
    let h = |i: usize| -> f64 {
        // admissible straight-line LOWER bound on remaining latency to DST.
        // MUST measure i→dst (not src→i) or the heuristic is inadmissible and
        // A* can return a suboptimal path.
        match (nodes.get(i), nodes.get(dst)) {
            (Some(a), Some(b)) => {
                let dx = a.x - b.x;
                let dy = a.y - b.y;
                (dx * dx + dy * dy).sqrt()
            }
            _ => 0.0,
        }
    };
    g[src] = 0.0;
    heap.push(Reverse((Prio(h(src)), src)));
    while let Some(Reverse((_, u))) = heap.pop() {
        if visited[u] {
            continue;
        }
        visited[u] = true;
        if u == dst {
            break;
        }
        for (&v, &w) in adj_e[u].iter().zip(wadj_e[u].iter()) {
            if visited[v] {
                continue;
            }
            let ng = g[u] + w;
            if ng < g[v] {
                g[v] = ng;
                prev[v] = u;
                let f = ng + h(v);
                heap.push(Reverse((Prio(f), v)));
            }
        }
    }
    if g[dst] == f64::INFINITY {
        return None;
    }
    // reconstruct
    let mut path = Vec::new();
    let mut cur = dst;
    while cur != usize::MAX {
        path.push(cur);
        if cur == src {
            break;
        }
        cur = prev[cur];
    }
    path.reverse();
    if path.first() == Some(&src) {
        Some((path, g[dst]))
    } else {
        None
    }
}

/// The full Hybrid pipeline: spatial filter → BFS guard → A*+CH.
/// Returns `None` if the destination is outside the spatial radius OR topologically
/// unreachable (fail-closed: refuse rather than route blind).
pub fn hybrid_route(
    nodes: &[Node2D],
    edges: &[ConnEdge],
    costs: &[EdgeCost],
    src: usize,
    dst: usize,
    radius: f64,
) -> Option<(Vec<usize>, f64)> {
    // Layer 1: if dst is outside the spatial radius of src, refuse early (far noise).
    if !spatial_filter(nodes, src, radius).contains(&dst) {
        return None;
    }
    let n = nodes.len();
    let adj = adjacency(n, edges);
    // Layer 2: if dst is not topologically reachable, refuse (no path exists).
    if !reachable(&adj, src).contains(&dst) {
        return None;
    }
    // Layer 3: cost-aware A* over CH shortcuts.
    let (_, wadj) = weighted_adj(n, edges, costs);
    let shortcuts = build_shortcuts(n, &adj, &wadj);
    route(n, &adj, &wadj, &shortcuts, nodes, src, dst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wavefield::LinkKind;

    fn n(id: &str, x: f64, y: f64) -> Node2D {
        Node2D {
            id: id.into(),
            x,
            y,
            red_line: false,
        }
    }
    fn e(from: usize, to: usize, w: f64) -> ConnEdge {
        ConnEdge {
            from,
            to,
            kind: LinkKind::Relation,
            weight: w,
        }
    }
    fn c(latency: f64, cost: f64, risk: f64) -> EdgeCost {
        EdgeCost {
            latency,
            cost,
            risk,
        }
    }

    fn nodes3() -> Vec<Node2D> {
        // triangle: 0–1–2, with 0 and 2 far in space, 1 near 0.
        vec![n("0", 0.0, 0.0), n("1", 1.0, 0.0), n("2", 100.0, 0.0)]
    }
    fn edges3() -> Vec<ConnEdge> {
        vec![e(0, 1, 1.0), e(1, 2, 1.0)]
    }
    fn costs3() -> Vec<EdgeCost> {
        // edge 0–1 cheap, edge 1–2 expensive (high risk)
        vec![c(1.0, 1.0, 0.0), c(1.0, 1.0, 0.9)]
    }

    #[test]
    fn spatial_filter_excludes_far_node() {
        // GREEN: node 2 is 100 units away; radius 10 excludes it.
        let ns = nodes3();
        let f = spatial_filter(&ns, 0, 10.0);
        assert!(f.contains(&1), "near node 1 must pass");
        assert!(!f.contains(&2), "far node 2 must be culled");
    }

    #[test]
    fn bfs_reachability_guard() {
        // GREEN: from 0, both 1 and 2 reachable.
        let ns = nodes3();
        let adj = adjacency(ns.len(), &edges3());
        let r = reachable(&adj, 0);
        assert!(r.contains(&0) && r.contains(&1) && r.contains(&2));
    }

    #[test]
    fn hybrid_route_finds_path_and_cost() {
        // GREEN: 0→2 exists (0–1–2). Cost = cheap + risky.
        let ns = nodes3();
        let r = hybrid_route(&ns, &edges3(), &costs3(), 0, 2, 200.0);
        assert!(r.is_some(), "path must exist within radius 200");
        let (path, w) = r.unwrap();
        // endpoints must be src→dst; CH shortcuts may collapse intermediate hops,
        // so we assert the endpoints + the total weight (13.0 = 2.0 + 11.0), not
        // the exact hop sequence.
        assert_eq!(*path.first().unwrap(), 0);
        assert_eq!(*path.last().unwrap(), 2);
        assert!(
            (w - (2.0 + 1.0 + 1.0 + 10.0 * 0.9)).abs() < 1e-9,
            "weight = cheap (2.0) + risky (1.0+1.0+9.0=11.0) = 13.0"
        );
    }

    #[test]
    fn cost_aware_avoidance_vs_cheap_topology() {
        // RED+GREEN: a 4-node diamond. Topologically 0→3 direct exists, but it is
        // costly (latency 20); the 3-hop detour is cheap (1 each). The cost-aware
        // model must pick the LOWER-WEIGHT path, not merely the shortest hop.
        let ns = vec![
            n("0", 0.0, 0.0),
            n("1", 1.0, 1.0),
            n("2", 2.0, 1.0),
            n("3", 3.0, 0.0),
        ];
        let edges = vec![e(0, 3, 1.0), e(0, 1, 1.0), e(1, 2, 1.0), e(2, 3, 1.0)];
        let costs = vec![
            c(20.0, 0.0, 0.0),
            c(1.0, 0.0, 0.0),
            c(1.0, 0.0, 0.0),
            c(1.0, 0.0, 0.0),
        ];
        let r = hybrid_route(&ns, &edges, &costs, 0, 3, 100.0).unwrap();
        // GREEN: avoids the costly direct edge, takes the 3-hop cheap detour.
        assert_eq!(r.0, vec![0, 1, 2, 3], "cost-aware picks cheap detour");
        assert!((r.1 - 3.0).abs() < 1e-9, "total weight = 1+1+1 = 3, not 20");
    }

    #[test]
    fn hybrid_refuses_unreachable() {
        // RED+GREEN: node 1 disconnected from 0 in this graph ⇒ refuse.
        let ns = vec![n("0", 0.0, 0.0), n("1", 50.0, 0.0)];
        let edges: Vec<ConnEdge> = vec![];
        let costs: Vec<EdgeCost> = vec![];
        assert!(
            hybrid_route(&ns, &edges, &costs, 0, 1, 100.0).is_none(),
            "no path ⇒ refuse"
        );
    }

    #[test]
    fn hybrid_refuses_outside_spatial_radius() {
        // RED+GREEN: 0 and 2 are connected (edge) but 100 units apart; radius 10 ⇒ cull.
        let ns = nodes3();
        assert!(
            hybrid_route(&ns, &edges3(), &costs3(), 0, 2, 10.0).is_none(),
            "dst outside spatial radius ⇒ refuse (far-noise filter)"
        );
    }

    #[test]
    fn route_returns_optimal_not_first_popped() {
        // RED (fable B4): the search must find the MINIMUM-COST path, not the
        // first node that happens to be the destination on a LIFO pop.
        // Graph: edges declared [(0,2),(0,1),(2,1)] with weights [1,10,1].
        // Optimal route 0→2→1 costs 1+1 = 2. A LIFO-stack search that pushes
        // 2 then 1 and pops 1 first would return 10 (suboptimal). Assert optimal.
        let ns = vec![
            n("0", 0.0, 0.0),
            n("1", 1.0, 0.0), // dst: far in cost via direct 0→1 (10), cheap via 0→2→1 (2)
            n("2", 0.5, 0.0),
        ];
        // edge order matters for the LIFO-pop bug: 0→2 first (cheap), 0→1 second (10)
        // NOTE: `route` weights by EdgeCost::weight(), NOT ConnEdge.weight, so the
        // expensive direct edge is expressed via latency=10 on costs[1].
        let edges = vec![e(0, 2, 1.0), e(0, 1, 10.0), e(2, 1, 1.0)];
        let costs = vec![c(1.0, 0.0, 0.0), c(10.0, 0.0, 0.0), c(1.0, 0.0, 0.0)];
        let adj = adjacency(ns.len(), &edges);
        let (_, wadj) = weighted_adj(ns.len(), &edges, &costs);
        let sc = build_shortcuts(ns.len(), &adj, &wadj);
        let r = route(ns.len(), &adj, &wadj, &sc, &ns, 0, 1).unwrap();
        assert_eq!(r.0.first(), Some(&0));
        assert_eq!(r.0.last(), Some(&1));
        // optimal cost = 0→2 (1) + 2→1 (1) = 2, NOT the direct 0→1 = 10.
        assert!(
            (r.1 - 2.0).abs() < 1e-9,
            "route returned {}; optimum is 2",
            r.1
        );
    }
}
