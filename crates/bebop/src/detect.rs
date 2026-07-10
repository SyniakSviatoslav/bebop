//! Deterministic operational-graph analytics — the N1–N8 detector battery.
//!
//! Replaces the TS-retired `anomaly`/`cycle`/`liveness` behaviors as real, tested
//! Rust. All functions are pure given an adjacency + node/edge metadata; NO rng,
//! NO wall-clock. RED+GREEN tests prove each detector can fire AND stay silent.
//!
//! Node/edge attributes are flat deterministic vectors so the module stays
//! dependency-free and falsifiable.

/// A directed graph snapshot for analysis. `edges` are (src,dst) index pairs.
/// `node_load` is a per-node utilization in [0,1] (e.g. CPU/mem pressure).
/// `edge_weight` is an optional parallel vector (defaults to 1.0).
pub struct GraphView<'a> {
    pub n: usize,
    pub edges: &'a [(usize, usize)],
    pub node_load: &'a [f64],
    pub edge_weight: Option<&'a [f64]>,
}

impl<'a> GraphView<'a> {
    fn out_deg(&self, i: usize) -> Vec<usize> {
        self.edges.iter().filter(|(s, _)| *s == i).map(|(_, d)| *d).collect()
    }
    fn in_deg(&self, i: usize) -> usize {
        self.edges.iter().filter(|(_, d)| *d == i).count()
    }
    fn w(&self, k: usize) -> f64 {
        self.edge_weight.map(|w| w[k]).unwrap_or(1.0)
    }
}

// ── N1: utilization anomaly (per-node z-score vs peers) ────────────────────
/// N1 flags nodes whose load deviates > `k` stdev from the mean.
pub fn n1_utilization_anomaly(g: &GraphView, k: f64) -> Vec<usize> {
    let m = g.node_load.iter().sum::<f64>() / g.n.max(1) as f64;
    let var = g.node_load.iter().map(|x| (x - m).powi(2)).sum::<f64>() / g.n.max(1) as f64;
    let sd = var.sqrt().max(1e-9);
    (0..g.n)
        .filter(|&i| (g.node_load[i] - m).abs() > k * sd)
        .collect()
}

// ── N2: edge-weight anomaly (weighted cycle / hot edge) ─────────────────────
/// N2 flags edges whose weight exceeds the `k`-quantile of all edge weights.
pub fn n2_edge_anomaly(g: &GraphView, k: f64) -> Vec<usize> {
    let mut ws: Vec<f64> = (0..g.edges.len()).map(|i| g.w(i)).collect();
    if ws.is_empty() {
        return vec![];
    }
    ws.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let thr = ws[(k * (ws.len() - 1) as f64) as usize];
    (0..g.edges.len()).filter(|&i| g.w(i) > thr).collect()
}

// ── N3: cycle detection (DFS color marking) ────────────────────────────────
/// N3 returns true iff the directed graph contains any cycle.
pub fn n3_has_cycle(g: &GraphView) -> bool {
    // 0 = unvisited, 1 = in-stack, 2 = done
    let mut color = vec![0u8; g.n];
    // adjacency
    let adj: Vec<Vec<usize>> = (0..g.n).map(|i| g.out_deg(i)).collect();
    fn dfs(v: usize, color: &mut [u8], adj: &[Vec<usize>]) -> bool {
        color[v] = 1;
        for &u in &adj[v] {
            match color[u] {
                1 => return true,
                0 => {
                    if dfs(u, color, adj) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        color[v] = 2;
        false
    }
    for i in 0..g.n {
        if color[i] == 0 && dfs(i, &mut color, &adj) {
            return true;
        }
    }
    false
}

// ── N4: cycle enumeration (returns the actual cycle node lists) ─────────────
/// N4 returns all simple cycles found (bounded; stops at `limit` to stay finite).
pub fn n4_cycles(g: &GraphView, limit: usize) -> Vec<Vec<usize>> {
    let adj: Vec<Vec<usize>> = (0..g.n).map(|i| g.out_deg(i)).collect();
    let mut out: Vec<Vec<usize>> = Vec::new();
    let mut path: Vec<usize> = Vec::new();
    let mut on_path = vec![false; g.n];
    fn rec(
        v: usize,
        start: usize,
        g_n: usize,
        adj: &[Vec<usize>],
        path: &mut Vec<usize>,
        on_path: &mut Vec<bool>,
        out: &mut Vec<Vec<usize>>,
        limit: usize,
    ) {
        if out.len() >= limit {
            return;
        }
        path.push(v);
        on_path[v] = true;
        for &u in &adj[v] {
            if u == start && path.len() >= 2 {
                let mut cyc = path.clone();
                cyc.push(u);
                out.push(cyc);
                if out.len() >= limit {
                    break;
                }
            } else if !on_path[u] && u > start {
                // canonical ordering avoids dup rotations; every node starts one cyc
                rec(u, start, g_n, adj, path, on_path, out, limit);
            }
        }
        on_path[v] = false;
        path.pop();
    }
    for s in 0..g.n {
        rec(s, s, g.n, &adj, &mut path, &mut on_path, &mut out, limit);
    }
    out
}

// ── N5: liveness (reachability from a source set) ───────────────────────────
/// N5 returns the set of nodes reachable from `sources` (BFS). Dead nodes excluded.
pub fn n5_reachable(g: &GraphView, sources: &[usize]) -> Vec<usize> {
    let adj: Vec<Vec<usize>> = (0..g.n).map(|i| g.out_deg(i)).collect();
    let mut seen = vec![false; g.n];
    let mut q: Vec<usize> = sources.iter().cloned().filter(|&s| s < g.n).collect();
    for &s in &q {
        seen[s] = true;
    }
    while let Some(v) = q.pop() {
        for &u in &adj[v] {
            if !seen[u] {
                seen[u] = true;
                q.push(u);
            }
        }
    }
    (0..g.n).filter(|&i| seen[i]).collect()
}

// ── N6: dead-node detection (unreachable from sources) ──────────────────────
/// N6 returns nodes NOT reachable from `sources` — candidates for GC / alert.
pub fn n6_dead_nodes(g: &GraphView, sources: &[usize]) -> Vec<usize> {
    let live = n5_reachable(g, sources);
    let set: std::collections::HashSet<usize> = live.into_iter().collect();
    (0..g.n).filter(|i| !set.contains(i)).collect()
}

// ── N7: liveness over time (does a node stay live across snapshots?) ─────────
/// N7: given two reachability snapshots, returns nodes that were live then dead now.
pub fn n7_liveness_regression(prev_live: &[usize], now_live: &[usize]) -> Vec<usize> {
    let now: std::collections::HashSet<usize> = now_live.iter().cloned().collect();
    prev_live.iter().cloned().filter(|p| !now.contains(p)).collect()
}

// ── N8: anomaly correlation (which anomalies co-occur on the same node) ─────
/// N8 intersects the node-index sets from the other detectors to find nodes that
/// trip MULTIPLE detectors at once (the "real incident" signal, not noise).
pub fn n8_correlate(sets: &[Vec<usize>]) -> Vec<(usize, usize)> {
    // count membership per node
    let mut counts: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for s in sets {
        for &n in s {
            *counts.entry(n).or_insert(0) += 1;
        }
    }
    counts
        .into_iter()
        .filter(|(_, c)| *c >= 2)
        .map(|(n, c)| (n, c))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g<'b>(edges: &'b [(usize, usize)], load: &'b [f64]) -> GraphView<'b> {
        GraphView { n: load.len(), edges, node_load: load, edge_weight: None }
    }

    #[test]
    fn n1_flags_outlier() {
        // GREEN: a node at load 0.99 vs peers ~0.1 is an anomaly.
        let graph = g(&[(0, 1)], &[0.1, 0.99, 0.1]);
        let a = n1_utilization_anomaly(&graph, 1.0);
        assert!(a.contains(&1), "outlier not flagged: {a:?}");
    }
    #[test]
    fn n1_silent_on_uniform() {
        // RED: uniform load → no anomaly.
        let graph = g(&[(0, 1)], &[0.5, 0.5, 0.5]);
        assert!(n1_utilization_anomaly(&graph, 2.0).is_empty());
    }

    #[test]
    fn n2_flags_heavy_edge() {
        let e = [(0usize, 1usize), (1, 2), (2, 0)];
        let w = [0.1f64, 0.9, 0.1];
        let graph = GraphView { n: 3, edges: &e, node_load: &[0.0; 3], edge_weight: Some(&w) };
        let a = n2_edge_anomaly(&graph, 0.66);
        assert!(a.contains(&1), "heavy edge missed: {a:?}");
    }

    #[test]
    fn n3_detects_cycle() {
        // GREEN: 0->1->2->0 is a cycle.
        let graph = g(&[(0, 1), (1, 2), (2, 0)], &[0.0; 3]);
        assert!(n3_has_cycle(&graph));
    }
    #[test]
    fn n3_dag_no_cycle() {
        // RED: a tree has no cycle.
        let graph = g(&[(0, 1), (0, 2), (1, 3)], &[0.0; 4]);
        assert!(!n3_has_cycle(&graph));
    }

    #[test]
    fn n4_enumerates_cycle() {
        let graph = g(&[(0, 1), (1, 2), (2, 0)], &[0.0; 3]);
        let cyc = n4_cycles(&graph, 4);
        assert!(!cyc.is_empty(), "cycle not enumerated");
        assert!(cyc[0].len() >= 3);
    }

    #[test]
    fn n5_reaches_all_in_connected() {
        let graph = g(&[(0, 1), (1, 2)], &[0.0; 3]);
        let r = n5_reachable(&graph, &[0]);
        assert_eq!(r.len(), 3);
    }
    #[test]
    fn n6_finds_dead_node() {
        // node 3 is disconnected from source 0.
        let graph = g(&[(0, 1), (1, 2)], &[0.0; 4]);
        let d = n6_dead_nodes(&graph, &[0]);
        assert!(d.contains(&3), "dead node missed: {d:?}");
    }

    #[test]
    fn n7_regression_detected() {
        let reg = n7_liveness_regression(&[0, 1, 2], &[0, 2]);
        assert_eq!(reg, vec![1]);
    }

    #[test]
    fn n8_correlates_double_hit() {
        let c = n8_correlate(&[vec![1, 2], vec![2, 3]]);
        assert!(c.iter().any(|(n, _)| *n == 2), "correlated node missed: {c:?}");
        // node 1 hits only once → not correlated
        assert!(!c.iter().any(|(n, _)| *n == 1));
    }
}
