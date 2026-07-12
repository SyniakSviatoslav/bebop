# bebop2 Remediation — Parallel Execution Plan (2026-07-12)

> Companion to `bebop2/RED-TEAM-REVIEW-2026-07-12.md` + `bebop2/REMEDIATION-BLUEPRINT-2026-07-12.md`.
> Maps the Phase 0–5 blueprint into **independently-buildable workstreams** for max-parallel agent launch.
> Operator rule (2026-07-11): memory-first → push-plans-first → ground-truth-outranks-plans. This doc is the plan.
> Red-line invariant: **NO-COURIER-SCORING** (structural, no rating fields) + **hybrid-only-until-audit** (both sig legs non-`Option`) + **no serde on the signed path** (ARCHITECTURE.md:75).

## 0. Recon — what is ALREADY done (do NOT re-do)

| Item | State | Evidence |
|---|---|---|
| Workspace + `Cargo.lock` | DONE | root `Cargo.toml` `[workspace]` incl. all 4 bebop2 crates; `Cargo.lock` present |
| proto-cap/proto-wire rewrite (replay+expiry-first, hybrid gate) | DONE on `fix/sovereign-core-gate` (unmerged) | 1350 LOC new; `HybridGate::check` checks `is_fresh`+nonce BEFORE classical verify |
| F2 ML-DSA-65 FIPS fix + ACVP | IN PROGRESS (unmerged) on `fix/mldsa-fips204-acvp` | `pq_dsa.rs` 893 lines changed; NOT yet ACVP-anchored (verify subagent spawned) |
| proto-crypto (F1 ladder) | SCAFFOLD on `review/proto-crypto` | `ladder.rs`/`fips_regen.rs`/`wycheproof.rs` stubs |

**Open (not yet touched by any branch):**
- `rng.rs` — still `from_seed` constant; **no entropy source** (F1) → DEADLIEST break.
- `capability.rs` — still signs over `serde_json` (F4 / §4A).
- proto-cap `subject_key` — still self-attested, no `AnchorRoster` (F3 / §3A).
- core numeric: `lyapunov.rs`/`kalman.rs`/`field.rs`/`fft.rs`/`lib.rs`(allocator,fexp)/`algebra.rs`(cosine) — wrong/out-of-envelope (3C).
- CI property gates: real empty-import gate, `deny.toml`, `cargo-audit` (F5 / 3G).
- `proto-wire` plaintext `ws://` accept, no rustls (F6); no channel binding (F7).

## 1. Workstreams (independent builders — parallel-safe)

Each builder works in its OWN `git worktree` (isolated cwd + branch), builds + runs `cargo test`
RED→GREEN, and **does NOT commit** (3-model hook). It reports: worktree path, files touched,
the RED test (fails on current tree) + GREEN test (passes after fix), and `cargo test` evidence.

| WS | Phase | Deliverable | Branch | Blocks | RED (today) → GREEN |
|---|---|---|---|---|---|
| **WS-1** | 0 F1 | Fail-closed entropy source in `core/src/rng.rs`: `Entropy` trait (`getrandom`/`RDRAND`/wasm `crypto.getRandomValues`), ChaCha20 DRBG seeded from it, reseed on fork; `from_seed` → `#[cfg(test)]`/test-only; production keygen returns `Err` if no provider; release profile w/o provider fails to compile | `feat/entropy-fail-closed` | F2/F3/F7 keygen | RED: `keygen([42u8;32])` compiles & silently predicts keys → GREEN: `keygen` requires entropy; constant seed is test-only; same-seed != prod path |
| **WS-2** | 0 F4 | Canonical TLV signing codec `proto-cap/src/tlv.rs`: fixed-layout `DOMAIN_TAG‖struct_tag‖wire_version‖field_count‖[field_id‖u32 len‖bytes]…`, `sha3_256(payload)` as signed field, channel-binding field; per-type domain tags | `feat/tlv-canonical` | F3/F7 | RED: `serde_json` reorder/float silently breaks → GREEN: TLV re-serialize stable; cross-structure reuse rejected by domain tag |
| **WS-3** | 5 3C | Numeric correctness in `core/src`: Lyapunov (Hessenberg+Francis QR, symmetric-only fast-path), Kalman (Potter/Carlson sqrt P=SSᵀ + PSD test), `active_diffuse` sign+CFL+steps guard, non-pow2 Bluestein/panic guard, allocator real-address align+atomic, `cosine_similarity` split-root, `fexp` i64 clamp | `feat/numeric-correct` | — | per-item RED→GREEN (e.g. `[[0,1],[-100,-2]]`→stable; `active_diffuse` energy decays; non-pow2 matches DFT oracle) |
| **WS-4** | 0/3G F5 | Property-gate CI: `deny.toml` (advisories+bans+sources+licenses, ban `openssl-sys`/`native-tls`), `cargo audit` in CI, real empty-import gate (parse RELEASE wasm import section w/ `wasmparser`, fail-closed, RED fixture), `Cargo.lock` `--locked` | `feat/property-gate-ci` | trustable verify | RED: self-captured/trivially-green → GREEN: bad import fixture rejected; advisory DB present |
| **WS-5** | 2 F3 | `AnchorRoster` + UCAN-subset delegation types in `proto-cap/src/roster.rs`: enrolled anchor set, `verify(chain)` enforces root∈roster → chain alignment → `effect ⊆ scope` → `tail.aud==subject_key`; kills self-issue | `feat/anchor-roster` | protocol trust | RED: self-signed cap accepted → GREEN: unknown `subject_key` rejected; scope>effect rejected |

## 2. Verify-in-parallel (not a builder — audits in-flight work)

- **V-1**: review `fix/mldsa-fips204-acvp` for ACVP byte-exactness (uniform A NTT-domain, c̃48, FIPS packing 1952/4032/3309, hint check). External NIST vectors, not self-KAT. → gates F2 "post-quantum" claim.

## 3. Launch order (max parallelism within `delegate_task` batch-of-3 limit)

- Wave 1 (launched): **WS-1 + WS-3 + WS-4** (disjoint: rng.rs / core numeric / repo infra).
- Wave 2 (launched): **WS-2 + WS-5 + WS-F2** (proto-cap tlv / proto-cap roster / F2 ACVP gate).
- Wave 3 (launched, additive): **WS-6 + WS-7** (rustls TLS replaces native-tls/OpenSSL [kills deadliest §2]; channel-binding on SignedFrame [F7]). WS-6/WS-7 are additive to proto-wire/proto-cap and launched in parallel with W1/W2 — they do NOT hard-block on W2; they coordinate merge via naming (binding_signing_domain keeps signing_domain name). Integration still ordered so WS-6/WS-7 merge after WS-2/WS-5 to avoid rebase churn.

### Wave 2 status (2026-07-12)
- WS-2 TLV codec: **GREEN** — `cargo test -p bebop-proto-cap` 21 passed; serde_json removed from signing path; tlv.rs + capability.rs + signed_frame.rs + scope.rs.
- WS-5 AnchorRoster: **GREEN** — `cargo test -p bebop-proto-cap` 15 passed; self-issue→UnknownIssuer, escalation→ScopeViolation, broken link→ChainBroken, valid chain→Ok; roster.rs + error.rs.
- WS-F2 ACVP gate: **GREEN (60/60)** — `cargo test -p bebop2-core` 157 passed; ML-DSA-65 byte-exact vs NIST ACVP FIPS204 (25 keyGen + 20 sigGen + 15 sigVer). The subagent's reported "1 failing sigVer tcId20" was a TEST-HARNESS TYPO (hardcoded `20 => false` vs JSON `testPassed=true`), not a crypto bug — fixed by deriving `want` from parsed testPassed and gating `mod acvp_tests` with `#[cfg(test)]`. Impl was already interoperable (25/25 keyGen + 20/20 sigGen proved it). Module doc already states ACVP verification (stale "network blocked" claim gone).
### Wave 3 status (2026-07-12, independently re-verified)
- WS-6 rustls TLS: **GREEN** — `cargo tree -p bebop-proto-wire -i openssl-sys` → "did not match any packages" (OpenSSL gone, kills deadliest §2); `cargo test -p bebop-proto-wire` → 8 passed; new rustls handshake smoke test passes. Independent re-check confirmed.
- WS-7 channel binding: **GREEN** — `cargo test -p bebop-proto-cap -p bebop-proto-wire` → 13 + 10 passed; cross-channel replay rejected, binding tamper rejected, legacy None→zero slot flagged.

### Integration / merge coordination (do NOT parallel-merge blindly)
- WS-2 and WS-7 BOTH edit `bebop2/proto-cap/src/signed_frame.rs` (WS-2 TLV signing_domain; WS-7 channel_binding + binding_signing_domain). → integrate in the SAME batch; expect a rebase/conflict at `signed_frame.rs`. WS-7 was built additive (kept `signing_domain` name, added `binding_signing_domain`/`with_binding`) to minimize conflict, but the file will still need a manual 3-way merge.
- WS-6 edits `bebop2/proto-wire/Cargo.toml` + `wss_transport.rs`; WS-7 edits `bebop2/proto-wire/src/handshake.rs` + `wss_transport.rs`. → WS-6 and WS-7 touch `wss_transport.rs` (WS-6 TLS setup, WS-7 passes binding into SignedFrame after handshake). Integrate together; WS-7's edit is a call-site addition, low conflict risk.
- WS-1 (rng.rs), WS-3 (core numeric), WS-4 (repo infra/CI) touch disjoint areas from WS-2/5/6/7 → can merge independently.
- WS-F2 lives on `feat/mldsa-acvp-gate` based on `fix/mldsa-fips204-acvp` (NOT review/proto-crypto) → merges into the F2 fix branch first, then that branch into main. It is the only Phase-0 item on a different base branch.
  (source: RustCrypto/signatures `ml-dsa/tests`, which are the canonical NIST ACVP-Server exports).
- Counts (ML-DSA-65): keyGen=25, sigGen=20, sigVer=15. Format: `testGroups[].parameterSet=ML-DSA-65`,
  tests carry `seed`/`pk`/`sk`/`msg`/`rnd`/`signature` as hex. `vsId=42, revision=FIPS204, isSample=false`.
- This makes F2 a TRUE property-gate: `fix/mldsa-fips204-acvp` already has correct FIPS-204 structure
  (sizes 1952/4032/3309, c̃=48, real NTT ζ, uniform-A NTT-domain) but only had a "differential probe"
  (prints STAGE bytes for manual diff) — NO self-checking external vector. WS-F2 wires the vendored
  ACVP json into a `#[test]` that asserts byte-exact keygen/sign/verify. → "post-quantum" claim gate.

## 4. Integration discipline (3-model review)

For each WS: builder (subagent) → independent REVIEWER subagent (reads diff, security lens) → OVERLAP
subagent (cross-checks reviewer vs blueprint spec). Orchestrator prepares `.review/staged.json`
(builder/reviewer/overlap = 3 distinct agent ids) and commits only after all 3 attest.
NO self-certification. NO merge-to-main without operator sign-off (red-line).

## 5. Honest caveats

- "post-quantum" label stays OFF until WS-1 (entropy) + F2 (ACVP) both green. Until then classical Ed25519 is the only load-bearing sig.
- WS-2/WS-5 make proto-cap a *protocol* (canonical encoding + anchored trust); proto-wire confidentiality (F6) + iroh (F4 mesh) remain after.
- Deleting README claims for absent `reloop/`/`kernel/`/`cli/` once WS-4 doc-truth scan lands counts as progress.

---

## 6. INTEGRATION COMPLETE — 2026-07-12 (operator waived sign-off; commit verified+tested)

All 7 workstreams merged into `review/proto-crypto` and pushed to origin.

- WS-1 entropy: pending (Wave 1 still running — notify on completion)
- WS-2 TLV (F4): merged — kills serde_json on signed path
- WS-3 numeric (Phase 0): merged — 7 numeric fixes
- WS-4 CI-gate (F5): merged — deny.toml bans OpenSSL, cargo-audit+deny in CI
- WS-5 AnchorRoster (F3): merged — kills self-issue auth bypass
- WS-6 rustls (F6): merged — OpenSSL GONE (cargo tree -i openssl-sys: no match)
- WS-7 channel binding (F7): merged — defeats cross-channel replay
- WS-F2 ACVP (F2): merged — ML-DSA-65 60/60 NIST ACVP byte-exact

**Final: 499 Rust tests pass, 0 fail (`cargo test --workspace`).**

### Integration fixes applied during merge (real bugs, not cosmetic)
1. `signed_frame.rs`: auto-merge produced a DUPLICATE `channel_binding` field
   (compile error) — resolved to single field, union of WS-2 + WS-7 tests.
2. `roster.rs` `Delegation::canonical_bytes`: WS-5 builder used `serde_json::to_vec`
   on the SIGNED delegation path — a serde_json-on-signed-path regression that
   violates ARCHITECTURE.md:75 / red-team §4A (the exact defect WS-2 was built to
   kill). FIXED: now uses canonical TLV codec (`DOMAIN_DELEGATION` tag). serde_json
   remains dev-only in proto-cap.
3. README/AGENTS test-count: the doc-claim verifier requires EXACT match to
   `cargo test --lib --workspace`; each merge shifted the total (411→430→436→499).
   Set final = 499. NOTE: the verifier's own `cargo` invocation has a 300s timeout
   that can truncate a COLD test build → false "test count mismatch" (saw 281 vs 416
   flap). Warm the test build before committing on these heavy crypto worktrees.

### Next (after WS-1 lands)
- Merge WS-1 (entropy fail-closed) → final Phase 0 closure.
- Then Phase 1–5 per blueprint. Red-team 3A/4A addressed; §2 (OpenSSL) KILLED.
