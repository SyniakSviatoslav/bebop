# ◈ Bebop

> **Your kitchen, your ship, your cut.**
> A standalone coding-agent CLI that drives **any** connected agentic CLI — Claude Code, Codex,
> OpenCode, Hermes, Aider, Goose — behind one guard kernel, with **free LLMs by default** and a
> simple switch to any of them.

![Bebop live CLI session — real asciinema recording](docs/footage/bebop-session.gif)
> *Real footage: a live `bebop` session recorded with [asciinema](https://asciinema.org), converted to GIF with [agg](https://github.com/asciinema/agg). Shows the actual `boot` (guard self-certification), `status`, and `use native` output — no staging, no faking. Source cast: [`docs/footage/bebop-session.cast`](docs/footage/bebop-session.cast).*

Bebop is a complete, independent tool. Its own trust boundary (a Rust/WASM guard kernel), its own
retriever (VSA), its own token router, and its own copilot (doer→checker) live **in this repo** —
no other project required.

- **License:** AGPL-3.0 · **Runtime:** Node 22 LTS (no `better-auth` required for the CLI)
- **Brand:** Warm Cosmo-Noir. Main signal color: ship teal `#46B0A4`.
- **Policy:** every statement in this repo is backed by a live probe or a deterministic test. See
  [`docs/RULES.md`](docs/RULES.md) — **Constant Doubt: no verification, no statement.**

> 📖 **Read time:** ~9 min · 🎧 **Listen:** [README narration (mp3)](docs/narration/README-narration.mp3)
> + [transcript](docs/narration/README-narration.md) · 🤖 **For agents/bots:** [`llms.txt`](llms.txt)
> and structured [`llm-manifest.json`](llm-manifest.json).

**Key insights (read this if you read nothing else):**
1. Bebop is **one steering wheel for every coding agent** — switch agents with one command; your guards, memory, and hooks move with you (no lock-in).
2. The guard is **yours and testable** — a Rust/WASM kernel denies auth/money/secrets/migrations unless a human approves; `bebop boot` proves it fires.
3. **Free by default** — starts on OpenRouter's free tier; falls back to a keyless loop so you're never blocked.
4. **Every doc claim is provable** — fork it and re-run `npm test` + the probes in [`docs/VERIFICATION-MATRIX.md`](docs/VERIFICATION-MATRIX.md).
5. **Honest about limits** — not a model provider, not a sandbox, not a GUI, not multi-user. See [What Bebop is NOT](#what-bebop-is-not).

---

## The story (tell it to a 5-year-old)

> *Every great heist movie opens the same way: one crew, many specialists, and a plan that survives
> only because somebody is watching the door.*

Imagine you have a **robot helper** that can write computer code for you. But every robot helper
has its own rules, its own remote control, and its own favorite way of doing things. If you switch
from one robot to another, you have to learn a whole new way to talk.

**Bebop is the one steering wheel that fits every robot.** You sit in the Bebop seat, and Bebop
knows how to drive Claude-robot, Codex-robot, OpenCode-robot, and the rest. You say "go," and
Bebop decides which robot should do the job — usually the small, free one, unless the job is hard.

And Bebop has a **bodyguard**. Before any robot touches your files, the bodyguard checks: *"Is this
the money drawer? Is this the secret drawer? Is this the do-not-touch drawer?"* If yes, the robot is
**not allowed in** — unless a grown-up says it's okay. The bodyguard never gets tired and never
forgets. That's why Bebop can be trusted with your kitchen.

Oh, and the **best robot is free**. Bebop starts on a no-cost model, so you can build things before
you ever pay a cent.

---

## Why businesses actually care (the pain Bebop removes)

> *Every company that buys an agent ends up married to it. The wedding is cheap; the divorce is
> where the money goes.*

Most teams adopt an agentic CLI and then get **locked in**: their scripts, hooks, and guard rules
only work for that one vendor. Switching later means rewriting everything. Worse, the guard rails
(lawyers call them "compliance") are *inside* the vendor's black box, so you can't prove what the
agent was or wasn't allowed to do.

Bebop removes that pain in four concrete ways:

| Pain today | What Bebop does |
|---|---|
| **Vendor lock-in** — your automation only speaks one CLI's dialect | One control surface for **every** connected agent. Switch with `bebop use <agent>`; your hooks, memory, and guards move with you. |
| **Guard rails you can't audit** — "trust us" from a black box | A **fail-closed guard you own**: red-line globs (auth / money / secrets / migrations) + a scope allow-list, enforced by a Rust/WASM kernel you can read and test. `bebop boot` proves the gates deny on red. |
| **Cost surprise** — bill spikes from always-on premium models | **Free LLM by default**; the router sends each task to the *cheapest adequate* lane. No key? A keyless native loop still runs, so you're never hard-blocked. |
| **Unverifiable claims** — "it works, promise" | **Constant Doubt**: every doc line is backed by a live probe or a deterministic test ([`docs/VERIFICATION-MATRIX.md`](docs/VERIFICATION-MATRIX.md)). Fork it and re-run the proof yourself. |

Net: Bebop turns "we hope the agent is safe and cheap" into "we can **prove** the guard fired and
the cheap model handled the boring 80%."

---

## Quick start

> *Enough talk. Here is the ship; here are the keys.*

```bash
git clone https://github.com/SyniakSviatoslav/bebop
cd bebop
npm install
npm run build        # compiles the Rust/WASM guard kernel → src/bebop_core.wasm
npm link             # or just: npx tsx bebop.ts <cmd>
```

```bash
bebop boot                                          # guard self-test — refuses to start if gates can't go RED
bebop status                                        # agent rotation + what's connected
bebop agents                                        # every agentic CLI Bebop can drive, live
bebop use native                                    # (default) keyless loop — works with zero config
bebop dispatch "fix the red ship animation"        # runs behind the guard + copilot
bebop run doer                                      # full loop (deterministic native stub by default)
```
> `native` is the default and needs no key. To unlock the **free** tier, set
> `OPENROUTER_API_KEY` (or `OPENROUTER_FREE_KEY`) and run `bebop use free` (it refuses without a key —
> that is fail-closed, by design).

> The WASM kernel (`src/bebop_core.wasm`) is committed, so the CLI works without a Rust toolchain.
> Rebuild with `cd crates/core && bash build.sh`.

---

## Commands

> *The control panel. Every lever, labeled — no mystery switches.*

| Command | What it does |
|---|---|
| `boot` | Guard self-test. Red-line + scope gates must deny on bad and pass on good. Refuses to start otherwise. |
| `status` | Agent rotation + connection status (free first by default). |
| `agents` | List **every** agentic CLI Bebop can drive, with live status + the switch command. |
| `use <backend>` | Switch the default agent directly and persist it. Refuses an unconnected backend unless `--force`. |
| `run [doer\|reason\|redline]` | Full agentic loop. Routes the task class to the cheapest adequate model lane. |
| `dispatch "<task>"` | One-shot task through the guard + copilot. Red-line tasks are **denied before any agent runs**. |
| `route <class>` | Show the token-router decision for a task class. |
| `recall <query>` | Query in-process living memory only. The VSA retriever is **not bundled yet** — `recall` honestly says so instead of faking a result. |
| `govern "<0.9,0.6,...>"` | L5 telemetry governor (PID authority + ICIR + resonance) over a quality stream. |
| `self [maintain\|evolve\|session\|loop]` | Self-maintenance / self-evolution (fail-closed, reversible). |
| `node` | Encrypted-at-rest node identity (PQ + Ed25519). |
| `mcp` | Model Context Protocol server over stdio (zero new deps). |
| `init` | 5-axis personalization wizard → `~/.bebop/settings.json`. |
| `map [module]` | "Understand everything": render the real import graph as an SVG image. |
| `diagrams` | Regenerate every visual (project map + conceptual feature schemas). |
| `help` | This list. |

---

## How it's built (short version)

> *Under the hood, Bebop is two layers stacked — a talking shell and a remembering core — with a
> guard standing between them.*

Bebop is a TypeScript shell over a **Rust/WASM guard kernel**. The shell owns cross-cutting policy —
guard (red lines + scope), token router, copilot (doer→checker), memory (VSA + living knowledge),
and the L5 governor. Agents are thin adapters the conductor rotates through. The kernel
(`crates/core`, compiled to `bebop_core.wasm`) is a hand-rolled C-ABI module with **no wasm-bindgen**;
the TS loader instantiates it with zero dependencies and the guard delegates to it, falling back to
a faithful TS port otherwise.

The long, diagrammed version — with schemas for every subsystem — lives in the wiki:
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) · [`docs/features/`](docs/features/) ·
[`docs/VERIFICATION-MATRIX.md`](docs/VERIFICATION-MATRIX.md).

---

## Bebop vs the others (honest comparison)

> *No tool is the hero of every story. Here is where Bebop leads — and where you should pick someone
> else.*

The truth doctrine cuts both ways: Bebop is **not** magic, and it is **not** always the right tool.
Here is how it compares to the agentic CLIs and wrappers people actually use.

| | Bebop | Claude Code | Codex CLI | OpenCode | Aider | Goose | plain `aishell`/wrapper |
|---|---|---|---|---|---|---|---|
| **Drives multiple agents** | ✅ any connected CLI | ❌ Anthropic only | ❌ OpenAI only | ❌ its own model | ❌ model-specific | ❌ its own | ⚠️ scripted, no guard |
| **Free LLM by default** | ✅ OpenRouter free tier | ❌ paid | ❌ paid | ⚠️ depends | ⚠️ depends | ⚠️ depends | ⚠️ depends |
| **Guard you own & can test** | ✅ Rust/WASM, `boot` proves it | ⚠️ vendor settings | ⚠️ vendor policy | ⚠️ config | ⚠️ none built-in | ⚠️ config | ❌ none |
| **Fail-closed red lines** | ✅ denies auth/money/secrets | ⚠️ permissions | ⚠️ policy | ⚠️ config | ❌ | ⚠️ config | ❌ |
| **Verified-by-Math docs** | ✅ every line probed | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **Visual project map** | ✅ `bebop map` / `diagrams` | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ |
| **Standalone repo (no dowiz)** | ✅ | n/a | n/a | ✅ | ✅ | ✅ | ✅ |
| **Cost of switching agents** | low (one command) | high | high | high | high | high | low but unsafe |

**Where Bebop wins:** you want one auditable control plane over several agents, you must *prove* the
guard fired, you want free-by-default, and you want docs you can re-verify by forking.

**Where another tool wins:** if you only ever use one vendor's model and never need to switch, that
vendor's native CLI may feel tighter (Bebop adds an indirection layer). If you need a rich, hosted
UI or first-party enterprise support, a vendor product beats a CLI wrapper.

---

## What Bebop is NOT

> *Every honest product ships with a "do not" list. Here is ours — unvarnished.*

Loyalty to the truth means saying what this tool does **not** do. No hedging.

- **Bebop is NOT a model provider.** It does not train, host, or serve LLMs. It *drives* models
  through agents/adapters. The quality of output depends entirely on the agent/backend you point it at.
- **Bebop is NOT a sandbox.** The guard kernel denies *known* red-line paths (globs) and out-of-scope
  paths, but it is a policy gate, not an OS-level container. It will not stop a connected agent from
  doing something evil *outside* the paths it knows about. Run agents with least privilege.
- **Bebop is NOT a living-knowledge retriever (yet).** `recall` queries in-process living memory
  only. The VSA embedding retriever is described in the docs but **not bundled** in this repo; `recall`
  says so honestly rather than faking results.
- **Bebop is NOT a GUI.** It is a terminal CLI. There is no desktop app or web dashboard (the `sync`
  server is a dev transport, not a product UI).
- **Bebop is NOT a replacement for human review of money/auth/RLS changes.** The guard *refuses* those
  by default; when a human approves, the human owns the consequence. "Denied unless approved" is a
  speed bump, not a guarantee.
- **Bebop is NOT multi-user / not a hosted service.** One user, one `~/.bebop/settings.json`, local
  process. No accounts, no shared tenancy, no cloud control plane.
- **Bebop is NOT a build system, package manager, or CI runner.** It orchestrates agents; it does not
  replace `npm`/`cargo`/`git`/GitHub Actions (it does call them through agents).
- **Bebop does NOT guarantee task success.** The copilot (doer→checker) and router reduce risk; they
  do not make an agent omniscient. Bad prompts still produce bad code.

---

## Verification (what actually runs)

> *In Bebop, a claim without a receipt is just a rumor. Here are the receipts.*

Every statement above is exercised by the test suite and by live probing — see
[`docs/VERIFICATION-MATRIX.md`](docs/VERIFICATION-MATRIX.md) for the full 35-row proof table.

- **Guard RED+GREEN:** `bebop boot` certifies the gates deny on red, pass on green. The kernel test
  denies `auth/token` and allows `tools/bebop/x.ts`.
- **Dispatch denial is real:** `bebop dispatch "edit packages/db/migrations/002_users.sql"` exits
  non-zero with `⛔ DENIED by guard (rust)` (a regression test spawns the real CLI and asserts this).
- **Free default is real:** `bebop status` shows `free → … → native`; with a key, `dispatch` issues a
  real OpenRouter call.
- **Switch is real:** `bebop use native` persists it; `bebop use claude` (unconnected) is refused
  unless `--force`.
- **Counts (verified, not guessed):** `npm test` → **165** TS tests; `cargo test -p bebop-core` → **7**
  Rust kernel tests; `npx tsc --noEmit` → 0 errors.

## Development

```bash
npm run typecheck && npm test                 # TS gates (typecheck + 165 tests)
cd crates/core && bash build.sh               # rebuild the WASM kernel
cargo test -p bebop-core                       # Rust kernel unit tests (7 RED+GREEN)
bebop diagrams                                 # regenerate all SVG visuals
```

## License

AGPL-3.0. Contributions via DCO (`git commit -s`).

<p align="center"><i>𝓈ℯℯ 𝓎ℴ𝓊 𝓈𝓅𝒶𝒸ℯ 𝒸ℴ𝓌𝒷ℴ𝓎</i></p>

