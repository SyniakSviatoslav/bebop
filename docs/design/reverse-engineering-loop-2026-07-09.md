# Reverse-Engineering Loop — Whole Project + Living Memory (2026-07-09)

- **Run:** `scanProjects` (bebop) + `reverseEngineeringLoop` (bebop + living-memory corpus).
- **Tool:** `src/integration/analytics/loop.ts` (deterministic; N4 `pointsOfFailure` / `findCycle` /
  `couplingClusters` under the hood). Loop test green (`arch-mine.test.ts`).
- **Pre-commit:** both gates GREEN; `npm run verify` → 456 pass / 0 fail.

## 1. What was scanned
- **bebop** (`/root/bebop-repo`): 240 nodes, 109 code edges (`.ts/.tsx`) + markdown docs.
- **living-memory** (`/root/.claude/projects/-root-dowiz/memory`): 151 nodes, 196 wikilink edges.
- Combined: 391 nodes, 305 edges.

## 2. Findings (honest — corrected after a first pass over-claimed)

### F1 — `loop.ts` cross-repo coupling is NOT actually computed (dead field)
The first pass reported "0 cross-repo edges" as if that were a measurement. On reading
`arch-mine.ts` `buildAdjacency`, its return is **`crossEdges: []` unconditionally** (the field is a
stub that is never populated). So the loop simply does NOT traverse edges between the `bebop:` and
`mem:` namespaces — the "0" was a default, not a finding.
- **Real gap:** to make the Operational (code) and Truth (memory) layers observable as a single graph,
  `buildAdjacency` must resolve `mem:`-prefixed wikilinks (in bebop docs) to the `mem:` namespace nodes.
  Small, flag-OFF enhancement. Recorded, not done this wave (no over-claim).
- **Not a runtime defect** — the loop is a CI/audit tool; its output is informational.

### F2 — Memory wikilink graph is a DENSE RELATEDNESS MESH (cycle is expected, not a defect)
`findCycle` returns a ≥3-node cycle in the memory corpus (e.g. `ground-truth → model-routing →
knowledge-as-circuits → metacognition → … → ground-truth`). I first thought this was a
"circular-reasoning hazard" and tried to break it — but removing one wikilink just surfaced another,
because the corpus's "Related:" sections form a near-clique. **A DAG-cycle detector on a relatedness
graph will always find a cycle.** So:
- `findCycle` is MEANINGFUL for the **bebop CODE graph** (where acyclic DAG is the invariant — and the
  code graph IS acyclic, see F3). It is INFORMATIONAL-only for the memory mesh.
- No memory edit was made to "fix" the cycle — doing so would delete genuine cross-references for no EV.
- **Honest conclusion:** F2 is not an action item; it is a scoping note about the tool's limits.

### F3 — bebop CODE graph is acyclic (good); 122 isolated nodes are expected leaves
`cycle === null` for bebop code — the dependency DAG is clean (the invariant `findCycle` is designed
to guard IS satisfied). The 122 "isolated" nodes are almost entirely top-level docs (`AGENTS.md`,
`README*`, `CONTRIBUTING`, `DCO`, `SECURITY`, `GOVERNANCE`) — expected leaves, not orphans-of-concern.
The densest cluster is the core hub: `bebop.ts · src/vault · src/mcp · src/knowledge · src/copilot ·
src/doctrine.test` (strength 7.17) — the agent's memory/dispatch core is correctly the gravitational center.

### F4 — Memory corpus clusters confirm the live arcs
Top clusters in memory match the active arcs in MEMORY.md: `ground-truth-over-proxy`,
`next-arc-agent-tooluse-hook-antirot`, `vsa-token-economy`, `rebuild-decision-rust-astro`. The corpus
graph is internally coherent (good) — F2's cycle is a by-product of that coherence, not a break.

## 3. Recommended next actions (max-EV, low cost)
1. **(Optional) Fix F1**: populate `crossEdges` in `buildAdjacency` by resolving `mem:`-prefixed
   wikilinks; then the loop can report true bebop↔memory coupling. Flag-OFF, CI-only.
2. **Leave F2/F3 as-is** — F2 is a tool-limit note; F3 is the desired healthy state.

## 4. Proof this loop is real (falsifiable)
- `arch-mine.test.ts` GREEN `reverseEngineeringLoop runs end-to-end with gap detectors` → pass.
- Code-graph acyclic: `node --import tsx -e "import('./src/integration/analytics/loop.ts').then(m=>console.log(m.scanProjects([{path:process.cwd(),prefix:'bebop'}]).cycle))"` → `null`.
- Memory mesh cyclic (expected): same call with the memory root → a ≥3-node cycle (informational).
- `crossEdges` is empty by construction: `buildAdjacency` returns `crossEdges: []` (read the source).
