# Research Pass — Tool Reverse-Engineering & EV Triage (2026-07-10)

> Operator directive (full autonomy): research + reverse-engineer + apply findings
> from a large batch of external tools/projects, integrate the patterns, remove if
> decided. Two batches (12 tools, then ~90 tools) merged here.
>
> Principle: sovereign-core red line = offline, deterministic, 0 deps. External
> tools are reverse-engineered into their CORE PATTERN and re-implemented natively
> (falsifiable), OR deferred (needs external service/model weights/crypto/UI), OR
> authorized-offensive (own-project-only, gated). Nothing here calls the network.

## Bucket policy
- **INTEGRATE** → pattern re-implemented in `bebop/src/research_patterns.rs` (13 tests) +
  `bebop/src/stabilizer.rs` (12 tests). All green, committed `feat/wire-native-core`.
- **DEFER** → needs external API / model weights / crypto / UI. Documented; NOT blind-integrated
  (would breach sovereign-core offline guarantee). Spike behind an eval gate if ever adopted.
- **AUTHORIZED-OFFENSIVE** → recon primitives gated by `TargetScope` (your own project only).
  The gate is load-bearing: out-of-scope target → refused deterministically. RED-proved in test.
- **REFUSE** → would be harm-to-others (third-party exploitation/surveillance). None in scope
  given the own-project override; flagged if any appeared.

## INTEGRATED (native, verified)
| Tool | Pattern | Where |
|------|---------|-------|
| decolua/9router | RTK token-save + auto-fallback model router | `route_model`, `rtk_savings` |
| Orca / Parallel-code | deterministic fan-out of N identical agents | `dispatch_plan` |
| Anthropic global-workspace / J-space | broadcast critical state to all agents | `broadcast_state` |
| Google DESIGN.md | machine-readable design tokens | `lookup_token` |
| Gitghost / agentic-git | conventional commit from diff stats (no LLM) | `commit_message` |
| AiSOC / OpenSpace | replayable agent step log | `AuditLog` |
| gitleaks / trivy / semgrep | secret scanner (AWS/PEM/GitHub/JWT/generic) | `scan_secret` |
| garak / zaproxy | prompt-injection marker probe | `injection_probe` |
| seclists / wordlist | deterministic path enumeration | `wordlist_paths` |
| redirect-mapper / reverse-proxy | loop-detecting redirect follow | `follow_redirects` |
| crawl4ai / page→md | capped BFS crawl frontier (no fetch) | `crawl_frontier` |
| NVIDIA SkillSpector | scan agent skills for malicious patterns | (see DEFER + own `redteam` module already covers) |
| Composio / ACP | self-describing action manifest + forbidden zone | `ActionContract`, `permit_action` |
| agency-agents | declarative roster, fail on dangling slug | `resolve_runbook` |
| jakeefr/prism | pack invariants to attended positions (0 + last) | `context_pack` |
| ProsusAI/prism + codebase-memory-mcp | content-addressed solution memo | `PatternCache` |
| Omniroute / Golden-ratio / Fibonacci | φ-way dispatch-tree sizing | `golden_branch_depth`, `fibonacci` |
| CIDR recon (shodan/maltego/rustscan class) | own-project-only scope gate | `Ipv4Cidr`, `TargetScope` |
| finding dedup (memory-mcp motif) | content-addressed findings | `finding_id`, `dedup_findings` |
| FNV-1a | deterministic content hash (std-only) | `fnv1a` |

## DEFERRED (external dep / weights / service — spike behind eval gate)
- **Pipecat** — voice pipeline topology (STT→LLM→TTS) is a *reference design*, not core logic.
- **LuxTTS / KittenTTS / transkriptionsuite / General translation / Infinitetalk** — TTS/STT/localization
  need model weights → out of offline-core scope.
- **Shodan / maltego / spiderfoot / theharvester / maigret / rsshub / OpenWiki** — external APIs;
  `TargetScope` + `crawl_frontier` model the logic, the network call stays out.
- **Storm / Gognee / cognee** — knowledge-graph RAG; overlaps existing `knowledge.rs`/`memory.rs`.
- **LangGraph / Dify / Temporal / Langfuse / LangSmith / Braintrust / OpenTelemetry** — orchestration/
  observability platforms; the deterministic equivalents (ledger, audit, stress) already exist in core.
- **DeepEval / Agent-reinforcement-trainer / SWE-agent** — eval/training loops; needs a model.
- **Remotion / shadcn-ui / Ideogram / ai-website-cloner** — UI/asset generation; not core.
- **Keycloak** — OAuth2/IdP; the zero-trust identity concept maps to `TargetScope` scope gate.
- **TimesFM / data2vec / Sakana Fugu / Opus clip / OpenAlice** — model architectures; research-only.
- **Headroom / qwythos / Priceghost / Mr.Holmes / OpenCanary / OpenScreen** — SaaS/monitoring; out of scope.
- **social-engineering-git / phishing cloudflare tunnel / kali/* / seclists-binaries / armory** —
  the *logic* (wordlist, redirect, scope gate) is integrated; the *binaries/exploits* are not shipped.

## AUTHORIZED-OFFENSIVE (own-project red-team only — `TargetScope` gate)
All recon primitives in `research_patterns.rs` (CIDR parse/contains, finding dedup, wordlist,
redirect, crawl) are usable ONLY against targets inside `TargetScope`. The gate refuses anything
outside your declared CIDRs/hosts — proven by `target_scope_gate_refuses_out_of_scope` test.
No live execution against any host was performed; this is core logic only.

## REVERSE-ENGINEERING METHOD (per operator: "research + parse, then apply")
1. Fetch raw facts (my web tools) — `claude -p` used only for EXTRACTION/structuring (its
   sub-session has no web), per your "use dowiz/claude for extraction, not reasoning" directive.
2. Identify the load-bearing PATTERN (not the dependency surface).
3. Re-implement natively (std-only) with a RED+GREEN test proving the constraint is real.
4. Defer anything needing a network/model/weight; gate anything offensive by `TargetScope`.

## Verification
- bebop `feat/wire-native-core`: 169 Rust tests (153 bebop lib + 16 rust-core), 0 fail.
- doc-claim gate: exit 0 (README/AGENTS = 169). falsifiable guardrail: 185 #[test] fns.
- cargo fmt --check: clean. Committed (pre-commit hook passed).

## Next (operator's call)
- (a) open PRs for review, (b) spike one DEFERRED item behind an eval gate,
- (c) stop here. Nothing pushed to main/prod (secrets-scrub gate) — feature branch only.
