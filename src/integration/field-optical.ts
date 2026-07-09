/**
 * field-optical.ts — OPTICAL (FFT-field) + VSA associative search over the tensor+graph field.
 *
 * EXPANSION of the operator's theory (2026-07-09): "the sim can be expanded with optical search &
 * realtime prediction on changes." Two additions on top of field-sim.ts:
 *
 *   1) OPTICAL SEARCH — rank nodes by OPTICAL-FIELD CORRELATION (FFT2D mask propagation, optic.ts) of
 *      each node's structural tensor, AND by VSA hypervector similarity (memory.ts). This is
 *      content-addressable: no sort key needed, the field itself is the index. Contrast with a
 *      binary-tree (k-d tree) search which needs a total order / metric and only sees Euclidean
 *      distance — blind to graph adjacency.
 *
 *   2) PREDICT-THEN-SEARCH — predictImpact (field-sim) gives the affected footprint of a change; the
 *      optical/VSA pass then ranks *what to look at first* inside that footprint (the wavefront is
 *      the priority queue). That is "realtime prediction on changes" made queryable.
 *
 * All deterministic, no RNG/Date in the math. FLAG-OFF: construct + call explicitly.
 */

import { opticalRecall } from './optical/field-recall.ts';
import { thinLensMask, type OpticalMask } from './optical/optic.ts';
import { embed, similarity } from '../memory.ts';
import { laplacian, FieldSim } from './field-sim.ts';
import { buildRepoGraph, type RepoGraph } from './reverse-engineer.ts';

export interface OptNodeHit { node: string; rel: string; graphDist: number; tensorSim: number; score: number; }

/** Reshape a flat structural tensor into an n×n grid (pad/truncate; n = next pow2 ≥ √len). */
function toGrid(flat: number[]): { grid: number[]; n: number } {
  let n = 1;
  while (n * n < flat.length) n <<= 1;
  const grid = new Array(n * n).fill(0);
  for (let i = 0; i < flat.length; i++) grid[i] = flat[i];
  return { grid, n };
}

/**
 * OPTICAL search: rank repo nodes by OPTICAL-FIELD correlation of their structural tensor against a
 * query node's tensor (FFT2D mask propagation). Returns nodes sorted by descending optical score.
 * The optical rank is the physical "which files light up together" signal — adjacency-aware because
 * the structural tensor already encodes the import graph.
 */
export function opticalNodeSearch(graph: RepoGraph, queryRel: string, mask?: OpticalMask): string[] {
  const qi = graph.rel.indexOf(queryRel);
  if (qi < 0) return [];
  const qGrid = toGrid(graph.tensors[qi]);
  const m = mask ?? thinLensMask(qGrid.n, 1, (2 * Math.PI) / qGrid.n);
  const cands = graph.tensors.map((t) => toGrid(t).grid);
  return opticalRecall(qGrid.grid, cands, m).map((i) => graph.rel[i]);
}

/**
 * VSA associative search: rank repo nodes by hypervector similarity of their rel-path (content
 * address). Deterministic, no sort key — pure associative recall. Complements the optical pass.
 */
export function vsaNodeSearch(graph: RepoGraph, queryRel: string): { rel: string; sim: number }[] {
  const qv = embed('file:' + queryRel);
  return graph.rel
    .map((rel) => ({ rel, sim: similarity(qv, embed('file:' + rel)) }))
    .sort((a, b) => b.sim - a.sim);
}

/**
 * PREDICT-THEN-SEARCH: given a change at `changeRel`, predict the affected footprint (field-sim
 * predictImpact) and then rank the footprint by OPTICAL + VSA score so the wavefront is inspected
 * first. Returns the ordered watchlist. This is "realtime prediction on changes" made queryable.
 */
export function predictThenSearch(
  graph: RepoGraph,
  changeRel: string,
  opts: { steps?: number; threshold?: number } = {},
): { rel: string; opticalRank: number; vsaSim: number; inFootprint: boolean }[] {
  const optical = opticalNodeSearch(graph, changeRel);
  const opticalRank = new Map(optical.map((rel, i) => [rel, i]));
  const vsa = vsaNodeSearch(graph, changeRel);
  const vsaSim = new Map(vsa.map((v) => [v.rel, v.sim]));
  const idx = graph.rel.indexOf(changeRel);
  const affected = new Set<number>();
  if (idx >= 0) {
    const L = laplacian(graph.A);
    const sim = new FieldSim(L, { mode: 'diffuse', dt: 0.1, coeff: 0.4, channels: 1 });
    sim.impulse(idx, 1);
    const r = sim.predictImpact(idx, { steps: opts.steps ?? 32, threshold: opts.threshold ?? 1e-3 });
    for (const a of r.affected) affected.add(a);
  }
  return graph.rel
    .map((rel, i) => ({
      rel,
      opticalRank: opticalRank.get(rel) ?? graph.rel.length,
      vsaSim: vsaSim.get(rel) ?? 0,
      inFootprint: affected.has(i),
    }))
    .sort((a, b) => (a.inFootprint === b.inFootprint ? a.opticalRank - b.opticalRank : a.inFootprint ? -1 : 1));
}
