# Plan Audit — bebop / bebop2 vs. Decentralized PQ-Secure Food-Vendor Delivery Protocol

> Auditor note (2026-07-11). Read-only inventory of all bebop-repo plans + crypto
> source relevant to unifying dowiz + bebop/bebop2 into ONE blueprint for a
> **decentralized, NON-AI, post-quantum-secure delivery protocol for food vendors
> (no third-party couriers)**. Every claim carries a concrete `file:line` ref.
> No code was edited.

---

## 0. TL;DR

- **bebop2 = the NON-AI PQ crypto core.** Excellent fit to back the protocol's crypto.
  It is a from-scratch, zero-dependency Rust core: ML-KEM-768, ML-DSA-65, Ed25519,
  XChaCha20-Poly1305, Argon2id, SHA-512/SHA3, in-tree CSPRNG. Fully deterministic,
  RNG-free hot path, no clock/network — the "physicality/empty-import" discipline.
- **Crypto status is BETTER than the roadmap claims.** The roadmap
  (`bebop2-roadmap-2026-07-10.md:9-11,45-47`) says ML-DSA + all symmetric are
  "STILL STUB / 2-line". That is **STALE**. Source shows pq_dsa.rs (746 lines),
  kdf.rs/Argon2id (616 lines), aead, hash, sign, rng, pq_kem are all implemented +
  KAT/roundtrip tested (deep-research confirms 83/83 green,
  `bebop2-deep-research-2026-07-11.md:11-12,137`).
- **Protocol design is mostly DESIGN, partly CODE.** The delivery-plane primitives
  (matcher, PoD, reputation, ledger, guard/killswitch, self-cert identity) exist in
  the `bebop` (not bebop2) crate and pass 275 tests (`F3:12-13,338`). Settlement/DLT,
  IPFS menu, p2p transport, dispute state-machine, payout contract, storefront, and
  courier marketplace are **poetry/GAP** (unwritten).
- **Two crypto cores exist and must be reconciled.** `bebop2/core` (the new PQ core)
  and the older `crates/bebop/src` (vault.rs/pod.rs using XChaCha20+scrypt). The
  protocol should be re-pointed at bebop2's PQ primitives.

---

## (a) Planned features list + status

| Feature | Source | Status |
|---|---|---|
| ML-KEM-768 (FIPS 203) KEM | roadmap:8; deep-research:26 | **DONE & GREEN** (schoolbook coeff-domain; NTT excluded as broken) |
| ML-DSA-65 (FIPS 204) sign | roadmap:9 ("STUB"); pq_dsa.rs:1-746 | **IMPLEMENTED + roundtrip/tamper tested** (roadmap stale). NOT bit-exact vs NIST ref (no oracle in sandbox) — `pq_dsa.rs:8-14` |
| Argon2id KDF (RFC 9106) | roadmap:10 ("STUB"); kdf.rs:1-616 | **IMPLEMENTED** w/ from-scratch BLAKE2b; anchored to RFC 9106 §5.3 KAT — `kdf.rs:7-13` |
| XChaCha20-Poly1305 AEAD | roadmap:10; aead.rs:1-473 | **IMPLEMENTED + GREEN** RFC8439/xchacha-03 (`aead.rs:1-12`; deep-research:27) |
| SHA-512 / SHA3 hash | roadmap:10; hash.rs:1-361 | **IMPLEMENTED + KAT-green** FIPS180-4/FIPS202 (`hash.rs:3-7`) |
| Ed25519 classical sign | roadmap:10; sign.rs:1-708 | **GREEN, RFC 8032 §7.1 bit-exact** (deep-research:25,147) |
| In-tree CSPRNG (ChaCha20/HChaCha20) | roadmap:11; rng.rs | **GREEN** — best native candidate (deep-research:29) |
| Math/spectral kernel (field/vsa/kalman/lyapunov/chebyshev/fft/active) | roadmap:12-13 | **DONE & GREEN** (54 lib tests) but architecture-hardening open (H3/H4/H5) — NOT crypto, NOT protocol-critical |
| wasm32 / no_std / empty-import gate | roadmap:19-20,32; deep-research:16-21 | **FAILS** (~94 errors: missing alloc, no panic_handler/allocator, std f64 trig). ~1 day mechanical work. This is the honest "machine-code" proof and is unproven today |
| Open matcher / dispatch (protocol) | F3:29-31,116-127; MAP:82-89 | **CODE (bebop crate), replicable, test-proven** `matcher.rs:74,274` |
| PoD / self-cert identity | F3:95-100,166-172 | **CODE** `pod.rs:73-96`, `vault.rs:106` |
| Reputation ledger | F3:80-83,311; F4:96-100 | **CODE (local HashMap)** `reputation.rs` |
| Consensus kill-switch | F3:198-201 | **CODE** ≥2/3 supermajority `guard.rs:107-113` |
| Settlement / escrow / DLT | F3:131-146,313; MAP:104-112 | **POETRY — 0 lines** |
| IPFS menu cache | F3:89-90,314 | **POETRY — 0 refs** |
| p2p / gossip transport | F3:186-196,315 | **STUB** (InMemory/zenoh/portkey local stand-ins) |
| Dispute / arbitration state-machine | F2:8-10,22-37 | **DESIGN-ONLY** (proposes external UMA/Kleros) |
| Sybil resistance (staking/PoP) | F3:173-179; MAP:121-123 | **DEFERRED** (design) |

---

## (b) Actual crypto status (verified from SOURCE, not the roadmap)

- **KAT-verified / GREEN:** ML-KEM-768 (`pq_kem.rs`, q=3329 correct per FIPS 203 —
  note the modulus reconciliation vs the ML-DSA q=8380417, `pq_kem.rs:3-9`),
  Ed25519 (RFC 8032 §7.1 bit-exact), AEAD XChaCha20-Poly1305 (RFC 8439/xchacha),
  SHA-512+SHA3 (FIPS 180-4/202), ChaCha20 CSPRNG. Aggregate: **83/83 bebop2-core
  tests pass** (`bebop2-deep-research-2026-07-11.md:11-12`).
- **Implemented + internally correct, NOT NIST-bit-exact:** ML-DSA-65
  (`pq_dsa.rs`): correct params Q=8380417, D=13, K=6, L=5, η=4 (`pq_dsa.rs:26-39`),
  schoolbook poly-mul ground truth (`pq_dsa.rs:145-166`), full keygen/sign/verify,
  decompose/hint, SampleInBall. **Honest flag:** signature/hint packing follows FIPS
  204 §6 structure but is not verified bit-exact against a reference Dilithium oracle
  (none in sandbox) — `pq_dsa.rs:8-14`. An NTT fast-path can swap behind the same tests.
- **Argon2id** (`kdf.rs`): faithful port of PHC reference w/ from-scratch BLAKE2b-512;
  version 0x13, type=2 (Argon2id); anchored to RFC 9106 §5.3 + RFC 7693 KAT
  (`kdf.rs:7-9,301-306`).
- **Remaining stub:** NONE of the crypto files are 2-line stubs anymore. The roadmap's
  "ALL STILL STUBS" (roadmap:11,45-47) is corrected by deep-research (`:137`).
- **Real open blocker:** wasm32 empty-import gate fails (94 errors) — the only honest
  "runs as machine code / no reachable clock/RNG/socket" proof. Until it compiles +
  runs under wasmtime bit-exact vs committed KAT, the bare-metal claim is aspirational
  (`bebop2-deep-research-2026-07-11.md:16-21,44-48`).
- **Entropy model (protocol-relevant, strong):** every primitive is RNG-free on the
  hot path; randomness enters only via caller-supplied seeds — no OS RNG, clock, or
  network reachable (`pq_dsa.rs:3-5`, `pq_kem.rs:13-16`, `sign.rs:7-9`). This is the
  NON-AI, deterministic, auditable property the protocol wants.

---

## (c) Hub / decentralization / sovereign-node architecture already designed

- **Five-layer stack + explicit centralization danger map**
  (`PROTOCOL-CENTRALIZATION-MAP.md:18-42,70-128`): L1 identity/PoD → L2 mapping →
  L3 matcher/sequencer (**DANGER #1**, the economic control point) → L4 settlement →
  L5 arbitration; plus the ACCESS/SDK bootstrap layer (**DANGER #2**, "open protocol,
  closed access"). This is the most important decentralization asset — it names the
  re-centralization traps before they are built.
- **Self-certifying identity (no issuer, no phone-home):** `id = H(pq_pub‖classical_pub)`,
  verified with no directory lookup (`F3:166-172,205-224`, `vault.rs:106`). Decentralized
  ✅, admin-free ✅, but recoverable ❌ (key-loss = identity-loss) and bot-proof ⚠️
  (Sybil only bounded by reputation, `F3:226-231`).
- **Open replicable matcher (kills the single sequencer):** pure function, any node
  runs it, two nodes produce identical fingerprints — test-proven
  `matcher_is_replicable_no_hidden_server` (`F3:116-127`, `matcher.rs:74,274`).
- **Consensus kill-switch (no central off-button):** ≥2/3 supermajority of known nodes
  (`F3:198-201`, `guard.rs:107-113`).
- **Settlement-only-DLT separation correctly designed** (hot path never syncs to
  chain; only final PoD settlement hits DLT) — `F3:235-258`, `MATCHER-API.md:104-108`.
  But the DLT half is unwritten (poetry).
- **Sovereign-node packaging tiers** (`bebop-sovereign-node-UNIKERNEL-2026-07-08.md:1-24`):
  Phase 1 OCI (default), Phase 2 WASI/WasmEdge (hardened core), Phase 3 unikernel
  (NanoVMs, no shell/SSH/exfil — the "fortress" single-tenant tier). Packaging, not
  architecture; relevant for running a vendor's sovereign node.
- **Partition resilience:** edge (compute/routing/PoD) degrades gracefully; reputation
  merge + settlement + cross-partition arbitration are fail-closed-by-design but
  UNIMPLEMENTED (`F3:262-297`).
- **NO hidden operator node in the engine:** grep found zero hardcoded endpoints /
  bootstrap / gateway in the executable path (`F3:110-114`). The one genuine
  centralization exposure is the **SDK/bootstrap access layer (DANGER #2)** — specified
  escape (thin client + reference alt-client) but not yet coded (`F3:148-164`).

---

## (d) Protocol design (fable F1-F4)

- **F1 — protocol vs platform** (`F1:...`): "0% fee + privacy = dominance / atomic bomb"
  is flagged **poetry (c)**; 0% is a subsidy, not a moat (`F1:14-21,92`). Binding
  constraint is **local liquidity**, not fee level (Rochet-Tirole, Katz-Shapiro;
  `F1:29-32`). Real moat = **earned local reputation graph + credible neutrality**
  (`F1:90-96`). Viable fee = 1–3% + value-added sinks (`F1:19,68`). Cold-start = 3-phase
  Trojan-horse widget with falsifiable RED tripwires (`F1:62-82`). "Restaurant-as-
  evangelist" and "courier loyalty" both refuted (`F1:50-56`).
- **F2 — dispute / arbitration** (`F2:...`): NO dispute code exists today
  (`F2:8-10`). Proposes a fail-closed state machine
  OPEN→EVIDENCE→AUTO_ARBITRATE→ESCALATE→JURY→SETTLE (`F2:22-37`) wired to existing
  primitives (ledger/killswitch/reputation). **Fail-closed law:** any timeout/ambiguity
  → escrow HOLD + default refund to claimant, never silent approval (`F2:35,71-87`).
  "L5 neuro-symbolic = judge" is **reification/poetry** unless routed through the
  Neuro-Symbolic Gate (advisor proposes, kernel decides — ADR-003) (`F2:100-111`).
  Juror Schelling reward + bonded stake = sound theory, **zero code** (`F2:59-67`).
- **F3 — architecture / hidden-centralization** (covered in (c)): ~70% real
  architecture, ~30% poetry (`F3:301-317`). Genuine single-operator risk = SDK/
  bootstrap (DANGER #2); settlement oracle + identity root mitigated in design, code
  absent.
- **F4 — StoryBrand + 50%-courier-drop stress** (`F4:...`): the trustworthy-delivery
  core (route + prove + attribute + replicable + fail-closed) is real and unusually
  honest; the "business shell" a vendor buys is absent: **G1 no storefront/menu/hours**
  (`Order={id,src,dst}`, `F4:180-181`), **G2 no courier marketplace** (orders pinned to
  one `src` — 50% drop = up to 50% lost throughput, the biggest resilience gap,
  `F4:182-184`), **G3 no node liveness**, **G4 no payout contract**, **G5 no economics
  model**, **G6 mid-route failure unhandled** (`F4:185-192`).

---

## (e) Gaps / conflicts vs the vendor-protocol goal

**Strong fit (keep):**
1. bebop2 is a **NON-AI, deterministic, from-scratch PQ crypto core** — exactly the
   trustless substrate the protocol needs (RNG-free, no phone-home, KAT-anchored).
   Use it to BACK: PoD signatures (ML-DSA-65 / Ed25519 hybrid), key exchange
   (ML-KEM-768), at-rest vault (XChaCha20-Poly1305 + Argon2id), self-cert identity.
2. Centralization danger map + open replicable matcher + consensus kill-switch +
   self-cert identity are genuine, code-backed decentralization wins.

**Gaps / conflicts to resolve in the unified blueprint:**
1. **Two crypto cores.** `bebop2/core` (new PQ) vs `crates/bebop/src` (vault.rs uses
   XChaCha20+scrypt, pre-PQ). Conflict: `MAP:118` cites vault.rs "XChaCha20 + scrypt"
   while bebop2 mandates Argon2id + ML-DSA/ML-KEM. **Action:** re-point vault/pod at
   bebop2 primitives; retire scrypt for Argon2id.
2. **Roadmap staleness.** roadmap:9-11,45-47 must be corrected — ML-DSA-65 + Argon2id +
   symmetric are implemented, not stubs (per deep-research:137). Risk: planning off a
   false "all stub" baseline.
3. **wasm32 empty-import gate fails** (94 errors) — the ONLY honest bare-metal/
   sovereign-node crypto proof. Blocker #1 before any "runs on the vendor's device with
   no reachable clock/RNG/socket" claim.
4. **ML-DSA-65 not NIST-bit-exact** (no oracle in sandbox). Cross-implementation
   interop with any external PQ verifier is unproven — needs an ACVP oracle before
   protocol keys are minted. Per-change human confirmation on crypto constants (the
   q=3329-vs-8380417 incident) — `bebop2-deep-research:54-58`.
5. **Settlement / escrow / payout contract = 0 lines.** The "vendor gets paid on PoD"
   promise has no code; must be built as a **device-sig threshold verifier**, NOT a
   single oracle (or it re-centralizes at DANGER #3) — `F3:131-146,331-333`.
6. **No storefront, no courier marketplace, no liveness** (F4 G1/G2/G3). For a
   *food-vendor* protocol with *no third-party couriers*, G2 is acute: vendor-employed/
   vendor-owned couriers still need reassignment/auction logic that does not exist.
7. **Physical-handoff PoD has no trustless anchor** (signature ≠ human received box) —
   the admitted weakest link (`MAP:143-152`, `F2:53-55`). Must design for
   "PoD is contestable" + route to arbitration, not treat the signature as ground truth.
8. **Dispute resolution unbuilt** — either build the F2 fail-closed state machine or
   integrate external UMA/Kleros; either way it is a GAP with a designed shape only.
9. **Fee framing** — retire the "0% atomic bomb" (poetry); adopt 1–3% + value-added
   sinks (F1). Ensure the unified pitch does not overrun the artifact (F4 fable rule).

---

## File index (read this session)

Plans: `bebop2-roadmap-2026-07-10.md`, `bebop2-deep-research-2026-07-11.md`,
`bebop-fundamental-principles-2026-07-09.md`,
`fable-protocol-2026-07-11/{F1-protocol-vs-platform,F2-dispute-arbitration,
F3-architecture-hidden-centralization,F4-storybrand-stress}.md`,
`delivery-protocol/PROTOCOL-CENTRALIZATION-MAP.md`,
`bebop-sovereign-node-UNIKERNEL-2026-07-08.md`.
Crypto src: `bebop2/core/src/{pq_dsa,kdf,pq_kem,sign,aead,hash}.rs`.
Not-yet-read but present (for follow-up): `delivery-protocol/{MATCHER-API,
DECOUPLED-MATCHER,SYSTEM-ARCHITECTURE-AUDIT}.md`,
`bebop-{fable,memory-optimisation-fable,math-physics-fable}-research-2026-07-11.md`,
`bebop-L5-*`, `bebop-sovereign-node-DEPLOYMENT-2026-07-08.md`.
