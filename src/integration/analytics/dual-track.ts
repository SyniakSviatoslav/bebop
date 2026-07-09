/**
 * N6 (2026-07-09): Dual-Track GNN hybrid seam — the "Constraint-Based Gatekeeper".
 *
 * The 2026 dump's core idea: keep a deterministic GRAPH (Truth Layer: facts, dependencies,
 * allowed routes) and a stochastic TENSOR advisor (Operational Layer: intuition). The advisor
 * may PROPOSE, but every proposal is overlaid on the graph and REJECTED if it contradicts a
 * known fact. This is the neuro-symbolic firewall made concrete for the agent's planning layer.
 *
 * This module is FLAG-OFF: it is a pure function `dualTrackGate`. Nothing imports it at runtime
 * unless a caller wires an advisor. No RNG, no SGD, no Date. Deterministic + falsifiable.
 */
import type { PointOfFailure } from './arch-mine.ts';

/** A stochastic advisor (LLM / GNN / heuristic). It PROPOSES, it never executes. */
export interface GnnAdvisor {
  /** Given a focus node, propose a target it thinks the system should move to/depend on. */
  propose(focus: string): { target: string; confidence: number } | null;
}

export interface TruthGraph {
  nodes: string[];
  /** A[i][j] > 0  ⇔  edge i→j exists in the deterministic Truth Layer. */
  A: number[][];
}

export interface DualTrackVerdict {
  honored: boolean; // true ⇒ the advisor's proposal is graph-consistent (kernel may act)
  reason: 'edge-exists' | 'no-advice' | 'unknown-focus' | 'no-such-edge' | 'low-confidence';
  advice: { target: string; confidence: number } | null;
  /** counterfactual blast-radius of `focus` (N4), surfaced for ops triage. */
  focusRisk: PointOfFailure | null;
}

/**
 * Gate an advisor proposal against the Truth Layer.
 *  - advisor returns null  ⇒ no-advice (don't act, don't hallucinate).
 *  - focus unknown to graph ⇒ unknown-focus (reject — advisor invented a node).
 *  - proposed target has NO edge from focus ⇒ no-such-edge (reject — advisor hallucinated a
 *    dependency/route that the deterministic graph says does not exist).
 *  - confidence below floor ⇒ low-confidence (reject — don't act on a weak hunch).
 *  - edge exists ⇒ honored.
 */
export function dualTrackGate(
  graph: TruthGraph,
  advisor: GnnAdvisor,
  focus: string,
  opts: { minConfidence?: number; counterfactual?: (adj: TruthGraph, f: string) => PointOfFailure | null } = {},
): DualTrackVerdict {
  const advice = advisor.propose(focus);
  if (!advice) return { honored: false, reason: 'no-advice', advice: null, focusRisk: null };
  const i = graph.nodes.indexOf(focus);
  if (i < 0) return { honored: false, reason: 'unknown-focus', advice, focusRisk: null };
  const minC = opts.minConfidence ?? 0;
  if (advice.confidence < minC) return { honored: false, reason: 'low-confidence', advice, focusRisk: null };
  const j = graph.nodes.indexOf(advice.target);
  if (j < 0 || graph.A[i][j] <= 0) return { honored: false, reason: 'no-such-edge', advice, focusRisk: null };
  const focusRisk = opts.counterfactual ? opts.counterfactual(graph, focus) : null;
  return { honored: true, reason: 'edge-exists', advice, focusRisk };
}
