# Bebop — Agent modes (build / plan / auto) + cinematic agent telemetry

Date: 2026-07-12 · Operator: SyniakSviatoslav · Status: RESEARCH + PLAN (not implemented)
Push-plans-first: this doc is committed + pushed BEFORE any code is written.
Verified-by: research (Claude Code modes, Dota/XCOM game-design patterns) + map to
existing bebop modules (enrich/tui/mission/multipilot/customize/agentic_git).

---

## 0. Scope (what the operator asked for, verbatim intent)

1. Adopt Claude's **build / plan / auto** mode practices.
   - **auto** = real autopilot: the user is NEVER bothered, agents decide everything,
     zero clarification prompts.
   - Exception: in auto mode, the **self-review of changes/sessions** must be MORE
     detailed — explaining *why* each change/action was chosen.
2. After ANY session / loop / series-of-loops, always show a **dynamic cinematic
   rewind** + **MVP / HIGHEST TOKEN USAGE / LEVELED UP / DEGRADED** agents.
   - Research game-design again (cinematic feel). Borrow agent telemetry "per match"
     from RTS (Dota 2 scoreboard). Agent comparison + dynamic rewind from XCOM 2
     mission debrief.
3. Add a **minimap** (zoomable in/out) showing agents' work across files.
4. Everything is CLI/terminal-native and **configurable in settings**.

---

## 1. Research findings (evidence-backed)

### 1.1 Claude Code permission modes (the model to adopt)
- Shift+Tab cycles: `default` → `acceptEdits` → `plan`. A 4th mode
  `bypassPermissions` exists via `--permission-mode bypassPermissions` or
  `--dangerously-skip-permissions` at launch (one-time confirm dialog).
- `plan` mode: reads + explores, but makes NO edits and runs NO commands that
  change state; it can only propose. This is exactly "build/plan/auto" intent.
- Headless: `claude -p` (or `--print`) runs non-interactively — used for CI/cron
  autonomous agents. Maps to bebop `auto` (no TTY, no prompts).
- Takeaway: the mode is a single axis (how much the agent may act without asking)
  + a headless flag. bebop already has `AgentState` + `tui` — we add a `Mode` axis.

### 1.2 Dota 2 post-game scoreboard (per-match agent telemetry)
Metrics that transfer 1:1 to an agent "match" (one session / one loop run):
| Dota stat      | bebop agent analog                                  |
|----------------|-----------------------------------------------------|
| K (kills)      | successful tool/command invocations                |
| D (deaths)     | failed calls / errors / reverted actions            |
| A (assists)    | actions that enabled another agent/pilot           |
| GPM            | tokens/min consumed                                 |
| XPM            | "experience"/learning gained (memory writes, skills)|
| Net worth      | value delivered (lines changed that passed review) |
| Hero damage    | impact: files touched / blast radius of a change    |
| Healing        | regressions prevented / drift corrected             |
Scoreboard is sortable, color-coded (team/gold), with a timeline of key events.

### 1.3 XCOM 2 after-action report (cinematic debrief)
- Per-soldier: kills, assists, hits, wounds, MISSIONS, rank-up (leveled up).
- Mission awards (named): e.g. "The Ranger", "Close Combat", "Iron Will".
- Rewind/replay: the debrief re-plays the mission timeline. bebop's `mission.rs`
  already has `scene()` + cigar-smoke `rewind` (cursor-up + redraw) — direct hook.
- Takeaway: debrief = scoreboard (Dota) + named awards (XCOM) + timeline rewind
  (XCOM replay, built on `mission.rs` + `agentic_git` history).

### 1.4 Minimap (RTS / XCOM tactical)
- Zoomable tile overview of the "arena" (the repo file-tree as territory).
- Agents rendered as blips moving across tiles as they touch files.
- Zoom in = see file-level detail; zoom out = see subsystem-level heat.
- TTY-native: ratatui `Canvas`/block-grid; degrade to a static ASCII map in pipes.

---

## 2. Design — Agent modes

Three modes on ONE axis (how much the agent may act without asking) + a headless
flag. No new abstraction beyond what `AgentState`/`tui` already imply.

| Mode    | Edits? | Commands? | Asks user? | Headless-safe | Notes |
|---------|--------|-----------|------------|---------------|-------|
| `plan`  | no     | read-only | never      | yes           | explore + propose only (Claude plan parity) |
| `build` | yes    | yes       | on red-line/trust boundary | yes | default dev mode; pauses on auth/money/RLS |
| `auto`  | yes    | yes       | **never**  | yes           | true autopilot; zero clarify() calls |

- **auto is the hard promise**: in `auto`, the agent MUST NOT call `clarify`.
  Any ambiguity is resolved by the operator's documented defaults / memory
  (red-line areas still gated, but by FAIL-CLOSED default, not by asking).
- **auto self-review is MORE detailed**: because nobody is watching live, the
  end-of-session self-review must explain *why* each change/action was chosen
  (the operator's rule). This is `mode == auto` → `verbose_self_review = true`.
- Config: `Profile { mode: "auto" | "build" | "plan", headless: bool, ... }`
  in `~/.bebop/profile.toml` (extend `customize::Profile`). Plus env override
  `BEBOP_MODE=auto` for CI/cron.

---

## 3. Design — Cinematic debrief (after session / loop / series)

Built on existing `mission.rs::mission_summary` + `agentic_git` history.

### 3.1 Trigger
- End of any `bebop` invocation that did work (session).
- End of a loop (stabilizer/opt-loop iteration).
- End of a series (batch of loops / multipilot fan-out).
- In `auto` headless: print the static (non-animated) debrief frame.

### 3.2 Structure (Dota scoreboard + XCOM awards + rewind)
```
  ◈ mission: <name>            mode=auto   dur=3m12s
  ── scoreboard (per agent/pilot) ──────────────────────────
  agent        K   D   A   tok/min  net¥   dmg    lvl
  copilot      14  2   5    1.8k      +320   feat  ▲L3
  pilot-2      9   1   3    2.1k      +180   core   ▲L2
  pilot-3      3   4   1    0.9k      -40    —      ▼DEGRADED
  ── MVP ───────────────────────────────────────────────────
  copilot  (highest net-worth delivered, 0 drift)
  ── HIGHEST TOKEN USAGE ────────────────────────────────────
  pilot-2  2.1k tok/min  (review: near context-window ceiling)
  ── LEVELED UP ─────────────────────────────────────────────
  copilot ▲L3 · pilot-2 ▲L2
  ── DEGRADED ───────────────────────────────────────────────
  pilot-3 ▼ (4 deaths, -40 net-worth, drift 71%)
  ── mission awards (XCOM-style, named) ─────────────────────
  ☆ "The Surgeon"  — copilot, zero-drift refactor
  ☆ "Glass Cannon" — pilot-2, high output / high token burn
  ── dynamic rewind ─────────────────────────────────────────
  [cigar-smoke rewind over agentic_git history: perceive→think→act→observe]
```
- **MVP / HIGHEST TOKEN USAGE / LEVELED UP / DEGRADED** are computed from the
  `enrich::Trace` + `tui::Telemetry` aggregates, not invented.
- **rewind** re-plays the `agentic_git` commit chain (content-addressed) as a
  cursor-animated timeline — reuses `mission.rs::scene` cursor-up/redraw.

### 3.3 Leveling model (XCOM rank-up)
- Each agent/pilot accumulates "xp" = verified-successful actions (K + A) minus
  reverts (D). `level = floor(sqrt(xp))` capped. Stored in `agentic_git`
  metadata so it persists across sessions (a real "leveled up" arc).
- DEGRADED = net-worth < 0 OR drift > threshold (reuses `Telemetry::drift`).

---

## 4. Design — Agent telemetry "per match" (Dota scoreboard)

Extend `enrich::Trace` + `tui::Telemetry` with match-scoped counters:
- K/D/A from tool-call outcomes (success/fail/enable).
- GPM = tokens / elapsed_min (already have `tokens` + `exec_ms`).
- XPM = memory writes + skills created (experience).
- Net worth = Σ(value of passed changes) − Σ(reverted).
- Hero damage = files touched / blast radius.
- Healing = drifts corrected / regressions prevented.
All counters are deterministic (LCG-free; they are real tallies). The scoreboard
widget renders in `tui.rs` (ratatui Table/Sparkline — already imported).

---

## 5. Design — Minimap (zoomable, file-territory)

- Arena = repo file-tree. Each file = a tile; tile heat = agent activity
  (commits/tokens touching it this match).
- Agents = blips (colored per `AgentState`/`Outfit` accent) that move across
  tiles as they work (driven by `agentic_git` per-step commit locations).
- Zoom: `zoom_level ∈ {repo, subsystem, file}`; `+`/`-` or `BEBOP_MINIMAP_ZOOM`.
- Render: ratatui `Canvas` (braille/dot) or a block-grid `Paragraph` for TTY;
  static ASCII in pipes. Reuses `tui` palette helpers (`c`, `blend`).

---

## 6. Configurability (all of it, in settings)

Extend `customize::Profile` (`~/.bebop/profile.toml`):
```toml
[agent]
mode = "auto"            # auto | build | plan
headless = false
# cinematic debrief
debrief.enabled = true
debrief.rewind = true
debrief.awards = true
debrief.verbose_self_review_in_auto = true   # operator rule
# scoreboard
scoreboard.metrics = ["K","D","A","GPM","networth","damage","level"]
scoreboard.min_agent_level_for_mvp = 1
# minimap
minimap.enabled = true
minimap.zoom = "subsystem"   # repo | subsystem | file
minimap.heat = "tokens"      # tokens | commits | files
# token policy (from the 17-point plan §token)
token_policy.default = "max-save"   # all agents except reasoning/review/research
```
Every flag has a sane default (operator: default ON, auto-adjusted by load).
No flag is hard-coded; all resolve through `Profile::load().resolve_outfit()`.

---

## 7. Map to existing modules (debt-aware, no new crate)

| New need                      | Existing module (extend, don't rewrite)     |
|-------------------------------|---------------------------------------------|
| Mode axis                     | `tui::AgentState` + `customize::Profile`    |
| Scoreboard counters           | `enrich::Trace` + `tui::Telemetry`          |
| Debrief + rewind animation    | `mission.rs::scene` / `mission_summary`     |
| Agent history / replay        | `agentic_git` (content-addressed)           |
| Multi-agent comparison        | `multipilot::Pilot` / `MultiPilotResult`    |
| Minimap blips                 | `tui` palette + `agentic_git` step locations|
| Settings                      | `customize::Profile` (`profile.toml`)       |

No new dependencies. ratatui (already a dep) covers Table/Canvas/Sparkline.

---

## 8. Implementation plan (phases, RED→GREEN gates)

**Phase A — Mode axis** (low risk)
- A1. Add `Mode` enum + `mode`/`headless` to `customize::Profile`; env `BEBOP_MODE`.
- A2. Wire `clarify` ban in `auto` (fail-closed: return operator default, never ask).
- A3. `auto` → `verbose_self_review = true`.
- GATE: `cargo test -p bebop` (Profile round-trips TOML; auto forbids clarify).

**Phase B — Scoreboard + Trace extension**
- B1. Add K/D/A/GPM/XPM/networth/damage counters to `enrich::Trace`.
- B2. Render scoreboard widget in `tui.rs` (ratatui Table).
- GATE: RED test — scoreboard shows 0 for empty trace; GREEN — matches Trace tallies.

**Phase C — Cinematic debrief**
- C1. Extend `mission.rs::mission_summary` to accept scoreboard + awards + rewind.
- C2. Compute MVP / HIGHEST TOKEN / LEVELED UP / DEGRADED from aggregates.
- C3. Rewind animates `agentic_git` history (reuse cursor-up/redraw).
- GATE: RED — debrief panics on missing history; GREEN — prints all 4 badges.

**Phase D — Minimap**
- D1. Tile arena from file-tree; heat from `agentic_git` step locations.
- D2. Zoom levels + static ASCII fallback in pipes.
- GATE: RED — zoom out of range errors; GREEN — blips track real commits.

**Phase E — Settings UI**
- E1. `bebop outfit` shows new `[agent]` block; `bebop init` sets defaults.
- E2. Every flag documented + defaulted in `Profile::default()`.
- GATE: doc-claim verifier still GREEN (test counts updated).

---

## 9. Open questions for operator (NOT blocking — auto mode decides)
- Q1: Should `plan` mode also be allowed to *propose* a PR, or only describe?
  (Default: describe only; auto may open the PR.)
- Q2: Minimap heat default — tokens or commits? (Default: tokens.)
- Q3: Leveling persistence location — `agentic_git` metadata vs a separate
  `agents.jsonl`? (Default: `agentic_git` metadata, content-addressed.)

These are surfaced here for transparency; in `auto` mode bebop picks the default
and records the choice in the verbose self-review.
