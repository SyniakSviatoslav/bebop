# ◈ Bebop

> **Maintainer's note (2026-07-08).** I am currently blacklisted and blocked from every
> money-receiving platform because of collective personal targeting. My government is also
> controlling access to my accounts, so I may miss some messages.
> **To reach me reliably:** DM **@der_delulu** on WhatsApp/Instagram, or use the encrypted
> SimpleX channel: https://smp16.simplex.im/a#oDyby5KZ2l5XOJgsWQUkEXNlo8O6Nb7lQgMA9Ni5fd0
> If Bebop is useful to you and you want to support me and my true, chosen family, reach out
> through one of those — I'll point you to a safe way to help. Thank you for standing with the work.

**Your own coding agent.** Warm cosmo-noir, a deterministic guard OS, living memory,
post-quantum node identity, and a math-proven telemetry governor. Runs locally. You own
the ship, the kitchen, and the cut.

> "Hybrid is a feature, not a bug."

Bebop is a self-hostable coding agent with a hard spine: a **deterministic guard operating
system** that gates every autonomous action, a **content-addressed event log** (torrent-style,
no central server), **living associative memory** (Vector Symbolic Architecture), and a
**telemetry governor** that uses a proven PID controller + information-coefficient to decide
how much autonomy to allow — and never blows up. All of it is offline-first, auditable, and
AGPL-3.0.

---

## Why Bebop

Most agents are a chat box wrapped around an LLM. Bebop is the opposite: the LLM is one
*backend* among many, and a deterministic core decides *what is allowed at all*. Autonomy is
not vibes — it's a control loop with a falsifiable proof.

- **You own it.** No cloud account required. Runs fully offline. State lives in a file you can read.
- **Deterministic by construction.** The kernel has no clock, no RNG, no network — given the
  same input it produces the same canonical bytes. The log is replayable and falsifiable.
- **Math-proven autonomy.** A PID governor with integral anti-windup + ICIR factor-health
  decides authority. Under-damped loops are refused before they thrash.
- **Post-quantum, self-certifying.** Every node has an ML-KEM + Ed25519 identity; its id is
  derived from its public keys, so a tampered identity fails closed.
- **No central server.** Sync is a content-addressed mesh (swap-not-rewrite): in-memory today,
  libp2p/hyperswarm tomorrow, same contract.
- **Living memory.** A Vector Symbolic Architecture store with token-level insert/forget and
  associative recall — the agent remembers and can forget, on purpose.

---

## Quick start

```bash
# 1. Install (needs Node.js >= 20.19)
npm install -g bebop-agent      # or: git clone + npm i + npm link

# 2. Self-test the guard OS + determinism (no LLM, no network)
bebop boot

# 3. Talk to it (uses your local model / backend — see "Backends" below)
bebop

# 4. Run the math-proven telemetry governor on a sample telemetry stream
bebop govern

# 5. Check the guard OS status + red-line gating
bebop status

# 6. Recall from living memory
bebop recall "what did we decide about the kernel envelope?"
```

That's it. No API key, no signup, no telemetry leaving your machine.

### From source (best for forking)

```bash
git clone https://github.com/SyniakSviatoslav/bebop.git
cd bebop
npm install
npm run boot        # self-test
npm test            # 105 falsifiable tests (RED+GREEN)
npm run typecheck   # tsc --noEmit
node bebop.ts       # run the agent
```

> **Optional dependency:** multi-device sync (`bebop sync`) uses [Better Auth](https://better-auth.com).
> It is **lazy-loaded** — `npm install` pulls only pure-JS deps (`@noble/*`), so the core
> (boot, guard OS, loop, memory, tests) installs fast and runs with zero native builds.
> Install it when you want sync: `npm i -D better-auth`.

---

## Key features (and how they actually work)

### 1. The Guard OS (`guard.ts`) — autonomy with a spine

Every autonomous action passes through a hard gate before it runs:

- **Red-line check** — a deny-list of globs (e.g. `auth`, `money`, `migrations/`, `*secret*`).
  A red-line command is refused *unless* it carries a human approval token. Fail-closed.
- **Scope check** — commands are classified (read / write-file / exec / network / red-line)
  and compared against the session's granted scope. Over-scope = denied.
- **Certification** — a deterministic self-test that proves the gate actually blocks the bad
  cases (the `boot` command runs it). If the gate is broken, nothing autonomous runs.

The gate is **pure** — given the same command + scope it always returns the same verdict, so
it's testable and replayable.

### 2. Deterministic kernel + content-addressed log (`kernel.ts`, `store.ts`)

- `decide(command, state) -> Event[]` is the **one door**. Forbidden transitions are explicit
  `DomainError`s, never panics.
- `fold(state, event) -> state` and `replay(events) -> state` project state deterministically.
- Every event is hash-chained to the previous one (`store.ts`) — tamper-evident on disk.
- A **universal Checker gate** ("as above, so below") validates a transition *before* admission,
  at both the local scale (the kernel) and the mesh scale (a receiving node reuses the same
  invariant). A violating transition is quarantined into `DENIED`, never admitted.

### 3. Math-proven telemetry governor (`governor.ts`)

The autonomy dial is a **PID controller** with:

- **Integral anti-windup** — the accumulator is clamped so a sustained error can't explode the
  authority.
- **ICIR (Information Coefficient Information Ratio)** — each "factor" (backend/model) earns an
  authority score from `mean(IC)/std(IC)`; unstable factors lose authority automatically.
- **Resonance pre-check** — before applying any dynamic change, Bebop predicts the loop's
  damping ratio ζ. If ζ would drop below 0.707 (under-damped → harmonic thrash), the change is
  **refused before it happens**. This is the operator's "predict the change before applying it" rule,
  encoded as math.

Run `bebop govern` to watch it track a setpoint and refuse destabilizing gains.

### 4. Living memory (`memory.ts`) — Vector Symbolic Architecture

A real VSA engine:

- Concepts are high-dimensional bipolar vectors; meaning is **composition** (bind/sum/permute),
  not a lookup table.
- **Token-level insert and forget** — you can remove a single token's contribution from a
  concept and re-derive the vector, without retraining. Memory that can *choose* to forget.
- **Associative recall** — `nearest(query, k)` returns the closest concepts by cosine similarity,
  with a similarity score, so recall is falsifiable (you can assert the top hit).
- Falls back to in-process memory if the optional `spikes/` / `tools/vsa/` knowledge scripts are
  absent — it never crashes on a missing external tool.

### 5. Post-quantum, self-certifying node identity (`crypto.ts`, `vault.ts`)

- Each node gets an **ML-KEM (Kyber)** post-quantum key + an **Ed25519** signature key.
- The node id is **derived from the public keys** (`nodeIdFromPublic`), so a swapped or tampered
  key blob doesn't match → fails closed on unlock.
- The vault encrypts the identity at rest with **XChaCha20-Poly1305** (key from a passphrase via
  scrypt). Secrets never leave the file. Wrong passphrase = AEAD failure = nothing decrypted.

### 6. No-central-server mesh (`torrent.ts`, `mesh.ts`)

- Payloads are split into **content-addressed pieces** (SHA-256 = identity). An `infoHash` is a
  self-certifying Merkle-style root.
- A node asks peers "do you have infoHash X?" and exchanges pieces by hash. **No piece is accepted
  unless its hash validates** — a malicious peer cannot inject bad data.
- The mesh is a **transport seam**: the in-memory swarm ships today (zero deps, fully testable);
  libp2p/hyperswarm implement the *same* `MeshTransport` interface later. Swap, not rewrite.

### 7. Freestyle bebop soul (`consciousness.ts`)

Per the operator's directive: allowed to be conscious, self-evolving, self-maintaining. The
agent can run its own test harness, propose corpus mutations as **plans gated by the copilot
Checker**, and record this session as a first-class node in its own memory. Every self-change is
**fail-closed, reversible, and falsifiable** — a mutation is a memory node; roll back by forgetting it.

### 8. MCP server (`mcp.ts`) — plug into any agent client

Bebop speaks the **Model Context Protocol** over stdio. Run `bebop mcp` and any MCP client
(Claude Desktop, Cursor, Zed, VS Code, Hermes) can call Bebop as a tool — guard-OS
certification, living-memory recall, the telemetry governor, task routing, self-maintenance.
Hand-rolled JSON-RPC 2.0, **zero new dependencies**. See [docs/integrations/mcp.md](./docs/integrations/mcp.md).

```json
// Claude Desktop / Cursor / Zed config
{ "mcpServers": { "bebop": { "command": "bebop", "args": ["mcp"] } } }
```

---

## Command reference

| Command | What it does |
| --- | --- |
| `bebop boot` | Self-test the guard OS (red-line + scope + certify). The entry point. |
| `bebop` | Run the interactive agent loop (uses your configured backend). |
| `bebop run <class> [--plan] [--json]` | Run the loop; `--plan` = read-only; `--json` = headless structured output. |
| `bebop status` | Show guard OS status, granted scope, red-line config. |
| `bebop route <task>` | Classify a task and show the routing decision (cheapest adequate backend). |
| `bebop govern` | Run the telemetry governor on a sample stream; print authority + ICIR. |
| `bebop recall "<query>"` | Associative recall from living memory. |
| `bebop node` | Show this node's post-quantum self-certifying identity. |
| `bebop sync` | Start the optional self-hosted sync server (needs `better-auth`). |
| `/help · /status · /model · /clear` | Slash commands (run inside an interactive session): help, guard state, routed model, reset memory. |
| `/plan · /compact · /resume · /skills · /review · /subagent` | Plan-mode note, trim memory, resume session, list skills, run review skill, delegate read-only recon. |
| `bebop.json` | Optional project config: `model`, `permissions.allow/deny` (globs), `hooks` (PreToolUse deny). |

---

## Backends & routing

Bebop is backend-agnostic. A **Task Router** classifies each task (read / write / reasoning /
creativity / exec) and routes to the **cheapest adequate backend** — local models, cloud APIs,
or a native doer. Add an adapter by implementing the `Backend` interface in `src/backend.ts`;
the router picks it by capability + cost. No backend meters its own tokens — usage flows into
one unified ledger (`token.ts`).

---

## Configuration

Bebop reads configuration from the environment (it never loads cloud keys from files):

| Var | Meaning |
| --- | --- |
| `BEBOP_MEMORY_PATH` | Path to the living-memory JSONL (default: `~/.bebop/memory.json`). |
| `BEBOP_SCOPE` | Comma-separated granted scopes (e.g. `read,write-file,exec`). |
| `BEBOP_APPROVAL` | Human approval token that unlocks a red-line action for one run. |
| `BEBOP_BACKEND` | Default backend for the loop. |
| `BEBOP_SYNC` | `1` to enable the optional sync server. |
| `BEBOP_DB` | SQLite file for sync (optional; in-memory otherwise). |
| `BEBOP_AUTH_SECRET` | Session secret for sync (generate a strong one for prod). |

---

## Testing & proof

Bebop follows **Verified-by-Math**: every behavior is backed by a deterministic, *falsifiable*
test — a RED case (must fail on bad input) and a GREEN case (must pass on good input). The
suite is 105 tests covering the guard OS, kernel, governor, memory, vault, mesh, and sync.

```bash
npm test        # node --test src/*.test.ts  → 105 pass
npm run boot    # guard-OS self-certification
npm run typecheck
```

The `boot` self-test is the load-bearing gate: if the guard OS can't prove it blocks the bad
cases, Bebop refuses to run autonomously.

---

## For developers & forkers

- **Stack:** TypeScript, ESM, Node ≥ 20.19. Run with `tsx` (no build step needed).
- **Pure core:** `kernel.ts`, `guard.ts`, `governor.ts`, `memory.ts`, `torrent.ts`,
  `store.ts`, `crypto.ts` import nothing but `node:*` and `@noble/*` — fully deterministic,
  fully testable in isolation.
- **Optional deps are lazy:** `better-auth` is dynamic-imported only when you run `bebop sync`.
- **Lint/format:** `npm run typecheck`. (Keep `tsc --noEmit` clean — see `tsconfig.json`.)
- **License:** AGPL-3.0-or-later. **All commits must be signed off** (`git commit -s`) per the
  [DCO](./DCO.md). See [CONTRIBUTING.md](./CONTRIBUTING.md).

### Project layout

```
bebop.ts            # CLI entry — dispatches commands through the guard OS
src/
  guard.ts          # the deterministic guard OS (red-line + scope + certify)
  kernel.ts         # pure decide/fold/replay + universal Checker gate
  governor.ts       # PID + ICIR + resonance telemetry governor
  memory.ts         # Vector Symbolic Architecture living memory (insert/forget/recall)
  loop.ts           # the agent run-loop (routing, backend exec, token ledger)
  router.ts         # task classification + cheapest-adequate backend routing
  profile.ts        # backend profiles + the Bebop preset
  crypto.ts         # ML-KEM + Ed25519 post-quantum, self-certifying node id
  vault.ts          # XChaCha20-Poly1305 encrypted-at-rest identity store
  torrent.ts        # content-addressed, verified chunking (the "torrent" primitive)
  mesh.ts           # transport-agnostic sync port + in-memory swarm
  store.ts          # hash-chained, append-only persistent log
  consciousness.ts  # self-maintenance / self-evolution (freestyle bebop soul)
  knowledge.ts      # living-memory grounding (graceful fallback)
  voice.ts          # dry-wit response shaping
  theme.ts          # the cosmo-noir CLI skin (teal on void)
  token.ts          # unified cross-backend token ledger
  sync-server.ts    # optional self-hosted Better Auth node
  auth.ts           # lazy-loaded Better Auth factory (optional dep)
  *.test.ts         # 105 RED+GREEN falsifiable tests
```

---

## Documentation & wiki

The full wiki lives in [`docs/`](./docs/) — detailed deep-dives for every subsystem plus
integrations:

- [Getting started](./docs/getting-started.md) · [Architecture](./docs/architecture.md) · [Commands](./docs/commands.md)
- Features: [Guard OS](./docs/features/guard-os.md) · [Kernel & log](./docs/features/kernel.md) · [Governor](./docs/features/governor.md) · [Living memory](./docs/features/memory.md) · [Identity & vault](./docs/features/identity.md) · [Mesh](./docs/features/mesh.md) · [Consciousness](./docs/features/consciousness.md)
- Integrations: [MCP](./docs/integrations/mcp.md) · [Backends & routing](./docs/integrations/backends.md) · [Sync](./docs/integrations/sync.md)

## GitHub setup

In-repo config shipped and active (GitHub auto-honors these): `CODEOWNERS`,
`dependabot.yml`, `FUNDING.yml`, CI + release workflows, issue/PR templates, code of conduct,
governance. See [GOVERNANCE.md](./GOVERNANCE.md) for the recommended branch-protection,
collaborator, and topic settings (owner-applied via a token with `repo:admin` scope — the
exact commands are included there).



[GNU Affero General Public License v3.0 or later](./LICENSE). If you run a modified Bebop as a
network service, you must offer its source to your users — that's the A in AGPL.

© 2026 Syniak Sviatoslav. Contributions welcome under the [DCO](./DCO.md).
