# System Architecture Audit — Integration Map

> Status: implementation + audit synthesis (2026-07-10). Maps the protocol's
> code primitives to the operator's system-architecture audit (investability,
> StoryBrand, security/reliability/core-math) and answers the blocker question.

## Blocker verdict: trust, not interface

The audit asks: is the blocker *no trust between nodes* or *no standard
interface*? **Trust is the binding constraint; the interface already exists.**

- Interface (DONE): the matcher is a serialized JSON contract (`MatcherRequest`/
  `MatcherResponse`) behind a `MatcherClient` trait. `RemoteMatcherClient` +
  `Transport` prove any node serves identically over the wire
  (`remote_matches_local_over_wire` — same fingerprint local vs remote). No
  lock-in, no privileged endpoint.
- Trust (DONE this batch): reproducibility ≠ accountability. `fingerprint()`
  proves two nodes *compute the same output from the same input* — it says
  nothing about whether the **input graph was honest**. So the missing primitive
  was **cryptographic, pseudonymous attribution + a reputation ledger** (who
  feeds honest graphs). `pod` + `reputation` close that. A network of strangers
  with a perfect interface but no reputation = "whoever feeds the most
  convincing (fake) graph wins" = DANGER #1 by another name. Reputation is the
  moat (audit 29160 #6): the trust graph is an asset competitors cannot copy.

## Section 1 — Architecture & Stability (Product/Operations)

| Audit item | Primitive | File | Proof |
|---|---|---|---|
| Input/Output Guard | `io_guard` (L5 proposal envelope: stable field + `max_delta` wall) | `guard.rs` | `io_guard_refuses_unstable_field`, `io_guard_refuses_out_of_envelope` |
| Kill switch (consensus-level) | `KillSwitch` (≥2/3 of known nodes vote to suspend a peer; no central off-button) | `guard.rs` | `killswitch_needs_supermajority_not_single_node` |
| Evals / fail-closed | every module RED+GREEN | all | 292 tests |
| Gradual rollout | `BEBOP_WAVE_GATE` env flag (existing) | `wavefield.rs` | `off_by_default_ignores_redline_reach` |

**Key design call:** the kill-switch is NOT a code flag — it is a *consensus
suspension* of a peer. One node cannot kill another; ≥2/3 must agree. This is
the operator's "kill-switch at consensus, not in code" requirement, met.

## Section 2 — Cryptography & Behavioral Logic

| Audit item | Primitive | File | Proof |
|---|---|---|---|
| Princess Pi attribution (29157) | `pod` — SHA512(claim) signed with vault hybrid sig (ML-DSA-65 ⊕ Ed25519); courier id = vault self-cert id (NOT PII) | `pod.rs` | `pod_sign_verify_roundtrip_pseudonymous`, `pod_refuses_misattribution`, `pod_fails_on_tampered_claim`, `pod_replay_at_wrong_location_fails` |
| Self-taught L5 (29153 / arxiv 2104.03902) | `stabilizer` (Lyapunov/MRAC bounded-delta + ensemble disagreement freeze) is the *self-optimizing* layer; new `reputation` feeds its cost surface from live data | `stabilizer.rs`, `reputation.rs` | `stabilizer` RED+GREEN; `reputation` RED+GREEN |

**Princess Pi mapping:** the claim `order:<id>|courier:<vault_id>|at:<ts>|loc:<x,y>`
is SHA512-hashed then signed. `courier_id` is the courier's self-certifying
vault id (a hash of their public key) — verifiers prove authorship without
learning a name. This is the trustless anchor for the physical handoff (the
weakest link from the centralization map): settlement can require a valid POD.

> Research note: the external `codex.churchofmalware.org` resource was not
> retrievable in-session (network-off / sandbox). The integrated scheme uses the
> SAME primitives the source describes (SSH/Ed-style key signature + SHA512
> content hash + pseudonymous distribution) on bebop's already-audited hybrid
> signature stack. Adapt before any production claim.

## Section 3 — Mathematical & Signal Apparatus (Core Engine)

| Audit item | Primitive | File | Proof |
|---|---|---|---|
| Dot/Cross (29155) | `vsa_similarity` (dot), `cosine_similarity` (norm-invariant), `cross_product` (orthogonality / collinear-degeneracy detector) | `rust-core` | `test_cosine_similarity_bounds`, `test_cross_product_orthogonality` |
| Sinc (29159) | `sinc` (removable singularity at 0; interpolation/windowing kernel) | `rust-core` | `test_sinc_singularity_and_zero` |
| Calculus vocabulary (29154) | already in `rust-core` (Laplacian/divergence via `field_*`) + `field_physics` | `rust-core`, `field_physics.rs` | `test_laplacian_zero_row_sum` |

**Anti-drift (audit focus #3):** weight computation in the protocol is built on
`cosine_similarity` (norm-invariant) so norm inflation cannot silently rotate a
decision, and `cross_product` detects collinear/degenerate tensor directions
(the literal "drift" vector). This is the deterministic guard against L5
hallucination the operator asked for.

## Investability (29160) — the 6 dimensions

1. **Founder-market fit** — operator builds delivery infra they use; documented.
2. **Market size** — food/logistics delivery is a known Trillion-class TAM.
3. **Traction** — 292 deterministic tests, 5 protocol modules, open matcher.
4. **Product/wedge** — decentralized dispatch with NO privileged server (DANGER #1 killed) + pseudonymous PoD.
5. **Distribution/GTM** — StoryBrand answers below; couriers/restaurants onboard via the open client.
6. **Defensibility / Moat** — the **reputation + trust graph** (`reputation.rs`) earned by real verified deliveries; Princess-Pi pseudonymous attribution; consensus kill-switch. Copied only by copying the network's earned trust — not the code.

## StoryBrand (29152) — 4 questions, answered

1. **Is this for me?** — restaurants: "keep 100% of margin, no 30% platform tax"; couriers: "get dispatched by math, not by a black-box".
2. **What's the risk?** — fail-closed by design (guards refuse; PoD is non-repudiable; kill-switch is consensus, not a vendor's mood).
3. **Worth the effort?** — open client + open matcher: integrate once, no lock-in.
4. **How does life change?** — you own your dispatch; the protocol is the courier, not the tollbooth.

## Agentic patterns (29156) — adopt vs delegate

The 482-page Google report covers ReAct, Plan-and-Execute, Reflection, Tool-use,
Multi-agent. **Adopt:** Tool-use (the matcher/cost/guard are tools behind a
contract) + Multi-agent ensemble (the stabilizer's consensual L5 defense already
does this). **Delegate / do NOT reinvent:** ReAct planning loops and Reflection —
bebop's `stabilizer` (bounded-delta, ensemble-disagreement-freeze) and
`wiring` 3-layer runtime already provide the orchestration substrate; wrapping
it in a generic agentic framework would add RNG/non-determinism the protocol
explicitly forbids (Verified-by-Math requires deterministic proofs).

## What remains (honest gaps)

- **PoD hardware attestation** — `pod` proves *authorship* of a claim; it cannot
  prove the courier was *physically present* without a hardware anchor (phone
  secure-element / NFC at drop). bebop cannot supply that; it's an integration
  point, flagged, not faked.
- **Real network transport** — `Transport` is specified with `InMemoryTransport`
  as the faithful stand-in; HTTP/p2p implementations are drop-in, no contract
  change.
- **Settlement layer** — matcher proposes intent; settlement on DLT is a
  separate, out-of-scope concern anchored only after a valid POD.
