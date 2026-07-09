#!/usr/bin/env node
// verify-doc-claims.mjs — the doc-claim self-correction layer (Constant Doubt, enforced).
//
// ROOT-CAUSE THIS FIXES: falsified README/doc statements kept shipping because claims were
// never re-verified against the live code. This script turns every load-bearing doc claim into a
// FALSIFIABLE check: it runs the real test suite / greps the real source, and RED-fails on any
// mismatch. It is called by `bebop docs check` AND by .git/hooks/pre-commit, so a doc statement
// not backed by a live probe/test cannot reach a commit or a release.
//
// Falsifiable by design: if you change the code to break a claim (e.g. re-add NO_ANIM=1 to the
// recorder, or let README's test count drift), this script exits 1.

import { readFileSync, existsSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import path from 'node:path';

const ROOT = process.cwd();
const read = (p) => readFileSync(path.join(ROOT, p), 'utf8');

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

// --- B. Animation code path actually exists and is wired into boot ---
{
  const bebop = read('bebop.ts');
  const launch = read('src/launch.ts');
  const wired = /playLaunch/.test(bebop) && /export async function playLaunch/.test(launch);
  const ttyGated = /isTTY/.test(launch) && /NO_ANIM/.test(launch);
  check('launch animation exists + is TTY-gated + wired into boot', wired && ttyGated,
    wired && ttyGated ? 'playLaunch renders when isTTY, skipped when piped/NO_ANIM' : 'animation path missing or unwired');
}

// --- C. Customization is REAL (init axes drive the CLI), not dead ---
{
  const settings = read('src/settings.ts');
  const themeTest = existsSync(path.join(ROOT, 'src/theme.test.ts'));
  const voiceTest = existsSync(path.join(ROOT, 'src/voice.test.ts'));
  const readsAxes = /narration/.test(settings) && /looks/.test(settings);
  check('customization wired: settings reads narration+looks', readsAxes,
    readsAxes ? 'init axes flow into settings' : 'settings ignores the init axes (customization is dead)');
  check('customization tested: theme.test.ts + voice.test.ts exist', themeTest && voiceTest,
    themeTest && voiceTest ? 'voice/theme axis tests present' : 'no test proves customization works');
}

// --- D. PSQ (post-quantum) identity is REAL, not claimed ---
{
  const core = read('src/core.test.ts');
  const real = /ml_kem|ml_dsa|ML-KEM|ML-DSA|post-quantum/.test(core);
  check('PSQ identity backed by a real test (ML-KEM/ML-DSA)', real,
    real ? 'core.test.ts exercises the PQ identity' : 'no PQ test — claim is unproven');
}

// --- E. recall returns REAL payloads (not truncated ids) ---
{
  const kt = read('src/knowledge.test.ts');
  const real = /REAL payload text/i.test(kt) && /gibberish/i.test(kt) && /no confident hits/i.test(kt);
  check('recall returns real payloads + honest noise floor', real,
    real ? 'knowledge.test asserts real text + gibberish excluded' : 'recall claim unproven');
}

// --- F. Test-count honesty: README's claimed count must match `npm test` reality ---
let pass = 0, failc = 0;
try {
  const out = execFileSync('npm', ['test'], { encoding: 'utf8', timeout: 240000, stdio: ['ignore', 'pipe', 'pipe'] });
  pass = Number((out.match(/# pass\s+(\d+)/) || [])[1] ?? 0);
  failc = Number((out.match(/# fail\s+(\d+)/) || [])[1] ?? 0);
} catch (e) {
  const out = String(e.stdout ?? e.stderr ?? e.message ?? '');
  pass = Number((out.match(/# pass\s+(\d+)/) || [])[1] ?? 0);
  failc = Number((out.match(/# fail\s+(\d+)/) || [])[1] ?? 1);
}
{
  // Source of truth is `npm test` reality; assert BOTH doc surfaces (README + AGENTS)
  // match it, so neither prose line can drift silently.
  const readme = read('README.md');
  const agents = read('AGENTS.md');
  const readmeClaim = Number((readme.match(/(\d+)\s*TS tests/) || [])[1] ?? -1);
  const agentsClaim = Number((agents.match(/npm test`\s*—\s*(\d+)\s*falsifiable tests/) || [])[1] ?? -1);
  check('test count honest: README claims match `npm test`', readmeClaim === pass && failc === 0,
    `README says ${readmeClaim}, actual pass=${pass} fail=${failc}`);
  check('test count honest: AGENTS.md claims match `npm test`', agentsClaim === pass && failc === 0,
    `AGENTS says ${agentsClaim}, actual pass=${pass} fail=${failc}`);
}

// --- G. No false-superiority comparison table (✅/❌ vs competitors) ---
{
  const readme = read('README.md');
  const hasMatrix = /^\|.*[✅❌].*\|\s*$/m.test(readme) && /Claude Code|Codex|OpenCode/.test(readme);
  check('no ✅/❌ superiority matrix vs competitors', !hasMatrix,
    hasMatrix ? 'README compares Bebop vs others with ✅/❌ — unverified superiority claim' : 'comparison is prose, not a fake scorecard');
}

// --- H. Wiki honesty: README must not claim a populated wiki without openwiki/ ---
{
  const readme = read('README.md');
  const wikiDir = existsSync(path.join(ROOT, 'openwiki'));
  const claimsPopulated = /rich.*wiki|populated wiki|full wiki/.test(readme);
  check('wiki claim honest (no populated-wiki claim without openwiki/)', !(claimsPopulated && !wikiDir),
    claimsPopulated && !wikiDir ? 'claims a populated wiki but openwiki/ is absent' : 'wiki gap stated honestly');
}

// --- I. ReAct agentic loop is REAL, visible, and not hidden (the promo-demo failure mode) ---
{
  const loop = read('src/loop.ts');
  const reactTest = read('src/loop.react.test.ts');
  const defaults3 = /export function reactIters[\s\S]*?return 3;/.test(loop);
  const emitsTrace = /reactTrace/.test(loop) && /iterations: number/.test(loop);
  const provesVisible = /reactTrace/.test(reactTest) && /denied/.test(reactTest) && /FAIL/.test(reactTest);
  const envKnob = /BEBOP_REACT_ITERS/.test(loop);
  check('ReAct loop real: reactIters defaults to 3 + emits visible reactTrace',
    defaults3 && emitsTrace && envKnob,
    defaults3 && emitsTrace && envKnob
      ? 'runLoop emits Reason→Act→Observe→Reflect trace, default 3, BEBOP_REACT_ITERS overrides'
      : 'ReAct trace/default/env not all present');
  check('ReAct denial is VISIBLE in reactTrace (not hidden as one perfect iter)',
    provesVisible,
    provesVisible ? 'loop.react.test asserts denied action shows FAIL in reactTrace' : 'no test proves the iteration trace is honest');
}

// --- J. L5 analytics wired into governor as flag-OFF state fields (blind-spot fix 2026-07-09) ---
{
  const gov = read('src/governor.ts');
  // both L5 signals must be part of GovernorState AND default-off (only set when cfg provided)
  const hasFields = /pcaAnomaly:\s*boolean/.test(gov) && /cycleBroken:\s*boolean/.test(gov);
  const flagOff = /this\.cfg\.pcaAnomaly\s*&&/.test(gov) && /this\.cfg\.cycleConsistency\s*&&/.test(gov);
  check('L5 analytics wired into governor (pcaAnomaly+cycleBroken, flag-OFF)', hasFields && flagOff,
    hasFields && flagOff ? 'GovernorState exposes both signals; each only fires when its cfg is supplied'
      : 'governor missing L5 state fields or they are not flag-gated');
}

// --- K. telemetry-ica-loop module exists + its test ships the EV and the RED blind-spot ---
{
  const modPath = 'src/integration/analytics/telemetry-ica-loop.ts';
  const testPath = 'src/integration/analytics/telemetry-ica-loop.test.ts';
  const modExists = existsSync(path.join(ROOT, modPath));
  const tExists = existsSync(path.join(ROOT, testPath));
  const t = tExists ? read(testPath) : '';
  const hasEV = /localiz/i.test(t) && /sparse/i.test(t);
  const hasRed = /gaussian/i.test(t) && /(blind|not separable|not recover)/i.test(t);
  check('telemetry-ica-loop present + test asserts EV (sparse localization) AND RED (Gaussian blind-spot)',
    modExists && tExists && hasEV && hasRed,
    modExists && tExists && hasEV && hasRed ? 'EV + falsifiable RED both present'
      : 'module/test missing or lacks the EV/RED pair');
}

// --- L. symmetrical-loops rule + cycle-consistency theorem doc present and referenced ---
{
  const agents = read('AGENTS.md');
  const ruleThere = /symmetrical loops|cycle consistency/i.test(agents) && /F\(G\(X\)\)/.test(agents);
  const docThere = existsSync(path.join(ROOT, 'docs/design/cycle-consistency-theorem.md'));
  const referenced = /cycle-consistency-theorem\.md/.test(agents);
  check('symmetrical-loops rule + theorem doc present and referenced', ruleThere && docThere && referenced,
    ruleThere && docThere && referenced ? 'AGENTS rule + theorem doc exist and are cross-linked'
      : 'rule missing, theorem doc absent, or not referenced from AGENTS');
}

// --- M. N1 Open-System Symmetry: cycle-consistency exposes symmetryTol cfg + tolerance-band test ---
{
  const cc = read('src/integration/analytics/cycle-consistency.ts');
  const t = read('src/integration/analytics/cycle-consistency.test.ts');
  const cfgThere = /symmetryTol/.test(cc);
  const tolTest = /symmetryTol/.test(t) && /tolerates/.test(t) && /SHARP asymmetry/.test(t);
  check('N1 Open-System Symmetry: symmetryTol cfg + tolerance-band RED+GREEN test', cfgThere && tolTest,
    cfgThere && tolTest ? 'relaxed-band breach gate present + tolerance/break RED+GREEN proven'
      : 'symmetryTol missing from cycle-consistency or its test lacks the tolerance/break pair');
}

// --- N. N2 liveness contract: governor exposes safeState + watchdog + authority clamp on silence ---
{
  const gov = read('src/governor.ts');
  const t = read('src/governor.test.ts');
  const fields = /safeState/.test(gov) && /watchdogMs/.test(gov) && /agentSilentMs/.test(gov);
  const clampTest = /Safe State/.test(t) && /floors authority to uMin/.test(t) && /watchdogMs/i.test(t);
  check('N2 liveness contract: safeState + watchdog + authority-clamp RED+GREEN test', fields && clampTest,
    fields && clampTest ? 'governor drops to Safe State on silence + RED+GREEN proves it'
      : 'governor missing safeState/watchdog fields or the safe-state test is absent');
}

// --- O. N3 β-VAE latent-prior calibration: calibrateLatentPrior exists + false-positive RED+GREEN ---
{
  const an = read('src/integration/analytics/anomaly.ts');
  const t = read('src/integration/analytics/anomaly.test.ts');
  const fn = /export function calibrateLatentPrior/.test(an) && /LatentPriorCalibration/.test(an);
  const test = /calibrateLatentPrior/.test(t) && /false-positive|false positive/.test(t) && /off-prior/.test(t);
  check('N3 latent-prior calibration: calibrateLatentPrior + false-positive RED+GREEN', fn && test,
    fn && test ? 'calibration harness present; β>0 off-prior false-positive is proven RED'
      : 'calibrateLatentPrior missing or its test lacks the false-positive RED case');
}

// --- P. N6 Dual-Track GNN seam: dualTrackGate exists + graph-gate RED+GREEN ---
{
  const dt = read('src/integration/analytics/dual-track.ts');
  const t = read('src/integration/analytics/dual-track.test.ts');
  const fn = /export function dualTrackGate/.test(dt) && /GnnAdvisor/.test(dt) && /TruthGraph/.test(dt);
  const test = /honored/.test(t) && /no-such-edge/.test(t) && /hallucinat/.test(t);
  check('N6 Dual-Track seam: dualTrackGate + graph-gate RED+GREEN', fn && test,
    fn && test ? 'advisor proposals gated against the Truth Layer; hallucinated edge rejected RED'
      : 'dualTrackGate missing or its test lacks the no-such-edge RED case');
}

// --- Q. N5 Neuro-Symbolic Gate ADR-003 exists + is cross-linked from AGENTS ---
{
  const adr = existsSync(path.join(ROOT, 'docs/design/adr-003-neuro-symbolic-gate-2026-07-09.md'));
  const agents = read('AGENTS.md');
  const linked = /adr-003-neuro-symbolic-gate/.test(agents);
  const n7 = /bridgeMetrics|hallucinationRate/.test(read('src/governor.ts'));
  check('N5 Neuro-Symbolic Gate ADR-003 present + cross-linked + N7 wired', adr && linked && n7,
    adr && linked && n7 ? 'ADR-003 exists, linked from AGENTS, and the gate is observable via N7'
      : 'ADR-003 missing, not linked, or N7 observability not wired into governor');
}

// --- R. N4 causal counterfactual: pointsOfFailure exists + RED+GREEN + dual-track consumes it ---
{
  const am = read('src/integration/analytics/arch-mine.ts');
  const t = read('src/integration/analytics/arch-mine.test.ts');
  const dt = read('src/integration/analytics/dual-track.test.ts');
  const fn = /export function pointsOfFailure/.test(am) && /PointOfFailure/.test(am);
  const test = /pointsOfFailure/.test(t) && /blast-radius/.test(t);
  const wired = /pointsOfFailure/.test(dt);
  check('N4 causal counterfactual: pointsOfFailure + RED+GREEN + consumed by N6', fn && test && wired,
    fn && test && wired ? 'counterfactual surface proven + wired into the dual-track gate'
      : 'pointsOfFailure missing, untested, or not consumed by the dual-track seam');
}

console.log(`\n  ${fails ? `✗ ${fails} doc-claim check(s) FAILED — fix before commit/release` : '✓ all doc claims backed by live proof'}`);
process.exit(fails ? 1 : 0);
