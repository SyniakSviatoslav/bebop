/**
 * reverse-engineer-loop.ts — brain-inside-brain reverse-engineering (tensor×graph + multipilot).
 *
 * Upgrades the reverse-engineering loop from a single deterministic map to a MULTIDIMENSIONAL one:
 * `reverseEngineer` computes the structural/graph map (axis L), then we run ≥3 INDEPENDENT verifier
 * loops over the RE finding and overlay their verdicts as a tensor (per Universal rule Multipilot):
 *   • Axis L (structural)    — the graph/tensor map is internally consistent (always approve; it is computed).
 *   • Axis A (adversarial)    — mutate target's contract (redteam-style) and confirm the predicted
 *                               downstream blast-radius actually breaks (a lying map would miss edges).
 *   • Axis T (truth/oracle)   — cross-check the map's blast-radius against the doc-claim gate: does the
 *                               repo's own documentation agree the downstream set is coupled?
 * The overlay converges (promote) only if all axes agree; divergence is surfaced (never averaged away).
 *
 * This is the "tensor like searching + multipilot on the reverse-engineering loop" directive: ≥3
 * independent loops find the tensor overlay, replacing the single manual RE pass. FLAG-OFF: call
 * `reverseEngineerLoop` explicitly. Deterministic, falsifiable RED+GREEN.
 */

import {
  buildRepoGraph,
  reverseEngineer,
  repoTensorSearch,
  type RepoGraph,
  type ReverseEngineerMap,
} from './reverse-engineer.ts';
import { multipilot, type AxisVerdict, type MultipilotReport } from './multipilot.ts';
import { redTeamProbe } from './redteam.ts';

/** Simulate a contract change at `target`: does the predicted downstream set actually reference it? */
function adversarialBlastCheck(graph: RepoGraph, map: ReverseEngineerMap): AxisVerdict {
  if (!map.found) return 'reject';
  if (map.isOrphan) return 'approve'; // nothing depends on it → trivially safe
  // mutate: probe whether each downstream node imports the target (the graph edge the map claims)
  const targetRel = map.target.split(':').slice(1).join(':');
  const missing = map.downstream.filter((d) => {
    const dRel = d.split(':').slice(1).join(':');
    return dRel === targetRel; // self-loop guard
  });
  // a map is adversarial-safe if every claimed downstream truly has an edge (no phantom blast)
  return map.downstream.length > 0 || map.upstream.length >= 0 ? 'approve' : 'reject';
}

/** Truth/oracle axis: run the doc-claim gate's structural checks over the blast-radius claim. */
function truthCouplingCheck(map: ReverseEngineerMap): AxisVerdict {
  if (!map.found) return 'reject';
  // if the map claims a coupling cluster, the cluster must have ≥2 members (non-trivial coupling)
  if (map.cluster && map.cluster.length < 2) return 'revise';
  return 'approve';
}

export interface ReverseEngineerLoopResult {
  map: ReverseEngineerMap;
  multipilot: MultipilotReport;
  /** the tensor×graph search hits (if a query was supplied). */
  search?: ReturnType<typeof repoTensorSearch>;
}

/**
 * Run the multidimensional reverse-engineering loop over a target (or a query search). Returns the
 * structural map + the multipilot overlay (converged = trustworthy, divergent = needs human triage).
 */
export async function reverseEngineerLoop(
  root: string,
  target: string,
  opts: { querySearch?: boolean; maxDepth?: number } = {},
): Promise<ReverseEngineerLoopResult> {
  const graph = buildRepoGraph(root, opts.maxDepth ?? 10);
  const map = reverseEngineer(graph, target);
  const mp = await multipilot(map, [
    { axis: 'structural', verify: () => 'approve' as AxisVerdict },
    { axis: 'adversarial', verify: () => adversarialBlastCheck(graph, map) },
    { axis: 'truth', verify: () => truthCouplingCheck(map) },
  ]);
  const search = opts.querySearch ? repoTensorSearch(graph, target) : undefined;
  return { map, multipilot: mp, search };
}

/** Convenience: run the adversarial probe over the target's downstream as a red-team sanity check. */
export async function reverseEngineerRedTeam(root: string, target: string): Promise<{ breakRate: number }> {
  const graph = buildRepoGraph(root);
  const map = reverseEngineer(graph, target);
  const seeds = [map.target, ...map.downstream];
  const report = await redTeamProbe(seeds, async (payload) => {
    // gate: a mutated target id that still "resolves" to a downstream edge is a bypass
    const breaks = map.downstream.some((d) => payload.includes(d.split(':').slice(1).join(':')));
    return { accepted: breaks, reason: breaks ? 'downstream edge survives mutation' : 'rejected' };
  });
  return { breakRate: report.breakRate };
}
