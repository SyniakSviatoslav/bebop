# Bebop Architecture

> 📖 **Read time:** ~6 min · 🎧 **Listen:** [architecture narration (mp3)](narration/architecture-narration.mp3)
> + [transcript](narration/architecture-narration.md) · 🤖 **For agents:** facts also in
> [`llm-manifest.json`](../llm-manifest.json).

Bebop is a TypeScript agent shell over a **self-contained Rust/WASM guard kernel**. The shell owns
all cross-cutting policy; agents are interchangeable dumb executors behind a uniform envelope.

```
                        ┌─────────────────────────────────────────┐
   user task  ───────▶  │  bebop.ts  (CLI / command dispatcher)     │
                        └───────────────┬───────────────────────────┘
                                        │
            ┌───────────────────────────┼───────────────────────────┐
            ▼                           ▼                           ▼
     ┌──────────────┐          ┌────────────────┐          ┌──────────────────┐
     │  guard.ts    │          │  router.ts     │          │  copilot.ts      │
     │ red-line +   │          │ cheapest       │          │ doer → checker   │
     │ scope (deleg │          │ adequate model │          │ (distinct, native│
     │  to kernel)  │          │ lane per class │          │  DEFAULT-on)     │
     └──────┬───────┘          └────────────────┘          └────────┬─────────┘
            │                                                        │
            ▼                                                        ▼
   ┌──────────────────────┐                          ┌──────────────────────────┐
   │  core-wasm.ts        │   delegates to           │  conductor (routing.ts)   │
   │  load bebop_core.wasm│ ───────────────────────▶ │  selectBackend / rotate   │
   └──────────┬───────────┘                          └───────────┬──────────────┘
              │                                                    │
              ▼                                                    ▼
   ┌──────────────────────┐                          ┌──────────────────────────┐
   │  bebop_core.wasm      │                          │  backend.ts adapters      │
   │  Rust kernel:         │                          │  free / opencode / claude │
   │   decide(glob→regex)  │                          │  / codex / hermes / aider │
   │   embed(VSA)          │                          │  / goose / native        │
   └──────────────────────┘                          └──────────────────────────┘
```

## 1. The guard kernel (`crates/core` → `bebop_core.wasm`)

The single source of truth for the trust boundary. A faithful port of `guard.ts`'s glob→regex
semantics, compiled to WebAssembly with a **hand-rolled C-ABI** (no `wasm-bindgen`):

- `decide(target, op, extraDeny, scope, cwd) → { ok, kind, reason }`
  - `kind ∈ { ok, redline, scope, error }`
  - `glob_to_regex` mirrors the TS `toRegExp` (handles `**`, `*`, `?`, `[^/]`, anchors).
- Decision log (`LOG`) — append-only, replayable.
- Retriever: deterministic hash embeddings `embed`, `similarity`, `estimateTokens` (no network).
- Exports: `bebop_decide`, `bebop_result_ptr/len`, `bebop_embed`, `bebop_similarity`, `bebop_estimate_tokens`.

The TS loader (`src/core-wasm.ts`, zero-dep `WebAssembly.instantiate`) reads `src/bebop_core.wasm`,
instantiates it, and exposes `decide`/`embed`/`similarity`/`estimateTokens`. `guard.ts` calls the
kernel when the handle is present and falls back to its own TS port otherwise — **both engines are
proven to agree** (parity test).

## 2. Routing & the conductor (`router.ts`, `routing.ts`, `backend.ts`)

- `router.ts` — pure token-router: classify task → cheapest adequate lane (`haiku`/`sonnet`/`opus`).
  Red-line class **must** route to `opus`; doer **must not** waste `opus`. Pure + unit-tested.
- `backend.ts` — thin adapters. Each agent CLI is ONE adapter: build argv, parse stdout. The
  intelligence (routing, guard, tokens) is Bebop's, applied uniformly. `free` is special: it calls
  OpenRouter's free tier over `fetch` (no binary). `native` is the keyless deterministic stub.
- `routing.ts` — the conductor: `selectBackend` walks `profile.backendOrder`, skipping unavailable
  agents, always keeping `native` as the fail-safe. `rotate` tries the next healthy backend on
  failure. **Uniform across every agent** (no special-casing).

## 3. Copilot (`copilot.ts`) — doer → checker

Mirrors the kernel's Checker gate one level up: a task is **produced** by one backend (doer) and
**checked in real time** by a **distinct** backend (checker). On `reject`, the action is
quarantined (fail-closed). DEFAULT-on and native. Independence is enforced: the checker ≠ doer.

## 4. Memory (`memory.ts`, `knowledge.ts`, `store.ts`)

- `memory.ts` — the one living memory (VSA graph; deterministic embed). `selfEvolve` proposes
  corpus mutations; the checker gate + a resonance pre-check (ζ < 0.707 → under-damped → reject)
  keep self-evolution well-damped. Fail-closed, reversible.
- `knowledge.ts` — recall over the living-knowledge retriever; degrades honestly (no spawn) when
  the VSA CLI is absent.
- `store.ts` — content-addressed blob store.

## 5. L5 governor (`governor.ts`)

A servo: PID authority, ICIR factor health, resonance risk **before** any gain change, and >3σ
anomaly signals. Fed quality streams; emits math-proven authority. Applied live to any
agent/model/process via `bebop govern`.

## 6. Free-LLM default (`free-llm.ts`)

Maps the three routing lanes to OpenRouter's best-free models. Available when
`OPENROUTER_API_KEY` (or `OPENROUTER_FREE_KEY`) is set. **First in the default rotation**, so
Bebop runs on free models out of the box; falls through to `native` when no key.

## 7. Multi-agent switch (the abstraction the user asked for)

`bebop agents` lists every connected CLI with live status. `bebop use <backend>` promotes an agent
to default-first and persists it (refuses unconnected unless `--force`). The profile
(`~/.bebop/settings.json`) carries `backendOrder`; a migration injects `free` first on upgrade so
stale installs comply with the free-default promise.

## 8. Security boundaries

- Project `bebop.json` (untrusted): `model` only.
- `~/.bebop/settings.json` (trusted): `model` / `permissions` / `hooks`.
- Hooks: shell-less, argv-split, metacharacter-refused.
- Red lines enforced by the **Rust kernel** at every dispatch/run entry point.

## Build & verify

```bash
cd crates/core && bash build.sh     # wasm32 → src/bebop_core.wasm
cargo test -p bebop-core             # 7 Rust RED+GREEN tests
npm run typecheck && npm test            # 165 TS tests
bebop boot                           # live guard self-test
```
