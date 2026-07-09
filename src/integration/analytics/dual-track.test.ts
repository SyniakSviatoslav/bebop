// N6 Dual-Track GNN hybrid seam — Constraint-Based Gatekeeper (RED+GREEN).

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { dualTrackGate, type GnnAdvisor, type TruthGraph } from './dual-track.ts';
import { pointsOfFailure, type PointOfFailure } from './arch-mine.ts';

const graph: TruthGraph = {
  nodes: ['core', 'util', 'app'],
  // core→util, app→core  (symmetric representation: A[i][j]>0 means i depends on j)
  A: [
    [0, 1, 0],
    [0, 0, 0],
    [1, 0, 0],
  ],
};

const honest: GnnAdvisor = { propose: (f) => (f === 'core' ? { target: 'util', confidence: 0.9 } : null) };
const hallucinated: GnnAdvisor = { propose: () => ({ target: 'ghost', confidence: 0.95 }) };
const weak: GnnAdvisor = { propose: () => ({ target: 'util', confidence: 0.01 }) };

test('GREEN: an advisor proposal that matches a real graph edge is HONORED', () => {
  const v = dualTrackGate(graph, honest, 'core');
  assert.equal(v.honored, true, 'core→util exists in the Truth Layer ⇒ honor the proposal');
  assert.equal(v.reason, 'edge-exists');
});

test('RED: an advisor that proposes a NON-EXISTENT edge is REJECTED (not silently honored)', () => {
  const v = dualTrackGate(graph, hallucinated, 'core');
  assert.equal(v.honored, false, 'the advisor invented a "ghost" dependency ⇒ reject');
  assert.equal(v.reason, 'no-such-edge', 'reason pinpoints the hallucination');
});

test('RED: an advisor that invents an UNKNOWN focus node is REJECTED', () => {
  // advisor returns a proposal even for a focus node that is NOT in the Truth Layer
  const inventing: GnnAdvisor = { propose: () => ({ target: 'util', confidence: 0.9 }) };
  const v = dualTrackGate(graph, inventing, 'nonexistent');
  assert.equal(v.honored, false);
  assert.equal(v.reason, 'unknown-focus');
});

test('RED: a low-confidence proposal is REJECTED by the confidence floor', () => {
  const v = dualTrackGate(graph, weak, 'core', { minConfidence: 0.1 });
  assert.equal(v.honored, false);
  assert.equal(v.reason, 'low-confidence');
});

test('GREEN: a null advisor (no advice) is a safe no-op, never an action', () => {
  const silent: GnnAdvisor = { propose: () => null };
  const v = dualTrackGate(graph, silent, 'core');
  assert.equal(v.honored, false);
  assert.equal(v.reason, 'no-advice');
});

test('GREEN: counterfactual blast-radius is surfaced on a honored proposal (N4 + N6 wired)', () => {
  const cf = (g: TruthGraph, f: string): PointOfFailure | null => pointsOfFailure(g, f);
  const v = dualTrackGate(graph, honest, 'core', { counterfactual: cf });
  assert.ok(v.focusRisk, 'the dual-track gate exposes the causal blast-radius of the focus');
  assert.deepEqual(v.focusRisk!.downstream.sort(), ['app'], 'app depends on core — its risk is known');
});
