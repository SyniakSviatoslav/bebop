# FABLE RESEARCH — ANGLE 2/10: DISTRIBUTED ARBITRATION / DISPUTE RESOLUTION

**Date:** 2026-07-11 · **Repo:** `/root/bebop-repo` · **Discipline:** verify-by-math + PRIMARY sources · **Mode:** research-only
**Operator thesis:** no support desk ⇒ L5 neuro-symbolic layer auto-arbitrates from history+evidence; hard cases escalate to a jury of network stakeholders rewarded for fair judgment.

---

## 0. Code reality check (what actually exists)

There is **no dispute-resolution code in `bebop` today.** A repo-wide search (`[Aa]rbitrat|[Jj]ury|[Ee]scrow`) finds only *design* mentions in `PROTOCOL-CENTRALIZATION-MAP.md`, which explicitly lists dispute resolution as a **GAP**: *"Dispute resolution — (use UMA/Kleros, don't build) — GAP (use external)"* (`PROTOCOL-CENTRALIZATION-MAP.md:141`). The buildable primitives that *do* exist and bear on arbitration:

- `ledger.rs` — double-entry money boundary, escrow settlement, fails-closed (`ledger.rs:89-113`).
- `pod.rs` — Proof-of-Delivery: signed SHA512 claim (`pod.rs:31-96`). The evidence primitive.
- `reputation.rs` — trust score, sticky suspensions, risk premium (`reputation.rs:39-93`).
- `guard.rs` — `io_guard` (fail-closed refuse) + `KillSwitch` (≥2/3 supermajority suspension) (`guard.rs:54-123`).
- `stabilizer.rs` — `ground_state` collapse-to-safe + `io_guard` refuse (`stabilizer.rs:96-98`, `guard.rs:54`).

The L5 layer is defined elsewhere as an **advisor**, not a decider — see ADR-003 (`adr-003-neuro-symbolic-gate-2026-07-09.md`: the kernel decides, symbolic arbiter clamps; advisor is *"a consultant, never a driver"*).

---

## 1. Dispute state-machine (open → evidence → auto-arbitrate → escalate → jury → settle)

No existing enum; proposing one wired to the primitives above. Messages + timeouts:

| State | Entry trigger | Message in | Timeout | Exit (success) | Exit (fail) |
|---|---|---|---|---|---|
| `OPEN` | customer/courier raises dispute on `order_id` | `DisputeOpen{order_id, claimant, respondent, reason}` | — | → `EVIDENCE` | — |
| `EVIDENCE` | both parties submit | `SubmitEvidence{pod_proof?, photo_hash?, geo?, complaint}` | **T_ev = 48h** | → `AUTO_ARBITRATE` | timeout ⇒ auto-bind POD if present else → `ESCALATE` |
| `AUTO_ARBITRATE` | L5 proposes verdict | `AutoVerdict{winner, confidence, evidence_refs}` | **T_aa = 10m** | confidence ≥ θ ⇒ `SETTLE` | confidence < θ **or no verdict** ⇒ `ESCALATE` |
| `ESCALATE` | jury empanelled (reputation-weighted sample) | `Empanel{jurors[]}` | **T_em = 24h** | → `JURY` | empanel fail ⇒ `SETTLE` as **refund/escrow-hold** (fail-closed) |
| `JURY` | jurors vote | `JuryVote{juror, side, stake}` | **T_j = 72h** | majority ≥ 2/3 ⇒ `SETTLE` | no quorum ⇒ `SETTLE` as **refund/escrow-hold** |
| `SETTLE` | final | `Settle{winner, payout, escrow_release}` | — | ledger release (`ledger.transfer`) | — |

**Fail-closed law (must hold):** any timeout, missing verdict, or ambiguous majority in `AUTO_ARBITRATE`/`ESCALATE`/`JURY` resolves to **`SETTLE` with escrow HOLD + default refund to claimant**, never to silent approval of the respondent. This is the single invariant the whole machine exists to protect (see §4).

**Mapping to code:** `SETTLE` executes through `ledger.transfer` (`ledger.rs:89`); the empanel set reuses `KillSwitch` supermajority math (`guard.rs:107-113`); juror weights reuse `reputation.rs::score` (`reputation.rs:69`).

---

## 2. What evidence binds (proof-of-delivery) — does the code support tamper-evidence?

**YES for the bytes; NO for the physical truth.** The POD primitive is cryptographically sound:

- Claim is bound to `order_id | courier_id | ts | (x,y)` (`pod.rs:31-39, 44-49`).
- `verify_delivery` returns `false` if id mismatches or key is tampered (`pod.rs:88-96`).
- Tampered claim ⇒ verification fails (RED test `pod_fails_on_tampered_claim`, `pod.rs:139-150`).
- Replay at wrong location ⇒ fails (RED test `pod_replay_at_wrong_location_fails`, `pod.rs:152-165`).
- Misattribution refused (`pod.rs:73-83`).

So **tamper-evidence holds for the signed claim**: you cannot alter the order/location/claimer after signing without breaking the signature. This is real engineering.

**The gap the operator must not paper over:** the `(x,y,t)` and `courier_id` in the claim are **self-asserted by the courier's device** — there is no secure-element, GPS-attestation, or multi-party (restaurant+customer+courier threshold) signature in `vault.rs` or `pod.rs`. A repo search of `vault.rs` for `geo|location|gps|secure element|attest` returns **0 matches**. The centralization map says this outright: *"signature ≠ human received box … the physical-handoff weakness must be designed-around, not hidden"* (`PROTOCOL-CENTRALIZATION-MAP.md:143-152`).

**Verdict on §2:** the evidence *binds cryptographically* (cites `pod.rs:88-96`); it does **not** bind *physically* (no `vault.rs` attestation). Auto-arbitration must therefore treat a POD as *prima facie* evidence, contestable — exactly the dispute path. Building "L5 as judge from POD" while ignoring this is reification of the signature as ground truth.

---

## 3. Game-theory check: is the juror reward Sybil-resistant & incentive-compatible?

**Current code:** no juror-reward, no staking, no Schelling mechanism exists. `KillSwitch` provides a Sybil *barrier* only in that `vote_suspend` ignores unknown voters (`guard.rs:94-98`) — but that list is a suspension registry, not a jury incentive. The centralization map proposes the fix: *"Sybil resistance via staking bonds (slashed on failure)"* (`PROTOCOL-CENTRALIZATION-MAP.md:121-123`). So the mechanism is **design-only**.

**Incentive compatibility (PRIMARY — Kleros whitepaper, kleros.io/assets/whitepaper.pdf):** Kleros makes truthful voting a Nash equilibrium via a **Schelling-point** mechanism: jurors are rewarded for voting with the eventual majority and penalized (slashed stake) for dissenting. The coordinated equilibrium (everyone votes the objectively-correct side) pays more than any collusive equilibrium *if* the honest majority is the cheaper-to-coordinate focal point. This is the standard result that **truthful reporting is incentive-compatible under proper stakes** (mechanism-design: the mechanism is *strictly* proper when reward-for-coincidence > reward-for-correctness-alone; cf. the classic result that a scoring/peer-prediction rule elicits truth as a dominant strategy, Miller et al.; and the Schelling coordination game where the "fair" outcome is the focal point, cyberjustice.ca 2022).

**Sybil resistance:** requires **bonded stake per juror** so a Sybil farm must fund N identities with N stakes; one-sided collusion is only profitable if the attacker controls a supermajority *and* out-spends the honest bond pool. The `KillSwitch` 2/3 threshold (`guard.rs:108`) is the right shape but the *stake* that makes it costly to fake is missing. Without stake, Sybil is trivial ⇒ mechanism is **NOT Sybil-resistant as proposed**.

**Verdict on §3:** the *math* (Schelling + bonded stake) is real and well-sourced, but **zero of it is implemented**. The honest status: *applicable theory, absent code.* A juror reward equal to the auto-arbitrator's confidence-weighted prior would let jurors free-ride on L5; recommend the reward be paid only on *divergence-with-correctness* (penalize bandwagoning) to keep the equilibrium strict.

---

## 4. RED-LINE: who bears liability when the auto-arbitrator is wrong? Falsifiable fail-closed test.

Liability attribution under fail-closed: when L5 is wrong, **the protocol/escrow bears it via default refund** — the customer is never silently denied. The auto-arbitrator has *no* authority to release funds on its own; only `SETTLE` → `ledger.transfer` can move money (`ledger.rs:89`), and `SETTLE` only fires after a confidence ≥ θ in `AUTO_ARBITRATE` *or* a jury majority. Therefore a wrong L5 verdict that drops below θ, or that the jury overturns, **cannot** have released funds.

**Falsifiable test (pseudo, must be a `#[test]`):**

```
GIVEN a disputed order in AUTO_ARBITRATE with escrow held in ledger
WHEN L5 returns AutoVerdict{winner=respondent, confidence=0.41}  (θ=0.6)
  OR L5 returns NO verdict within T_aa
THEN state MUST transition to ESCALATE (never SETTLE)
AND ledger.balance(respondent) MUST be UNCHANGED (no release)
AND escrow account MUST remain == order value (hold intact)
// RED: if balance(respondent) increased, the test FAILS => system approved silently
```

Equivalent tests for `ESCALATE` empanel-fail and `JURY` no-quorum: both must resolve to `SETTLE(refund_to_claimant, escrow_hold)`. This reuses the ledger's own fails-closed invariant — *"insufficient funds … fail closed"* (`ledger.rs:105-107`) and conservation (`ledger.rs:79-81`): a silent approval would break `conserved()==true`. The bound is falsifiable by construction: **any state exit that increments `respondent` balance without a jury majority or θ-confidence is a RED failure.**

---

## 5. Honest verdict — is "L5 as judge" real engineering or reification?

**3-way verdict (a=genuine / b=applicable / c=poetry):**

| Claim | a | b | c | Note |
|---|---|---|---|---|
| POD tamper-evidence (bytes) | ✅ | ✅ | — | `pod.rs:88-96` real |
| Fail-closed ledger settlement | ✅ | ✅ | — | `ledger.rs:89-113` real |
| KillSwitch supermajority | ✅ | ✅ | — | `guard.rs:107-113` real |
| "L5 neuro-symbolic = automatic arbitrator/judge" | ⚠️ | ⚠️ | ✅ | **POETRY if read literally** |
| Juror Schelling reward, Sybil-bonded | — | ✅ | ⚠️ | theory only, no code |
| Physical-handoff truth anchor | — | — | ✅ | gap admitted, `PROTOCOL-CENTRALIZATION-MAP.md:143-152` |

**The reification trap:** calling L5 "the judge" conflates the *stochastic advisor* with the *deterministic decider*. ADR-003 is explicit: *"The advisor … is a consultant, never a driver. The kernel … is the only actor that writes authority"* (`adr-003-neuro-symbolic-gate-2026-07-09.md:14-23`). If "L5 as judge" means *L5 proposes a verdict, the symbolic arbiter + ledger + jury ratify it*, that is **genuine engineering** and already has the right shape in `stabilizer.rs`/`guard.rs`/`ledger.rs`. If it means *L5 unilaterally decides disputes*, it is **reification** — and it violates the Neuro-Symbolic Gate contract that the operator's own ADR-003 accepted.

**Poetry to flag:**
- *"L5 neuro-symbolic layer as automatic arbitrator"* — reifies a control-theory advisor as a legal subject. The arbitrator is the **symbolic gate + ledger + jury**, not L5.
- *"jury of network stakeholders rewarded for fair judgment"* — "fair" is undefined; the mechanism rewards *coordination with consensus* (Schelling), which is *correlated with* fair, not identical. Honest rename: "rewarded for voting with the coordinated majority."
- *"history+evidence"* as L5 input — fine as advisor context; must not become the decisive input without the θ-confidence gate.

**Bottom line:** The *dispute envelope* (evidence binding, fail-closed settlement, supermajority escalation) is real and code-backed. The *L5-judge metaphor* is poetry unless explicitly routed through the Neuro-Symbolic Gate where L5 proposes and the deterministic core decides. Build the state machine in §1, keep `SETTLE` gated by `ledger.transfer`, implement the bonded-Schelling jury from §3, and the "L5 arbitrator" becomes engineering — not consciousness metaphor.

---

## Falsifiable summary (RED + GREEN)

- **GREEN (holds today):** POD tamper-evidence `pod.rs:88-96`; ledger fail-closed `ledger.rs:105-107`; supermajority suspension `guard.rs:107-113`.
- **RED (must be proven before "L5 judge" ships):** no juror reward/stake code; no geo-attestation in `vault.rs` (0 matches); no dispute state-machine exists; default-refund fail-closed test (§4) is unwritten.
- **RED-LINE classification:** silent L5 approval of a respondent payout = **catastrophic RED**; the §4 test is the gate that must fail-closed.
