# Focus research: OpenScience · CasaOS · SimpleMem → Bebop integration

Date: 2026-07-12 · Operator: SyniakSviatoslav · Status: RESEARCH + INTEGRATION PLAN (push-plans-first)

## TL;DR
- **SimpleMem** → reverse into **ПВМЛА** (memory subsystem): add `consolidate()` (abstraction over
  related nodes) + `adaptive_recall()` (query-complexity → k). Port the CONCEPT, NOT the OpenAI
  dependency. Use local deterministic primitives (rust-core `vsa_similarity`), keep offline.
- **CasaOS** → validates category **K (Collections)**: manifest-driven curated store, one-click
  install, local registry. Adopt as benchmark; align `coll` CLI semantics.
- **OpenScience / OSF** → validates operator policy (memory-first, push-plans-first, content-addressed
  `agentic_git`). No new code; cite as provenance/pre-registration precedent.

## 1. What each tool actually is (reverse-engineered from source/paper)

### 1.1 SimpleMem (arXiv:2601.02553v1, ICML 2026)
Three-stage lifelong-memory pipeline for LLM agents:
1. **Semantic Structured Compression** — entropy-aware filtering, 10-turn window, stride 5,
   info threshold τ=0.35, distills unstructured chat → compact multi-view indexed memory units
   (strict JSON: entities/topic/salience).
2. **Recursive Consolidation** — async; embeds units (text-embedding-3-small, 1536d), stores in
   LanceDB (IVF-PQ); merges related units when cosine ≥ τ_cluster=0.85; temporal decay λ=0.1.
3. **Adaptive Query-Aware Retrieval** — estimates query complexity (gpt-4o-mini head), picks
   k∈[3,20] (min depth 3 symbolic, max 20 semantic). Re-ranking disabled; multi-view score fusion.
Result: +26.4% F1, −30× inference tokens vs full-context on LoCoMo.

### 1.2 CasaOS (IceWhaleTech/CasaOS, Go, 36.5k★)
Personal-cloud OS: Docker app-store with curated one-click apps, message-bus event system,
RISC-V support, friendly UI. App model = manifest + install script + icon.

### 1.3 OpenScience Framework (Center for Open Science)
Research lifecycle platform: plan → pre-register → collaborate → version → share. Provenance +
open licensing + reproducible artifacts. Culture/process, not a runtime library.

## 2. Descartes-square comparison (exact pros / cons, per J3 default policy)

### 2.1 SimpleMem vs Bebop ПВМЛА (LivingMemory + knowledge::recall)

| | SimpleMem | Bebop ПВМЛА (current) |
|---|---|---|
| **PRO (adv)** | +26.4% F1, −30× tokens; principled 3-stage; adaptive k | Deterministic, offline, no external API; honest noise floor; VSA in rust-core |
| **CON (disadv)** | Needs OpenAI embeddings + gpt-4o-mini (NOT offline); LanceDB dep; async LLM cost | No consolidation/abstraction; flat cosine only; fixed k; no semantic compression |
| **PRO (adv)** | Recursive abstraction reduces redundancy | Non-destructive eviction (attic) — reversible forgetting |
| **CON (disadv)** | Black-box LLM compression (unverifiable density) | Bag-of-bytes hashing loses word order/semantics |

**Exact advantages to steal:** (a) consolidation/abstraction stage, (b) adaptive query-complexity → k,
(c) multi-view scoring (entities/topic/salience metadata).
**Exact disadvantages to avoid:** external LLM/embeddings dependency (violates offline), opaque
compression (violates Verified-by-Math).

### 2.2 CasaOS vs Bebop Collections (category K)

| | CasaOS | Bebop Collections (planned) |
|---|---|---|
| **PRO** | Battle-tested app-store UX; curated + one-click; message-bus | Manifest TOML; `coll` CLI; local registry; 32-bit icon (chafa) |
| **CON** | Go/Docker runtime; not agent-shaped; needs daemon | Not yet built; no install orchestration yet |
| **PRO** | RISC-V + multi-arch; friendly UI | Native Rust; no container daemon required |
| **CON** | Centralized app index (IceWhale) | Must stay local + manual-enable for dual-use (wormgpt FLAGGED) |

**Exact advantages to steal:** curated-one-click semantics, manifest+icon model, message-bus events.
**Exact disadvantages to avoid:** external centralized index (keep local), daemon requirement.

### 2.3 OpenScience vs Bebop governance

| | OSF | Bebop operator policy |
|---|---|---|
| **PRO** | Pre-registration = plan-before-execute; provenance; versioning | push-plans-first; agentic_git content-addressed; memory-first |
| **CON** | Web platform, account-bound | N/A (policy already matches) |
| **PRO** | Reproducible artifacts | `cargo test` RED+GREEN falsifiable gate |
| **CON** | — | — |

**Verdict:** OSF validates existing policy. No code. Cite as precedent in docs.

## 3. Integration plan (what to actually build)

### 3.1 ПВМЛА upgrade (memory.rs + knowledge.rs) — OFFLINE, deterministic
- `LivingMemory::consolidate()` — group nodes by VSA similarity (rust-core `vsa_similarity`),
  emit abstract parent node when similarity ≥ τ_cluster (default 0.85, configurable). Non-destructive:
  keep children, add parent in `Long` layer. Deterministic, no LLM.
- `knowledge::adaptive_recall(mm, query, complexity)` — map query length/entropy → k∈[3,20]
  (min 3 symbolic, max 20 semantic), keep noise floor. Complexity estimator = local entropy, not gpt.
- Metadata: store entities/topic/salience per node (extend `MemoryNode`).
- Tests: RED (unrelated nodes NOT consolidated) + GREEN (related nodes consolidate; adaptive k grows
  with query complexity). Keep offline — NO OpenAI.

### 3.2 Collections (category K) — align with CasaOS semantics
- `coll` CLI: `list/add/rm/rename/snapshot/backup/share/install/icon` (already planned).
- Manifest TOML = CasaOS-style (name, icon, install, deps). Local registry only.
- Dual-use (termux/OSINT) = manual-enable + vuln scan (already policy).

### 3.3 Agents / Skills
- Agent progress (level/xp/awards) = living-memory nodes (Q3 decision, already in plan).
- Skills = searchable store; SimpleMem's multi-view metadata informs skill tagging (topic/salience).
- No external dependency introduced.

## 4. What NOT to do (ceilings / YAGNI)
- Don't pull OpenAI embeddings/LanceDB — breaks offline + Verified-by-Math. Use rust-core VSA.
- Don't build a web platform (OSF) — policy already covers it.
- Don't daemonize Collections like CasaOS — native Rust CLI is enough.

## 5. Verification
- ПВМЛА upgrade: `cargo test -p bebop memory::` + `knowledge::` RED+GREEN.
- Collections: `cargo test -p bebop coll::` after build.
- doc-claim verifier stays GREEN (counts match `cargo test --workspace`).
