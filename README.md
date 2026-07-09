# ◈ Bebop

> **Bebop is a local-first coding-agent CLI with its own deterministic Rust/WASM guard kernel, a
> post-quantum node identity, and a living (VSA) memory — that drives any agent you already use
> (Claude Code, Codex, OpenCode, Aider, Goose) behind one auditable, free-by-default, offline
> control plane, and self-evolves via a "freestyle bebop soul" loop.**

![Bebop live CLI session](docs/footage/bebop-session.gif)

What makes it **not just another wrapper** (verified, not claimed):

- **Bebop core** — a Rust/WASM guard OS (`bebop_core.wasm`) that *denies* auth/money/secrets/migrations
  unless a human approves. `bebop boot` proves the gates go RED. Math, not vibes.
- **PSQ identity** — every node gets a post-quantum (ML-KEM + ML-DSA) self-certifying identity and an
  encrypted-at-rest vault. No central server, ever. `bebop node` prints it.
- **Living memory** — a bundled VSA hypervector + graph store with forgetting and a persisted
  snapshot. `bebop recall "kernel law"` returns real payloads. (The richer §0·GP retriever lives in
  the dowiz monorepo and is an optional add-on — `recall` says so honestly.)
- **Bebop soul** — `bebop self maintain|evolve|session` runs a fail-closed self-loop: the system
  tests itself, proposes corpus mutations through a checker gate, and records this session as a node.
- **Narration + looks** — `bebop init` picks a voice (bebop / plain / sarcastic / corporate-killer)
  and a theme accent (bebop / claude / opencode / codex / custom). They actually change the CLI.
- **Privacy by construction** — local-first, no telemetry, no account, no cloud control plane. Your
  keys stay in your `~/.bebop/settings.json`; the guard refuses to transmit your PQ identity.

---

## Quick start

```bash
git clone https://github.com/SyniakSviatoslav/bebop
cd bebop && npm install && npm run build   # builds src/bebop_core.wasm (committed; no Rust needed to run)
bebop boot              # guard self-test — refuses to start if gates can't go RED
bebop use native        # default: keyless loop, zero config
bebop dispatch "fix the red ship animation"   # runs behind the guard + copilot
bebop init              # pick narration + looks (real customization)
```

`native` needs no key. For the free tier: `export OPENROUTER_API_KEY=… && bebop use free`.

## Commands

| Command | What it does |
|---|---|
| `boot` | Guard self-test (Rust/WASM). Denies on red, passes on green, or refuses to start. |
| `status` · `agents` | Agent rotation + every agentic CLI Bebop can drive, live. |
| `use <backend>` | Switch + persist the default agent (refuses an unconnected one unless `--force`). |
| `run [doer\|reason\|redline]` | Full loop; routes the task class to the cheapest adequate model. |
| `dispatch "<task>"` | One-shot task through guard + copilot. Red-line tasks are **denied before any agent runs**. |
| `route <class>` | Show the token-router decision. |
| `recall <query>` | Query the bundled living memory (VSA + graph). |
| `govern "<0.9,0.6,…>"` | L5 telemetry governor (PID authority + ICIR + resonance). |
| `self [maintain\|evolve\|session\|loop]` | Freestyle bebop soul — self-test / self-evolve / session-as-node. |
| `node` | Post-quantum self-certifying identity + encrypted vault. |
| `mcp` | Model Context Protocol server over stdio. |
| `init` | 5-axis personalization (narration + looks are real). |
| `map [module]` · `diagrams` | Render the real import graph as SVG; regenerate all visuals. |

## Why businesses care

- **No lock-in** — one control surface for every connected agent; your guards, memory, hooks move with you.
- **Guard you own & can test** — red-line globs + scope allow-list, enforced by a Rust/WASM kernel. `boot` proves it.
- **Free by default** — router sends each task to the cheapest adequate lane; keyless `native` still runs.
- **Every claim is probable** — fork it, run `npm test` + [`docs/VERIFICATION-MATRIX.md`](docs/VERIFICATION-MATRIX.md).

## What Bebop is NOT

- **Not a model provider.** It drives models through agents; output quality depends on the backend.
- **Not a sandbox.** The guard is a policy gate, not an OS container. Run agents with least privilege.
- **Not a GUI.** Terminal CLI only.
- **Not a replacement for human review** of money/auth/RLS — it *refuses* those by default; approval is yours.
- **Not multi-user / not hosted.** One user, one local process.

## Bebop vs the others (honest)

Bebop is a **combiner above** other agentic CLIs, not a replacement. It credits each tool's strengths:

- **Where Bebop leads:** one auditable guard over *several* agents, post-quantum node identity, a
  self-evolving living memory, free-by-default, fully offline. Other agents are strong at *their* models
  (Claude Code → Anthropic, Codex → OpenAI, OpenCode → local models, Aider → diffs, Goose → MCP/goals).
- **Where another tool wins:** if you only use one vendor's model and never switch, that vendor's native
  CLI is tighter (Bebop adds an indirection layer). If you need a hosted UI or first-party enterprise
  support, a vendor product beats a CLI wrapper.

## Sovereign Node (integrations + autonomy)

Bebop's deterministic kernel is the single authority ("as above, so below"). The reverse-engineered
tools below are wired as **non-invasive, feature-flagged** layers that compose with the kernel's
universal gate — they extend it, never fork it. Every wiring is RED+GREEN tested and falsifiable.

| Layer | What it adds | Status |
|---|---|---|
| **zkVM `decide()` journal** | Every admitted command gets a tamper-evident digest over `(state, commandHash, seq)`. On by default at the kernel gate. Replay-verifiable. **Scope:** detects *accidental* bit-flips of stored digests (tamper-*detection*); the digest is keyless recomputation, so it is forgeable by anyone who controls the stored state — not a cryptographic signature/MAC (see CHANGELOG 0.3.0 F5). | LIVE (native TS port; RISC Zero STARK *receipt* gated behind `cfg.zkReceipt` when the prover is available) |
| **TigerBeetle money boundary** | Structural conservation law (`amount>0`, `debit≠credit`, idempotent) composed into the kernel gate via `applyCommandChecked(.., money=true)`. | LIVE (engaged when `money` flag set) |
| **Active Inference advisor** | FEP policy selector (`adviseLoop`) over `{stuck, progressing, done}` belief, behind `cfg.activeInference`. Advisory; the guard still decides admission. | LIVE (loop.ts) |
| **Optical field recall** | SVETlANNa/Meep optical primitive re-ranks `recall` candidates by field correlation, behind `opts.opticalRecall`. Advisory only — graph score dominates. | LIVE (knowledge.ts) |
| **Zenoh mesh** | Drop-in inter-node transport. Ships as an interface; swap in when a peer mesh exists. | INTERFACE (single-node: no-op) |
| **FinalSpark wetware** | Bio-safe (50 mV) LIF co-processor research slot. Never in the guard gate. | RESEARCH ONLY |

**Autonomy:** `bebop self evolve` now records each approved corpus mutation as a tamper-evident kernel
command and exposes `verifySelfEvolution()` — the agent can prove its own evolution history is unbroken.
`bebop self maintain` runs the **full** test suite (`npm test` now covers `src/integration/**`).

## Verification

```
npm test            # 525 TS tests (RED+GREEN), 0 fail  [authoritative: node --test --import tsx 'src/**/*.test.ts']
cargo test -p bebop-core   # 7 Rust kernel tests
npm run typecheck   # 0 errors
```

> The authoritative runner is `node --test --import tsx 'src/**/*.test.ts'` — `npm test` now uses this
> glob and covers the integration layer. (Older `pnpm run test` missed `src/integration/**`.)

Full proof table: [`docs/VERIFICATION-MATRIX.md`](docs/VERIFICATION-MATRIX.md).

## Development & docs

- Rules: [`docs/RULES.md`](docs/RULES.md) — *Constant Doubt: no verification, no statement.* Also:
  **better less than sorry** — never state what isn't fact-checked.
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) · features: [`docs/features/`](docs/features/)
- Translations: 🇺🇦 [Українська](README.uk.md) · License: AGPL-3.0 · DCO (`git commit -s`).

<p align="center"><i>𝓈ℯℯ 𝓎ℴ𝓊 𝓈𝓅𝒶𝒸ℯ 𝒸ℴ𝓌𝒷ℴ𝓎</i></p>
