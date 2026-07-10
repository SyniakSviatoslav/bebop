# Delivery Protocol — Centralization Map

> Status: design draft (2026-07-10). Purpose: make the hidden-centralization
> points of a "protocol not platform" delivery network **explicit and visible**
> before any code is written, so we do not rebuild DoorDash with extra steps.
>
> Research backing: `/root/dowiz/hybrid-routing-sota.md`,
> `/root/dowiz/web3-logistics-postmortem.md`,
> `/root/dowiz/crypto-primitives-research.md`,
> `/root/dowiz/platform-vs-protocol-logistics.md`.
>
> Thesis (platform-vs-protocol): centralized platforms (Uber Eats, Glovo,
> DoorDash) extract ~30% rent and are structurally fragile (margin destruction,
> non-loyal supply, single regulatory target). A *protocol* is a utility: cheap,
> fast, private. But protocols silently re-centralize at the **access/control
> layer** even when the ledger is decentralized. This doc marks those points.

## 1. The five-layer stack (and who controls each)

```
┌─────────────────────────────────────────────────────────────────────┐
│ LAYER 5  Arbitration / Dispute resolution   (Kleros / UMA style)     │
│          → STRONGEST primitive (battle-tested, near-free undisputed)  │
├─────────────────────────────────────────────────────────────────────┤
│ LAYER 4  Settlement / Escrow              (DLT, auto-release on PoD)  │
│          → X% restaurant / Y% courier / Z% protocol, NO middleman     │
├─────────────────────────────────────────────────────────────────────┤
│ LAYER 3  Matching / Dispatch (the SEQUENCER)  ← DANGER #1            │
│          "which courier gets which order, at what price, in what      │
│           sequence" — economic control lives HERE, not in L4          │
├─────────────────────────────────────────────────────────────────────┤
│ LAYER 2  Mapping / Graph state            (live edge weights,         │
│          congestion → W_uv; graceful degradation)  ← DANGER-adjacent  │
├─────────────────────────────────────────────────────────────────────┤
│ LAYER 1  Identity / PoD attestation       (device-signed handoff)     │
│          → WEAKEST link (physical-handoff binding has no trustless    │
│             production anchor)                                        │
└─────────────────────────────────────────────────────────────────────┘
        ▲
        │  ACCESS LAYER (SDK / bootstrap server)  ← DANGER #2
        │  "open protocol, closed access" = re-centralization at the edge
```

## 2. Restaurant ↔ Protocol ↔ Courier interaction schema

```
        ┌──────────────┐         order (direct widget)        ┌──────────────┐
        │  RESTAURANT  │ ───────────────────────────────────▶ │   PROTOCOL   │
        │  (own CRM/   │                                      │  MATCHER API │
        │   POS: Toast/│ ◀────────── settlement escrow ─────── │  (open, not  │
        │   Square)    │        X% on PoD                      │   a server)  │
        └──────────────┘                                      └──────┬───────┘
                                                                   │ dispatch
                                                                   │ (signed,
                                                                   │  verifiable)
                                                                   ▼
                                                          ┌──────────────┐
                                                          │   COURIER    │
                                                          │ device-signed│
                                                          │ PoD → escrow │
                                                          │ auto-release │
                                                          └──────────────┘
   Notes:
   - Restaurant keeps its existing POS/CRM. Protocol is a TRANSPARENT PROXY /
     API connector (Telegram bot, webhook) — never forces a software change.
   - PoD is a device-signed attestation (courier↔restaurant↔customer keys).
   - Settlement is DLT escrow, released by the PoD proof — not a platform account.
```

## 3. Centralization danger map (the five points)

Each point: what it is → why it re-centralizes → bebop primitive mapping →
the replaceable design that avoids it.

### DANGER #1 — Matching / Dispatch Sequencer (MOST LIKELY)
- **What:** whoever orders "courier ↔ order" controls the network economically,
  even if settlement is on-chain. Precedent: every major L2 (Arbitrum, Base,
  Optimism, zkSync, Linea) still runs a SINGLE-OPERATOR sequencer in 2026;
  Base went down Feb 2025, Linea was unilaterally paused June 2024 to censor.
- **Why re-centralizes:** founders ship it "temporarily centralized" and never
  fix it. The matcher is where value + censorship live (logistics MEV).
- **bebop mapping:** `cost_estimate::hybrid_route` (k-d + BFS + A*/CH) is the
  routing *logic* — but it must run as an **open, replicable matcher API**, not
  a single hosted server. The algorithm is already decentralized-friendly
  (deterministic, no RNG); the *deployment* is the risk.
- **Replaceable design:** matcher = stateless function over the open graph API;
  any node can run it; results are signed + verifiable. Bootstrap SDK is a thin
  client over that API (DANGER #2). If our hosted backend is the only way in,
  we re-centralized at the access layer.

### DANGER #2 — SDK / Bootstrap Server (the Trojan horse becomes the trap)
- **What:** restaurants/couriers can only reach the protocol through *our*
  hosted SDK/backend.
- **Why re-centralizes:** the chain is open, the *access layer* is not. This is
  the subtle one — exactly the "be the backend for direct orders" cold-start
  wedge (correct strategy) that rots into a chokepoint if the SDK is the only
  client.
- **bebop mapping:** none yet — this is a deployment constraint, not code.
- **Replaceable design:** SDK MUST be a thin client over the open matcher API;
  document the API fully; ship a reference alternative client. Profit from
  value-added services (insurance, instant payout, arbitration), NOT from being
  the only door.

### DANGER #3 — Settlement Oracle (off-chain → on-chain truth)
- **What:** someone attests "the food was delivered" to trigger escrow release.
- **Why re-centralizes:** a single oracle/service signing delivery proofs is the
  real controller (same risk class as a centralized L2 sequencer).
- **bebop mapping:** `vault.rs` device-signed attestation is the primitive; the
  oracle should be the *device signatures themselves* (threshold of
  restaurant+courier+customer keys), not a trusted third party.
- **Replaceable design:** PoD = threshold signature from the three parties'
  `vault.rs` keys; the "oracle" is the cryptographic proof, not a server.

### DANGER #4 — Identity Root-of-Trust (KYC / DID issuer)
- **What:** "who is a verified courier/restaurant" decided by one issuer/key.
- **Why re-centralizes:** a single KYC issuer = gatekeeper; OFAC showed states
  act at the *interface/identity layer*, not the base chain (Tornado Cash).
- **bebop mapping:** `vault.rs` self-certifying identity (XChaCha20 + scrypt,
  sign/verify). Good base, but issuer trust ("who verifies the verifier") is
  unresolved.
- **Replaceable design:** Sybil resistance via staking bonds (slashed on
  failure) + optional Proof-of-Personhood (Gitcoin Passport / BrightID) as a
  *soft* signal, not a hard gate. No single root key.

### DANGER #5 — Liquidity / Sequencer (ordering & matching)
- Covered by #1 (the matcher IS the sequencer for logistics). Listed separately
  only to note: value and control live in ordering, not in the settlement layer.
  Decentralize the matcher (DANGER #1), and this resolves.

## 4. Where bebop already provides decentralization

| Need                      | bebop primitive                         | Status        |
|---------------------------|-----------------------------------------|---------------|
| PoD / DID attestation     | `vault.rs` (device-signed, self-cert)   | HAVE          |
| Cost-aware routing        | `cost_estimate::hybrid_route`           | HAVE (A, committed) |
| Mapping resilience        | `reconnect.rs` (MHD graceful degrade)   | HAVE          |
| Cost surface (W_uv)       | `field.rs` spectral heat-kernel         | HAVE          |
| Topological guard         | `wavefield::wave_bounce_path`           | HAVE          |
| Open matcher deployment   | — (deployment, not algorithm)           | GAP (DANGER #1/2) |
| Reputation-as-stake       | —                                       | GAP (downstream of PoD) |
| Dispute resolution        | — (use UMA/Kleros, don't build)         | GAP (use external) |

## 5. Weakest link (honest admission)

**Proof-of-Delivery physical-handoff binding** is the root dependency and has
NO trustless production anchor today (signature ≠ human received box; NFC/BLE
relay, TEE breaks, coerced signatures). Reputation needs its objective failure
signal; slashing on a forged PoD is worse than none. bebop's `vault.rs` gives
the *cryptographic* half but NOT the *physical* half — that needs hardware
attestation (secure element + location proof) we do not own. We must design
for "PoD can be contested" and route disputes to LAYER 5 (UMA/Kleros), not
pretend the signature is ground truth.

## 6. Cold-start / interoperability (the bootstrap, not the trap)

- **Trojan horse:** a white-label ordering widget/SDK that lets a restaurant take
  direct orders; protocol provides dispatch + settlement for ~3–5% (keeps margin
  vs 30%). Bootstrap demand + supply simultaneously without fighting DoorDash.
- **Interoperability:** transparent proxy to existing POS/CRM (Toast, Square,
  GloriaFood) via API connector / Telegram bot / webhook. Never force a software
  change.
- **Fee model:** low nonzero protocol fee (1–3%, gas-equivalent) + monetize
  *value-added* layers (insurance, instant payout, arbitration, reputation) — NOT
  the order flow. Avoids race-to-zero.

## 7. Verdict + next code steps

- The single most likely hidden-centralization point is the **matcher/sequencer**
  (DANGER #1). Build it as an open, signed, replicable API — never a single
  hosted server. The algorithm (`cost_estimate`) is already decentralized-friendly.
- PoD (DANGER #3 primitive) is cryptographically covered by `vault.rs`; the
  physical-handoff weakness (§5) must be designed-around, not hidden.
- Next code: (1) telemetry proving Layer-3 CH collapses the bottleneck
  (hundreds-of-ms → ~5 ms); (2) Mapping live edge-weight refresh (congestion →
  W_uv) over `reconnect`; (3) ship the open matcher API spec + reference client.
