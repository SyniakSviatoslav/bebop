# Delivery Protocol — Decoupled Matcher Design

> Status: design draft (2026-07-10). Author: bebop agentic synthesis.
> Companion code: `crates/bebop/src/cost_estimate.rs` (Hybrid Cost-Aware Engine, Layer 1–3).
> Research basis: `hybrid-routing-sota.md`, `web3-logistics-postmortem.md`,
> `crypto-primitives-research.md`, `platform-vs-protocol-logistics.md` (in /root/dowiz).

## 0. Thesis

A logistics protocol that runs a **single dispatch server is DoorDash with extra steps.**
Decentralize the **matcher**, not just the ledger. Settlement on a DLT is cheap and easy;
ordering/courier-assignment is where value and control actually live (the logistics
equivalent of an L2 sequencer / MEV-Boost relay). Whoever answers "which courier gets
this order, in what sequence, at what price" economically controls the network — even if
the escrow is on-chain.

Every "decentralized" protocol silently re-centralizes at one of five points. Each is
marked below with its **centralization-risk grade** and the **concrete escape** that keeps
it replaceable.

## 1. The actor graph (no hidden owner)

```
        ┌─────────────────────────────────────────────────────┐
        │  OPEN MATCHER API  (read-only order book + bid ABI)  │  ◄── the ONLY control surface
        └───────────────┬───────────────────────┬─────────────┘
                        │                        │
              ┌─────────▼────────┐     ┌─────────▼────────┐
              │  COURIER NODES   │     │  RESTAURANT NODES │
              │  (edge compute:  │     │  (POS/CRM bridge: │
              │   wave_bounce +  │     │   transparent API │
              │   A*/CH routing, │     │   connector, no   │
              │   cost_estimate) │     │   software change)│
              └─────────┬────────┘     └─────────┬────────┘
                        │                        │
                        └───────────┬────────────┘
                                    │  PoD attestation (device-signed)
                                    ▼
                          ┌───────────────────────┐
                          │  DLT SETTLEMENT LAYER │  (escrow, auto-release on PoD)
                          │  X% restaurant / Y%   │  (finality only — NOT routing)
                          │  courier / Z% protocol│
                          └───────────────────────┘
```

**Centralization points (each rated):**

| # | Point | Risk | Escape (keep it replaceable) |
|---|-------|------|------------------------------|
| C1 | **Matcher / dispatch sequencer** | 🔴 CRITICAL | Open order-book API + multiple competing matcher clients; no privileged assigner. Matcher is stateless re: who may run it. |
| C2 | **Settlement oracle** | 🔴 HIGH | PoD is device-signed (courier+restaurant keys, `vault.rs`); oracle only *forwards* signed attestations, cannot forge. Multi-oracle threshold, not single. |
| C3 | **SDK / bootstrap server** | 🟠 MED | Thin client over the open Matcher API. If our hosted backend is the only way in, we re-centralized at the access layer. Must be replaceable. |
| C4 | **Identity root-of-trust** | 🟠 MED | Decentralized issuer set (not one KYC key). Key-loss = identity-loss, so offer social-recovery / stake-bonded issuance. |
| C5 | **Liquidity / sequencer** | 🔴 CRITICAL | Same as C1 — whoever orders matches controls rent. Mitigate with permissionless matcher + auditable assignment rule (deterministic, reproducible). |

## 2. The three braking nodes, mapped to bebop primitives

| Business node | bebop primitive today | Gap |
|---------------|----------------------|-----|
| **Routing** (order→courier) | `wave_bounce_path` (Layer 2) + `field.rs` spectral cost surface (Layer 3 weights) | Assemble into `cost_estimate::route()` (k-d filter → BFS guard → A*/CH). |
| **Mapping** (live graph) | `reconnect` (graceful degradation on overload) | Add live edge-weight refresh (congestion → `W_uv`). |
| **Cost Estimation** | **none** (only `jz` stress proxy) | `cost_estimate` module — built in companion code. |

**Most toxic node = Cost Estimation** (zero substrate). Routing/Mapping have primitives;
Cost has no price model at all. That is what `cost_estimate.rs` fixes.

## 3. Layer 1–3 Hybrid Cost-Aware Engine (companion code contract)

```
Layer 1  Spatial filter   k-d / R-tree radius cull  → 1–50 µs (in-RAM)
Layer 2  Topological guard BFS reachability on filtered → sub-ms  ("does a path exist?")
Layer 3  Cost refinement  A*/Dijkstra with W_uv = f(latency,cost,risk) on field.rs surface
                          → add Contraction Hierarchy shortcuts to beat the uncontracted-graph
                             tens–hundreds-of-ms bottleneck (target ~5 ms like OSRM/Valhalla)
```

**Math note (verified):** a damped wavefront with edge speed `F_uv = 1/W_uv` *is* the
Fast Marching Method solving the Eikonal equation `|∇T| = 1/F`; Tsitsiklis (1995) proved
Dijkstra↔Eikonal equivalence; in the L∞ norm the wavefront reduces *exactly* to Dijkstra.
So we do **not** ship a PDE solver — `cost_estimate` uses A*/Dijkstra with `W_uv = 1/F_uv`.
The wave intuition (slow on costly edges, fast on cheap) is preserved as edge weights.

## 4. Protocol economics (avoid race-to-zero)

- **Zero-commission without a sink = you lose the race-to-zero.** Viable path:
  low nonzero protocol fee (1–3%, gas/settlement-equivalent) + **value-added sinks**
  (insurance, instant payout, dispute arbitration, reputation) monetized optionally —
  NOT the order flow itself.
- **Cold start:** be the *backend for direct orders* — white-label widget/SDK letting a
  restaurant take direct orders; protocol provides dispatch + settlement for ~3–5%.
  Restaurant keeps margin, you bootstrap supply + demand without fighting DoorDash.
- **Interoperability:** transparent API connector / Telegram bot into existing POS
  (Toast/Square/GloriaFood) — no forced software change.

## 5. Crypto primitive maturity (research verdict)

- **Proof-of-Delivery (physical handoff)** — 🔴 WEAKEST LINK. Device-signed attestation +
  escrow auto-release + TEE are individually mature, but *signature ≠ human received box*
  has no trustless production anchor (NFC/BLE relay, TEE breaks, coerced signatures open).
  Mitigate: require two-party device-signed handoff (courier+restaurant) + optional
  customer ack; treat forged-PoD as the slashing trigger (but never slash on a *missing*
  attestation alone — that is the failure mode that worsens trust).
- **DID + VCs** — 🟠 MED. W3C ratified; courier/restaurant-specific onboarding not in prod.
- **Sybil / PoP** — 🟠 MED-LOW. Stake bonds (lowest friction) > Worldcoin (regulated, gaps).
- **Dispute resolution** — 🟢 STRONGEST. UMA Optimistic Oracle / Kleros live + cheap undisputed.
- **Reputation-as-stake** — 🟠 LOW. Theory rich, no prod delivery protocol binds it;
  downstream of PoD.

## 6. Web3 logistics post-mortem (do not repeat)

- ShipChain: DEAD (SEC cease-and-desist, $2.05M, token-first, no carrier adoption).
- FOAM: ZOMBIE (great proof-of-location, no paying demand, beacon-bootstrapping unsolved).
- OriginTrail: ALIVE via pivot to neutral Decentralized Knowledge Graph (dropped "logistics token").
- VeChain / Chronicled(MediLedger) / CargoX / Morpheus.Network: ALIVE via **enterprise-first,
  permissioned, real compliance budget, no token-only fantasy**.
- **Lessons:** (1) mandated pain beats "trust" abstraction; (2) value at node one or die at
  cold-start; (3) permissioned + real budget survives where token-first dies.

## 7. Hard boundary (ethics)

The "Invisible Broker / Snake Surprise" pattern — feeding competitor APIs routes that look
like optimization but drain their resources — is **out of scope / refused**. It is
adversarial sabotage (anti-competitive, likely unauthorized-API) and violates the project
Ethics Charter. Interoperability here is *transparent and cooperative*, never parasitic.
