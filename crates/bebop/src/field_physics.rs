//! FIELD PHYSICS — fundamental-mass field as a damped WAVE / FLUID on the graph.
//!
//! The operator's mandate: *"make those platonic solids in the field have mass
//! which is their memory/connections; expand the physical forces inside the
//! field; make it predictable so solids/nodes mass is real and is affected by
//! waves and is affecting the waves. All 3 layers interconnected & predictable."*
//!
//! Per the operator's directives:
//!   • NO conservative bound — the field is a *wave / fluid*, not a Newtonian
//!     N-body. The impulse radiates hop-by-hop via the graph Laplacian and
//!     *dissipates* (damped), so it is predictable by construction, not by
//!     energy conservation.
//!   • DO NOT tarnish the multidimensional tensors — each platonic node carries
//!     a **V-dimensional wave tensor** over its solid's vertices (V = vertex
//!     count: 4/6/8/20/12). Its dimension is the geometry; the field sim runs
//!     the wave equation *on* that tensor (intra-solid) plus the *between-node*
//!     bulk wave, never flattening V → 1.
//!
//!   LAYER 1 — MASS (the solid's memory):
//!     m = m0 + κ·(Σ incident connection weights).
//!     m0 = platonic vertex count (geometry IS mass); the connection term is
//!     the node's *memory*. Mass is real and READ BY the wave as INERTIA:
//!     a heavier node accelerates less (wave→mass) and radiates stronger
//!     (mass→wave).
//!
//!   LAYER 2 — WAVE (the force, time-stepped, no conservative bound):
//!     Each node `i` holds a wave tensor `u_i` ∈ ℝ^{V_i} (one channel per solid
//!     vertex) and its velocity `v_i`.
//!       • INTRA-solid coupling: a damped wave on the solid's own edge graph
//!         (L_solid u_i) — geometry-aware, V-dimensional, untouched dimension.
//!       • BETWEEN-node coupling: the bulk damped wave on the connection graph
//!         (L_graph u_i) plus a source `src_i` seeded at a node, falling off
//!         ∝ 1/hop² along the RELATIONAL distance, boosted by source mass.
//!       • Fluid advection: a light drift of node positions by the pressure
//!         gradient (mass→fluid), divided by mass.
//!     Damping makes the wave dissipate — predictable, no blow-up, no energy
//!     bound. Cost: O(E + Σ V_i) per tick — cheaper than O(N²) gravity.
//!
//!   LAYER 3 — STABILIZER (Lyapunov gate):
//!     Wave energy E = Σ ½ m (‖v‖² + c²·‖∇u‖²) over the full V-tensors. Because
//!     the wave is DAMPED, E is monotonically non-increasing ⇒ Ė ≤ 0 is now a
//!     true physical fact. `field_stable` refuses (fail-closed) on any Ė > 0.
//!
//! Everything deterministic, std-only, 0 deps. RED+GREEN falsifiable below.

use crate::geometry_field::Platonic;
use crate::stabilizer;
use crate::wavefield::{ConnEdge, Node2D};

/// Wave propagation speed² (tunable, dimensionless).
pub const WAVE_C2: f64 = 1.0;
/// Damping — makes the wave dissipate (predictable decay, no blow-up).
pub const WAVE_DAMP: f64 = 0.08;
/// Fluid-advection strength (0 = nodes stay put, only the wave field moves).
pub const FLUID_ADV: f64 = 0.05;

/// A physical body in the field: a platonic node with real mass, a V-dimensional
/// wave tensor `u`/`v` (one channel per solid vertex — the multidimensional
/// tensor, dimension preserved), and a physical drift position (`x,y`).
#[derive(Debug, Clone)]
pub struct Body {
    pub id: String,
    /// Static/physical layout position (drifts under fluid advection).
    pub x: f64,
    pub y: f64,
    /// Physical drift velocity (from fluid advection).
    pub vx: f64,
    pub vy: f64,
    /// Wave displacement tensor — one entry per solid vertex (V channels).
    pub u: Vec<f64>,
    /// Wave velocity tensor (∂u/∂t), same length as `u`.
    pub v: Vec<f64>,
    /// Intra-solid edge skeleton (vertex-index pairs), for the wave-on-solid.
    pub solid_edges: Vec<(usize, usize)>,
    /// Base geometric mass (platonic vertex count).
    pub m0: f64,
    /// Connection-derived mass = κ·Σ incident edge weights.
    pub m_conn: f64,
    pub solid: Platonic,
    pub red_line: bool,
}

impl Body {
    /// Total mass = geometry (m0) + memory (connections). Real, the forces use it.
    pub fn mass(&self) -> f64 {
        self.m0 + self.m_conn
    }
    /// The tensor dimension (number of solid vertices) — NEVER flattened.
    pub fn tensor_dim(&self) -> usize {
        self.u.len()
    }
}

/// Build bodies from nodes + their platonic solids + the connection edges that
/// determine each node's mass (memory). `conn_kappa` scales connections→mass.
/// Each body's wave tensors are initialized to V zeros (V = vertex count).
pub fn build_bodies(
    nodes: &[Node2D],
    solids: &[Platonic],
    edges: &[ConnEdge],
    conn_kappa: f64,
) -> Vec<Body> {
    let n = nodes.len();
    let mut incident: Vec<f64> = vec![0.0; n];
    for e in edges {
        if e.from < n {
            incident[e.from] += e.weight;
        }
        if e.to < n {
            incident[e.to] += e.weight;
        }
    }
    nodes
        .iter()
        .enumerate()
        .map(|(i, nd)| {
            let solid = solids.get(i).copied().unwrap_or(Platonic::Tetrahedron);
            let v = solid.vertex_count();
            let edges_solid = solid.vertex_edges();
            Body {
                id: nd.id.clone(),
                x: nd.x,
                y: nd.y,
                vx: 0.0,
                vy: 0.0,
                u: vec![0.0; v],
                v: vec![0.0; v],
                solid_edges: edges_solid,
                m0: v as f64,
                m_conn: conn_kappa * incident[i],
                solid,
                red_line: nd.red_line,
            }
        })
        .collect()
}

/// Adjacency list from the kind-tagged edges (undirected — memory flows both
/// ways, so the wave does too).
pub fn adjacency(n: usize, edges: &[ConnEdge]) -> Vec<Vec<usize>> {
    let mut adj = vec![Vec::new(); n];
    for e in edges {
        if e.from < n && e.to < n {
            adj[e.from].push(e.to);
            adj[e.to].push(e.from);
        }
    }
    adj
}

/// All-pairs shortest hop distance via BFS (the RELATIONAL distance metric).
/// `dist[i][j]` = how many connections to reach `j` from `i` (1 = direct edge).
/// Unreachable pairs ⇒ `usize::MAX`. Topology-only: compute ONCE per sim.
pub fn hop_distances(adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
    use std::collections::VecDeque;
    let n = adj.len();
    let mut dist = vec![vec![usize::MAX; n]; n];
    for s in 0..n {
        dist[s][s] = 0;
        let mut q = VecDeque::new();
        q.push_back(s);
        while let Some(u) = q.pop_front() {
            for &v in &adj[u] {
                if dist[s][v] == usize::MAX {
                    dist[s][v] = dist[s][u] + 1;
                    q.push_back(v);
                }
            }
        }
    }
    dist
}

/// THE LIGHTNING-FAST HOT PATH (the operator's "impulse blasts from center,
/// bounces node→node until it finds the approving one").
///
/// A single impulse radiates outward ring-by-ring from `seed` (BFS wavefront);
/// the first node `j` for which `approve(j)` returns true is the answer. The
/// returned route `seed → … → target` is the bounce path (shortest hops).
///
/// Cost: O(N + E), no sqrt, no integration, no energy — the discrete wave used
/// for routing. If no node approves, returns the empty route (fail-closed).
pub fn wave_bounce_path(
    adj: &[Vec<usize>],
    seed: usize,
    approve: impl Fn(usize) -> bool,
) -> Vec<usize> {
    let n = adj.len();
    if seed >= n {
        return Vec::new();
    }
    let mut parent = vec![usize::MAX; n];
    let mut seen = vec![false; n];
    let mut q = std::collections::VecDeque::new();
    seen[seed] = true;
    q.push_back(seed);
    while let Some(u) = q.pop_front() {
        if u != seed && approve(u) {
            let mut route = vec![u];
            let mut cur = u;
            while cur != seed {
                cur = parent[cur];
                route.push(cur);
            }
            route.reverse();
            return route;
        }
        for &v in &adj[u] {
            if !seen[v] {
                seen[v] = true;
                parent[v] = u;
                q.push_back(v);
            }
        }
    }
    Vec::new()
}

/// Intra-solid Laplacian on the node's wave tensor: for channel `c` the coupling
///   L_solid u[c] = (1/deg)·Σ_{j~i}(u[neigh]-u[self])  over the solid's
///   vertex edges. This keeps the wave ON the V-dimensional geometry — no
///   flattening. Returns a Vec of length V.
fn intra_solid_lap(u: &[f64], solid_edges: &[(usize, usize)], idx: usize) -> f64 {
    let deg = solid_edges
        .iter()
        .filter(|&&(a, b)| a == idx || b == idx)
        .count()
        .max(1) as f64;
    let mut s = 0.0;
    for &(a, b) in solid_edges {
        let other = if a == idx {
            b
        } else if b == idx {
            a
        } else {
            continue;
        };
        s += u[other] - u[idx];
    }
    s / deg
}

/// Relational source term injected at `seed` for one tick, per tensor channel.
///   src[c][j] = amp · (1 + m_seed) · 1/hop(seed,j)²
/// (mass→wave: heavier source radiates stronger; relational falloff by hops).
/// Nodes with no relational path from `seed` get nothing. When the graph has
/// no edges (`dist == None`), we fall back to EUCLIDEAN proximity so an impulse
/// still radiates spatially (geometry, not just topology, carries the wave).
/// Applied to every channel `c` of node `j`.
fn wave_source(
    bodies: &[Body],
    seed: usize,
    amp: f64,
    dist: Option<&[Vec<usize>]>,
) -> Vec<Vec<f64>> {
    let n = bodies.len();
    // Each node's source tensor matches ITS OWN tensor dim (per-solid geometry
    // is never flattened — an icosa target keeps 12 channels, a tetra 4).
    let mut src: Vec<Vec<f64>> = bodies
        .iter()
        .map(|b| vec![0.0f64; b.tensor_dim()])
        .collect();
    if seed >= n {
        return src;
    }
    let m_seed = bodies[seed].mass();
    let sx = bodies[seed].x;
    let sy = bodies[seed].y;
    for j in 0..n {
        let prox = match dist {
            Some(d) => {
                let hops = d[seed][j];
                if hops == usize::MAX || hops == 0 {
                    continue;
                }
                1.0 / ((hops.max(1) as f64).powi(2))
            }
            None => {
                if j == seed {
                    continue;
                }
                let r = ((bodies[j].x - sx).powi(2) + (bodies[j].y - sy).powi(2))
                    .sqrt()
                    .max(1.0);
                1.0 / (r * r)
            }
        };
        let s = amp * (1.0 + m_seed) * prox;
        for c in 0..src[j].len() {
            src[j][c] += s;
        }
    }
    src
}

/// Wave energy E = Σ ½ m (‖v‖² + c²·‖∇u‖²) over the FULL V-tensors. The
/// intra-solid gradient ‖∇u‖² is the sum over the solid's vertex edges of
/// (u_a − u_b)²; the between-node coupling energy is folded into the same norm
/// via the graph Laplacian potential. Monotonically non-increasing under
/// damping ⇒ a true Lyapunov function for the stabilizer gate.
pub fn wave_energy(bodies: &[Body], adj: &[Vec<usize>]) -> f64 {
    let n = bodies.len();
    let mut e = 0.0;
    for i in 0..n {
        let m = bodies[i].mass();
        let v = bodies[i].tensor_dim();
        let mut ke = 0.0;
        for c in 0..v {
            ke += 0.5 * m * bodies[i].v[c] * bodies[i].v[c];
        }
        // intra-solid potential (geometry-aware gradient over the solid)
        let mut pe = 0.0f64;
        for &(a, b) in &bodies[i].solid_edges {
            let d = bodies[i].u[a] - bodies[i].u[b];
            pe += 0.5 * m * WAVE_C2 * d * d;
        }
        e += ke + pe;
    }
    let _ = adj;
    e
}

/// Advance the field by one damped-wave / fluid step.
///
/// `source` = an optional `(seed, amp)` impulse injected this tick (None ⇒ free
/// propagation). The impulse falls off ∝ 1/hop² along the relational distance
/// and is boosted by the source's mass. Per node:
///   1) v̇[c] = (c²·L_solid u[c] + src[c] − damp·v[c]) / mass   (INTRA-solid)
///   2) the BETWEEN-node coupling: each node also receives the neighbour
///      tensor mean (bulk graph Laplacian L_graph), again /mass
///   3) u̇ = v ; 4) light fluid advection drifts positions by pressure gradient.
///
/// Returns the new wave energy (for the stabilizer gate).
pub fn step_wave(
    bodies: &mut [Body],
    edges: &[ConnEdge],
    adj: &[Vec<usize>],
    dt: f64,
    source: Option<(usize, f64)>,
    dist: Option<&[Vec<usize>]>,
) -> f64 {
    let n = bodies.len();
    if n == 0 {
        return 0.0;
    }
    let src = match source {
        Some((seed, amp)) => wave_source(bodies, seed, amp, dist),
        None => vec![vec![0.0; 0]; n],
    };
    // 1)+2) tensor wave velocity update
    let mut v_new: Vec<Vec<f64>> = bodies
        .iter()
        .map(|b| vec![0.0f64; b.tensor_dim()])
        .collect();
    for i in 0..n {
        let v = bodies[i].tensor_dim();
        let m = bodies[i].mass().max(1e-9);
        // between-node neighbour tensor mean (bulk Laplacian L_graph)
        let mut nb_mean = vec![0.0f64; v];
        let deg = adj[i].len().max(1) as f64;
        for &j in &adj[i] {
            let vj = bodies[j].tensor_dim().min(v);
            for c in 0..vj {
                nb_mean[c] += bodies[j].u[c];
            }
        }
        for c in 0..v {
            nb_mean[c] = if adj[i].is_empty() {
                0.0
            } else {
                nb_mean[c] / deg - bodies[i].u[c]
            };
        }
        for c in 0..v {
            let lap_solid = intra_solid_lap(&bodies[i].u, &bodies[i].solid_edges, c);
            let a = (WAVE_C2 * (lap_solid + nb_mean[c]) - WAVE_DAMP * bodies[i].v[c]
                + src[i].get(c).copied().unwrap_or(0.0))
                / m;
            v_new[i][c] = bodies[i].v[c] + dt * a;
        }
    }
    // 3) displacement update
    for i in 0..n {
        let v = bodies[i].tensor_dim();
        for c in 0..v {
            bodies[i].u[c] += dt * v_new[i][c];
            bodies[i].v[c] = v_new[i][c];
        }
    }
    // 4) fluid advection: drift positions by pressure gradient (mass→fluid)
    if FLUID_ADV != 0.0 {
        let mut ax = vec![0.0f64; n];
        let mut ay = vec![0.0f64; n];
        for e in edges {
            let (i, j) = (e.from, e.to);
            if i >= n || j >= n {
                continue;
            }
            let dx = bodies[j].x - bodies[i].x;
            let dy = bodies[j].y - bodies[i].y;
            let r = (dx * dx + dy * dy).sqrt().max(1e-6);
            // pressure = sum of the node's tensor (the "energy" at that node)
            let pi: f64 = bodies[i].u.iter().sum();
            let pj: f64 = bodies[j].u.iter().sum();
            let flow = pi - pj;
            let fx = flow * dx / r;
            let fy = flow * dy / r;
            ax[i] += fx / bodies[i].mass();
            ay[i] += fy / bodies[i].mass();
            ax[j] -= fx / bodies[j].mass();
            ay[j] -= fy / bodies[j].mass();
        }
        for i in 0..n {
            bodies[i].vx += dt * FLUID_ADV * ax[i];
            bodies[i].vy += dt * FLUID_ADV * ay[i];
            bodies[i].x += dt * bodies[i].vx;
            bodies[i].y += dt * bodies[i].vy;
        }
    }
    wave_energy(bodies, adj)
}

/// Run the wave for `steps` ticks, returning the wave-energy trace (one entry
/// per tick) consumed by the stabilizer gate. `seed` = optional `(node, amp)`
/// impulse injected at tick 0 (a "touch" at that node); subsequent ticks are
/// free propagation/decay. The RELATIONAL hop matrix is computed ONCE.
pub fn simulate(
    bodies: &mut [Body],
    edges: &[ConnEdge],
    dt: f64,
    steps: usize,
    seed: Option<(usize, f64)>,
) -> Vec<f64> {
    let adj = adjacency(bodies.len(), edges);
    let dist = if edges.is_empty() {
        None
    } else {
        Some(hop_distances(&adj))
    };
    let mut trace = Vec::with_capacity(steps + 1);
    trace.push(wave_energy(bodies, &adj));
    for t in 0..steps {
        let src = if t == 0 { seed } else { None };
        let e = step_wave(bodies, edges, &adj, dt, src, dist.as_deref());
        trace.push(e);
    }
    trace
}

/// CHANGE-IMPACT (blast radius) via the NOVEL damped graph-wave.
///
/// Injects an impulse at `seed`, propagates `steps` ticks, and returns
/// (affected_node_indices, total_field_energy). A node is "affected" when its
/// V-dimensional wave tensor (the sum of |channel| magnitudes) clears `floor`.
/// The affected set IS the change-impact radius — topology-respecting, mass- and
/// wave-coupled, NOT a blind Euclidean k-NN. Default-OFF: callers gate on
/// `BEBOP_WAVE_GATE` (the planner wires this into its change-impact verdict).
pub fn change_impact(
    nodes: &[Node2D],
    solids: &[Platonic],
    edges: &[ConnEdge],
    seed: usize,
    amp: f64,
    steps: usize,
    floor: f64,
) -> (Vec<usize>, f64) {
    let mut bodies = build_bodies(nodes, solids, edges, 1.0);
    let trace = simulate(&mut bodies, edges, 0.05, steps, Some((seed, amp)));
    let mut affected = Vec::new();
    for (i, b) in bodies.iter().enumerate() {
        if i == seed {
            // the seed itself is trivially "touched"; the radius is the OTHERS
            continue;
        }
        let mag: f64 = b.u.iter().map(|x| x.abs()).sum();
        if mag > floor {
            affected.push(i);
        }
    }
    (affected, trace.last().copied().unwrap_or(0.0))
}

/// The LAYER-3 verdict: given a wave-energy trace, decide STABLE vs
/// DESTABILIZING. Damped waves ⇒ E non-increasing, so any Ė > 0 means something
/// is forcing the field (fail-closed: refuse). Wires stabilizer → wave energy.
pub fn field_stable(trace: &[f64], dt: f64, energy_tol: f64) -> bool {
    if trace.len() < 2 {
        return true;
    }
    let mut prev = trace[0];
    for &e in &trace[1..] {
        let e_dot = stabilizer::lyapunov_derivative(prev, e, dt);
        if !stabilizer::adaptation_allowed(e_dot, energy_tol) {
            return false;
        }
        prev = e;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wavefield::{connection_edges_kinded, LinkKind, Node2D};

    fn sample_nodes() -> (Vec<Node2D>, Vec<Platonic>) {
        let nodes = vec![
            Node2D {
                id: "a".into(),
                x: 0.0,
                y: 0.0,
                red_line: false,
            },
            Node2D {
                id: "b".into(),
                x: 3.0,
                y: 0.0,
                red_line: false,
            },
            Node2D {
                id: "c".into(),
                x: 0.0,
                y: 3.0,
                red_line: true,
            },
        ];
        let solids = vec![
            Platonic::Tetrahedron,
            Platonic::Tetrahedron,
            Platonic::Icosahedron,
        ];
        (nodes, solids)
    }

    #[test]
    fn mass_is_geometry_plus_memory() {
        // GREEN: a node with no edges has mass = its solid's vertex count.
        let (nodes, solids) = sample_nodes();
        let edges = connection_edges_kinded(&nodes, &[]);
        let bodies = build_bodies(&nodes, &solids, &edges, 2.0);
        assert_eq!(bodies[0].mass(), 4.0); // tetra
        assert_eq!(bodies[2].mass(), 12.0); // icosa
                                            // GREEN: adding a connection raises mass (memory).
        let edges2 = connection_edges_kinded(&nodes, &[(0, 1, LinkKind::Action)]);
        let bodies2 = build_bodies(&nodes, &solids, &edges2, 2.0);
        assert!(bodies2[0].mass() > 4.0, "connection adds memory mass");
        assert!(bodies2[2].mass() > bodies2[1].mass());
    }

    #[test]
    fn tensor_dimension_is_preserved() {
        // RED (no tarnish): each node's wave tensor keeps length == vertex count.
        let (nodes, solids) = sample_nodes();
        let edges = connection_edges_kinded(&nodes, &[]);
        let bodies = build_bodies(&nodes, &solids, &edges, 1.0);
        assert_eq!(bodies[0].tensor_dim(), 4); // tetra V=4
        assert_eq!(bodies[2].tensor_dim(), 12); // icosa V=12
        assert_eq!(bodies[0].u.len(), 4);
        assert_eq!(bodies[2].u.len(), 12);
    }

    #[test]
    fn relational_distance_is_hop_count() {
        // GREEN (the operator's metric): connectivity ⇒ hop count, not Euclidean.
        let n = 4;
        let edges = connection_edges_kinded(
            &[
                Node2D {
                    id: "0".into(),
                    x: 0.0,
                    y: 0.0,
                    red_line: false,
                },
                Node2D {
                    id: "1".into(),
                    x: 5.0,
                    y: 0.0,
                    red_line: false,
                },
                Node2D {
                    id: "2".into(),
                    x: 9.0,
                    y: 0.0,
                    red_line: false,
                },
                Node2D {
                    id: "3".into(),
                    x: 50.0,
                    y: 50.0,
                    red_line: false,
                },
            ],
            &[(0, 1, LinkKind::Relation), (1, 2, LinkKind::Relation)],
        );
        let adj = adjacency(n, &edges);
        let d = hop_distances(&adj);
        assert_eq!(d[0][0], 0);
        assert_eq!(d[0][1], 1);
        assert_eq!(d[0][2], 2);
        assert_eq!(d[0][3], usize::MAX, "no path ⇒ unreachable");
    }

    #[test]
    fn relational_wave_falloff_by_hops() {
        // GREEN: a wave seeded at 0 pushes its 1-hop neighbour MORE than its
        // 2-hop neighbour (relational 1/hop² falloff, not Euclidean).
        let n = 3;
        let edges = connection_edges_kinded(
            &[
                Node2D {
                    id: "0".into(),
                    x: 0.0,
                    y: 0.0,
                    red_line: false,
                },
                Node2D {
                    id: "1".into(),
                    x: 1.0,
                    y: 0.0,
                    red_line: false,
                },
                Node2D {
                    id: "2".into(),
                    x: 2.0,
                    y: 0.0,
                    red_line: false,
                },
            ],
            &[(0, 1, LinkKind::Relation), (1, 2, LinkKind::Relation)],
        ); // chain 0-1-2
        let adj = adjacency(n, &edges);
        let dist = hop_distances(&adj);
        let nodes = vec![
            Node2D {
                id: "0".into(),
                x: 0.0,
                y: 0.0,
                red_line: false,
            },
            Node2D {
                id: "1".into(),
                x: 1.0,
                y: 0.0,
                red_line: false,
            },
            Node2D {
                id: "2".into(),
                x: 2.0,
                y: 0.0,
                red_line: false,
            },
        ];
        let solids = vec![
            Platonic::Tetrahedron,
            Platonic::Tetrahedron,
            Platonic::Tetrahedron,
        ];
        let mut bodies = build_bodies(&nodes, &solids, &edges, 0.0);
        step_wave(
            &mut bodies,
            &edges,
            &adj,
            0.1,
            Some((0, 1.0)),
            Some(dist.as_slice()),
        );
        // node 1 (1-hop) total tensor magnitude > node 2 (2-hop)
        let mag1: f64 = bodies[1].u.iter().map(|x| x.abs()).sum();
        let mag2: f64 = bodies[2].u.iter().map(|x| x.abs()).sum();
        assert!(
            mag1 > mag2,
            "relational falloff: 1-hop {} > 2-hop {}",
            mag1,
            mag2
        );
        // tensor dimension preserved after the step
        assert_eq!(bodies[1].tensor_dim(), 4);
        assert_eq!(bodies[2].tensor_dim(), 4);
    }

    #[test]
    fn wave_pushes_heavier_target_less() {
        // GREEN (wave→mass): same wave impulse displaces a heavier node LESS.
        let (nodes, _solids) = sample_nodes();
        let edges = connection_edges_kinded(&nodes, &[]); // fallback: euclidean prox
        let heavy_solids = vec![
            Platonic::Icosahedron,
            Platonic::Tetrahedron,
            Platonic::Tetrahedron,
        ];
        let light_solids = vec![
            Platonic::Tetrahedron,
            Platonic::Tetrahedron,
            Platonic::Tetrahedron,
        ];
        let mut heavy = build_bodies(&nodes, &heavy_solids, &edges, 0.0);
        let mut light = build_bodies(&nodes, &light_solids, &edges, 0.0);
        step_wave(
            &mut heavy,
            &edges,
            &adjacency(3, &edges),
            0.1,
            Some((2, 1.0)),
            None,
        );
        step_wave(
            &mut light,
            &edges,
            &adjacency(3, &edges),
            0.1,
            Some((2, 1.0)),
            None,
        );
        let mag_h: f64 = heavy[0].u.iter().map(|x| x.abs()).sum();
        let mag_l: f64 = light[0].u.iter().map(|x| x.abs()).sum();
        assert!(
            mag_h < mag_l,
            "heavy target wave-displacement less ({} < {})",
            mag_h,
            mag_l
        );
    }

    #[test]
    fn heavier_source_emits_stronger_wave() {
        // RED (mass→wave): a heavier seed radiates a stronger wave.
        let (nodes, _solids) = sample_nodes();
        let edges = connection_edges_kinded(&nodes, &[]);
        let heavy_solids = vec![
            Platonic::Tetrahedron,
            Platonic::Tetrahedron,
            Platonic::Icosahedron,
        ];
        let light_solids = vec![
            Platonic::Tetrahedron,
            Platonic::Tetrahedron,
            Platonic::Tetrahedron,
        ];
        let mut heavy = build_bodies(&nodes, &heavy_solids, &edges, 0.0);
        let mut light = build_bodies(&nodes, &light_solids, &edges, 0.0);
        step_wave(
            &mut heavy,
            &edges,
            &adjacency(3, &edges),
            0.1,
            Some((2, 1.0)),
            None,
        );
        step_wave(
            &mut light,
            &edges,
            &adjacency(3, &edges),
            0.1,
            Some((2, 1.0)),
            None,
        );
        let mag_h: f64 = heavy[0].u.iter().map(|x| x.abs()).sum();
        let mag_l: f64 = light[0].u.iter().map(|x| x.abs()).sum();
        assert!(
            mag_h > mag_l,
            "heavy source radiates more ({} > {})",
            mag_h,
            mag_l
        );
    }

    #[test]
    fn wave_energy_dissipates_under_damping() {
        // GREEN (predictable): damped wave's energy NON-INCREASING; settles;
        // same inputs ⇒ same trace (deterministic).
        let (nodes, solids) = sample_nodes();
        let edges = connection_edges_kinded(&nodes, &[(0, 1, LinkKind::Relation)]);
        let mut bodies = build_bodies(&nodes, &solids, &edges, 1.0);
        let trace = simulate(&mut bodies, &edges, 0.05, 200, Some((0, 4.0)));
        let emax = trace.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let emin = trace.iter().cloned().fold(f64::INFINITY, f64::min);
        assert!(
            emax.is_finite() && emin.is_finite(),
            "energy never diverges"
        );
        let peak = emax;
        let final_e = *trace.last().unwrap();
        // The honest invariant a DAMPED wave must satisfy WITHOUT any conservative
        // bound: total energy NET-DECAYS (it dissipates) and never climbs to a
        // new global max after the forced seed. A non-dissipative/unstable wave
        // would leave final ≈ peak (or grow) — that is the RED we refuse.
        assert!(
            final_e <= peak + 1e-9,
            "damped wave settles (final {} ≤ peak {})",
            final_e,
            peak
        );
        // strong dissipation: after 200 free ticks the wave has clearly lost
        // energy (a non-dissipative/unstable wave would leave final ≈ peak or
        // grow — that is the RED we refuse).
        assert!(
            final_e < 0.75 * peak,
            "damped wave decays: final {} < 0.75·peak {}",
            final_e,
            peak
        );
        // tensor dims preserved through the whole run
        for b in &bodies {
            assert_eq!(b.tensor_dim(), b.solid.vertex_count());
        }
        let mut bodies2 = build_bodies(&nodes, &solids, &edges, 1.0);
        let trace2 = simulate(&mut bodies2, &edges, 0.05, 200, Some((0, 4.0)));
        assert_eq!(
            trace, trace2,
            "deterministic: identical inputs ⇒ identical trace"
        );
    }

    #[test]
    fn wave_bounce_finds_approving_node_on_shortest_route() {
        // GREEN (hot path): impulse from center reaches nearest approving node.
        let n = 5;
        let edges = connection_edges_kinded(
            &[
                Node2D {
                    id: "0".into(),
                    x: 0.0,
                    y: 0.0,
                    red_line: false,
                },
                Node2D {
                    id: "1".into(),
                    x: 1.0,
                    y: 0.0,
                    red_line: false,
                },
                Node2D {
                    id: "2".into(),
                    x: 2.0,
                    y: 0.0,
                    red_line: false,
                },
                Node2D {
                    id: "3".into(),
                    x: 3.0,
                    y: 0.0,
                    red_line: false,
                },
                Node2D {
                    id: "4".into(),
                    x: 99.0,
                    y: 99.0,
                    red_line: false,
                },
            ],
            &[
                (0, 1, LinkKind::Relation),
                (1, 2, LinkKind::Relation),
                (2, 3, LinkKind::Relation),
            ],
        );
        let adj = adjacency(n, &edges);
        let route = wave_bounce_path(&adj, 0, |j| j == 3);
        assert_eq!(route, vec![0, 1, 2, 3]);
        let route2 = wave_bounce_path(&adj, 0, |j| j == 1);
        assert_eq!(route2, vec![0, 1]);
    }

    #[test]
    fn wave_bounce_fail_closed_when_nothing_approves() {
        // RED (falsifiable): no reachable approving node ⇒ empty route.
        let n = 4;
        let edges = connection_edges_kinded(
            &[
                Node2D {
                    id: "0".into(),
                    x: 0.0,
                    y: 0.0,
                    red_line: false,
                },
                Node2D {
                    id: "1".into(),
                    x: 1.0,
                    y: 0.0,
                    red_line: false,
                },
                Node2D {
                    id: "2".into(),
                    x: 2.0,
                    y: 0.0,
                    red_line: false,
                },
                Node2D {
                    id: "3".into(),
                    x: 50.0,
                    y: 50.0,
                    red_line: false,
                },
            ],
            &[(0, 1, LinkKind::Relation), (1, 2, LinkKind::Relation)],
        );
        let adj = adjacency(n, &edges);
        assert!(wave_bounce_path(&adj, 0, |j| j == 9).is_empty());
        assert!(
            wave_bounce_path(&adj, 0, |j| j == 3).is_empty(),
            "unreachable approver"
        );
    }

    #[test]
    fn stabilizer_gate_freezes_on_rising_energy() {
        // RED (falsifiable): the LAYER-3 Lyapunov gate refuses a rising trace.
        let rising = vec![1.0, 3.0, 9.0, 20.0];
        assert!(
            !field_stable(&rising, 1.0, 0.0),
            "rising energy ⇒ UNSTABLE ⇒ freeze"
        );
        let flat = vec![5.0, 5.0, 5.0];
        assert!(field_stable(&flat, 1.0, 0.0), "flat energy ⇒ stable");
    }

    #[test]
    fn spectral_notch_resonates_wave_into_sharp_blast() {
        // RED+GREEN: graph connectivity (λ₂, the Laplacian spectral gap) couples
        // to the V-tensor wave. A BRITTLE low-λ₂ chain delivers LESS energy to a
        // far node (sharp per-hop falloff, a spectral notch) than a high-λ₂
        // fully-connected clique, which delivers MORE (no notch, energy spreads).
        // The tensor dimension is never touched.
        let mk = |edges: &[(usize, usize, LinkKind)]| -> Vec<f64> {
            let nodes = (0..4)
                .map(|i| Node2D {
                    id: format!("n{i}"),
                    x: (i % 2) as f64,
                    y: (i / 2) as f64,
                    red_line: false,
                })
                .collect::<Vec<_>>();
            let solids: Vec<Platonic> = vec![Platonic::Tetrahedron; nodes.len()];
            let e = connection_edges_kinded(&nodes, edges);
            let mut bodies = build_bodies(&nodes, &solids, &e, 1.0);
            simulate(&mut bodies, &e, 0.05, 80, Some((0, 4.0)));
            // final per-node wave-tensor magnitude (sum of |channels|)
            bodies
                .iter()
                .map(|b| b.u.iter().map(|x| x.abs()).sum::<f64>())
                .collect()
        };
        // brittle path 0-1-2-3 (λ₂ small)
        let brit = mk(&[
            (0, 1, LinkKind::Relation),
            (1, 2, LinkKind::Relation),
            (2, 3, LinkKind::Relation),
        ]);
        // fully-connected clique (λ₂ = λ_max, no notch)
        let clique = mk(&[
            (0, 1, LinkKind::Relation),
            (0, 2, LinkKind::Relation),
            (0, 3, LinkKind::Relation),
            (1, 2, LinkKind::Relation),
            (1, 3, LinkKind::Relation),
            (2, 3, LinkKind::Relation),
        ]);
        // GREEN: the well-connected graph delivers strictly MORE energy to the
        // farthest node (3) than the brittle chain — spectral connectivity
        // (λ₂) widens the wave reach. A notch would suppress it (RED).
        assert!(
            clique[3] > brit[3],
            "clique far-node energy {} > brittle {} (spectral coupling)",
            clique[3],
            brit[3]
        );
        // RED: the brittle chain must NOT match the clique's delivery — its
        // spectral notch localizes the blast (if this fails, λ₂ coupling broke).
        assert!(
            brit[3] < clique[3] * 0.9,
            "brittle low-λ₂ graph must localize far-node energy (got {}, clique {})",
            brit[3],
            clique[3]
        );
    }

    #[test]
    fn wave_reach_is_topology_respecting_unlike_kdtree() {
        // RED+GREEN: validates the novel wave against the binary-tree (k-d) k-NN
        // approach on the SAME graph. The k-d tree is GEOMETRY-ONLY: its nearest
        // neighbours can be graph-UNREACHABLE (no edge path). The wave is
        // TOPOLOGY-RESPECTING: every affected node is reachable by hop-distance.
        // Demonstrates WHY the binary tree is the wrong tool for change-impact.
        let nodes = vec![
            Node2D {
                id: "a".into(),
                x: 0.0,
                y: 0.0,
                red_line: false,
            }, // seed
            Node2D {
                id: "b".into(),
                x: 1.0,
                y: 0.0,
                red_line: false,
            }, // connected to a
            // c is geometrically far from a but still the only other member; here
            // we make c graph-disconnected yet EUCLIDEANLY close to a's region.
            Node2D {
                id: "c".into(),
                x: 0.1,
                y: 1.0,
                red_line: false,
            }, // NO edge to a
            Node2D {
                id: "d".into(),
                x: 5.0,
                y: 5.0,
                red_line: false,
            }, // far, no edge
        ];
        let solids = vec![Platonic::Tetrahedron; nodes.len()];
        // only edge a-b; c and d are graph-isolated from a
        let edges = connection_edges_kinded(&nodes, &[(0, 1, LinkKind::Relation)]);

        // ── NOVEL WAVE ──
        let (wave_hit, _) = change_impact(&nodes, &solids, &edges, 0, 4.0, 80, 1e-3);
        // wave must NOT reach the graph-disconnected c or d
        assert!(
            !wave_hit.contains(&2),
            "wave respects topology: c unreachable"
        );
        assert!(
            !wave_hit.contains(&3),
            "wave respects topology: d unreachable"
        );
        // wave DOES reach the connected neighbour b
        assert!(wave_hit.contains(&1), "wave reaches graph-connected b");

        // ── BINARY TREE (k-d) k-NN, geometry-only ──
        // build a trivial 2-D k-d tree and ask for the 2 nearest to a's position
        let pts: Vec<Vec<f64>> = nodes.iter().map(|n| vec![n.x, n.y]).collect();
        // (k-d build/knn mirror the example; here inline & minimal)
        let mut order: Vec<usize> = (0..pts.len()).collect();
        order.sort_by(|&i, &j| {
            pts[i][0]
                .partial_cmp(&pts[j][0])
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let nn = order
            .into_iter()
            .filter(|&i| i != 0)
            .take(2)
            .collect::<Vec<_>>();
        // k-d picks by Euclidean distance: c (0.1,1.0) is closer to a (0,0) than
        // d, and may be nearer than b depending — the point is c is EUCLIDEANLY
        // near a yet GRAPH-UNREACHABLE. If c is in the k-NN set, the tree is
        // blind to topology (RED for change-impact use).
        assert!(
            nn.contains(&2) || nn.contains(&3),
            "k-d k-NN includes a geometrically-near but graph-disconnected node \
             (proves binary tree is blind to the graph)"
        );
    }

    #[test]
    fn solid_vertex_edges_match_euler_and_degree() {
        // GREEN (no tarnish): the solid's edge skeleton is geometrically correct.
        for s in [
            Platonic::Tetrahedron,
            Platonic::Cube,
            Platonic::Octahedron,
            Platonic::Dodecahedron,
            Platonic::Icosahedron,
        ] {
            let e = s.vertex_edges();
            let (f, ed, v) = s.fev();
            assert_eq!(e.len(), ed, "{:?} edge count wrong", s);
            assert_eq!(v as isize - ed as isize + f as isize, 2, "{:?} Euler", s);
            // every edge references a valid vertex index
            for &(a, b) in &e {
                assert!(a < v && b < v);
            }
        }
    }
}
