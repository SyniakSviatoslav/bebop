# Reverse-Engineering Loop — bebop active Rust repo (deterministic, deep, total-scope)

> Run: 2026-07-10, `/root/bebop-repo`, branch `feat/wire-native-core`.
> Method: tool-assisted structural sweep over ALL 54 `crates/bebop/src/*.rs` modules
> (15,974 LOC) + `rust-core` + key docs. Every claim cites file:line or a live grep count.
> This is the RE loop half of the twin task; the claude-fable adversarial pass is the other half.

## 1. Inventory (live)
- 54 bebop modules, 15,974 LOC total.
- Tests present in 51/54 modules (only `cli`, `lib`, `main` have 0 unit tests — see §4).
- RED/GREEN markers in 51/54 modules (the 3 missing = cli/lib/main, which carry no behavioral logic).
- `unsafe` REAL call sites in the whole codebase: **3** (all in `field.rs` FFI into rust-core C-API,
  each preceded by a defensive CSR invariant check — fail-closed).
- The word "unsafe" appears in doc-comments of `stabilizer`/`wavefield` but those are prose, not code.

## 2. Determinism-as-security-model — SUBSTANTIATED (the deepest claim)
- No network in ANY bebop module: `grep reqwest|TcpStream|hyper` over `crates/bebop/src/*.rs` → empty.
- `rust-core` (the kernel) has ZERO RNG/time/network/env leaks: `grep std::time|SystemTime|thread_rng|
  rand::|reqwest|TcpStream|std::env crates/rust-core/src/*.rs` → empty.
- "Determinism leaks" found by grep were ALL either:
  - presentation-layer animation gating (mission.rs:13,44; tui.rs:901,905,919 — `Duration`/`std::env`
    `BEBOP_NO_ANIM`/`CI`/`NO_ANIM`), explicitly non-core; `launch.rs` uses a const-seeded LCG to stay
    reproducible (launch.rs:7-9);
  - the intentional flag-OFF gate read `std::env::var("BEBOP_WAVE_GATE")` (wavefield.rs:528) —
    this IS Cross-pattern C (flag-OFF by default), not a leak;
  - `mcp.rs`/`radio.rs`/`customize.rs` hits were doc-comment prose, not code.
- VERDICT: principle holds in code. The trust boundary is real determinism, not a vibe.

## 3. As-above-so-below / fail-closed — SUBSTANTIATED in new code
New modules (audit-phase) each carry a deterministic verifier + RED/GREEN tests:
- pod.rs: 4 tests, 5 RED/GREEN markers (sign_delivery/verify_delivery, hybrid sig).
- guard.rs: 5 tests, 6 markers (io_guard + KillSwitch ≥2/3, self-vote ignored — guard.rs:37,92).
- reputation.rs: 4 tests, 6 markers (score + precedence decay; zero RNG).
- matcher.rs: 5 tests, 6 markers (match_orders pure; fingerprint deterministic; RemoteMatcherClient
  == LocalMatcherClient fingerprint).
- sandbox.rs: explicit "If `unshare` is missing..." fail-closed says-so (sandbox.rs:55).
- vault.rs: classical fallback path (vault.rs:84).
The pattern recurs at kernel/agent/plan/tool-arg scale even in freshly-added code.

## 4. Genuine gaps found (honest, not over-claimed)
- **G1 — cli dispatcher is untested at unit level** (cli.rs 614 LOC, 0 tests; main.rs 6 LOC thin
  wrapper calling `bebop::cli::run()`). This is the "one door" (lib.rs:15). It is exercised
  indirectly by the 293 passing `bebop::*` tests, but there is no direct RED test asserting
  `bebop <cmd>` routes correctly / refuses an unknown command. FOLLOW-UP: add
  `tests/cli_route.rs` asserting dispatch of a known + an unknown subcommand.
- **G2 — `core-legacy` is an orphan coupling target**: grep for dependents → none. Dead weight;
  should be deleted or the live `rust-core` promoted. Not a runtime defect (unused).
- **G3 — rust-core's field C-API keeps graph state PROCESS-GLOBAL** (field.rs:14 "keeps its graph in
  PROCESS-GLOBAL state"). This breaks pure-thread-safe determinism for concurrent callers; acceptable
  for a single-tenant core but a latent hazard if bebop ever runs the field sim concurrently.
  FOLLOW-UP: document the single-owner invariant or move to instance state.

## 5. Cross-patterns confirmed (extend the repo's A–H list)
- The repo's named patterns A–H (as-above-so-below, propose-don't-execute, flag-OFF→shadow→gate,
  determinism-as-security, named-blind-spots, math-not-metaphor, RED-is-the-proof,
  deterministic-twin-for-risky-IO) are ALL substantiated in the active Rust code.
- NEW cross-pattern observed: **"unsafe is a guarded seam, not a surface"** — the only `unsafe`
  in 16K LOC is 3 FFI call sites, each fronted by a Rust-side invariant check. The trust boundary
  is explicit and narrow (field.rs:22-40).
- NEW cross-pattern: **"presentation may be stochastic, core may not"** — animation/tui/launch use
  `std::env`/`Duration` freely, but the moment control reaches a behavioral module it is pure. The
  line is drawn at the TUI→core seam, not at the process boundary.

## 6. Honest limitations of THIS loop
- This loop reads source + runs grep/cargo; it does NOT execute the FABLE adversarial pass (that is
  the parallel task). Logical-fallacy hunting is delegated to fable.
- `zenoh/ledger/zkvm/portkey` did not match the degrade-grep; they are thin wrappers or use different
  vocabulary. Flagged for fable to read directly (they ARE in fable's scoped list).

## 7. Evidence index (live this run)
- 54 modules, 15,974 LOC: `ls crates/bebop/src/*.rs | wc -l` + `cat ... | wc -l`.
- 293 tests pass: `cargo test --workspace` → TOTAL PASS=293.
- unsafe real sites = 3: `grep -nE "\bunsafe\b" crates/bebop/src/field.rs` (40,48,58).
- network leaks = 0: `grep -rl "reqwest|TcpStream" crates/bebop/src/*.rs` → empty.
- rust-core determinism leaks = 0: grep empty.
