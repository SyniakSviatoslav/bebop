/**
 * reverse-engineer.ts — automated TENSOR + GRAPH reverse-engineering harness.
 *
 * The manual RE loop (walk the tree file-by-file with search/read calls) is replaced by a SINGLE
 * deterministic pass over the repository as a graph + a per-module STRUCTURE TENSOR:
 *   • GRAPH layer  — `arch-mine.buildAdjacency` (TS imports + markdown wikilinks) → dependency matrix A.
 *   • TENSOR layer — every module gets a structure vector (degree / in / out / SVD spectrum / PCA
 *     latent) computed in ONE pass; the whole repo is then a point cloud in latent space.
 *
 * `repoTensorSearch` joins the two: rank hits by GRAPH proximity (BFS distance from the seed match)
 * AND TENSOR similarity (cosine in latent space) — the overlay is the tensor×graph score. This is the
 * "tensor like searching instead of manual step by step" directive applied to the code tree.
 *
 * `reverseEngineer(target)` returns the full RE map for a file/concept in one shot: downstream
 * blast-radius (`causalCounterfactual`), upstream supply (`pointsOfFailure`), hidden coupling cluster
 * (`couplingClusters`), and a structure-tensor fingerprint — no step-by-step walk required.
 *
 * FLAG-OFF: inert unless you call buildRepoGraph / repoTensorSearch / reverseEngineer. READ-ONLY:
 * it never mutates the repo. Deterministic: no RNG / Date / network. Falsifiable RED+GREEN.
 */

import { readFileSync, readdirSync, statSync, existsSync } from 'node:fs';
import path from 'node:path';
import {
  buildAdjacency,
  couplingClusters,
  isolatedNodes,
  findCycle,
  pointsOfFailure,
  causalCounterfactual,
  structureVector,
  mineGraph,
  type MineReport,
} from './analytics/arch-mine.ts';
import { svd, pcaFit, pcaProject, type Mat } from './analytics/matrix.ts';

export interface RepoGraph {
  root: string;
  nodes: string[]; // namespaced ids "<prefix>:<relpath>"
  rel: string[]; // relpath parallel to nodes
  A: Mat;
  /** per-node flattened structure tensor (degree/in/out + SVD spectrum + PCA latent). */
  tensors: number[][];
  /** PCA model over the tensor cloud (for projecting queries). */
  pca: ReturnType<typeof pcaFit>;
  byRel: Map<string, number>; // relpath -> node index
}

const SKIP = new Set(['node_modules', '.git', 'target', 'dist', 'build', '.bebop', 'spikes']);

/** Recursively collect .ts/.tsx/.md files under `root` (bounded depth, deterministic order). */
export function scanRepo(root: string, maxDepth = 10, prefix = 'repo'): { id: string; source: string; isMarkdown?: boolean }[] {
  const out: { id: string; source: string; isMarkdown?: boolean }[] = [];
  const walk = (dir: string, depth: number) => {
    if (depth > maxDepth) return;
    let entries: string[];
    try { entries = readdirSync(dir); } catch { return; }
    entries.sort();
    for (const e of entries) {
      if (SKIP.has(e)) continue;
      const full = path.join(dir, e);
      let st;
      try { st = statSync(full); } catch { continue; }
      if (st.isDirectory()) { walk(full, depth + 1); continue; }
      if (!/\.(ts|tsx|md)$/.test(e) || e.endsWith('.test.ts') || e.endsWith('.d.ts')) continue;
      const rel = path.relative(root, full).replace(/\\/g, '/').replace(/\.(ts|tsx|md)$/, '');
      let source = '';
      try { source = readFileSync(full, 'utf8'); } catch { continue; }
      out.push({ id: `${prefix}:${rel}`, source, isMarkdown: e.endsWith('.md') });
    }
  };
  walk(root, 0);
  return out;
}

/** Build the tensor+graph model of a repo in ONE pass. Deterministic. */
export function buildRepoGraph(root: string, maxDepth = 10): RepoGraph {
  const modules = scanRepo(root, maxDepth);
  const adj = buildAdjacency(modules);
  const { nodes, A } = adj;
  const rel = nodes.map((n) => n.split(':').slice(1).join(':'));
  const byRel = new Map<string, number>();
  rel.forEach((r, i) => byRel.set(r, i));

  // TENSOR layer: per-node structure vector.
  const raw = nodes.map((_, i) => {
    let inD = 0, outD = 0;
    for (let j = 0; j < nodes.length; j++) { outD += A[i][j]; inD += A[j][i]; }
    const sv = svd(adj.A).S;
    return [outD, inD, outD + inD, sv[0] ?? 0, sv[1] ?? 0, sv[2] ?? 0];
  });
  const pca = raw.length >= 2 ? pcaFit(raw) : pcaFit([[0, 0, 0, 0, 0, 0], [0, 0, 0, 0, 0, 0]]);
  const tensors = raw.map((r) => (raw.length >= 2 ? pcaProject(pca, r) : [0, 0, 0, 0, 0, 0]));

  return { root, nodes, rel, A, tensors, pca, byRel };
}

function cosine(a: number[], b: number[]): number {
  let dot = 0, na = 0, nb = 0;
  for (let i = 0; i < a.length; i++) { dot += a[i] * b[i]; na += a[i] * a[i]; nb += b[i] * b[i]; }
  return na && nb ? dot / Math.sqrt(na * nb) : 0;
}

export interface TensorSearchHit {
  node: string;
  rel: string;
  /** graph BFS distance from nearest seed match (0 = seed itself). -1 if unreachable. */
  graphDist: number;
  /** tensor cosine similarity to the query latent (0..1). */
  tensorSim: number;
  /** combined overlay score (weighted graph + tensor). */
  score: number;
}

/**
 * Tensor+graph joint search. Instead of walking the tree step-by-step, this:
 *   1. seeds from nodes whose relpath OR source contains `query` (substring, deterministic);
 *   2. computes GRAPH distance via BFS over A from the seeds;
 *   3. computes TENSOR similarity of every node to the seed-mean latent;
 *   4. returns the top-K overlay (graphDist + tensorSim + combined score).
 * A node that is BOTH graph-near AND tensor-similar ranks highest — the tensor×graph search.
 */
export function repoTensorSearch(
  graph: RepoGraph,
  query: string,
  opts: { topK?: number; graphWeight?: number; tensorWeight?: number } = {},
): TensorSearchHit[] {
  const topK = opts.topK ?? 10;
  const gw = opts.graphWeight ?? 0.5;
  const tw = opts.tensorWeight ?? 0.5;
  const q = query.toLowerCase();

  // 1) seed set
  const seedIdx = new Set<number>();
  graph.nodes.forEach((id, i) => {
    if (id.toLowerCase().includes(q) || graph.rel[i].toLowerCase().includes(q)) seedIdx.add(i);
  });
  // fall back: match by source content (bounded scan) if no path/rel match
  if (seedIdx.size === 0) {
    graph.nodes.forEach((id, i) => {
      const src = readRepoSource(graph, i);
      if (src.toLowerCase().includes(q)) seedIdx.add(i);
    });
  }
  if (seedIdx.size === 0) return [];

  // 2) graph distance (BFS over undirected adjacency from seeds)
  const dist = new Array(graph.nodes.length).fill(-1);
  const queue = [...seedIdx];
  seedIdx.forEach((s) => (dist[s] = 0));
  while (queue.length) {
    const u = queue.shift()!;
    for (let v = 0; v < graph.nodes.length; v++) {
      if ((graph.A[u][v] > 0 || graph.A[v][u] > 0) && dist[v] === -1) {
        dist[v] = dist[u] + 1;
        queue.push(v);
      }
    }
  }

  // 3) tensor query latent = mean of seed tensors; similarity to each node
  const dim = graph.tensors[0]?.length ?? 0;
  const qLat = new Array(dim).fill(0);
  for (const s of seedIdx) for (let j = 0; j < dim; j++) qLat[j] += graph.tensors[s][j] / seedIdx.size;
  const maxDist = Math.max(1, ...dist.filter((d) => d >= 0));

  const hits: TensorSearchHit[] = [];
  for (let i = 0; i < graph.nodes.length; i++) {
    const gd = dist[i];
    if (gd === -1) continue;
    const ts = cosine(graph.tensors[i], qLat);
    // graph proximity normalized to [0,1] (0 dist = 1.0); tensor sim already ~[-1,1]→clamp to [0,1]
    const gProx = 1 - gd / maxDist;
    const tNorm = (ts + 1) / 2;
    const score = gw * gProx + tw * tNorm;
    hits.push({ node: graph.nodes[i], rel: graph.rel[i], graphDist: gd, tensorSim: ts, score });
  }
  return hits.sort((a, b) => b.score - a.score).slice(0, topK);
}

// cache source reads inside a single search (deterministic, read-only)
const _srcCache = new Map<number, string>();
function readRepoSource(graph: RepoGraph, i: number): string {
  if (_srcCache.has(i)) return _srcCache.get(i)!;
  const full = path.join(graph.root, graph.rel[i] + (graph.nodes[i].endsWith('.md') ? '.md' : '.ts'));
  let s = '';
  if (existsSync(full)) { try { s = readFileSync(full, 'utf8'); } catch { /* ignore */ } }
  _srcCache.set(i, s);
  return s;
}

export interface ReverseEngineerMap {
  target: string;
  found: boolean;
  /** transitive downstream blast-radius (what breaks if target's contract changes). */
  downstream: string[];
  directDownstream: string[];
  /** upstream supply (what target depends on — target's own risk surface). */
  upstream: string[];
  isOrphan: boolean;
  inCycle: string[] | null;
  /** the hidden coupling cluster target belongs to (latent SVD band). */
  cluster: string[] | null;
  /** structure-tensor fingerprint (degree/in/out + SVD spectrum). */
  fingerprint: number[];
  /** full architecture-mining report for the repo. */
  report: MineReport;
}

/**
 * One-shot reverse-engineering map for a target file/concept. Replaces the manual step-by-step walk:
 * the dependency graph is already built, so the blast-radius is a BFS closure and the coupling is an
 * SVD band — both computed, not discovered by reading files one at a time.
 */
export function reverseEngineer(graph: RepoGraph, target: string): ReverseEngineerMap {
  const idx = graph.byRel.get(target) ?? graph.nodes.findIndex((n) => n.endsWith(':' + target) || n === target);
  const report = mineGraph(graph.nodes.map((id, i) => ({ id, source: readRepoSource(graph, i), isMarkdown: id.endsWith('.md') })));
  if (idx < 0) {
    return { target, found: false, downstream: [], directDownstream: [], upstream: [], isOrphan: true, inCycle: null, cluster: null, fingerprint: [], report };
  }
  const cf = causalCounterfactual(graph, graph.nodes[idx]);
  const pof = pointsOfFailure(graph, graph.nodes[idx]);
  const clusters = couplingClusters(graph);
  const myCluster = clusters.find((c) => c.members.includes(graph.nodes[idx]))?.members ?? null;
  const fp = structureVector(graph);
  void fp; // structureVector is whole-graph; per-node fingerprint computed inline:
  let inD = 0, outD = 0;
  for (let j = 0; j < graph.nodes.length; j++) { outD += graph.A[idx][j]; inD += graph.A[j][idx]; }
  const sv = svd(graph.A).S;
  const fingerprint = [outD, inD, outD + inD, sv[0] ?? 0, sv[1] ?? 0, sv[2] ?? 0];
  return {
    target: graph.nodes[idx],
    found: true,
    downstream: cf?.broken ?? [],
    directDownstream: cf?.direct ?? [],
    upstream: pof?.upstream ?? [],
    isOrphan: pof?.isOrphan ?? false,
    inCycle: pof?.inCycle ?? null,
    cluster: myCluster,
    fingerprint,
    report,
  };
}
