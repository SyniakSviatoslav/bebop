# Escalations — human-arbitrated truth resolutions

When `scripts/logic-gate.mjs` (Enforcement model in `LOGIC-LAWS.md`) cannot
establish that a claim is true — because it is **unbacked**, **self-referential
(paradox)**, or a **suspected logical contradiction** — it writes an entry here
and returns exit code `2` (commit allowed, but tracked). A human arbiter (the
operator or a designated user) fills the `Resolution` field.

**Rules**
- `OPEN` escalations may ship, but must be resolved before a release cut.
- Resolution values: `TRUE — <ref>`, `FALSE`, `DEFER — <reason>`.
- Machine state lives in `.bebop/escalations.jsonl` (deduped, regenerated).
  This file is a rendered summary — do not hand-edit below the marker.
- Never delete an `OPEN` entry to make the gate green.

<!-- LOGIC-GATE:OPEN-ITEMS (regenerated each run; do not edit by hand) -->
## Open escalations (16) — human arbiter required

- **ESC-b259452574aa** [unbacked] `README.md:38` — - **Narration + looks** — `bebop init` picks a voice (bebop / plain / sarcastic / corporate-killer)
  - Arbiter: operator · Status: OPEN
- **ESC-aa8900135b97** [unbacked] `README.md:123` — | **zkVM `decide()` journal** | Every admitted command gets a tamper-evident digest over `(state, commandHash, seq)`. On by default at the kernel gate. Replay-verifiable. **Scope:** detects *accidenta
  - Arbiter: operator · Status: OPEN
- **ESC-c72c1edace5a** [unbacked] `README.md:126` — | **Optical field recall** | SVETlANNa/Meep optical primitive re-ranks `recall` candidates by field correlation, behind `opts.opticalRecall`. Advisory only — graph score dominates. | LIVE (knowledge.t
  - Arbiter: operator · Status: OPEN
- **ESC-6146ded2191c** [unbacked] `AGENTS.md:65` —    decisions, and ground-truth facts to the canonical corpus. Source of truth = the corpus, not chat.
  - Arbiter: operator · Status: OPEN
- **ESC-47b70acd788f** [unbacked] `docs/ARCHITECTURE.md:84` — A servo: PID authority, ICIR factor health, resonance risk **before** any gain change, and >3σ
  - Arbiter: operator · Status: OPEN
- **ESC-16bdc71ebfec** [unbacked] `docs/ARCHITECTURE.md:85` — anomaly signals. Fed quality streams; emits math-proven authority. Applied live to any
  - Arbiter: operator · Status: OPEN
- **ESC-76ccf002dde5** [unbacked] `docs/design/LOGIC-LAWS.md:242` — ## 16. Fast & deep learning — Feynman (for ALL agents' self-improvement)
  - Arbiter: operator · Status: OPEN
- **ESC-8d2889a2685e** [unbacked] `docs/design/LOGIC-LAWS.md:259` —   engage deliberate S2 for trust-boundary / red-line / math-proven claims.
  - Arbiter: operator · Status: OPEN
- **ESC-23aa9f9e29b1** [unbacked] `docs/design/LOGIC-LAWS.md:263` —   appeal to authority/bandwagon, correlation≠causation, slippery slope,
  - Arbiter: operator · Status: OPEN
- **ESC-bf11c41f4d85** [unbacked] `docs/design/LOGIC-LAWS.md:306` — voice (cosmo-noir, warm, a little chaotic-good). Therefore:
  - Arbiter: operator · Status: OPEN
- **ESC-01f19461a247** [unbacked] `docs/design/LOGIC-LAWS.md:324` —   exhibits a *named, detectable* fallacy (§17) or asserts a causal link on
  - Arbiter: operator · Status: OPEN
- **ESC-00163551ec3b** [unbacked] `docs/design/LOGIC-LAWS.md:325` —   mere correlation — same grounding path as §4/§9/§10–§14.
  - Arbiter: operator · Status: OPEN
- **ESC-4e5e02ac58eb** [unbacked] `docs/design/LOGIC-LAWS.md:383` —   asserts style-adaptation or prompt-expansion *correctness* without a
  - Arbiter: operator · Status: OPEN
- **ESC-1c5fe99a6ae3** [unbacked] `docs/design/LOGIC-LAWS.md:384` —   ground (e.g. "adapts to every user" unbacked).
  - Arbiter: operator · Status: OPEN
- **ESC-63eb279423bc** [unbacked] `docs/design/LOGIC-LAWS.md:389` —   perfectly (subjective), so it relies on the agent's own §17 self-
  - Arbiter: operator · Status: OPEN
- **ESC-9f594d9df384** [unbacked] `bebop2/README.md:3` — > Greenfield rebuild of bebop. NOT a refactor of `crates/bebop` — a parallel implementation
  - Arbiter: operator · Status: OPEN

## Resolved (0)
_none_
