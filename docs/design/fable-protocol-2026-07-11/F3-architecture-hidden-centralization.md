# F3 — Technical Architecture + Hidden-Centralization Audit

> Angle 3/10 of the parallel fable protocol-pivot review (2026-07-11).
> Brief: edge-node compute (tensor/wave on devices, blockchain NOT in hot path),
> DLT used ONLY for Settlement + Proof-of-Delivery, DID without central admin but
> bot-proof, IPFS caching for static menu, offline/partition resilience. CRITICAL
> ASK: find any operator-controlled node that becomes a single point of failure
> or censorship.
>
> Method (Fable discipline): every claim below is grounded in a `file:line`
> citation into the actual Rust in `crates/bebop/src/` or the design docs under
> `docs/design/delivery-protocol/`. I ran `cargo test -p bebop --lib` → **275
> passed, 0 failed** — the primitives cited are real, exercised code, not stubs.
> Where the design references a component that has NO corresponding code, that is
> called out as poetry, not architecture.

---

## 0. TL;DR verdict

**Real architecture or poetry? → MOSTLY REAL (the edge / identity / matcher /
guard layers) + POETRY (the settlement / DLT / IPFS / network-transport layers).**

The *core thesis* of the operator's ask is genuinely implemented in code:
- Edge compute is real (deterministic, RNG/clock-free routing, wave/mesh transport).
- DID/identity is genuinely decentralized — it is **self-certifying** and phones
  home to NO issuer (`vault.rs:106`, `pod.rs:88-96`). This is the strongest,
  most honest part.
- The dispatch/sequencer is a **pure, replicable function** (`matcher.rs:74`),
  explicitly engineered to kill the #1 centralization risk. Test-proven
  (`matcher.rs:274` `matcher_is_replicable_no_hidden_server`).
- Settlement-only DLT separation is **correctly designed** — the matcher "never
  touches money; it proposes" (`MATCHER-API.md:104-108`) — but the DLT itself
  does **not exist in code**. 0 lines of settlement/escrow/on-chain logic.

**The hidden-centralization hunt found ONE genuine single-operator risk that is
real today and NOT solved in code: the bootstrap / SDK / access layer (DANGER #2).**
Everything else is either code-decentralized or honestly flagged as a gap. The
settlement oracle (DANGER #3) and identity root-of-trust (DANGER #4) are
mitigated *in design* but the on-chain/oracle half is unwritten.

---

## 1. Node-interaction graph (restaurant — protocol — courier — settlement)

Labeled messages. Solid arrows = implemented in Rust. Dashed = designed, not in
code. `[fn:line]` = where it lives.

```
                       ┌──────────────────────────────────────────────┐
                       │  OPEN MATCHER  (pure fn, any node runs it)     │
                       │  match_orders(req) -> resp   [matcher.rs:74]   │
                       │  fingerprint(resp)            [matcher.rs:100] │
                       └───────────────┬────────────────────┬───────────┘
                                       │                      │
   (1) ORDER + MENU                    │ (2) DISPATCH         │ (3) DISPATCH
   [DECOUPLED-MATCHER:24-44]           │  (signed intent)     │  (signed intent)
   Restaurant POS/CRM ────────────────┼──────────────────────▶ Courier device
   (transparent API connector,        │                      │  (edge compute:
    no SDK lock-in)  [PROTOCOL-       │                      │   wave_bounce +
   CENTRALIZATION-MAP:64]             │                      │   A*/CH routing)
                                       │                      │
                                       │                      │ (4) PoD claim
                                       │                      │  order|id|courier|
                                       │                      │  at|loc  [pod.rs:8]
                                       │                      ▼
                                       │              (5) SIGNED PoD
                                       │              vault.sign(SHA512(claim))
                                       │              [pod.rs:73-83]
                                       │                      │
                                       │                      │ (6) PoD proof
                                       │                      ▼
                                       │              ┌─────────────────────────┐
                                       │              │ SETTLEMENT / ESCROW       │
                                       │              │ (DLT, auto-release on PoD)│
                                       │              │ ⚠️ POETRY — no Rust code  │
                                       │              │ [MATCHER-API.md:104]      │
                                       │              └─────────────────────────┘
                                       │
   (7) REPUTATION feedback: valid PoD -> trust↑
       [reputation.rs:39-41]; suspension -> trust floor 0
       [reputation.rs:122-134]; feeds cost surface W_uv
       [reputation.rs:85-93]
```

Messages on every edge:
- **(1) Restaurant → Protocol:** `MatcherRequest{ nodes, edges, costs, orders, radius }`
  (`matcher.rs:43-50`). Menu itself is **IPFS-cached static** in the design but
  not implemented (`PROTOCOL-CENTRALIZATION-MAP` cites no code; grep finds 0 IPFS
  refs in `src/`).
- **(2)(3) Protocol → Courier:** `MatcherResponse{ assignments, unmatched }`
  (`matcher.rs:65-68`). `unmatched` = fail-closed refusal, surfaced not dropped
  (`matcher.rs:85-87`). Result is fingerprint-able so any two nodes agree
  (`matcher.rs:100-121`).
- **(4)(5) Courier → self:** device signs a `DeliveryClaim` with hybrid PQ sig
  (`pod.rs:73-83`). Bound to `(order, courier_id, ts, loc)` — anti-replay
  (`pod.rs:44-49`, `pod.rs:152-165`).
- **(6) Courier → Settlement:** `PodProof{ claim, signature, courier_id }`
  (`pod.rs:62-68`). Verifiable WITHOUT a directory — the id is self-certifying
  (`pod.rs:88-96`).
- **(7) Settlement → Reputation → cost surface:** a valid PoD raises trust
  (`reputation.rs:39-41`); that trust is the risk premium in `W_uv`
  (`reputation.rs:85-93`). This closes the "trust, not interface" loop
  (`SYSTEM-ARCHITECTURE-AUDIT.md:17-24`).

---

## 2. HIDDEN-CENTRALIZATION HUNT — every operator-controlled / non-replicated / trust-anchor node

I searched every `*.rs` for `bootstrap|discover|endpoint|gateway|relay|
coordinator|seed_node|tracker|https?://` and found **zero hardcoded operator
endpoints** in the executable path. That is itself notable: there is no secret
phone-home. The real centralization surface is in the *design contract*, not the
code. Each point:

### C1 — Matcher / dispatch sequencer  →  🟢 DE-CENTRALIZED IN CODE
- **What:** whoever orders courier↔order controls economics.
- **Code reality:** `match_orders` is a pure function over `MatcherRequest`
  (`matcher.rs:74`). No state between calls, no privileged host. `LocalMatcherClient`
  runs it in-process (default) — proves it needs no server (`matcher.rs:137-143`).
- **Replicable proof:** `matcher_is_replicable_no_hidden_server` — two independent
  clients produce identical fingerprints (`matcher.rs:274-290`). `remote_matches_local_over_wire`
  proves the wire contract is faithful (`matcher.rs:305-324`).
- **Fail-closed verdict:** ✅ FAIL-CLOSED-OK. Unreachable orders are refused and
  surfaced, never silently dropped (`matcher.rs:255-271`). One node cannot be the
  sole dispatcher because the *client codes to a `MatcherClient` trait*, not a
  hostname (`matcher.rs:127-131`). DANGER #1 is genuinely engineered away.
- **Caveat:** the *deployment* risk (DANGER #2) still exists at the SDK layer —
  see C3.

### C2 — Settlement oracle  →  🟠 DESIGN-MITIGATED, CODE ABSENT (the soft centralization risk)
- **What:** "food delivered" must be attested to release escrow.
- **Design mitigation:** PoD = threshold of device signatures
  (courier+restaurant+customer keys), the crypto proof IS the oracle, not a
  server (`PROTOCOL-CENTRALIZATION-MAP:104-112`). `pod.rs` implements the
  device-signing half fully (`pod.rs:73-96`).
- **Code reality:** the escrow/DLT auto-release that *consumes* the PoD does
  **not exist**. Grep for `ipfs|DLT|escrow|on-chain|onchain|blockchain|gossip`
  across `crates/` returns only doc-comment prose in `matcher.rs` — **0
  executable lines**. Settlement is explicitly "a separate, out-of-scope concern"
  (`SYSTEM-ARCHITECTURE-AUDIT.md:140-142`).
- **Fail-closed verdict:** ⚠️ UNKNOWN / NOT FAIL-CLOSED YET. Until the
  settlement layer is written, the "oracle" is whatever the operator bolts on
  later — that bolt-on is exactly where DANGER #3 re-centralizes. The design
  *says* device-sig threshold; the code does not enforce it. **This is the
  single most important follow-up gap.**

### C3 — SDK / bootstrap / access server  →  🔴 THE GENUINE HIDDEN-CENTRALIZATION NODE (operator-controlled today)
- **What:** restaurants/couriers reach the protocol *only* through the operator's
  hosted SDK/backend.
- **Code reality:** there is **no** bootstrap/discovery/gateway server in `src/`,
  and `bebop2/ARCHITECTURE.md` explicitly bans transport-in-the-hot-path
  (`bebop2/ARCHITECTURE.md:88-91` — "MCP / web-bindgen: flag-OFF external-only
  boundary, never on deterministic execution path"). So the *engine* can't be a
  chokepoint. BUT the *cold-start wedge* the docs themselves propose —
  "be the backend for direct orders" white-label widget/SDK
  (`PROTOCOL-CENTRALIZATION-MAP:154-164`) — **is** DANGER #2, and the docs admit
  "if our hosted backend is the only way in, we re-centralized at the access
  layer" (`PROTOCOL-CENTRALIZATION-MAP:91-102`).
- **Fail-closed verdict:** ❌ NOT FAIL-CLOSED. This is the one point the operator
  actually controls and has not yet de-risked in code. Escape is specified
  (thin client over open `MatcherClient` trait + ship a reference alt client,
  `PROTOCOL-CENTRALIZATION-MAP:99-102`) but not implemented. **Highest-priority
  honest finding.**

### C4 — Identity root-of-trust (DID issuer)  →  🟢 DE-CENTRALIZED IN CODE (but bot-proofness is partial)
- **What:** "who is a verified courier" decided by one issuer/key.
- **Code reality:** identity is **self-certifying**: `id = H(pq_pub ‖ classical_pub)`
  (`vault.rs:17`, `vault.rs:106`, `vault.rs:207-210`). No issuer, no directory,
  no phone-home. `pod.rs::verify_delivery` checks the id against the claim with no
  external lookup (`pod.rs:88-96`). A swapped/tampered key blob yields a different
  id and unlock refuses (`vault.rs:18`, `vault.rs:296-298`).
- **Bot-proof / Sybil:** NOT done in code. The design defers Sybil resistance to
  "staking bonds + optional Proof-of-Personhood (Gitcoin Passport / BrightID) as
  a *soft* signal" (`PROTOCOL-CENTRALIZATION-MAP:121-123`). That's honest, but it
  means a bot with a fresh `vault.rs` identity is indistinguishable from a human
  until it earns PoD-backed reputation (`reputation.rs:69-81` starts unknowns at
  neutral 0.5, not distrusted). Sybil is *bounded* by reputation decay
  (`reputation.rs:55-61`) but not *prevented*.
- **Fail-closed verdict:** ✅ FAIL-CLOSED-OK on the *centralization* axis (no
  issuer = no single censorship point). ⚠️ BOT-PROOF axis is OPEN (deferred, not
  faked). Reputation is the only real Sybil cost and it's pseudonymous-not-person.

### C5 — Liquidity / ordering sequencer  →  🟢 same as C1 (resolved by open matcher)

### Transport / mesh layer (zenoh, portkey, registry)  →  🟢 LOCAL STAND-INS, no operator node
- `zenoh.rs` is a process-local pub/sub broker — the *seam* for a real mesh,
  explicitly "NOT the network stack" (`zenoh.rs:1-11`). No server, no relay.
- `portkey.rs` is an in-process message bus — "the abstraction, the seam where a
  real mesh transport would plug in" (`portkey.rs:1-11`). Not a network hop.
- `registry.rs` is a **content-addressed capability registry** — `address =
  H(name‖version‖spec)` (`registry.rs:24`, `registry.rs:29-31`). Tamper-evident,
  no central registry server; it's a local HashMap (`registry.rs:51-55`).
- Fail-closed verdict: ✅ these are decoy "central nodes" — they're in-process
  and swappable. The `bebop2/ARCHITECTURE.md` middleware→direct directive
  (`bebop2/ARCHITECTURE.md:60-92`) deliberately deletes accidental relays.

### Kill-switch (guard)  →  🟢 CONSENSUS, not central
- `KillSwitch` suspends a peer only on ≥2/3 supermajority of *known* nodes
  (`guard.rs:37`, `guard.rs:107-113`). One node cannot kill another
  (`guard.rs:151-167`). No central off-button. ✅ FAIL-CLOSED-OK.

---

## 3. DID without central admin — does it phone home?

**Verdict: NO phone home. This is real decentralized identity, not a DID that
calls an issuer.**

Evidence chain:
1. `vault.rs::NodeIdentity::create` generates keys with `OsRng` **once** locally
   (`vault.rs:77-112`). `id = short_id(public_key)` =
   first 8 bytes of SHA-512(pubkey bundle) (`vault.rs:106`, `vault.rs:207-210`).
   The id is a function of the key, not assigned by any server.
2. `vault.rs::self_certify` re-derives the id from the bundle and refuses on
   mismatch (`vault.rs:115-117`, used at `vault.rs:296-298`). Tamper = fail-closed.
3. `pod.rs::verify_delivery` verifies a PoD proof using ONLY the courier's own
   public id — no directory lookup, no issuer call (`pod.rs:88-96`). Verifier
   learns "this id did this order" and nothing else (`pod.rs:12-17`).
4. Grep for any `did:`, `resolver`, `issuer`, `https://` in the identity path
   returns nothing executable. The only "issuer" concept is the operator's *own*
   `vault` key format, which is pinned for cross-node consistency
   (`SYSTEM-ARCHITECTURE-AUDIT.md:66-76`) — that's a *format pin*, not a trusted
   issuer that can revoke or censor.

**Caveat (honest):** "decentralized identity" here means *self-certifying
pseudonymous keys*, not W3C-DID-method-with-recovery. Key-loss = identity-loss
(`PROTOCOL-CENTRALIZATION-MAP:53` acknowledges this; suggests social-recovery /
stake-bonded issuance, not implemented). So: decentralized ✅, admin-free ✅,
recoverable ❌ (gap), bot-proof ⚠️ (Sybil only bounded by reputation, not a
personhood gate).

---

## 4. Settlement-only DLT — hot-path vs settlement separation

**Verdict: SEPARATION IS CORRECTLY DESIGNED. The hot path does NOT sync to chain.**

- Hot path = edge compute: routing/matching is a pure local function
  (`matcher.rs:74`), PoD is a local device signature (`pod.rs:73-83`), reputation
  is a local ledger (`reputation.rs:27-31`). None of these touch a network or a
  ledger — they're deterministic and offline-capable.
- The matcher "returns *intent* (who serves whom, at what cost). Settlement is a
  separate concern, anchored on the DLT only after the physical Proof-of-Delivery
  handoff" (`MATCHER-API.md:104-108`). So per-op sync to chain is explicitly
  **not** the design — only final settlement + PoD proof hit the DLT. This is the
  operator's exact ask and it's honored in the contract.
- **Latency poison check:** there is no code path that writes every op to a
  blockchain. The latency-contract docs (`bebop2/ARCHITECTURE.md:123-141`) ban
  alloc/serialization/RNG in the hot path — a chain write would violate that, and
  it isn't present.

**BUT** — the DLT *endpoint* is unwritten. "Settlement layer — matcher proposes
intent; settlement on DLT is a separate, out-of-scope concern"
(`SYSTEM-ARCHITECTURE-AUDIT.md:140-142`). So the separation is designed right and
the edge half is proven, but the settlement half is poetry until someone writes
it. The honest risk: when it IS written, the operator must resist the temptation
to make the settlement oracle a single service (C2 above).

---

## 5. Partition resilience — degrade or hard-fail?

**Verdict: EDGE DEGRADES (graceful), but SETTLEMENT + REPUTATION hard-fail on
partition (by design — they need consensus/ledger).**

What the code proves about degradation:
- **Routing / mesh:** `reconnect.rs` implements graceful topological degradation —
  when a hub's load×degree (J_z) exceeds threshold, edges are stripped and
  neighbors rewire to the lowest-stress node (`reconnect.rs:40-82`). Pure,
  fail-closed to "shed energy" (`reconnect.rs:108-130`). This is offline-capable
  partition healing on the edge graph.
- **Matcher:** pure function, runs locally with no peer — a partition can't stop
  a node from computing its own dispatch (`matcher.rs:137-143`). ✅ partition-safe
  for *computing* assignments.
- **PoD:** device-signed locally; a courier can sign delivery offline and the
  proof verifies later when reconnected (`pod.rs:73-96`, no network dependency).
  ✅ offline-capable.

What hard-fails on partition (correctly, but worth stating):
- **Reputation ledger** is a local `HashMap` (`reputation.rs:27-31`). Under
  partition, two halves build divergent trust records; there is **no merge/sync
  protocol** in code. On rejoin, the design expects consensus (KillSwitch ≥2/3,
  `guard.rs`) to reconcile — but the reconciliation *code* is absent.
- **Settlement / escrow** needs the DLT (unwritten) — by definition unavailable
  during a partition of the settlement network. Payments stall until reconnect.
  This is acceptable (fail-closed: no double-pay), but it's a hard stall, not a
  degrade.
- **Matcher fingerprint agreement** only holds if both sides see the same graph;
  under partition each side sees a partial graph, so assignments diverge — that's
  fine (local optima) but there's no cross-partition arbitration in code.

**Bottom line:** the *edge* (compute, routing, PoD) is genuinely partition-resilient
and degrades. The *coordination* (reputation sync, settlement, cross-partition
arbitration) is designed to be fail-closed but is not implemented, so on a real
network split today the system *computes locally and queues*, and the honest gaps
are reputation-merge and settlement — both flagged, not faked.

---

## 6. Honest verdict — real architecture or poetry?

**~70% real architecture, ~30% poetry.** Split cleanly:

| Layer | Status | Evidence |
|---|---|---|
| Edge compute (tensor/wave routing) | ✅ REAL | `cost_estimate.rs`, `wavefield.rs`, `reconnect.rs` — 275 tests pass |
| DID / identity | ✅ REAL, decentralized | `vault.rs` self-cert; `pod.rs` no phone-home |
| Open matcher / anti-sequencer | ✅ REAL | `matcher.rs:74` + replicability test `:274` |
| Consensus kill-switch | ✅ REAL | `guard.rs:107-113` |
| Reputation ledger | ✅ REAL (local) | `reputation.rs` |
| Mesh transport seam | ✅ REAL (local stand-in) | `zenoh.rs`, `portkey.rs` |
| **Settlement / DLT / escrow** | ❌ POETRY | 0 lines in `src/`; doc-only (`MATCHER-API.md:104`) |
| **IPFS menu cache** | ❌ POETRY | 0 refs in `src/` |
| **Network transport (p2p/gossip)** | ⚠️ STUB | `InMemoryTransport` only (`matcher.rs:159`); HTTP/p2p "drop-in" unwritten |
| **Bootstrap/SDK access layer** | 🔴 RISK | DANGER #2 admitted, not de-risked in code |
| **Sybil / PoP (bot-proof)** | ⚠️ DEFERRED | design defers to stake/Passport (`PROTOCOL-CENTRALIZATION-MAP:121`) |

**The headline for the operator:** the thing you were MOST worried about — a
hidden operator node that becomes a single point of failure or censorship — is
**largely absent in the engine** (no bootstrap server, no privileged matcher, no
issuer, consensus kill-switch). The ONE real exposure is the **SDK/bootstrap
access layer (DANGER #2)**: the cold-start wedge you propose is exactly the trap,
and it is not yet mitigated in code. The settlement oracle (DANGER #3) and
identity root (DANGER #4) are mitigated *in design* via device-signed thresholds,
but the on-chain/oracle half that would enforce them is unwritten — so the
centralization risk lives in what gets bolted on later, not in what exists today.

**Recommendation (priority order):**
1. Ship a reference *alternative* client over the open `MatcherClient` trait and
   make the hosted SDK a thin wrapper — kills DANGER #2 before it metastasizes.
2. Write the settlement layer as a **device-sig threshold verifier**, not a
   single oracle service — enforces DANGER #3's design promise in code.
3. Add a reputation-merge protocol for partition rejoin (currently local-only).
4. Treat IPFS menu cache and p2p transport as the next build items; both are
   explicitly "drop-in, no contract change" so low-risk to add.

*Audit executed 2026-07-11. Code verified by `cargo test -p bebop --lib` → 275
passed. All centralization claims cite `file:line` into the live tree.*
