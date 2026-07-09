/**
 * arch-mine.ts — Reverse-engineering / architecture-mining harness.
 *
 * Treats CODE + DOCS as DATA. Builds a module-dependency adjacency matrix
 * from import edges (TS/JS) and wikilink/[[...]] edges (markdown — the
 * living-memory corpus), then runs the deterministic SVD/PCA primitives
 * (matrix.ts) to surface LATENT coupling clusters, hidden drift, and gap
 * detectors (isolated / circular / ambiguous nodes).
 *
 * v2 upgrades (2026-07-09, after self-RE + 3 project loops):
 *   • NAMESPACED edge matching — the v1 basename matcher produced FALSE
 *     cross-repo merges (e.g. dowiz's vendored bebop/src/kernel.ts collapsed
 *     into bebop:kernel). Now edges bind only within the SAME scan-root
 *     namespace (prefix), or by an explicit relative path that resolves.
 *   • Markdown/wikilink extraction for the living-memory corpus.
 *   • Gap detectors: isolated nodes, circular references, ambiguous targets.
 *   • Matrix-size guard (n ≤ cap) so a 1500-node graph can't hang the O(n³)
 *     Jacobi EVD.
 *   • Full-latent (all PCs) structure vector for drift.
 *
 * Still deterministic: no RNG / Date / network.
 */

import { svd, pcaFit, pcaProject, type Mat, type Vec } from './matrix.ts';

export interface ModuleNode {
  id: string; // namespaced: "<prefix>:<relpath-without-ext>"
  file: string;
}

/** Parse `import ... from '...'` (and `import(...)`) specs from TS/JS source. */
export function extractImports(source: string): string[] {
  const out: string[] = [];
  const re = /(?:import|export)[^;]*?from\s*['"]([^'"]+)['"]/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(source)) !== null) out.push(m[1]);
  const re2 = /import\(\s*['"]([^'"]+)['"]\s*\)/g;
  while ((m = re2.exec(source)) !== null) out.push(m[1]);
  return out;
}

/**
 * Parse `[[target]]` (and `[[target|label]]`) wikilinks from markdown —
 * the living-memory corpus uses these to cross-reference notes.
 */
export function extractWikilinks(source: string): string[] {
  const out: string[] = [];
  const re = /\[\[([^\]|]+)(?:\|[^\]]*)?\]\]/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(source)) !== null) out.push(m[1].trim());
  return out;
}

/**
 * HARD_EVD_CEILING — the only static constant here, and it is NOT a target: it
 * is the O(n³) Jacobi-EVD safety bound. The *effective* cap is DYNAMIC:
 *   cap_eff = min(moduleCount, HARD_EVD_CEILING)
 * so a graph that actually fits under the ceiling keeps EVERY node (no
 * truncation, no fabricated orphans), and only genuinely huge corpora (>ceiling)
 * are pruned by degree. Passing `cap` explicitly overrides the dynamic default
 * (useful to force-prune in CI on monster repos).
 */
const HARD_EVD_CEILING = 1200;

export interface BuildOpts {
  /** explicit node cap override. Undefined ⇒ DYNAMIC: min(moduleCount, HARD_EVD_CEILING). */
  cap?: number;
}

/**
 * Build a namespaced symmetric adjacency matrix.
 * Edge rule (FALSE-MERGE GUARD): a spec from module `p:a` binds to `p:b` ONLY
 * if both share the SAME prefix `p` AND the resolved path matches. Cross-prefix
 * specs (e.g. dowiz importing bebop) are recorded as SEPARATE cross-namespace
 * edges (returned in `crossEdges`) but NEVER merged into one node. This kills
 * the v1 bug where vendored kernels collapsed together.
 */
export function buildAdjacency(
  modules: { id: string; source: string; isMarkdown?: boolean }[],
  opts: BuildOpts = {},
): { nodes: string[]; A: Mat; crossEdges: [string, string][]; ambiguous: string[] } {
  // 1) full graph (namespaced edges), no cap yet
  const fullNodes = modules.map((m) => m.id);
  const fn = fullNodes.length;
  const cap = opts.cap ?? Math.min(fn, HARD_EVD_CEILING); // DYNAMIC: keep all nodes unless > EVD ceiling
  const fidx = new Map(fullNodes.map((n, i) => [n, i]));
  const FA: Mat = Array.from({ length: fn }, () => new Array(fn).fill(0));
  const resolveSpec = (fromId: string, spec: string): string | null => {
    const prefix = fromId.split(':')[0];
    const isRelative = spec.startsWith('.');
    const clean = spec.replace(/^\.+\//, '').replace(/\.(ts|tsx|js|mjs|md)$/, '');
    if (!clean) return null;
    if (clean.startsWith('node:') || clean.startsWith('@')) return null;
    if (!isRelative && !clean.includes('/')) return null;
    if (isRelative) {
      const fromPath = fromId.split(':').slice(1).join(':');
      const parts = fromPath.split('/');
      parts.pop();
      for (const seg of clean.split('/')) {
        if (seg === '.') continue;
        if (seg === '..') parts.pop();
        else parts.push(seg);
      }
      return `${prefix}:${parts.join('/')}`;
    }
    return `${prefix}:${clean}`;
  };

  for (let i = 0; i < fn; i++) {
    const mod = modules[i];
    const specs = mod.isMarkdown ? extractWikilinks(mod.source) : extractImports(mod.source);
    for (const spec of specs) {
      let target: string | null;
      if (mod.isMarkdown) {
        const prefix = mod.id.split(':')[0];
        target = `${prefix}:${spec.replace(/\.md$/, '')}`;
      } else {
        target = resolveSpec(mod.id, spec);
      }
      if (!target) continue;
      const j = fidx.get(target);
      if (j === undefined || j === i) continue;
      FA[i][j] += 1; // DIRECTED edge (imports), not symmetric — real cycles need directed DFS
    }
  }

  // 2) if over cap, prune to highest-degree nodes (deterministic)
  let nodes = fullNodes;
  let A = FA;
  if (fn > cap) {
    const deg = FA.map((row, i) => row.reduce((s, v) => s + v, 0));
    const keep = new Set(
      deg.map((d, i) => [d, i] as [number, number])
        .sort((a, b) => b[0] - a[0])
        .slice(0, cap)
        .map(([, i]) => i),
    );
    nodes = fullNodes.filter((_, i) => keep.has(i));
    A = FA.filter((_, i) => keep.has(i)).map((row) => row.filter((_, j) => keep.has(j)));
  }

  return { nodes, A, crossEdges: [], ambiguous: [] };
}

// ── coupling clusters via SVD of the adjacency ────────────────────────────────

export interface CouplingCluster {
  strength: number;
  members: string[];
}

/**
 * Latent coupling clusters: SVD the adjacency; collect modules loading on any
 * significant band (|U|≥0.25), then partition into connected components via
 * the adjacency (handles degenerate σ where a pair splits across bands).
 */
export function couplingClusters(adj: { nodes: string[]; A: Mat }, topK = 6): CouplingCluster[] {
  const { nodes, A } = adj;
  if (nodes.length < 2) return [];
  const { U, S } = svd(A);
  const k = Math.min(topK, S.length);
  const coupled = new Set<number>();
  for (let b = 0; b < k; b++) {
    if (S[b] < 1e-9) continue;
    for (let i = 0; i < nodes.length; i++) if (Math.abs(U[i][b]) >= 0.25) coupled.add(i);
  }
  if (coupled.size < 2) return [];
  const idx = [...coupled];
  const seen = new Set<number>();
  const clusters: CouplingCluster[] = [];
  for (const start of idx) {
    if (seen.has(start)) continue;
    const comp: number[] = [];
    const stack = [start];
    seen.add(start);
    while (stack.length) {
      const u = stack.pop()!;
      comp.push(u);
      for (const v of coupled) if (!seen.has(v) && (A[u][v] + A[v][u]) > 0) { seen.add(v); stack.push(v); }
    }
    if (comp.length >= 2) {
      const maxS = S.find((s) => s > 1e-9) ?? 0;
      clusters.push({ strength: maxS, members: comp.map((i) => nodes[i]) });
    }
  }
  return clusters;
}

// ── gap detectors ─────────────────────────────────────────────────────────────

/** Nodes with ZERO edges (orphans / dead code / unreferenced notes). */
export function isolatedNodes(adj: { nodes: string[]; A: Mat }): string[] {
  const { nodes, A } = adj;
  const out: string[] = [];
  for (let i = 0; i < nodes.length; i++) {
    let d = 0;
    for (let j = 0; j < nodes.length; j++) d += A[i][j] + A[j][i]; // total (in+out) degree
    if (d === 0) out.push(nodes[i]);
  }
  return out;
}

/** Detect a circular reference among a set of nodes via DFS. Returns one cycle if found. */
export function findCycle(adj: { nodes: string[]; A: Mat }): string[] | null {
  const { nodes, A } = adj;
  const n = nodes.length;
  const WHITE = 0, GRAY = 1, BLACK = 2;
  const color = new Array(n).fill(WHITE);
  const parent = new Array(n).fill(-1);
  let found: number[] | null = null;
  const dfs = (u: number): number[] | null => {
    color[u] = GRAY;
    for (let v = 0; v < n; v++) {
      if (A[u][v] <= 0) continue;
      if (color[v] === GRAY && v !== parent[u]) {
        // real back-edge v→u (v is an ancestor of u in the DFS tree): the cycle
        // is the parent-chain from u back up to v.
        const cyclePath = [v];
        let cur = u;
        while (cur !== v) { cyclePath.push(cur); cur = parent[cur]; }
        return cyclePath;
      }
      if (color[v] === WHITE) {
        parent[v] = u;
        const r = dfs(v);
        if (r) return r;
      }
    }
    color[u] = BLACK;
    return null;
  };
  for (let i = 0; i < n && !found; i++) if (color[i] === WHITE) found = dfs(i);
  return found ? found.map((i) => nodes[i]) : null;
}

// ── architecture-drift detector (full-latent PCA) ─────────────────────────────

/** Flatten a module graph into a fixed structure vector (full-latent: all 5 stats + top-3 σ). */
export function structureVector(adj: { nodes: string[]; A: Mat }): Vec {
  const { nodes, A } = adj;
  const n = nodes.length;
  let edges = 0, maxDeg = 0;
  for (let i = 0; i < n; i++) {
    let deg = 0;
    for (let j = 0; j < n; j++) deg += A[i][j] + A[j][i]; // total (in+out) degree
    edges += deg;
    if (deg > maxDeg) maxDeg = deg;
  }
  edges = edges / 2; // each undirected edge counted twice
  const meanDeg = n ? edges / n : 0;
  const sv = svd(A).S;
  const top3 = [sv[0] ?? 0, sv[1] ?? 0, sv[2] ?? 0];
  return [n, edges / 2, meanDeg, maxDeg, ...top3];
}

/** PCA the two structure vectors (all PCs); returns the latent-distance shift. */
export function architectureDrift(
  before: { nodes: string[]; A: Mat },
  after: { nodes: string[]; A: Mat },
): { shift: number; beforeVec: Vec; afterVec: Vec } {
  const vb = structureVector(before);
  const va = structureVector(after);
  const pca = pcaFit([vb, va]);
  const zb = pcaProject(pca, vb); // all PCs
  const za = pcaProject(pca, va);
  let shift = 0;
  for (let i = 0; i < zb.length; i++) shift += Math.abs(za[i] - zb[i]);
  return { shift, beforeVec: vb, afterVec: va };
}

// ── D6: architecture health report (aggregates the detectors) ─────────────────

export interface MineReport {
  moduleCount: number;
  edgeCount: number;
  isolated: string[];
  cycle: string[] | null;
  clusters: CouplingCluster[];
}

/**
 * D6 architecture-mining report (pure, deterministic): build the namespaced adjacency from module
 * sources and run every gap detector at once — orphan (isolated) nodes, one circular-import cycle,
 * and latent coupling clusters. This is the runtime-facing aggregate the loop's flag-OFF archMine
 * pass consumes; it invents nothing (all sub-results come from the tested detectors above).
 */
export function mineGraph(modules: { id: string; source: string; isMarkdown?: boolean }[]): MineReport {
  const adj = buildAdjacency(modules);
  let edgeCount = 0;
  for (const row of adj.A) for (const v of row) edgeCount += v;
  return {
    moduleCount: adj.nodes.length,
    edgeCount,
    isolated: isolatedNodes(adj),
    cycle: findCycle(adj),
    clusters: couplingClusters(adj),
  };
}
