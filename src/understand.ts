// Bebop "understand everything" — a zero-dependency project mapper.
//
// It does NOT guess the architecture. It reads the REAL source tree, extracts the REAL
// import edges, classifies each module by layer, and renders a deterministic SVG graph.
// Every edge in the picture is a real `import` in the code — falsifiable, not decorative.
//
// Pure + testable: `scan`, `parseImports`, `buildGraph`, `layerOf`, `renderSvg` take explicit
// inputs so tests never touch the live FS layout (Verified-by-Math).

import * as fs from 'node:fs';
import * as path from 'node:path';

export interface GraphNode {
  id: string; // relative path from repo root, e.g. "src/guard.ts"
  layer: string; // logical layer (guard, kernel, memory, ...)
  stem: string; // filename without ext, e.g. "guard"
  isTest: boolean;
}

export interface GraphEdge {
  from: string; // node id
  to: string; // node id
}

export interface Graph {
  root: string;
  nodes: GraphNode[];
  edges: GraphEdge[];
}

// Layer ordering — left→right in the diagram. Driven by real filenames, not opinion.
export const LAYERS = [
  'shell', // entrypoint + personalization
  'guard', // fail-closed trust boundary
  'core', // deterministic event-sourcing kernel
  'routing', // model/backend selection
  'agent', // loop / copilot / consciousness
  'memory', // VSA living memory + vault
  'mesh', // content-addressed transport
  'platform', // mcp / hooks / skills / sync
  'other',
] as const;

export type Layer = (typeof LAYERS)[number];

// Filename-stem → layer. Anything not listed falls to `other` (still plotted, never hidden).
const STEM_LAYER: Record<string, Layer> = {
  bebop: 'shell',
  init: 'shell',
  profile: 'shell',
  theme: 'shell',
  voice: 'shell',
  launch: 'shell',
  guard: 'guard',
  'core-wasm': 'guard',
  kernel: 'core',
  store: 'core',
  router: 'routing',
  routing: 'routing',
  backend: 'routing',
  'free-llm': 'routing',
  token: 'routing',
  loop: 'agent',
  copilot: 'agent',
  consciousness: 'agent',
  doctrine: 'agent',
  memory: 'memory',
  knowledge: 'memory',
  vault: 'memory',
  crypto: 'mesh',
  torrent: 'mesh',
  mesh: 'mesh',
  mcp: 'platform',
  hooks: 'platform',
  skills: 'platform',
  'sync-server': 'platform',
  auth: 'platform',
  settings: 'platform',
};

export function layerOf(relPath: string): Layer {
  const stem = path.basename(relPath, path.extname(relPath));
  return STEM_LAYER[stem] ?? 'other';
}

const SKIP_DIRS = new Set(['node_modules', '.git', 'target', '.bebop', 'dist', 'build']);

/** Walk the repo for source files (`.ts`/`.tsx`, excluding `.test.`). Pure over `readdir`. */
export function scan(root: string, rel: string = '.'): string[] {
  const abs = path.join(root, rel);
  let entries: fs.Dirent[];
  try {
    entries = fs.readdirSync(abs, { withFileTypes: true });
  } catch {
    return [];
  }
  const out: string[] = [];
  for (const e of entries) {
    if (e.name.startsWith('.') && e.name !== '.github') continue;
    if (SKIP_DIRS.has(e.name)) continue;
    const childRel = path.join(rel, e.name);
    if (e.isDirectory()) {
      out.push(...scan(root, childRel));
    } else if (/\.(ts|tsx)$/.test(e.name) && !e.name.includes('.test.')) {
      out.push(childRel.split(path.sep).join('/'));
    }
  }
  return out;
}

/** Extract relative import specifiers (`'./x'`, `'../y'`) from source text. Pure. */
export function parseImports(text: string): string[] {
  const specs: string[] = [];
  const re = /(?:import|export)[^'"]*?from\s*['"]([^'"]+)['"]|import\s*\(\s*['"]([^'"]+)['"]\s*\)/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text))) {
    const spec = m[1] ?? m[2];
    if (spec && (spec.startsWith('./') || spec.startsWith('../'))) specs.push(spec);
  }
  return specs;
}

/** Resolve a relative specifier against the importing file's dir → repo-relative id. Pure. */
export function resolveImport(fromId: string, spec: string, root: string): string | null {
  const fromDir = path.dirname(fromId);
  const resolved = path.normalize(path.join(fromDir, spec));
  const rel = resolved.split(path.sep).join('/');
  // only keep edges that point at a real scanned file
  const candidate = rel.endsWith('.ts') || rel.endsWith('.tsx') ? rel : `${rel}.ts`;
  const exists =
    fs.existsSync(path.join(root, candidate)) || fs.existsSync(path.join(root, rel + '.tsx'));
  return exists ? candidate : null;
}

/** Build the full graph from a repo root. Reads files (IO lives only here). */
export function buildGraph(root: string): Graph {
  const files = scan(root).filter((f) => !f.includes('.test.'));
  const nodes: GraphNode[] = files.map((f) => ({
    id: f,
    layer: layerOf(f),
    stem: path.basename(f, path.extname(f)),
    isTest: false,
  }));
  const ids = new Set(nodes.map((n) => n.id));
  const edges: GraphEdge[] = [];
  for (const f of files) {
    let text: string;
    try {
      text = fs.readFileSync(path.join(root, f), 'utf8');
    } catch {
      continue;
    }
    for (const spec of parseImports(text)) {
      const to = resolveImport(f, spec, root);
      if (to && ids.has(to) && to !== f) edges.push({ from: f, to });
    }
  }
  return { root, nodes, edges };
}

// ── SVG rendering (zero deps) ───────────────────────────────────────────────

export interface RenderOpts {
  width?: number;
  height?: number;
  focus?: string[]; // node ids to highlight (feature docs)
  title?: string;
}

const COL_W = 190;
const ROW_H = 30;
const BOX_W = 150;
const BOX_H = 22;
const MARGIN = 24;
const LAYER_COLORS: Record<string, string> = {
  shell: '#46B0A4',
  guard: '#E0573E',
  core: '#C9A227',
  routing: '#5B8DEF',
  agent: '#9B6DDE',
  memory: '#3FB68B',
  mesh: '#D98E2B',
  platform: '#7A8AA0',
  other: '#9AA0A6',
};

/** Render the graph to a standalone SVG string. Deterministic layout (no RNG). */
export function renderSvg(graph: Graph, opts: RenderOpts = {}): string {
  const title = opts.title ?? 'Bebop project map';
  // order layers; collect nodes per layer
  const byLayer = new Map<string, GraphNode[]>();
  for (const l of LAYERS) byLayer.set(l, []);
  for (const n of graph.nodes) byLayer.get(n.layer)!.push(n);
  // stable sort within layer
  byLayer.forEach((arr) => arr.sort((a, b) => a.id.localeCompare(b.id)));

  const activeLayers = LAYERS.filter((l) => byLayer.get(l)!.length > 0);
  const maxRows = Math.max(1, ...activeLayers.map((l) => byLayer.get(l)!.length));
  const width = opts.width ?? MARGIN * 2 + activeLayers.length * COL_W;
  const height = opts.height ?? MARGIN * 2 + maxRows * ROW_H + 40;

  const focus = new Set(opts.focus ?? []);
  const pos = new Map<string, { x: number; y: number; col: number }>();
  activeLayers.forEach((layer, col) => {
    const arr = byLayer.get(layer)!;
    const colX = MARGIN + col * COL_W + (COL_W - BOX_W) / 2;
    arr.forEach((n, row) => {
      const y = MARGIN + 30 + row * ROW_H + (maxRows - arr.length) * (ROW_H / 2);
      pos.set(n.id, { x: colX, y, col });
    });
  });

  const colorFor = (n: GraphNode) => LAYER_COLORS[n.layer] ?? '#9AA0A6';
  const dim = (id: string) => (focus.size === 0 ? false : !focus.has(id) && !isNeighbor(graph, focus, id));

  // edges first (under nodes)
  const edgeSvg: string[] = [];
  for (const e of graph.edges) {
    const a = pos.get(e.from);
    const b = pos.get(e.to);
    if (!a || !b) continue;
    const x1 = a.x + BOX_W;
    const y1 = a.y + BOX_H / 2;
    const x2 = b.x;
    const y2 = b.y + BOX_H / 2;
    const mx = (x1 + x2) / 2;
    const d = `M ${x1} ${y1} C ${mx} ${y1}, ${mx} ${y2}, ${x2} ${y2}`;
    const faint = focus.size > 0 && !(focus.has(e.from) && focus.has(e.to));
    edgeSvg.push(
      `<path d="${d}" fill="none" stroke="${faint ? '#E5E7EB' : '#B7C0CC'}" stroke-width="${faint ? 0.6 : 1.1}" opacity="${faint ? 0.5 : 0.9}"/>`,
    );
  }

  // nodes
  const nodeSvg: string[] = [];
  for (const n of graph.nodes) {
    const p = pos.get(n.id)!;
    const c = colorFor(n);
    const isDim = dim(n.id);
    const opacity = isDim ? 0.28 : 1;
    const stroke = focus.has(n.id) ? '#111' : '#fff';
    nodeSvg.push(
      `<g opacity="${opacity}">` +
        `<rect x="${p.x}" y="${p.y}" width="${BOX_W}" height="${BOX_H}" rx="5" fill="${c}" stroke="${stroke}" stroke-width="${focus.has(n.id) ? 2 : 1}"/>` +
        `<text x="${p.x + BOX_W / 2}" y="${p.y + BOX_H / 2 + 4}" font-family="monospace" font-size="10" fill="#fff" text-anchor="middle">${n.stem}</text>` +
        `</g>`,
    );
  }

  // layer headers
  const headerSvg: string[] = [];
  activeLayers.forEach((layer, col) => {
    const colX = MARGIN + col * COL_W + COL_W / 2;
    headerSvg.push(
      `<text x="${colX}" y="${MARGIN + 14}" font-family="sans-serif" font-size="11" font-weight="bold" fill="${LAYER_COLORS[layer]}" text-anchor="middle">${layer}</text>`,
    );
  });

  const legend = LAYERS.filter((l) => byLayer.get(l)!.length > 0)
    .map((l, i) => {
      const lx = MARGIN + i * 96;
      return `<rect x="${lx}" y="${height - 18}" width="11" height="11" rx="2" fill="${LAYER_COLORS[l]}"/><text x="${lx + 16}" y="${height - 8}" font-family="sans-serif" font-size="10" fill="#444">${l}</text>`;
    })
    .join('');

  return (
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">` +
    `<rect width="${width}" height="${height}" fill="#FBFCFD"/>` +
    `<text x="${MARGIN}" y="18" font-family="sans-serif" font-size="13" font-weight="bold" fill="#1a1a1a">${title}</text>` +
    headerSvg.join('') +
    edgeSvg.join('') +
    nodeSvg.join('') +
    `<g>${legend}</g>` +
    `</svg>`
  );
}

function isNeighbor(graph: Graph, focus: Set<string>, id: string): boolean {
  for (const e of graph.edges) {
    if (focus.has(e.from) && e.to === id) return true;
    if (focus.has(e.to) && e.from === id) return true;
  }
  return false;
}

/** Convenience: scan + render in one call. */
export function mapRepo(root: string, opts: RenderOpts = {}): string {
  return renderSvg(buildGraph(root), opts);
}
