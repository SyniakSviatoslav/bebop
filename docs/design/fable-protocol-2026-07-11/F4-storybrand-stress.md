# FABLE ANGLE 4/10 — STORYBRAND UX AUDIT + 50% COURIER-DROP STRESS TEST

Scope: read `field.rs`, `wavefield.rs`, `multipilot.rs` (operator-named) **plus** the real
delivery-plane code the brief implies: `matcher.rs`, `cost_estimate.rs`, `reputation.rs`,
`pod.rs`, `ledger.rs`. Discipline: every claim cites `file:line`; gaps are named, not papered.

> **FRAMING CORRECTION (load-bearing).** `field.rs` and `multipilot.rs` are NOT the delivery
> dispatch path. `field.rs` is the *agent-planner* arbiter (veto tasks that touch
> secrets/auth/money/`matcher.rs`-adjacent keywords — see `field_gate_verdict` keyword map at
> `field.rs:129-144`). `multipilot.rs` is *LLM copilot* fan-out (distinct pilots argue, a
> synthesizer decides — `multipilot.rs:45`). Neither routes a courier to a customer. The actual
> courier-drop surface is `matcher.rs` (`match_orders`) + `cost_estimate.rs` (`hybrid_route`) +
> `reputation.rs` (trust → cost). The 50%-drop trace below goes through the REAL surface. Routing
> it through `field.rs`/`multipilot.rs` would be a category error; this doc does not.

---

## 1. THE FOUR STORYBRAND QUESTIONS

### Q1 — "Will this work for someone like me?"

**Restaurateur objection:** *"Does it understand my menu, my opening hours, my neighbourhood?
Will my customers even find me?"*

- **Code answer:** The delivery model is a **pure routing primitive**. `Order` is literally
  `{ id, src, dst }` — `matcher.rs:34-38`. There is **no menu, no hours, no catalog, no
  locality profile** anywhere in the read set. The protocol moves a packet courier→customer; it
  does not model a storefront.
- **Verdict:** PARTIAL / GAP. The *dispatch + settlement attribution* is real and solid. The
  *restaurateur storefront* (menu/hours/discovery) is **absent from code** — it would have to be
  an app layer on top of `match_orders`. Honest line for the restaurateur: *"It will route and
  pay for a delivery you already have; it does not yet know what you sell or when you're open."*

**Courier objection:** *"Won't it steal my profit? Is it too complex to run?"*

- **Code answer (profit):** Proof-of-Delivery (`pod.rs`) gives a **pseudonymous, non-repudiable**
  delivery claim: `order:<id>|courier:<vault_id>|at:<ts>|loc:<x,y>`, SHA512 + hybrid signature —
  `pod.rs:8-10, 44-56`. Settlement *can require* a valid POD (`pod.rs:19-22` — "settlement can
  require a valid POD proof"). Reputation feeds the cost surface so high-trust couriers are
  **preferred** (`reputation.rs:11-12, 85-93`), i.e. more jobs.
- **Code answer (complexity):** The matcher needs **no server** — `LocalMatcherClient` runs
  `match_orders` in-process (`matcher.rs:133-143`); the `MatcherClient` trait makes the
  dispatch box interchangeable (`matcher.rs:127-143`). That is genuinely low-ops.
- **GAP:** There is **no coded "POD → automatic payout" path** in the read set. `ledger.rs` is a
  generic double-entry money boundary (TigerBeetle invariant, `ledger.rs:78`), not a
  "release funds to courier X on valid POD Y" contract. The *no-30%-rent* claim is **structural**
  (no central server taking a cut) but the actual fee/payout model is **not modeled or verified
  here**. "Won't steal my profit" is therefore **real as architecture, unproven as accounting**.

### Q2 — "What is the risk / what aren't you telling me?"

**Objection:** *"Decentralized means no support desk. Who actually guarantees I get paid?"*

- **Code answer (attribution + ledger):** Payout is **cryptographically attributable**
  (`pod.rs` verify is pure-signature, no PII — `pod.rs:88-96`) and the ledger **fails closed on
  imbalance** — the TigerBeetle law: if a transfer doesn't balance, the ledger is corrupt
  (`ledger.rs:78`). Dispatch is **replicable + fingerprintable** — two surviving nodes MUST
  agree on assignments (`matcher.rs:100-121`, proven at `matcher.rs:274-290`).
- **GAP (the thing not being told):** The **fund-release / escrow contract** that turns a valid
  POD into actual money is **not in these files**. Who guarantees payout = an external
  settlement layer (on-chain / smart contract) that the read set does not contain. Also: **what
  happens if a courier's node drops *mid-delivery*** (after pickup, before POD)? Nothing in the
  read set models partial-progress payout or penalty. Honest disclosure: *"Your payout is
  attributable and the ledger can't silently lose money, but the actual release-of-funds contract
  lives outside this code, and mid-route failure is unhandled."*
- **Sybil risk (named in code):** `matcher.rs:6` states the blocker plainly — *"whoever feeds the
  most convincing (fake) graph wins."* Reputation mitigates: a fresh node is neutral `0.5`
  (`reputation.rs:70-71`) and **1 valid delivery ⇒ 0.75** (`reputation.rs:76-78`); a sybil still
  must produce real PODs to climb. Suspensions are **sticky** (`reputation.rs:53-61`) and make a
  node unreachable (`risk_premium ⇒ ∞`, `reputation.rs:87-88`). This is a real, honest mitigation
  — not a gap — but the bootstrap window (cheap throwaway PODs) is acknowledged only in comments.

### Q3 — "Is this worth the time/money/effort?"

**Objection:** *"Do I retrain staff? What do I save vs Glovo/Bolt?"*

- **Savings:** Structural — no hosted sequencer means no rent-taker (`matcher.rs:1-22`). That part
  is **real**. The specific "30% vs Glovo/Bolt" number is a **claim, not modeled** — no economics
  module exists in the read set.
- **Retrain / effort:** The protocol is a **library (`crate`)**, not an app. There is **no
  restaurateur UI, no staff workflow, no menu-entry screen** in the code. Effort to *run a node* is
  low (`LocalMatcherClient`, `matcher.rs:137`); effort to *operate a storefront on it* is unknown
  because that layer is absent. **GAP:** the "worth it" question for staff is unanswerable from
  this code — the operator-facing app is not in scope.

### Q4 — "What will life look like after?"

**Objection (restaurateur):** *"Will I own my data and my clients?"*
**Objection (courier):** *"Will I be my own boss, with no secret penalizing algorithm?"*

- **Own data / clients:** POD is **pseudonymous** — `courier_id` is a self-certifying vault id,
  "PII stays local" (`pod.rs:12-17`). Good for privacy. **GAP:** there is no
  *restaurateur customer-list / CRM* concept — the protocol deliberately does not centralize
  customer PII, but it also does not *give* the restaurateur a portable client list. "Own my
  clients" is **true as 'not held hostage by a platform' but false as 'there's a feature for it.'**
- **Courier own boss / no penalizing algo:** **STRONGEST REAL ANSWER.** Reputation is
  **deterministic and transparent**: `score = 0.5 + 0.5·d/(d+1)` (`reputation.rs:76-78`),
  `risk_premium = 1/trust` (`reputation.rs:85-92`). Route choice is explainable from those
  numbers. Suspensions only via **consensus KillSwitch**, sticky (`reputation.rs:43-47, 53-61`).
  No black box, no secret demerit. This is a genuine StoryBrand win and it is **in the code**.

---

## 2. UX TRUST INDICATORS — 3 CONCRETE, CODE-DERIVABLE UI SIGNALS

These are not mockups; each is computed from functions that already exist.

1. **Dispatch fingerprint + "unmatched" panel (proves no hidden sequencer).**
   Show the `fingerprint(resp)` content-hash (`matcher.rs:100-121`) for every dispatch batch, and
   render `resp.unmatched` as an explicit red "not yet assigned" list (`matcher.rs:64-92`) — never
   a silent drop. A restaurateur can paste the fingerprint into any other node and confirm it
   computed the identical assignment (`matcher.rs:274-290`). *Signal: "two random nodes agreed —
   nobody is secretly reordering your deliveries."*

2. **Per-order POD proof trail (proves the delivery actually happened).**
   For each completed order, render the canonical claim `order:<id>|courier:<vid>|at:<ts>|loc:<x,y>`
   (`pod.rs:44-49`) plus a green `verify_delivery` tick (`pod.rs:88-96`). Shows the **pseudonymous
   courier id, timestamp, geo-bound drop** — customer sees cryptographic completion with **zero
   PII**. *Signal: "your food was signed-for at 12:03 at your door, by a verifiable courier."*

3. **Transparent courier trust/risk badge (proves no penalizing algorithm).**
   Show the chosen courier's `score` and the `risk_premium` multiplier that drove the route
   (`reputation.rs:69-93`). *Signal: "This courier was picked because trust = 0.83 ⇒ ×1.2 cost,
   not because of a hidden rank."* The formula is on-screen and auditable.

---

## 3. 50%-COURIER-DROP STRESS TEST — TRACE THROUGH THE REAL SURFACE

**Setup:** a fleet graph; 50% of courier (source) nodes are removed from the `MatcherRequest`
before `match_orders`. (Detecting *that* a node dropped is **out of scope of these files** — see
Gap G3; the caller/transport must supply liveness.)

**Trace:**
- `match_orders` (`matcher.rs:74-93`) loops orders, calls `hybrid_route` per order.
- `hybrid_route` (`cost_estimate.rs:305-327`): Layer-1 spatial filter → Layer-2 BFS
  `reachable` (`cost_estimate.rs:123-142`) → Layer-3 A*+CH (`cost_estimate.rs:209-300`).
- If the courier source node is gone, `src >= n` ⇒ `hybrid_route` returns `None`
  (`cost_estimate.rs:318` BFS guard; `route` early-returns `None` at `cost_estimate.rs:218`).
- `match_orders` puts that order in `unmatched` (`matcher.rs:86`) — **surfaced, not dropped**.
- **No crash, no partial state.** `fingerprint` still computed on survivors (`matcher.rs:100`).

**Does it degrade gracefully or collapse?**
- **Graceful on safety:** never crashes; every order is either assigned or explicitly `unmatched`;
  0 silent drops. ✅
- **But throughput is linear in surviving couriers with NO redistribution** — see Gap G2: `Order`
  hard-binds `src` to one courier node (`matcher.rs:34-38, 78-84`). There is **no courier
  marketplace / auction** where any available courier can take any order. So if couriers are the
  scarce resource, **50% courier drop ≈ up to 50% of orders unmatched**, with no fallback. That is
  *graceful collapse* — safe, but not resilient.

**FALSIFIABLE RESILIENCE METRICS (each is a RED+GREEN test you can ship today):**
- **R1 (no-silent-drop invariant):** on a `MatcherRequest` with 50% courier nodes removed,
  `assignments.len() + unmatched.len() == orders.len()` AND no panic. *Refutes "collapse."*
- **R2 (assign-time monotonic non-increasing):** single-order `hybrid_route` time on the reduced
  graph ≤ on the full graph (smaller graph ⇒ ≤ same A* work; `cost_estimate.rs:209-300` is
  O(E + V log V)). So **p99 assign-time stays ≤ baseline T** — dropping nodes cannot *increase*
  it. Proves dispatch latency does not degrade under node loss.
- **R3 (replicability preserved):** fingerprint agreement among surviving independent nodes still
  holds (`matcher.rs:274-290`) on the reduced graph.
- **R4 (the honest weakness, falsifiable):** matched-fraction under 50% courier drop ≈
  (fraction of orders whose bound `src` survived). Assert `matched ≤ 0.5·orders + ε` when each
  order is pinned to a distinct dropped courier. This *proves* the no-redistribution gap (G2).

---

## 4. HONEST VERDICT — REAL vs MARKETING POETRY

**REAL (code-backed, citeable):**
- Dispatch is a pure, deterministic, **replicable** function — kills the single-sequencer
  (`matcher.rs:74, 100, 274-290`).
- POD = pseudonymous, non-repudiable delivery attribution (`pod.rs:8-96`).
- Reputation is **open, transparent, sybil-mitigated, sticky-suspension** (`reputation.rs`).
- Ledger **fails closed on imbalance** (`ledger.rs:78`).
- Planner/wave arbiter **fails closed** on red-line (secrets/money) — `field.rs:88-95`,
  `wavefield.rs:518-593`.
- 50% courier drop ⇒ **no crash, no silent drop** (R1); latency does not degrade (R2).

**POETRY / GAP (say it or lose credibility):**
- **G1 — No storefront.** Menu/hours/local discovery absent (`Order` is `{id,src,dst}`,
  `matcher.rs:34`). Restaurateur Q1 only half-answered.
- **G2 — No courier marketplace.** Orders are pinned to one `src` courier (`matcher.rs:34,78`);
  no reassignment/auction. 50% drop = up to 50% lost throughput with no fallback (R4). This is
  the single biggest resilience gap.
- **G3 — No node liveness.** Nothing in the read set detects a dropped node (no heartbeat/
  gossip health). Drop must be supplied by an external p2p layer (hinted `zenoh.rs`, not read
  here). The stress test *assumes* liveness input the code doesn't produce.
- **G4 — No payout contract.** "POD → funds released" and "who guarantees payout" are external
  (`pod.rs:19` says *can require*, not *does release*). Ledger is generic, not courier-settling.
- **G5 — No economics.** "30% cheaper than Glovo/Bolt" is structural inference, not modeled.
- **G6 — Mid-route failure unhandled.** Courier drops after pickup, before POD ⇒ no defined
  outcome in code.
- **G7 — Framing fix.** `field.rs`/`multipilot.rs` are agent control-plane, **not** delivery
  dispatch. Any StoryBrand copy implying the "physics veto" routes your courier is false.

**Bottom line for the pitch:** The *trustworthy-delivery core* (route + prove + pay-attributable +
replicable + fails-closed) is genuinely real and unusually honest. The *business shell* a
restaurateur/courier actually buys (storefront, live courier pool, guaranteed payout, liveness)
is **not yet in this code** — say so, or the StoryBrand promise outruns the artifact and the
fable discipline (RED-is-the-proof) is violated.
