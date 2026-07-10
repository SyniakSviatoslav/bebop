# ◈ Bebop

> **Bebop is a local-first coding-agent CLI with its own deterministic Rust/WASM guard kernel, a
> a living (VSA) memory — that drives any agent you already use.
> (Claude Code, Codex, OpenCode, Aider, Goose) behind one auditable, free-by-default, offline
> control plane, and self-evolves via a "freestyle bebop soul" loop.**

![Bebop live CLI — luminous helm, radio, boot guard, mission sign-off](docs/footage/bebop-session.gif)
> *Real output of the built `bebop` binary, rendered from a live PTY capture (no staging, no
> post-editing). Shows the sun-warm helm — ship loader, working feed, hints, context/authority
> gauges — then `radio` tuning SomaFM Lofi/Jazz, `boot` certifying the guard, and the `mission`
> cigar sign-off. Palette is the cosmo-noir luminous spec: ship `#F4C25A` · tele `#F2933E` ·
> void `#12100E`.*

What makes it **not just another wrapper** (verified, not claimed):

- **Bebop core** — a Rust/WASM guard OS (`bebop_core.wasm`) that *denies* auth/money/secrets/migrations
  unless a human approves. `bebop boot` proves the gates go RED. Math, not vibes.
- **Node identity** — every node gets a **hybrid post-quantum** self-certifying identity and an
  encrypted-at-rest vault: **ML-KEM-768 ⊕ X25519** KEM, **ML-DSA-65 ⊕ Ed25519** signature,
  **Argon2id** KDF, **XChaCha20-Poly1305** AEAD (`src/vault.rs`, pure Rust). The classical half
  keeps the node safe even if a PQ primitive regresses; a wrong passphrase / tampered blob /
  tampered id all fail closed. No central server, ever. `bebop node` prints it.
- **Living memory** — a bundled VSA hypervector + graph store with forgetting and a persisted
  snapshot. `bebop recall "kernel law"` returns real payloads. (The richer §0·GP retriever lives in
  the dowiz monorepo and is an optional add-on — `recall` says so honestly.)
- **Bebop soul** — `bebop self maintain|evolve|session` runs a fail-closed self-loop: the system
  tests itself, proposes corpus mutations through a checker gate, and records this session as a node.
- **Narration + looks** — `bebop init` picks a voice (bebop / plain / sarcastic / corporate-killer)
  and a theme accent (bebop / claude / opencode / codex / custom). They actually change the CLI.
- **New outfit (cosmo-noir)** — `bebop outfit` prints the ship's identity contract: Warm Cosmo-Noir
  (Cowboy Bebop × cosmo-gothic × Ukrainian irony), signal tele `#F2933E`, ship `#F4C25A` on void
  `#12100E`. *One meaningful color per view.* `bebop init` changes the accent for real.
- **Multipilot** — `bebop dispatch` now fans a task to N *specialist* pilots (distinct backends so no
  single failure mode dominates), a distinct synthesizer merges them, and the Rust field arbiter can
  veto the plan. "Copilot is now a multipilot." Falsifiable, no RNG.
- **Field-as-cost-surface (the unique feature)** — Bebop's planner reads a *deterministic graph-PDE
  field* as its cost function. You can **see** where a disruption will hurt (not just that it will),
  and **Top-K Contours** rank the worst-hit nodes so the arbiter's "no" is auditable. Visual explainer:
  [`docs/diagrams/field-sim-explainer.svg`](docs/diagrams/field-sim-explainer.svg). Comparison report:
  [`docs/design/field-sim-comparison-2026-07-09.md`](docs/design/field-sim-comparison-2026-07-09.md).
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
| `multipilot "<task>"` | Fan the task to N specialist pilots + distinct synthesizer; field arbiter may veto. |
| `outfit` | Print the ship's cosmo-noir identity contract (the "new outfit"). |
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

```bash
cargo test           # 259 Rust tests (243 bebop + 16 rust-core), RED+GREEN, 0 fail
cargo check          # 0 errors (typecheck)
node scripts/verify-doc-claims.mjs        # doc-claim falsifiability gate
node scripts/guardrail-falsifiable-proof.mjs   # every #[test] must be falsifiable
```

> The native runtime is **Rust/WASM** (no TypeScript in the live path). `cargo test` is the
> authoritative runner. The legacy TypeScript layer was archived to `archive/` (recoverable) but is
> no longer built or executed.

Full proof table: [`docs/VERIFICATION-MATRIX.md`](docs/VERIFICATION-MATRIX.md).

## Development & docs

- Rules: [`docs/RULES.md`](docs/RULES.md) — *Constant Doubt: no verification, no statement.* Also:
  **better less than sorry** — never state what isn't fact-checked.
- Architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) · features: [`docs/features/`](docs/features/)
- **Wiki** (the GitHub *wiki* tab needs a one-click enable in repo Settings, or a `repo`-admin token —
  the full wiki content already ships here and renders on GitHub): [`docs/wiki/`](docs/wiki/) —
  [Home](docs/wiki/Home.md) · [Field-Sim Comparison](docs/wiki/Field-Sim-Comparison.md) (unique feature) ·
  [Multipilot](docs/wiki/Multipilot.md) · [Outfit](docs/wiki/Outfit.md) · [Verification](docs/wiki/Verification.md).
- Translations: 🇺🇦 [Українська](README.uk.md) · License: AGPL-3.0 · DCO (`git commit -s`).

<p align="center"><i>𝓈ℯℯ 𝓎ℴ𝓊 𝓈𝓅𝒶𝒸ℯ 𝒸ℴ𝓌𝒷ℴ𝓎</i></p>
