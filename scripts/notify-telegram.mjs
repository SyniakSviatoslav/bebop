// scripts/notify-telegram.mjs — finished-task report to the operator's Telegram channel.
//
// Usage:
//   TG_BOT_TOKEN=xxxx TG_CHAT_ID=yyyy node scripts/notify-telegram.mjs
//
// Reads both from env (never hard-coded). If either is missing, prints the report and exits 2
// so the caller knows the ping didn't send. No secrets are logged.
import { readFileSync } from 'node:fs';

const token = process.env.TG_BOT_TOKEN;
const chat = process.env.TG_CHAT_ID;

const report = `🎷 *Bebop 0.4.0 — shipped to main*

*What landed (2026-07-09c)*
• **Multipilot** — copilot is now a multipilot: a task fans out to N specialist pilots + a distinct synthesizer; the Rust field arbiter can veto the plan. \`bebop multipilot "<task>"\`.
• **New outfit** — cosmo-noir identity contract v1.0.0 in \`src/outfit.ts\`; \`bebop outfit\` prints it. Warm Cosmo-Noir, teal #46B0A4 on void #12100E.
• **Field core** — f32-packed CSR (bit-identical to f64, <1e-12), SIMD128 (+simd128, measured 1.08×), 1 GiB wasm ceiling, *sensitivity bootstrap* from |Δu| (zero new infra).
• **Top-K Contours** surfaced to explainability (field-planner.ts) — the unique feature: the planner reads a *deterministic graph-PDE field* as cost; you can SEE where a disruption will hurt.
• Field-sim comparison report + visual explainer SVG in docs/.

*Verified (falsifiable, RED+GREEN)*
• Rust kernel: **16** tests, wasm32 clean.
• TS suite: **547** tests, 0 fail.
• \`npm run typecheck\`: 0 errors.
• doc-gate: all claims backed by live proof.

*Real telemetry*
• JS field 19.4/50.5 ms → Rust/WASM 0.72/1.9 ms (**26.8× / 26.5×**).
• SIMD128: **1.08×** (measured, not claimed).

*Links*
• Repo: https://github.com/SyniakSviatoslav/bebop
• Wiki: https://github.com/SyniakSviatoslav/bebop/tree/main/docs/wiki
• Field-sim report: docs/design/field-sim-comparison-2026-07-09.md

*Note:* the GitHub *wiki tab* needs a one-click enable in repo Settings (or a repo-admin token) to push \`*.wiki.git\`; the full wiki content ships in \`docs/wiki/\` and renders on GitHub now.
`;

if (!token || !chat) {
  console.error('✗ TG_BOT_TOKEN and/or TG_CHAT_ID not set — ping NOT sent.');
  console.error('  Set them and re-run: TG_BOT_TOKEN=… TG_CHAT_ID=… node scripts/notify-telegram.mjs');
  console.log('\n--- REPORT THAT WOULD BE SENT ---\n' + report);
  process.exit(2);
}

const url = `https://api.telegram.org/bot${token}/sendMessage`;
const res = await fetch(url, {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify({ chat_id: chat, text: report, parse_mode: 'Markdown' }),
});
const json = await res.json();
if (!json.ok) {
  console.error('✗ Telegram send failed:', JSON.stringify(json).slice(0, 300));
  process.exit(1);
}
console.log('✓ Telegram report sent to chat', chat);
