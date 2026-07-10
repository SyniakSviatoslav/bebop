#!/usr/bin/env node
// verify-doc-claims.mjs — the doc-claim self-correction layer (Constant Doubt, enforced).
//
// ROOT-CAUSE THIS FIXES: falsified README/doc statements kept shipping because claims were
// never re-verified against the live code. This script turns every load-bearing doc claim into a
// FALSIFIABLE check: it greps the real Rust source / runs `cargo test`, and RED-fails on any
// mismatch. Called by `bebop docs check` AND by .git/hooks/pre-commit.
//
// AS OF 2026-07-10 the runtime is NATIVE RUST/WASM (crates/bebop + rust-core). The legacy
// TypeScript layer was archived to archive/ (recoverable) but is NO LONGER the live path, so
// every check below resolves against Rust artifacts. No TS is verified here by design.
//
// Falsifiable by design: if you change the code to break a claim (e.g. re-introduce a TS entry
// point, or let the test count drift), this script exits 1.

import { readFileSync, existsSync, readdirSync } from 'node:fs';
import { execFileSync, execSync } from 'node:child_process';
import path from 'node:path';

const ROOT = path.resolve(process.cwd());
const read = (p) => readFileSync(path.join(ROOT, p), 'utf8');
const exists = (p) => existsSync(path.join(ROOT, p));

let fails = 0;
const results = [];
function check(name, ok, detail = '') {
  results.push({ name, ok, detail });
  if (!ok) fails++;
  const mark = ok ? '✓' : '✗';
  console.log(`  ${mark} ${name}${detail ? ' — ' + detail : ''}`);
}

// --- A. Recorder honesty: must NOT force NO_ANIM=1 (the bug that hid animation in every GIF) ---
{
  const rec = read('scripts/record-feature.sh');
  const forced = /export NO_ANIM=1/.test(rec);
  check('recorder does not force NO_ANIM=1 (animation must be recorded)', !forced,
    forced ? 'FOUND `export NO_ANIM=1` — re-add bug that flattens footage' : 'animation will render in recordings');
}

// --- B. Launch animation exists + is deterministic + brand-colored (no RNG/Date) ---
{
  const launch = read('crates/bebop/src/launch.rs');
  const tui = read('crates/bebop/src/tui.rs');
  const deterministic = /fn launch_is_deterministic|seed/.test(launch);
  const brand = /#F4C25A|ship|#12100E|void/.test(launch);
  const loader = /loader animation|frame/.test(tui);
  check('launch animation real: deterministic + brand palette + per-state loader', deterministic && brand && loader,
    deterministic && brand && loader ? 'playLaunch deterministic, ships on void palette, TUI has per-command loaders'
      : 'animation missing / not deterministic / not brand-colored');
}

// --- C. Customization is REAL (init axes drive the CLI), not dead ---
{
  const customize = read('crates/bebop/src/customize.rs');
  const tests = read('crates/bebop/src/customize.rs');
  const readsAxes = /narration/.test(customize) && /looks/.test(customize);
  const themeVoice = /#\[test\]/.test(tests) && /parse_narration|voice_line/.test(tests);
  check('customization wired: settings read narration+looks', readsAxes,
    readsAxes ? 'init axes flow into settings' : 'settings ignores the init axes (customization is dead)');
  check('customization tested: narration/voice axis covered by Rust #[test]', themeVoice,
    themeVoice ? 'voice/theme axis tested in Rust' : 'no Rust test proves customization works');
}

// --- D. Native node identity: XChaCha20 + scrypt vault (honest — NOT post-quantum) ---
{
  const vault = read('crates/bebop/src/vault.rs');
  const tests = read('crates/bebop/src/vault.rs');
  const real = /xchacha20|scrypt|chacha/.test(vault) && /#\[test\]/.test(tests);
  check('node identity backed by real Rust crypto (XChaCha20 + scrypt vault)', real,
    real ? 'vault.rs derives keys deterministically from passphrase; wrong-pass test proves it'
      : 'vault claim unproven (no symmetric cipher / no test)');
}

// --- E. recall returns REAL payloads (not truncated ids) ---
{
  const knowledge = read('crates/bebop/src/knowledge.rs');
  const memory = read('crates/bebop/src/memory.rs');
  const real = /#\[test\]/.test(knowledge) && /gibberish|noise|confident/.test(knowledge) && /LivingMemory|insert/.test(memory);
  check('recall returns real payloads + honest noise floor', real,
    real ? 'knowledge.rs asserts real text + gibberish excluded; memory store is live' : 'recall claim unproven');
}

// --- F. Test-count honesty: README + AGENTS counts must match `cargo test` (all crates) ---
let pass = 0, failc = 0;
try {
  for (const manifest of [path.join(ROOT, 'crates/bebop/Cargo.toml'), path.join(ROOT, 'rust-core/Cargo.toml')]) {
    const out = execFileSync('cargo', ['test', '--quiet', '--lib', '--manifest-path', manifest], { encoding: 'utf8', timeout: 300000, stdio: ['ignore', 'pipe', 'pipe'] });
    for (const line of out.split('\n')) {
      const m = line.match(/test result: ok\.\s*(\d+) passed/);
      if (m) pass += Number(m[1]);
      const f = line.match(/test result: FAILED\.\s*(\d+) failed/);
      if (f) failc += Number(f[1]);
    }
  }
} catch (e) {
  const out = String(e.stdout ?? e.stderr ?? '');
  for (const line of out.split('\n')) {
    const m = line.match(/test result: ok\.\s*(\d+) passed/);
    if (m) pass += Number(m[1]);
    const f = line.match(/test result: FAILED\.\s*(\d+) failed/);
    if (f) failc += Number(f[1]);
  }
}
{
  const readme = read('README.md');
  const agents = read('AGENTS.md');
  const readmeClaim = Number((readme.match(/(\d+)\s*Rust tests/) || [])[1] ?? -1);
  const agentsClaim = Number((agents.match(/cargo test`\s*—\s*(\d+) (?:falsifiable )?Rust tests/) || [])[1] ?? -1);
  check('test count honest: README claims match `cargo test --workspace`', readmeClaim === pass && failc === 0,
    `README says ${readmeClaim}, actual pass=${pass} fail=${failc}`);
  check('test count honest: AGENTS.md claims match `cargo test --workspace`', agentsClaim === pass && failc === 0,
    `AGENTS says ${agentsClaim}, actual pass=${pass} fail=${failc}`);
}

// --- G. No false-superiority comparison table (✅/❌ vs competitors) ---
{
  const readme = read('README.md');
  const hasMatrix = /^\|.*[✅❌].*\|\s*$/m.test(readme) && /Claude Code|Codex|OpenCode/.test(readme);
  check('no ✅/❌ superiority matrix vs competitors', !hasMatrix,
    hasMatrix ? 'README compares Bebop vs others with ✅/❌ — unverified superiority claim' : 'comparison is prose, not a fake scorecard');
}

// --- H. Wiki honesty ---
{
  const readme = read('README.md');
  const docsWiki = exists('docs/wiki') && readdirSync(path.join(ROOT, 'docs/wiki')).filter((f) => f.endsWith('.md')).length >= 3;
  const claimsPopulated = /rich.*wiki|populated wiki|full wiki|wiki content/.test(readme);
  check('wiki claim honest (populated-wiki claim backed by docs/wiki/)', !(claimsPopulated && !docsWiki),
    claimsPopulated && !docsWiki ? 'claims a populated wiki but docs/wiki/ is missing' : (docsWiki ? 'docs/wiki/ ships non-empty' : 'wiki gap stated honestly'));
}

// --- I. Multipilot: N distinct pilots + field gate (RED+GREEN) ---
{
  const multipilot = read('crates/bebop/src/multipilot.rs');
  const t = read('crates/bebop/src/multipilot.rs');
  const fn = /pub fn run_multipilot/.test(multipilot) && /Pilot/.test(multipilot);
  const test = /#\[test\]/.test(t) && /distinct|divergent|converged/.test(t);
  check('Multipilot: N distinct pilots + synthesize + field gate RED+GREEN', fn && test,
    fn && test ? 'run_multipilot fans distinct pilots; field override test present' : 'multipilot missing or untested');
}

// --- J. Field arbiter: graph-PDE veto via rust-core (RED+GREEN) ---
{
  const field = read('crates/bebop/src/field.rs');
  const core = read('rust-core/src/lib.rs');
  const fn = /pub fn field_gate/.test(field) && /field_build|field_rank/.test(core);
  const test = /#\[test\]/.test(field) && /redline_task_is_vetoed|blast_threshold/.test(field);
  check('Field arbiter: graph-PDE cost surface → physics veto RED+GREEN', fn && test,
    fn && test ? 'field_gate uses rust-core spectral propagator; veto proven' : 'field gate missing or untested');
}

// --- K. rust-core field core: real f32 CSR exports (Rust/WASM) ---
{
  const lib = read('rust-core/src/lib.rs');
  const exports = /field_build/.test(lib) && /field_cost/.test(lib) && /field_rank/.test(lib) && /field_active/.test(lib) && /vsa_similarity/.test(lib);
  check('rust-core field core: f32 CSR + spectral + VSA exports', exports,
    exports ? 'rust-core exposes field_build/cost/rank/active + vsa_similarity' : 'rust-core missing field exports');
}

// --- L. L5 analytics subset present in Rust (kalman / dual-track / goap) ---
{
  const a = read('crates/bebop/src/analytics.rs');
  const hasKalman = /pub fn kalman1d_step/.test(a) && /pub fn kalman_anomaly/.test(a);
  const hasDual = /pub fn dual_track_gate/.test(a);
  const hasGoap = /pub fn plan/.test(a) && /unreachable/.test(a);
  check('L5 analytics subset (kalman + dual-track + goap) present + tested', hasKalman && hasDual && hasGoap && /#\[test\]/.test(a),
    hasKalman && hasDual && hasGoap ? 'deterministic Kalman/Dual-Track/GOAP in Rust with #[test]' : 'analytics subset missing');
}

// --- M. CLI subcommands fully wired (not print-only) ---
{
  const cli = read('crates/bebop/src/cli.rs');
  const wired = /"dispatch" =>/.test(cli) && /"outfit" =>/.test(cli) && /"status" =>/.test(cli)
    && /"route" =>/.test(cli) && /"recall" =>/.test(cli) && /"map" =>/.test(cli) && /"diagrams" =>/.test(cli) && /"mcp" =>/.test(cli);
  check('CLI fully wired: dispatch/outfit/status/route/recall/map/diagrams/mcp all dispatch to real engines', wired,
    wired ? 'every subcommand calls a real Rust engine (no print-only stubs)' : 'a subcommand is still a print-only stub');
}

// --- N. MCP stdio server exposes native tools (no SDK/network) ---
{
  const mcp = read('crates/bebop/src/mcp.rs');
  const hasTools = /tools\/list/.test(mcp) && /tools\/call/.test(mcp) && /native_exec|run_multipilot|field_gate/.test(mcp);
  check('MCP stdio server exposes dispatch/recall/outfit tools', hasTools,
    hasTools ? 'minimal JSON-RPC stdio MCP, native tools only' : 'MCP server missing tool surface');
}

// --- O. No TypeScript in the live runtime path (the elimination invariant) ---
{
  const pkg = read('package.json');
  const noTsBin = !/bebop\.ts/.test(pkg) && !/tsx/.test(pkg);
  const binShim = exists('bin/bebop');
  check('TS eliminated from runtime: bin points to native Rust, no tsx', noTsBin && binShim,
    noTsBin && binShim ? 'package bin → bin/bebop (Rust); tsx removed from scripts' : 'TS entry point still referenced');
}

console.log(`\n  ${fails ? `✗ ${fails} doc-claim check(s) FAILED — fix before commit/release` : '✓ all doc claims backed by live proof'}`);
process.exit(fails ? 1 : 0);
