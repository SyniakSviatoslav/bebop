// Bebop conceptual schema builder — draws a vertical/horizontal flow diagram as SVG.
//
// These diagrams explain HOW a subsystem behaves (decide→fold→replay, PID loop, VSA
// insert/forget/recall, …). Every step label is a direct quote of a real function/field in the
// code (verified by reading the source) — the diagram is a picture of the code, not an opinion.
// Pure + zero-dep. `flowSchema` is unit-tested in schema.test.ts.

export interface Step {
  label: string;
  sub?: string;
  kind?: 'in' | 'gate' | 'ok' | 'deny' | 'proc' | 'out';
}

const KIND_COLOR: Record<string, string> = {
  in: '#5B8DEF',
  gate: '#E0573E',
  ok: '#3FB68B',
  deny: '#C0392B',
  proc: '#46B0A4',
  out: '#9B6DDE',
};

export interface SchemaOpts {
  title?: string;
  orientation?: 'v' | 'h';
  width?: number;
}

/** Render a flow diagram from an ordered list of steps. Deterministic (no RNG). */
export function flowSchema(steps: Step[], opts: SchemaOpts = {}): string {
  const title = opts.title ?? 'schema';
  const horiz = opts.orientation === 'h';
  const boxW = horiz ? 190 : 320;
  const boxH = 40;
  const gap = 34;
  const padX = 24;
  const padTop = 40;
  const padBottom = 20;
  const n = steps.length;
  const width = opts.width ?? (horiz ? padX * 2 + n * boxW + (n - 1) * gap : padX * 2 + boxW);
  const height = horiz ? padTop + boxH + padBottom : padTop + n * boxH + (n - 1) * gap + padBottom;

  const boxes: string[] = [];
  const arrows: string[] = [];
  steps.forEach((s, i) => {
    const color = KIND_COLOR[s.kind ?? 'proc'];
    const x = horiz ? padX + i * (boxW + gap) : padX;
    const y = horiz ? padTop : padTop + i * (boxH + gap);
    const num = `<text x="${x + 9}" y="${y + 19}" font-family="sans-serif" font-size="11" font-weight="bold" fill="#fff">${i + 1}</text>`;
    const label = `<text x="${x + 24}" y="${y + 17}" font-family="sans-serif" font-size="11" font-weight="bold" fill="#fff">${esc(s.label)}</text>`;
    const sub = s.sub
      ? `<text x="${x + 24}" y="${y + 32}" font-family="monospace" font-size="9" fill="#eef">${esc(s.sub)}</text>`
      : '';
    boxes.push(
      `<g><rect x="${x}" y="${y}" width="${boxW}" height="${boxH}" rx="6" fill="${color}"/>${num}${label}${sub}</g>`,
    );
    if (i < n - 1) {
      if (horiz) {
        const ax1 = x + boxW;
        const ay = y + boxH / 2;
        const ax2 = x + boxW + gap;
        arrows.push(`<line x1="${ax1}" y1="${ay}" x2="${ax2}" y2="${ay}" stroke="#8a94a0" stroke-width="1.4"/><path d="M ${ax2 - 6} ${ay - 4} L ${ax2} ${ay} L ${ax2 - 6} ${ay + 4} Z" fill="#8a94a0"/>`);
      } else {
        const ax = x + boxW / 2;
        const ay1 = y + boxH;
        const ay2 = y + boxH + gap;
        arrows.push(`<line x1="${ax}" y1="${ay1}" x2="${ax}" y2="${ay2}" stroke="#8a94a0" stroke-width="1.4"/><path d="M ${ax - 4} ${ay2 - 6} L ${ax} ${ay2} L ${ax + 4} ${ay2 - 6} Z" fill="#8a94a0"/>`);
      }
    }
  });

  return (
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">` +
    `<rect width="${width}" height="${height}" fill="#FBFCFD"/>` +
    `<text x="${padX}" y="22" font-family="sans-serif" font-size="13" font-weight="bold" fill="#1a1a1a">${esc(title)}</text>` +
    arrows.join('') +
    boxes.join('') +
    `</svg>`
  );
}

function esc(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

// ── Feature schema definitions (verified step lists; see scripts/gen-diagrams.ts usage) ──
// Every step label quotes a real function/field in the corresponding source module.

export const FEATURE_SCHEMAS: Record<string, { title: string; steps: Step[] }> = {
  guard: {
    title: 'Guard OS — fail-closed gate (src/guard.ts)',
    steps: [
      { label: 'task / target path', kind: 'in' },
      { label: 'checkRedLine', sub: 'RED_LINE_GLOBS', kind: 'gate' },
      { label: 'RED-LINE? deny', sub: 'unless human approval', kind: 'deny' },
      { label: 'checkScope', sub: 'DEFAULT_SCOPE_GLOBS', kind: 'gate' },
      { label: 'OUT-OF-SCOPE? deny', kind: 'deny' },
      { label: 'selfTest() certify', sub: 'deny-on-red, pass-on-green', kind: 'proc' },
      { label: 'ALLOW → run', kind: 'ok' },
    ],
  },
  kernel: {
    title: 'Deterministic kernel — decide / fold / replay (src/kernel.ts)',
    steps: [
      { label: 'Command {actor, action, payload, nonce}', kind: 'in' },
      { label: 'commandHash', sub: 'content address (cause)', kind: 'proc' },
      { label: 'decide()', sub: 'forbidden → DomainError', kind: 'gate' },
      { label: 'Event[]', sub: 'INGESTED/DISPATCHED/...', kind: 'proc' },
      { label: 'Checker gate', sub: 'applyCommandChecked', kind: 'gate' },
      { label: 'fold(State, event)', sub: 'project forward', kind: 'proc' },
      { label: 'Envelope {seq, cause}', sub: 'replay() rebuilds state', kind: 'out' },
    ],
  },
  governor: {
    title: 'Telemetry governor — autonomy as a control loop (src/governor.ts)',
    steps: [
      { label: 'quality stream (0..1)', kind: 'in' },
      { label: 'error = setpoint − quality', kind: 'proc' },
      { label: 'pidStep', sub: 'kp·e + ∫(anti-windup) + kd·Δe', kind: 'gate' },
      { label: 'clamp authority u', sub: '[uMin,uMax], maxStep', kind: 'proc' },
      { label: 'loopResonance ζ', sub: 'refuse if ζ < 0.707', kind: 'gate' },
      { label: 'ICIR per factor', sub: 'volatile/dead → less authority', kind: 'proc' },
      { label: 'anomaly > 3σ? flag', kind: 'deny' },
    ],
  },
  memory: {
    title: 'Living memory (VSA) — insert / recall / forget (src/memory.ts)',
    steps: [
      { label: 'concept + payload', kind: 'in' },
      { label: 'bind(token → concept)', sub: 'bipolar vectors', kind: 'proc' },
      { label: 'store contribution', sub: 'bundle + normalize', kind: 'ok' },
      { label: 'recall(query, k)', sub: 'nearest by cosine', kind: 'out' },
      { label: 'forget(token)', sub: 'subtract bound vec', kind: 'gate' },
      { label: 're-normalize', sub: 'vector moves away', kind: 'proc' },
      { label: 'tick() decay clock', sub: 'working/short/long', kind: 'proc' },
    ],
  },
  identity: {
    title: 'Post-quantum identity & vault (src/crypto.ts, src/vault.ts)',
    steps: [
      { label: 'ML-KEM (Kyber) keypair', kind: 'proc' },
      { label: 'Ed25519 keypair', kind: 'proc' },
      { label: 'nodeId = H(pqPub ‖ edPub)', sub: 'self-certifying', kind: 'gate' },
      { label: 'vault encrypt', sub: 'XChaCha20-Poly1305 (scrypt)', kind: 'proc' },
      { label: 'tamper?', sub: 'different id → refuse', kind: 'deny' },
      { label: 'unlock (correct pass)', kind: 'ok' },
    ],
  },
  mesh: {
    title: 'No-central-server mesh (src/torrent.ts, src/mesh.ts)',
    steps: [
      { label: 'payload', kind: 'in' },
      { label: 'split → pieces', sub: 'SHA-256 each', kind: 'proc' },
      { label: 'infoHash (Merkle root)', kind: 'gate' },
      { label: 'announce / query peers', kind: 'proc' },
      { label: 'pull piece by hash', sub: 'verify before accept', kind: 'gate' },
      { label: 'corrupt? reject', kind: 'deny' },
      { label: 'assemble → bytes', kind: 'out' },
    ],
  },
  consciousness: {
    title: 'Freestyle bebop soul — self loop (src/consciousness.ts)',
    steps: [
      { label: 'selfMaintain()', sub: 'test harness + invariant', kind: 'proc' },
      { label: 'health report', kind: 'ok' },
      { label: 'selfEvolve(idea)', sub: 'copilot Checker gate', kind: 'gate' },
      { label: 'accepted → memory node', sub: 'auditable', kind: 'ok' },
      { label: 'rollback?', sub: 'forget the node', kind: 'proc' },
      { label: 'selfLoop(ideas)', sub: 'maintain + batch evolve', kind: 'out' },
    ],
  },
};

