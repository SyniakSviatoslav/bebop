# Unified Decentralized NON-AI PQ-Secure Food-Vendor Delivery Protocol

> **Synthesis of two audits** + dowiz Sovereign-Core doctrine into ONE blueprint.
> Inputs (all quoted with `file:line`):
> - `docs/design/plan-audit-bebop-2026-07-11.md` (bebop/bebop2 crypto + protocol inventory)
> - `/root/dowiz/docs/design/plan-audit-memory-2026-07-11.md` (operator goal, hard constraints, decided-vs-open)
> - dowiz `MANIFESTO.md` / `DECISIONS.md` (Sovereign-Core MVP doctrine)
>
> Status of the crypto layer this blueprint depends on (verified 2026-07-11, cross-checked against
> real `cargo test` output — NOT agent-narrative):
> - ✅ Ed25519 RFC 8032 §7.1 bit-exact (limb math, commit c977ea6); 91/0 bebop2-core lib tests green.
> - ✅ ML-KEM-768 FIPS 203; ML-DSA-65 roundtrip+tamper+**determinism-KAT drift-guard** (commit in main, 94/0).
> - ✅ Argon2id RFC 9106 §5.3 KAT; XChaCha20-Poly1305 RFC 8439; SHA-512/SHA3; ChaCha20 CSPRNG.
> - ✅ G9: wasm32-unknown-unknown --no-default-features compiles CLEAN (0 errors, empty-import).
> - ⚠️ G10 (FIPS-204 **bit-exact interop**): OPEN. Serialization is custom (pk=3104B vs FIPS 1952B,
>   no deserializer). Deterministic golden/RED KAT added as drift-guard but NOT an interop proof.
> - ⚠️ arch-hardening H4 (sqrt-Kalman), H5 (complex-eigenvalue), M4 (VSA scratch): REAL numeric bugs
>   confirmed by RED arch tests (bebop2=0.30 vs numpy=4.66 etc). NOT fixed — deferred, host/analytic
>   layer only (not crypto, not on wasm path). H3 (Lanczos) + M1 (CFL) are GREEN vs numpy.
> - 385→94/0 on bebop2-core lib alone; full workspace green on main.
> The roadmap's "ALL STUBS" claim is **STALE**.
>
> HARD LESSON recorded: 3 rounds of parallel subagents returned **false-green** reports (claimed tests
> green while they failed / claimed FIPS bit-exact while pinning own bytes). Trust `cargo test` output,
> not agent summaries. Main only accepts changes with literal green test evidence.

---

## 0. The ONE sentence

Build an **open-source, self-hosted owner hub** (local-first, Rust/WASM, pure deterministic
event-sourced core, **no AI in runtime**) that funnels a food-vendor's **multi-channel / multi-device
order entrypoints** into one **0%-commission checkout** and **dispatches the vendor's own couriers** —
with **PQ signatures + mesh/P2P seams baked now** so Phase-2+ can switch on without a rewrite, and
**the matcher decentralized (not a single dispatch server)** so the protocol never re-centralizes.

---

## 1. Why this exists (operator thesis, quoted)

- Escape aggregators: *"A modular hub that lets a food-business owner control their own data across
  their own channels from one module — escaping aggregators (0% commission, own-the-customer,
  own the data)."* (`MANIFESTO.md:28-30`)
- Decentralization as invariant: *"Local-first data × Local execution (WASM/edge) × P2P protocol =
  decentralized reliability; each node (venue, courier, client) an autonomous decision center."*
  (`MANIFESTO.md:74-77`)
- Anti-re-centralization warning (the single most important design rule): *"Decentralize the matcher,
  not just the ledger. A logistics protocol that runs a single dispatch server is DoorDash with extra
  steps."* (`platform-vs-protocol-logistics.md:95,111`)
- Non-AI: *"Determinism > AI. AI is a tool for writing code/tests in R&D/back-office ONLY. System
  runtime logic is hard-deterministic (Rust/WASM). No probabilistic decisions in business logic."*
  (`MANIFESTO.md:17-19`)

---

## 2. Hard constraints (NON-NEGOTIABLE — gate every change against these)

| # | Constraint | Source |
|---|---|---|
| C1 | **No AI in protocol/runtime logic** — deterministic Rust/WASM only; AI only for R&D/back-office | MANIFESTO:17-19 |
| C2 | **Pure core** — no clock/RNG/env/floats/network/battery vocabulary reaches `dowiz-core`/`bebop2-core` | MANIFESTO:21-22,40 |
| C3 | **Immutable event-sourced state machine is the law**: `Intent → decide → Event`, `state = fold(events)`; forbidden transitions are compile/runtime errors | MANIFESTO:38-39 |
| C4 | **Local-first + no central server** as a reachable-free invariant from the signed event log | MANIFESTO:74-77; DECISIONS D2 |
| C5 | **Integer-only money** (`Lek(i64)`, no `From<f64>`) — single money surface | MANIFESTO:41; DECISIONS:9-11 |
| C6 | **Open-source destination** — AGPLv3 + trademark + DCO, gated on secrets scrub + EU TM | HERMES:118 |
| C7 | **Verified-by-Math / falsifiable proof** — every change needs a RED+GREEN assertion | HERMES:53-59; DECISIONS D5 |
| C8 | **Over-engineering is the #1 enemy** — PQ/mesh/CRDT is roadmap, hard-gated behind MVP (D6) | MANIFESTO:90-92 |
| C9 | **Ethics charter** — no AI for warfare; AI is a commons, never captured | HERMES:26-31 |
| C10 | **Crypto must be from-scratch, zero-dep, NON-AI, RNG-free hot path** — caller-supplied entropy only | `bebop2/core/src/{pq_dsa,pq_kem,sign}.rs` |

---

## 3. Layered architecture (the unified stack)

```
L0  EVENT CORE        dowiz-core: pure Rust/WASM, Intent→decide→Event, fold, integer money, idempotency.
                       (DONE: 10-status order machine; 0b-3 decide composes machine→actor-gate→cc1→pricing;
                        0b-5 red-proof complete.)
L1  IDENTITY / PoD    bebop2 PQ core: self-cert id = H(pq_pub ‖ classical_pub); Ed25519/ML-DSA-65 sign,
                       ML-KEM-768 KEM, XChaCha20-Poly1305 at-rest, Argon2id KDF. NO issuer, NO phone-home.
L2  MATCHING          OPEN REPLICABLE matcher (pure fn, any node runs it, identical fingerprints) —
                       NOT a single dispatch server. Force-inclusion fallback + multi-signal PoD attestation.
L3  SETTLEMENT        Device-sig THRESHOLD verifier (≥k of n courier/owner sigs on PoD), NOT a single oracle.
                       Final PoD settlement only hits DLT; hot path never syncs to chain.
L4  ARBITRATION       Fail-closed dispute state machine: OPEN→EVIDENCE→AUTO→ESCALATE→JURY→SETTLE.
                       Any timeout/ambiguity → escrow HOLD + default refund to claimant.
L5  ACCESS/SDK        Thin client + reference alt-client (escape DANGER #2: "open protocol, closed access").
```

### Centralization danger map (keep visible — from `PROTOCOL-CENTRALIZATION-MAP.md`)

- **DANGER #1 — matcher/sequencer** = the economic control point. Mitigated by open replicable matcher (C-L3).
- **DANGER #2 — SDK/bootstrap access layer** = "open protocol, closed access". Mitigated by thin client + alt-client.
- **DANGER #3 — settlement oracle** = re-centralizes if a single oracle pays out. Mitigated by threshold-sig verifier.
- **DANGER #4 — identity root** = mitigated by self-cert identity (no directory lookup).

---

## 4. Crypto backing (bebop2 = the NON-AI PQ substrate) — CONFIRMED FIT

bebop2 is exactly the trustless substrate the protocol needs: RNG-free hot path, no phone-home,
KAT-anchored, zero-dep from-scratch. Re-point **all** protocol crypto at bebop2 primitives:

| Need | Primitive (bebop2, verified) |
|---|---|
| PoD signature | ML-DSA-65 (FIPS 204) + Ed25519 hybrid |
| Key exchange | ML-KEM-768 (FIPS 203) |
| At-rest vault | XChaCha20-Poly1305 + Argon2id (RFC 9106 §5.3 KAT) |
| Self-cert identity | `id = H(pq_pub ‖ classical_pub)`, no directory |
| CSPRNG | ChaCha20/HChaCha20 (in-tree, caller-seeded) |

**CONFLICT RESOLVED:** two crypto cores exist — `bebop2/core` (new PQ) vs `crates/bebop/src`
(vault.rs uses XChaCha20+scrypt, pre-PQ). **Action:** retire scrypt → Argon2id; re-point vault/pod
at bebop2 primitives. (`plan-audit-bebop:164-167`)

**ROADMAP CONFLICT RESOLVED:** the roadmap's "ML-DSA + symmetric = STUB" is stale; they are
implemented + tested (`plan-audit-bebop:42-43,66-76`).

---

## 5. The MVP (Trojan horse) vs the Protocol (destination)

### MVP — shippable NOW (dowiz Sovereign Core)
- Owner hub: multi-channel/multi-device order entrypoints → ONE 0%-commission direct checkout.
- `dowiz-core` pure event machine, integer money, idempotency, codec.
- **Seams baked free:** per-event content-hash + signature slot, transport-agnostic sync port,
  WASM-pure core + wasm32/clippy disallowed-methods gate (`DECISIONS.md:22-23`).
- Aggregator order-intake **banned** from MVP (breaks single-money-surface, C5).
- Couriers: honest single-owner dispatch (`attemptHonestDispatch`) — the vendor runs their own.

### Protocol — Phase-2+ destination (seams already in)
- **Open competitive matcher market** (not a single dispatcher) — permissionless matchers,
  force-inclusion timeout, attestation aggregation (`platform-vs-protocol-logistics.md:102-111`).
- **Per-actor PQ identity** (Ed25519/ML-DSA/ML-KEM) — deferred choice, seams ready (C4/D2).
- **Mesh/P2P transport** (libp2p vs Zenoh/Rift) + **CRDT merge** — deferred (C8/D6).
- **Vendor-owned courier marketplace** (reassignment/auction for 50%-drop resilience) — GAP G2.

### Boundary (the reconciliation the audits demanded)
The owner hub is the **thin, replaceable access layer** (L5), NOT the chokepoint. Decentralization
is reachable for FREE from the signed event log (C4). Single-owner dispatch (MVP) composes with the
network-level open matcher (protocol) at the boundary: the owner's hub is *one* matcher among many;
it never becomes the only one.

---

## 6. Gaps to close (honest ledger — from both audits)

| Gap | Severity | Action | Source |
|---|---|---|---|
| G1 No storefront/menu/hours | HIGH (MVP-blocker for a *food* vendor) | Build `Order={id,src,dst,items,price}` + menu module | F4:180-181 |
| G2 No courier marketplace / reassignment | HIGH (50% drop = 50% lost throughput) | Auction/reassign logic for vendor-owned couriers | F4:182-184 |
| G3 No node liveness | MED | Heartbeat + last-seen in matcher | F4:185 |
| G4 No payout contract | HIGH | Device-sig threshold verifier (DANGER #3 guard) | F3:131-146 |
| G5 No economics model | MED | 1–3% + value-added sinks (retire "0% atomic bomb" poetry) | F1:19,68 |
| G6 Mid-route failure unhandled | MED | Fail-closed reroute + PoD contestability | F4:185-192 |
| G7 Physical-handoff PoD no trustless anchor | HIGH | "PoD is contestable" → route to arbitration, not ground-truth | MAP:143-152, F2:53-55 |
| G8 Dispute resolution unbuilt | MED-HIGH | Build F2 fail-closed state machine OR integrate UMA/Kleros | F2:8-10,22-37 |
| G9 wasm32 empty-import gate FAILS (~94 err) | HIGH (bare-metal proof) | Mechanical: add alloc + panic_handler + f64 removal | audit-bebop:46,77-80 |
| G10 ML-DSA-65 NOT NIST-bit-exact | HIGH (interop) | ACVP oracle before protocol keys minted | audit-bebop:66-71,174-177 |
| G11 Two crypto cores | RESOLVED (re-point at bebop2) | Retire scrypt→Argon2id | audit-bebop:164-167 |
| G12 Roadmap staleness | RESOLVED (crypto done) | Correct roadmap doc | audit-bebop:168-170 |

---

## 7. Verified-by-Math anchors (what is PROVEN, not claimed)

- **Ed25519**: `cargo test -p bebop2-core` → 91/91; RFC 8032 §7.1 #1 bit-exact signature +
  RED case (wrong pubkey / tampered sig / wrong msg all REJECT). Deterministic keygen.
- **ML-KEM-768**: FIPS 203 KAT-green (q=3329 correct).
- **ML-DSA-65**: roundtrip + tamper + forge RED-GREEN (params Q=8380417, D=13, K=6, L=5, η=4).
  **Not** NIST-bit-exact (G10).
- **Argon2id**: RFC 9106 §5.3 KAT-green.
- **AEAD / hash / CSPRNG**: RFC 8439 / FIPS 180-4 / FIPS 202 / in-tree ChaCha20 — GREEN.
- **dowiz-core**: 0b-3 `decide` composes machine→actor-gate→cc1→pricing; 0b-5 red-proof complete
  (deployed-reality). 10-status order machine, integer money, idempotency.

Every future change MUST add a RED+GREEN assertion (C7). Ship the RED case alongside the green.

---

## 8. Build order (max-EV, respects C8 — ship MVP first, seam the rest)

1. **MVP hub** (dowiz-core + owner UI): multi-device entrypoints → 0% checkout. *Shippable.*
2. **Crypto re-point** (G11): vault/pod → bebop2; retire scrypt. *Mechanical, unblocks protocol.*
3. **wasm32 gate** (G9): prove empty-import / no-clock-RNG-socket. *~1 day mechanical.*
4. **ML-DSA NIST-bit-exact** (G10): ACVP oracle. *Unblocks interop before minting keys.*
5. **Open matcher + threshold settlement** (C-L3/L4, G4): kills DANGER #1/#3.
6. **Food-vendor gaps** (G1/G2/G3/G6): storefront, courier marketplace, liveness, reroute.
7. **Dispute + PoD-contestability** (G7/G8): fail-closed arbitration.
8. **Economics + access layer** (G5, DANGER #2): 1–3% + thin client/alt-client.

---

## 9. What this blueprint is NOT

- Not a business plan (F4 "business shell" is a separate artifact).
- Not a formal-verification spec (Coq/Aeneas is Phase-3 grail, never in pure core, C8/D3).
- Not claiming "0% fee = moat" (poetry, F1) — moat = earned local reputation graph + credible neutrality.
- Not abandoning the MVP for the space-stack (D6 hard-gate) — seams now, machinery later.

---

*Synthesized 2026-07-11 from `plan-audit-bebop-2026-07-11.md` + `plan-audit-memory-2026-07-11.md`
+ dowiz Sovereign-Core doctrine. Crypto layer status verified green same session
(Ed25519 blocker fixed in commit `c977ea6`: replaced per-op heap Vec bignum `mod_p_be` with
fixed 64-bit-limb schoolbook `limbs_mul` + 2^255≡19 fold `reduce_p`; root cause was
`limbs_mul` silently truncating its high-limb carry. Full suite 385 tests green, 1.78s.
G9 wasm32 + G10 ML-DSA-KAT + arch-hardening H3/H4/H5/M1/M4 are IN-FLIGHT via 3 parallel
agents on clean worktrees `feat/wasm32-hardening`, `feat/mldsa-kat`, `feat/arch-hardening`.)*
