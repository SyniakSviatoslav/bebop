//! MAPPING — the live graph-state layer (the "Mapping" node of the Hybrid Engine).
//!
//! Keeping the connection graph honest under real traffic: edge weights are NOT
//! static. Live congestion at the endpoints raises the cost `W_uv` (so the
//! cost-aware router avoids jammed corridors), and when a hub's load×degree
//! (MHD stress `J_z`) crosses a threshold the graph TOPOLOGICALLY re-wires
//! (graceful degradation) instead of collapsing. This is `reconnect.rs` with a
//! live congestion→weight feed bolted on.
//!
//! Pipeline (per tick):
//!   1. refresh weights: W_uv = base + cong_penalty·(load_u + load_v)
//!   2. if any node's J_z > thr: reconnect (drop hot edges, rewire neighbors to
//!      the lowest-J_z candidate) — the `reconnect` MHD operator
//!   3. re-apply refreshed weights to the surviving/rewired edges
//!
//! Deterministic, std-only, 0 deps. RED+GREEN falsifiable below.

use crate::reconnect;
use crate::wavefield::{ConnEdge, LinkKind};

/// Refresh edge weights from live congestion. `W_uv = base_weight + penalty·
/// (load_u + load_v)`. High load at either endpoint ⇒ more expensive edge ⇒ the
/// cost-aware A* (cost_estimate) routes around it. `penalty` is the congestion
/// sensitivity (0 = static graph). Returns a new edge list (does not mutate in).
pub fn refresh_edge_weights(edges: &[ConnEdge], loads: &[f64], penalty: f64) -> Vec<ConnEdge> {
    edges
        .iter()
        .map(|e| {
            let lu = loads.get(e.from).copied().unwrap_or(0.0);
            let lv = loads.get(e.to).copied().unwrap_or(0.0);
            let w = (e.weight + penalty * (lu + lv)).max(1e-6);
            ConnEdge {
                from: e.from,
                to: e.to,
                kind: e.kind,
                weight: w,
            }
        })
        .collect()
}

/// One live mapping tick: refresh weights by congestion, then gracefully
/// re-wire any overloaded hub. Returns the new (weighted) edge list and the set
/// of nodes that reconnected this tick (empty = healthy graph, untouched).
///
/// The `reconnect` operator works on UNWEIGHTED topology (J_z = load×degree), so
/// we feed it the raw topology + loads, get the surviving/rewired edge pairs,
/// then re-apply the congestion-refreshed weights to those pairs.
pub fn live_mapping(
    edges: &[ConnEdge],
    loads: &[f64],
    penalty: f64,
    reconnect_thr: f64,
) -> (Vec<ConnEdge>, Vec<usize>) {
    let n = loads.len();
    // Step 1: congestion-refreshed weights (used at the end to re-label edges).
    let refreshed = refresh_edge_weights(edges, loads, penalty);
    let wmap: std::collections::HashMap<(usize, usize), f64> = refreshed
        .iter()
        .map(|e| ((e.from.min(e.to), e.from.max(e.to)), e.weight))
        .collect();

    // Step 2: MHD reconnect on the raw topology + loads.
    let topo: Vec<(usize, usize)> = edges.iter().map(|e| (e.from, e.to)).collect();
    let g = reconnect::Graph {
        n,
        edges: &topo,
        load: loads,
    };
    let (new_topo, hot) = reconnect::reconnect(&g, reconnect_thr);

    // Step 3: re-apply refreshed weights to surviving/rewired pairs.
    let out: Vec<ConnEdge> = new_topo
        .into_iter()
        .map(|(a, b)| {
            let w = wmap.get(&(a.min(b), a.max(b))).copied().unwrap_or_else(|| {
                1.0 + penalty
                    * (loads.get(a).copied().unwrap_or(0.0) + loads.get(b).copied().unwrap_or(0.0))
            });
            ConnEdge {
                from: a,
                to: b,
                kind: LinkKind::Relation,
                weight: w,
            }
        })
        .collect();
    (out, hot)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(from: usize, to: usize, w: f64) -> ConnEdge {
        ConnEdge {
            from,
            to,
            kind: LinkKind::Relation,
            weight: w,
        }
    }

    #[test]
    fn congestion_raises_edge_weight() {
        // GREEN: an edge between two loaded nodes must be heavier than the same
        // edge between idle nodes (congestion ⇒ cost).
        let edges = vec![e(0, 1, 1.0)];
        let idle = [0.0f64, 0.0];
        let loaded = [0.9f64, 0.9];
        let w_idle = refresh_edge_weights(&edges, &idle, 5.0)[0].weight;
        let w_hot = refresh_edge_weights(&edges, &loaded, 5.0)[0].weight;
        assert!(
            w_hot > w_idle,
            "congested edge ({w_hot}) must be heavier than idle ({w_idle})"
        );
        // exact: 1.0 + 5.0*(0.9+0.9) = 10.0 vs 1.0
        assert!((w_hot - 10.0).abs() < 1e-9);
        assert!((w_idle - 1.0).abs() < 1e-9);
    }

    #[test]
    fn penalty_zero_is_static_graph() {
        // GREEN: penalty 0 ⇒ weights unchanged (static mapping).
        let edges = vec![e(0, 1, 2.0), e(1, 2, 3.0)];
        let loads = [1.0f64, 1.0, 1.0];
        let out = refresh_edge_weights(&edges, &loads, 0.0);
        assert!((out[0].weight - 2.0).abs() < 1e-9);
        assert!((out[1].weight - 3.0).abs() < 1e-9);
    }

    #[test]
    fn live_mapping_sheds_overloaded_hub() {
        // RED+GREEN: a hub at load 1.0 with degree 3 ⇒ J_z = 3 > thr = 1.0.
        // live_mapping MUST reconnect the hub AND lower max J_z, while staying
        // non-empty (graceful, not a collapse).
        let edges = vec![e(0, 1, 1.0), e(0, 2, 1.0), e(0, 3, 1.0), e(1, 2, 1.0)];
        let loads = [1.0f64, 0.1, 0.1, 0.1];
        let before = reconnect::Graph {
            n: 4,
            edges: &edges.iter().map(|e| (e.from, e.to)).collect::<Vec<_>>(),
            load: &loads,
        }
        .max_jz();
        assert!(before > 1.0, "hub must be over threshold");

        let (out, hot) = live_mapping(&edges, &loads, 1.0, 1.0);
        assert_eq!(hot, vec![0], "hub 0 must reconnect");
        assert!(!out.is_empty(), "graceful: edges survive reconnection");

        // post-reconnect max J_z must be strictly lower (energy shed).
        let topo: Vec<(usize, usize)> = out.iter().map(|e| (e.from, e.to)).collect();
        let ng = reconnect::Graph {
            n: 4,
            edges: &topo,
            load: &loads,
        };
        assert!(
            ng.max_jz() < before,
            "reconnect must shed J_z: {before} -> {}",
            ng.max_jz()
        );
    }

    #[test]
    fn live_mapping_healthy_graph_untouched() {
        // GREEN: low loads everywhere ⇒ no reconnect, weights only refreshed.
        let edges = vec![e(0, 1, 1.0), e(1, 2, 1.0)];
        let loads = [0.1f64, 0.1, 0.1];
        let (out, hot) = live_mapping(&edges, &loads, 2.0, 5.0);
        assert!(hot.is_empty(), "healthy graph ⇒ no reconnect");
        assert_eq!(out.len(), 2, "edge count preserved");
        // weights still refreshed by congestion (uniformly low ⇒ base + small):
        assert!((out[0].weight - (1.0 + 2.0 * 0.2)).abs() < 1e-9);
    }
}
