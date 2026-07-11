# Living-Memory Optimization — NON-DESTRUCTIVE SAFE-APPLY AUDIT

> Source: deleg_a73ef040 (fable-style deep research, 2026-07-11). RESEARCH ONLY — no signal-loss changes.
> Topic: memory-optimisation hacks for a LIVING MEMORY LAYER, specifically NON-DESTRUCTIVE SAFE-APPLYING
> (reversible, no signal loss, graceful degradation).
> Full raw paste: `/root/.hermes/pastes/paste_9_152604.txt`.

## 0. Source ground-truth (file:line)
- dowiz living-memory spec: MEMORY.md:3-4 — index injected per-call; closed topics live in ATTIC; promote back when reactivated.
- ATTIC pattern (tiering design): MEMORY-ATTIC.md:1-5 — "Moved from MEMORY.md 2026-07-05… files remain intact… promote a line back only when its topic goes active again." ~95 archived lines, each a back-pointer to an intact .md.
- Hermes secondary cache: /root/.hermes/cache/delegation/subagent-summary-*.txt, /root/.hermes/memories/MEMORY.md (operator pref ledger, NOT a memory tier). No dedup/retrieval/index layer.
- bebop LivingMemory (the *code* analogue): memory.rs:60-66 — `tick()` does `nodes.retain(|_, n| hash(n.concept)%7 != clock%7)`. This DELETES nodes from the HashMap **permanently**. No cold tier, no restore pointer, no raw-preservation. **DESTRUCTIVE.**
- Padovan aperiodic TTL: PROPOSED, UNIMPLEMENTED in bebop (bebop-fable-research-2026-07-11.md:23,48-49,74,146).

## 1. Strategy safety matrix (9 items)
| # | Strategy | Verdict | RED / GREEN |
|---|----------|---------|-------------|
| 1 | Dedup → DUP-VAULT (move, never rm) | ✅ SAFE | vault+dup+mem+attic == pre_total; RED if < |
| 2 | Selective retrieval over full-inject (top-k + standing rules) | ✅ SAFE (additive) | recall@k(Oracle-probe) >= baseline; RED if inject-all beats retrieval |
| 3 | Compaction as DERIVED index only (keep source .md intact) | ✅ SAFE | faithfulness >= 0.95; RED if source fact unretrievable |
| 4 | Cold-storage tiering (ATTIC) | ✅ SAFE (move not delete) | attic+mem <= total ever_written; RED if moved line's target missing |
| 5 | Compression lossless (VSA frame 34.3%, reversible) | ✅ SAFE | decode(encode(x)) == x byte-exact |
| 6 | Compression lossy (VSA-VIZ image) | ❌ UNSAFE alone | MUST keep authoritative JSON server-side |
| 7 | Importance scoring | ✅ SAFE as *ranking*; dangerous if used to *delete* | ρ(score,hits)>0; RED if high-hit item culled |
| 8 | Cold-storage tiering (ATTIC) | ✅ SAFE | move not delete; source .md intact; back-pointer present |
| 9 | (Optional) Padovan tier-demotion IF implemented | ✅ SAFE | RESTORE(evicted) == payload; raw always retained |

Primary-source anchors: MemGPT (tiered recall-archive ≡ ATTIC/INBOX), RAPTOR/AutoCompressors, LLMLingua (arXiv:2310.05736, lossy prompt compression), "Lost in the Middle" (arXiv:2307.03172, justifies importance-weighting over naive recency), OS ARC buffer-cache (Megiddo & Modha 2003, FAST — 2-hand hot/cold tiering ≡ ATTIC hot/INBOX cold).

## 2. Audit of the EXISTING dowiz layer
ATTIC/INBOX tiering is a SOUND non-destructive design — already discipline-correct:
- MEMORY.md:3-4 + MEMORY-ATTIC.md:1-5: index lines *moved to ATTIC*, target .md never deleted, promotion is a one-line re-add. Exactly "move not delete".

## 3. SAFE-APPLY PLAYBOOK (reversible, no signal loss, graceful fallback)
Each step: (a) preserves full-fidelity raw, (b) reversible via move+pointer, (c) falls back to full-memory if optimiser errors. Every step carries RED (must-fail) + GREEN (must-pass).
- Step 0 — Snapshot (immutable source): `cp -r memory memory.bak-<ts>`. RED: `diff -r` non-empty → abort. GREEN: dir exists && count==before.
- Step 1 — Dedup → DUP-VAULT (never rm). Move merged near-dups to MEMORY-DUP-VAULT.md with canonical pointer. RED: vault+mem+attic < pre_total → restore from .bak.
- Step 2 — Selective retrieval over full-inject (additive). Build VSA match-style retriever over MEMORY.md+ATTIC; inject top-k + standing rules. Raw untouched. RED: recall@5(probe) < full-inject baseline → fall back.
- Step 3 — Compaction as DERIVED index only. Summarise ATTIC clusters into MEMORY-ATTIC-INDEX.md; keep every source .md intact. RED: any probed fact in source .md unretrievable AND source deleted → must not happen.
- Step 4 — Importance scoring → RANK only, never CULL. Tag lines hit:N; use for retrieval order. Never delete on low score. RED: ρ(score,hits) <= 0 or any high-hit line removed → abort.
- Step 5 — (Optional) Padovan tier-demotion IF implemented. Demote Long→ATTIC on Padovan ticks; raw .md always retained. RED: RESTORE(evicted_id) != payload → abort.

## 4. CRITICAL FINDING — bebop memory.rs is DESTRUCTIVE
Unlike dowiz ATTIC (move not delete), bebop's `tick()` permanently evicts nodes with no restore path. Before bebop LivingMemory can hold real state:
- Refactor `tick()` to move-to-ATTIC (raw-preserving) instead of `nodes.retain(...)`.
- Add restore pointer so any evicted node is recoverable.
- Gate the change with a RED test: insert node, tick past eviction, assert node recoverable from ATTIC, not gone.

## 5. Consolidated conclusion
Retrieval, dedup-to-vault, lossless compression, importance-ranking, move-based tiering are all safe. The single concrete defect found is bebop `memory.rs:60-66` destructive eviction — fix before relying on it.
