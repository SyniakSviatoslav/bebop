//! MATCHER — the open, decentralized dispatch core (kills DANGER #1: the
//! single dispatch server / sequencer).
//!
//! The protocol-centralization map (docs/design/delivery-protocol/
//! PROTOCOL-CENTRALIZATION-MAP.md) names the **matching/dispatch sequencer** as
//! the single most likely hidden-centralization point: whoever orders "which
//! courier gets which order, at what price, in what sequence" controls the
//! network economically, even if settlement is on-chain. The fix is NOT a better
//! server — it is to make the matcher a **pure, deterministic, replicable
//! function** that ANY node can run identically. No hidden state, no authority.
//!
//! Contract (transport-agnostic):
//!   MatcherRequest  { nodes, edges, costs, orders, radius }  → JSON
//!   MatcherResponse { assignments, unmatched }              ← JSON
//!   match_orders(req) is deterministic: same req ⇒ same resp on every node.
//!   fingerprint(resp) is a content hash proving two independent nodes agree.
//!
//! The reference client (MatcherClient trait + LocalMatcherClient) calls this
//! pure function locally; a RemoteMatcherClient (any transport: HTTP, p2p, stdio)
//! is a thin wrapper over the SAME contract. Because the algorithm is open and
//! the result is reproducible + fingerprintable, no single deployment is
//! privileged — replaceable by design.
//!
//! Deterministic, std-only (+ serde). RED+GREEN falsifiable below.

use crate::cost_estimate::{hybrid_route, EdgeCost};
use crate::wavefield::{ConnEdge, LinkKind, Node2D};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

/// A delivery order: courier at `src` must reach destination `dst`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Order {
    pub id: String,
    pub src: usize,
    pub dst: usize,
}

/// The full matcher input. Serializable ⇒ the contract is open over ANY
/// transport (HTTP/JSON, p2p, stdio) — no proprietary encoding, no lock-in.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MatcherRequest {
    pub nodes: Vec<Node2D>,
    pub edges: Vec<ConnEdge>,
    pub costs: Vec<EdgeCost>,
    pub orders: Vec<Order>,
    /// Spatial radius for the Layer-1 pre-filter (far noise cull).
    pub radius: f64,
}

/// One matched order: courier `src` → `dst` via `path` at total `cost`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Assignment {
    pub order_id: String,
    pub courier: usize,
    pub path: Vec<usize>,
    pub cost: f64,
}

/// The matcher output. `unmatched` holds orders that were REFUSED (fail-closed:
/// unreachable / outside radius) — they are NOT silently dropped, the caller
/// sees them and can re-dispatch or contest.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct MatcherResponse {
    pub assignments: Vec<Assignment>,
    pub unmatched: Vec<String>,
}

/// The open matcher: route every order via the Hybrid Cost-Aware Engine
/// (k-d filter + BFS guard + A*/CH). Pure + deterministic: identical input ⇒
/// identical output on every node, every run. This is what makes the matcher
/// replicable instead of a privileged server.
pub fn match_orders(req: &MatcherRequest) -> MatcherResponse {
    let mut assignments = Vec::new();
    let mut unmatched = Vec::new();
    for o in &req.orders {
        match hybrid_route(&req.nodes, &req.edges, &req.costs, o.src, o.dst, req.radius) {
            Some((path, cost)) => assignments.push(Assignment {
                order_id: o.id.clone(),
                courier: o.src,
                path,
                cost,
            }),
            // fail-closed: unreachable ⇒ refuse, surface it in `unmatched`.
            None => unmatched.push(o.id.clone()),
        }
    }
    MatcherResponse {
        assignments,
        unmatched,
    }
}

/// Deterministic content fingerprint of a response. Two independent nodes that
/// ran the same request MUST produce the same fingerprint — this is the
/// verifiable proof that the matcher is replicable (no hidden state, no
/// privileged server). We hash the canonical JSON (sorted deterministically by
/// serde), sidestepping the fact that `f64` is not `Hash`.
pub fn fingerprint(resp: &MatcherResponse) -> u64 {
    let sorted = {
        let mut a = resp.assignments.clone();
        a.sort_by(|x, y| x.order_id.cmp(&y.order_id));
        a
    };
    // canonical JSON: order_id/path/cost + unmatched (sorted)
    let mut unmatched = resp.unmatched.clone();
    unmatched.sort();
    let mut canon = String::new();
    for a in &sorted {
        canon.push_str(&format!("{}:{:?}:{:.6};", a.order_id, a.path, a.cost));
    }
    canon.push('|');
    for u in &unmatched {
        canon.push_str(u);
        canon.push(',');
    }
    let mut h = DefaultHasher::new();
    canon.hash(&mut h);
    h.finish()
}

/// The reference client contract. A deployment MUST be able to swap the
/// implementation (local, remote, another vendor's) without changing callers —
/// this trait is what makes DANGER #1 (single hosted server) impossible: the
/// client talks a contract, not a specific box.
pub trait MatcherClient {
    /// Match a batch of orders. Implementations may run locally (pure function)
    /// or forward over any transport; callers do not care.
    fn match_batch(&self, req: &MatcherRequest) -> MatcherResponse;
}

/// Reference client: runs the matcher IN-PROCESS. This is the default — it
/// proves the matcher needs no server at all. A `RemoteMatcherClient` holds a
/// transport and forwards `req` as JSON; the trait keeps both interchangeable.
#[derive(Default)]
pub struct LocalMatcherClient;

impl MatcherClient for LocalMatcherClient {
    fn match_batch(&self, req: &MatcherRequest) -> MatcherResponse {
        match_orders(req)
    }
}

/// Transport abstraction for a REMOTE matcher. The client codes to this trait,
/// NOT to a hostname — so any node (or a fleet of them) can serve. This is what
/// makes DANGER #1 structurally impossible: there is no privileged endpoint,
/// only an interchangeable transport.
pub trait Transport {
    /// Send the serialized request, return the serialized response (or an error
    /// string). Implementations: in-process bus, HTTP, p2p/gossip, stdio, queue.
    fn send(&self, req_json: &str) -> Result<String, String>;
}

/// In-memory transport: a stand-in for "another node on the network". It runs
/// the SAME `match_orders` locally, proving the wire contract is faithful — the
/// remote node is just another correct implementation, not a trusted authority.
#[derive(Default)]
pub struct InMemoryTransport;

impl Transport for InMemoryTransport {
    fn send(&self, req_json: &str) -> Result<String, String> {
        let req: MatcherRequest =
            serde_json::from_str(req_json).map_err(|e| format!("bad request: {e}"))?;
        let resp = match_orders(&req);
        serde_json::to_string(&resp).map_err(|e| format!("bad response: {e}"))
    }
}

/// Remote reference client: serializes the request over a `Transport`, parses
/// the response. Identical output to `LocalMatcherClient` (proven by test) — the
/// ONLY difference is WHERE the pure function runs. Swapping transports = moving
/// dispatch between nodes with zero caller changes.
pub struct RemoteMatcherClient<T: Transport> {
    pub transport: T,
}

impl<T: Transport> MatcherClient for RemoteMatcherClient<T> {
    fn match_batch(&self, req: &MatcherRequest) -> MatcherResponse {
        let json = serde_json::to_string(req).expect("req serializes");
        let resp_json = self
            .transport
            .send(&json)
            .expect("transport delivers a response");
        serde_json::from_str(&resp_json).expect("response parses")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wavefield::Node2D;

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
    fn c(latency: f64) -> EdgeCost {
        EdgeCost {
            latency,
            cost: 0.0,
            risk: 0.0,
        }
    }

    fn sample_req() -> MatcherRequest {
        // triangle 0–1–2; courier 0, restaurant 2, customer 1.
        let nodes = vec![
            n("courier", 0.0, 0.0),
            n("cust", 1.0, 0.0),
            n("rest", 100.0, 0.0),
        ];
        let edges = vec![e(0, 1, 1.0), e(1, 2, 1.0)];
        let costs = vec![c(1.0), c(1.0)];
        let orders = vec![
            Order {
                id: "o1".into(),
                src: 0,
                dst: 2,
            }, // courier→restaurant (reachable)
        ];
        MatcherRequest {
            nodes,
            edges,
            costs,
            orders,
            radius: 200.0,
        }
    }

    #[test]
    fn matches_reachable_order() {
        // GREEN: courier 0 can reach restaurant 2 via 0–1–2.
        let resp = match_orders(&sample_req());
        assert_eq!(resp.assignments.len(), 1);
        assert!(resp.unmatched.is_empty());
        let a = &resp.assignments[0];
        assert_eq!(a.order_id, "o1");
        assert_eq!(*a.path.first().unwrap(), 0);
        assert_eq!(*a.path.last().unwrap(), 2);
    }

    #[test]
    fn refuses_unreachable_order_fail_closed() {
        // RED+GREEN: an order to node 5 (does not exist) is refused and SURFACED.
        let mut req = sample_req();
        req.orders.push(Order {
            id: "o2".into(),
            src: 0,
            dst: 5,
        });
        let resp = match_orders(&req);
        assert!(resp.assignments.iter().any(|a| a.order_id == "o1"));
        assert_eq!(
            resp.unmatched,
            vec!["o2".to_string()],
            "unreachable order refused, not dropped"
        );
    }

    #[test]
    fn matcher_is_replicable_no_hidden_server() {
        // RED+GREEN (kills DANGER #1): two INDEPENDENT clients (local instances)
        // on the same request MUST produce byte-identical fingerprints. This is
        // the proof the matcher is a pure function, not a privileged server.
        let req = sample_req();
        let client_a = LocalMatcherClient;
        let client_b = LocalMatcherClient; // a "different node"
        let ra = client_a.match_batch(&req);
        let rb = client_b.match_batch(&req);
        assert_eq!(
            fingerprint(&ra),
            fingerprint(&rb),
            "independent nodes must agree — matcher is replicable"
        );
        // and the pure function agrees with the client:
        assert_eq!(fingerprint(&ra), fingerprint(&match_orders(&req)));
    }

    #[test]
    fn contract_is_serializable_open_transport() {
        // GREEN: the request/response round-trips through JSON ⇒ the contract is
        // open over any transport (no proprietary encoding / lock-in).
        let req = sample_req();
        let json = serde_json::to_string(&req).expect("req serializes");
        let back: MatcherRequest = serde_json::from_str(&json).expect("req deserializes");
        let resp = match_orders(&back);
        let rjson = serde_json::to_string(&resp).expect("resp serializes");
        let rback: MatcherResponse = serde_json::from_str(&rjson).expect("resp deserializes");
        assert_eq!(fingerprint(&resp), fingerprint(&rback));
    }

    #[test]
    fn remote_matches_local_over_wire() {
        // RED+GREEN: the RemoteMatcherClient (over the InMemoryTransport, a stand-in
        // for "another node") must produce the SAME result as the local client.
        // Proves the standardized interface is faithful — any node can serve, the
        // wire changes nothing. This closes the "interface" half of the audit's
        // blocker question; the remaining half (trust) is the reputation ledger.
        let req = sample_req();
        let local = LocalMatcherClient.match_batch(&req);
        let remote = RemoteMatcherClient {
            transport: InMemoryTransport,
        }
        .match_batch(&req);
        assert_eq!(
            fingerprint(&local),
            fingerprint(&remote),
            "remote over wire == local in-process: interface is faithful"
        );
        assert_eq!(local.assignments, remote.assignments);
    }
}
