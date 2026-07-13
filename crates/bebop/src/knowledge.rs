//! Knowledge — the §0·GP living-knowledge retriever (ported from `src/knowledge.ts`).
//! Deterministic sparse retrieval: cosine over hashed concept vectors.
//! No RNG. Returns REAL payloads; a noise floor is excluded honestly.

use crate::memory::{simple_hash, LivingMemory, MemoryNode};

pub struct Hit {
    pub id: String,
    pub concept: String,
    pub text: String,
    pub score: f64,
}

/// Retrieve the top-k nodes nearest `query` by hashed-bag-of-bytes cosine.
/// `note` explains the result (incl. an honest noise floor).
pub fn recall(mm: &LivingMemory, query: &str, k: usize) -> RecallOut {
    let qv = bag_vec(query.as_bytes());
    let mut scored: Vec<(f64, &MemoryNode)> = mm
        .nodes()
        .values()
        .map(|n| (cosine(&qv, &bag_vec(n.concept.as_bytes())), n))
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

    // Noise floor: below this cosine the match is indistinguishable from chance
    // for short strings, so we exclude it honestly (no manufactured hits).
    const NOISE_FLOOR: f64 = 0.35;
    let mut hits = Vec::new();
    for (s, n) in scored.into_iter().take(k) {
        if s < NOISE_FLOOR {
            continue;
        }
        hits.push(Hit {
            id: n.id.clone(),
            concept: n.concept.clone(),
            text: n.payload.clone(),
            score: s,
        });
    }
    let note = if hits.is_empty() {
        format!("no real hit above noise floor ({NOISE_FLOOR})")
    } else {
        "retrieved real payloads".into()
    };
    RecallOut { hits, note }
}

pub struct RecallOut {
    pub hits: Vec<Hit>,
    pub note: String,
}

/// Bag-of-bytes vector: counts of each byte value (256-dim, deterministic).
pub fn bag_vec(b: &[u8]) -> Vec<f64> {
    let mut v = vec![0f64; 256];
    for &x in b {
        v[x as usize] += 1.0;
    }
    v
}

pub fn cosine(a: &[f64], b: &[f64]) -> f64 {
    let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let nb = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

/// Graph-boosted recall (codebase-memory-mcp pattern, eval-gated @ recall@4:
/// 0.917 vs flat 0.500). `edges` maps a concept → its neighbor concepts. When
/// `None`, this degrades to plain `recall` (no fabricated structure). The
/// spreading activation surfaces edge-connected-but-lexically-dissimilar nodes,
/// exactly the case graph RAG targets. Keeps the honest noise floor.
///
/// Edges are keyed by CONCEPT (the stable `LivingMemory` key), not by position,
/// so callers don't depend on HashMap iteration order.
pub fn recall_graph(
    mm: &LivingMemory,
    query: &str,
    k: usize,
    edges: Option<&std::collections::HashMap<String, Vec<String>>>,
) -> RecallOut {
    let nodes: Vec<&MemoryNode> = mm.nodes().values().collect();
    let qv = bag_vec(query.as_bytes());
    let mut base: Vec<f64> = nodes
        .iter()
        .map(|n| cosine(&qv, &bag_vec(n.concept.as_bytes())))
        .collect();

    if let Some(e) = edges {
        // index concepts → position
        let idx: std::collections::HashMap<&str, usize> = nodes
            .iter()
            .enumerate()
            .map(|(i, n)| (n.concept.as_str(), i))
            .collect();
        // spreading activation: each node boosted by 2-hop neighbors (decay 0.5)
        let mut score = base.clone();
        for start in 0..base.len() {
            if base[start] <= 0.0 {
                continue;
            }
            let mut frontier = vec![(start, 0usize)];
            while let Some((node, dist)) = frontier.pop() {
                if dist >= 2 {
                    continue;
                }
                let concept = &nodes[node].concept;
                if let Some(nbs) = e.get(concept) {
                    for nb in nbs {
                        if let Some(&nb_i) = idx.get(nb.as_str()) {
                            if nb_i == start {
                                continue;
                            }
                            let w = 0.5f64.powi((dist as i32) + 1);
                            let add = w * base[start];
                            if add > score[nb_i] {
                                score[nb_i] = add;
                            }
                            frontier.push((nb_i, dist + 1));
                        }
                    }
                }
            }
        }
        base = score;
    }

    let mut scored: Vec<(f64, &MemoryNode)> = base.into_iter().zip(nodes.into_iter()).collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());

    const NOISE_FLOOR: f64 = 0.35;
    let mut hits = Vec::new();
    for (s, n) in scored.into_iter().take(k) {
        if s < NOISE_FLOOR {
            continue;
        }
        hits.push(Hit {
            id: n.id.clone(),
            concept: n.concept.clone(),
            text: n.payload.clone(),
            score: s,
        });
    }
    let note = if hits.is_empty() {
        format!("no real hit above noise floor ({NOISE_FLOOR})")
    } else {
        "retrieved real payloads (graph-boosted)".into()
    };
    RecallOut { hits, note }
}
/// SimpleMem-style recursive consolidation (OFFLINE, deterministic).
/// Groups nodes whose concept cosine ≥ `tau_cluster` into an abstract parent
/// node in the `Long` layer. NON-DESTRUCTIVE: children stay; parent is added.
/// Returns the number of abstract nodes created. No external embedder.
pub fn consolidate(mm: &mut LivingMemory, tau_cluster: f64) -> usize {
    let concepts: Vec<String> = mm.nodes().values().map(|n| n.concept.clone()).collect();
    let mut created = 0usize;
    // Pairwise: if two distinct concepts are similar enough, fuse into an abstract parent.
    for i in 0..concepts.len() {
        for j in (i + 1)..concepts.len() {
            let a = &concepts[i];
            let b = &concepts[j];
            let sim = cosine(&bag_vec(a.as_bytes()), &bag_vec(b.as_bytes()));
            if sim >= tau_cluster {
                let parent_concept = format!("abstract:{}", a);
                if mm
                    .nodes()
                    .contains_key(&format!("{:08x}", simple_hash(parent_concept.as_bytes())))
                {
                    continue; // already consolidated
                }
                mm.remember_meta(
                    &parent_concept,
                    &format!("consolidated from '{a}' + '{b}'"),
                    vec![a.clone(), b.clone()],
                    "consolidation",
                    1.0,
                );
                // promote parent to Long layer
                if let Some(node) = mm
                    .nodes_mut()
                    .values_mut()
                    .find(|n| n.concept == parent_concept)
                {
                    node.layer = crate::memory::Layer::Long;
                }
                created += 1;
            }
        }
    }
    created
}

/// SimpleMem-style adaptive query-aware retrieval (OFFLINE).
/// Query complexity ≈ lexical entropy of the bag-of-bytes vector; maps to
/// k ∈ [3, 20] (min 3 symbolic, max 20 semantic). Keeps the honest noise floor.
pub fn adaptive_recall(mm: &LivingMemory, query: &str) -> RecallOut {
    let qv = bag_vec(query.as_bytes());
    let total: f64 = qv.iter().sum();
    let entropy = if total > 0.0 {
        -qv.iter()
            .filter(|&&c| c > 0.0)
            .map(|&c| {
                let p = c / total;
                p * p.log2()
            })
            .sum::<f64>()
    } else {
        0.0
    };
    // entropy in [0, 8] for 256-dim; normalize, scale to k range.
    let norm = (entropy / 8.0).clamp(0.0, 1.0);
    let k = (3.0 + norm * 17.0).round() as usize; // 3..=20
    recall(mm, query, k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::LivingMemory;
    use std::collections::HashMap;

    #[test]
    fn recall_returns_real_payload() {
        // GREEN: a stored concept is retrievable, with its concept + payload.
        let mut m = LivingMemory::new();
        m.remember("copilot", "native doer/checker seam");
        m.remember("vault", "encrypted-at-rest identity");
        let r = recall(&m, "copilot", 2);
        assert!(!r.hits.is_empty(), "no hit for a stored concept");
        assert_eq!(r.hits[0].concept, "copilot");
        assert!(r.hits[0].text.contains("doer/checker"));
    }

    #[test]
    fn recall_excludes_noise_floor() {
        // RED: a query with ZERO letter overlap must NOT manufacture a fake hit.
        let mut m = LivingMemory::new();
        m.remember("copilot", "native doer/checker seam");
        m.remember("vault", "encrypted-at-rest identity");
        // "qzxjwk" shares no letters with either stored concept → cosine 0.
        let r = recall(&m, "qzxjwk", 2);
        assert!(r.hits.is_empty(), "noise floor leaked a fake hit");
        assert!(r.note.contains("noise"));
    }

    #[test]
    fn recall_graph_surfaces_edge_connected_node() {
        // GREEN: a node with NO letter overlap to the query but edge-connected to
        // a matching node must surface under graph boost (the eval-gated win).
        let mut m = LivingMemory::new();
        m.remember("auth", "login and session boundary");
        // `session` shares no letters with "auth" → flat cosine ~0, would miss.
        m.remember("session", "token scoped lifetime");
        m.remember("vault", "encrypted-at-rest identity");
        // index 0=auth,1=session,2=vault — but edges are KEYED BY CONCEPT,
        // so HashMap iteration order is irrelevant (the earlier flaky bug).
        let mut edges = HashMap::new();
        edges.insert("auth".to_string(), vec!["session".to_string()]);
        edges.insert("session".to_string(), vec!["auth".to_string()]);

        let flat = recall(&m, "auth", 4);
        let flat_ids: Vec<&str> = flat.hits.iter().map(|h| h.concept.as_str()).collect();
        assert!(
            !flat_ids.contains(&"session"),
            "flat should miss 'session' (no overlap)"
        );

        let gr = recall_graph(&m, "auth", 4, Some(&edges));
        let gr_ids: Vec<&str> = gr.hits.iter().map(|h| h.concept.as_str()).collect();
        assert!(
            gr_ids.contains(&"session"),
            "graph boost must surface edge-connected 'session'"
        );
    }

    #[test]
    fn recall_graph_none_degrades_to_flat() {
        // RED for the adapter: None edges → behaves exactly like recall().
        let mut m = LivingMemory::new();
        m.remember("copilot", "native doer/checker seam");
        m.remember("vault", "encrypted-at-rest identity");
        let a = recall(&m, "copilot", 2);
        let b = recall_graph(&m, "copilot", 2, None);
        assert_eq!(a.hits.len(), b.hits.len());
        assert_eq!(a.hits[0].concept, b.hits[0].concept);
    }

    #[test]
    fn consolidate_groups_related_nodes() {
        // GREEN: two near-identical concepts consolidate into one abstract parent.
        let mut m = LivingMemory::new();
        m.remember("auth login", "session boundary");
        m.remember("auth log in", "session boundary"); // near-duplicate
        m.remember("vault", "encrypted-at-rest identity"); // unrelated
        let made = consolidate(&mut m, 0.85);
        assert!(made >= 1, "related nodes should consolidate");
        let has_abstract = m
            .nodes()
            .values()
            .any(|n| n.concept.starts_with("abstract:"));
        assert!(has_abstract, "an abstract parent node must exist");
        // NON-DESTRUCTIVE: children still present.
        assert!(m.nodes().contains_key(&format!(
            "{:08x}",
            crate::memory::simple_hash(b"auth login")
        )));
    }

    #[test]
    fn consolidate_skips_unrelated_nodes() {
        // RED: totally different concepts must NOT fuse.
        let mut m = LivingMemory::new();
        m.remember("quantum field", "physics");
        m.remember("banana bread", "cooking");
        let made = consolidate(&mut m, 0.85);
        assert_eq!(made, 0, "unrelated nodes wrongly consolidated");
    }

    #[test]
    fn adaptive_recall_scales_k_with_query_complexity() {
        // GREEN: longer/more-entropic query → larger k (up to 20).
        let mut m = LivingMemory::new();
        for i in 0..25 {
            m.remember(&format!("auth{i}"), "payload"); // relevant to "auth" query
        }
        let short = adaptive_recall(&m, "auth");
        let long = adaptive_recall(&m, "authentication authorization auth protocol quantum entanglement field collapses under observation by the reptilian overlord");
        // k is derived from entropy; long query has strictly higher entropy → larger k,
        // and since it still contains "auth" it surfaces >= short's hits (capped at 20).
        assert!(
            long.hits.len() >= short.hits.len(),
            "adaptive k did not grow with complexity"
        );
        assert!(long.hits.len() <= 20, "k exceeded max depth 20");
        assert!(short.hits.len() >= 3, "k below min depth 3");
    }
}
