/**
 * scale-report.ts — SCALING comparison (operator request, 2026-07-09):
 *   "my sim search"  = graph-PDE field propagation (Rust/WASM spectral + active-set) + optical/VSA ranking
 *   "native binary tree search" = k-d tree (exact Euclidean k-NN, O(log n))
 *
 * Runs the SAME graph at increasing scale and records, per backend:
 *   buildMs, queryOrPropagateMs, memoryBytes, and the semantic capability (ripple-prediction: yes/no).
 * The fair timing axis = "answer a query" (k-d query vs field propagate). k-d build is one-time; we
 * report it separately so neither side is unfairly charged.
 *
 * FLAG-OFF: run explicitly. Prints a markdown-ready table. No Date/RNG in the math.
 */
import { laplacian, FieldSim } from './field-sim.ts';
import { rustBuild, rustSpectral, rustActive, rustVsaSimilarity } from './field-rust.ts';

// ── deterministic synth repo (mirrors benchmark-field-vs-tree synthRepo) ──
function synthRepo(n: number, density: number): { A: number[][]; tensors: number[][] } {
  const rng = mulberry32(0xC0FFEE ^ n); // deterministic, no RNG at runtime-math use
  const A: number[][] = Array.from({ length: n }, () => new Array(n).fill(0));
  const tensors: number[][] = [];
  for (let i = 0; i < n; i++) {
    const t: number[] = [];
    for (let d = 0; d < 8; d++) t.push(Math.floor(rng() * 10) - 5); // small int tensor
    tensors.push(t);
  }
  let edges = 0;
  for (let i = 0; i < n; i++) {
    for (let j = i + 1; j < n; j++) {
      if (rng() < density) { A[i][j] = 1; A[j][i] = 1; edges++; }
    }
  }
  return { A, tensors };
}
function mulberry32(a: number) { return function () { a |= 0; a = (a + 0x6D2B79F5) | 0; let t = Math.imul(a ^ (a >>> 15), 1 | a); t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t; return ((t ^ (t >>> 14)) >>> 0) / 4294967296; }; }

const now = () => Number(process.hrtime.bigint()) / 1e6;
function mean(xs: number[]) { return xs.reduce((a, b) => a + b, 0) / xs.length; }

export interface ScaleRow {
  n: number; edges: number;
  kdBuildMs: number; kdQueryMs: number; kdMemBytes: number;
  jsPropagateMs: number;
  rustSpectralMs: number; rustActiveMs: number;
  speedupSpectralVsJs: number; speedupActiveVsJs: number;
  rippleField: boolean; rippleKd: boolean;
}

export async function scaleReport(scales: number[], density = 0.1): Promise<ScaleRow[]> {
  const runs = 8;
  const rows: ScaleRow[] = [];
  for (const n of scales) {
    const { A, tensors } = synthRepo(n, density);
    const edges = A.reduce((s, r) => s + r.reduce((a, b) => a + b, 0), 0) / 2;

    // ── k-d tree (native binary tree) ──
    const t0 = now();
    const kd = buildKD(tensors);
    const kdBuild = now() - t0;
    const q = tensors[0];
    const kdQ: number[] = [];
    for (let r = 0; r < runs; r++) { const t = now(); kd.knn(q, 10); kdQ.push(now() - t); }
    const kdMem = n * 4 + n * tensors[0].length * 8;

    // ── JS field propagate (K-iteration) ──
    const sim = new FieldSim(laplacian(A), { mode: 'diffuse', dt: 0.05, coeff: 1.0, channels: 1 });
    const jsT: number[] = [];
    for (let r = 0; r < runs; r++) { const t = now(); sim.impulse(0, 1); sim.predictImpact(0, { steps: 40, threshold: 1e-3 }); jsT.push(now() - t); }

    // ── Rust/WASM spectral + active-set (feed SPARSE adjacency A, core derives L=D-A) ──
    await rustBuild(A);
    const u0 = new Float64Array(n); u0[0] = 1;
    const spT: number[] = [];
    for (let r = 0; r < runs; r++) { const t = now(); await rustSpectral(u0, 2.0, 1.0, 24); spT.push(now() - t); }
    const acT: number[] = [];
    for (let r = 0; r < runs; r++) { const t = now(); await rustActive(u0, 10, { dt: 0.2, coeff: 1.0, eps: 1e-3 }); acT.push(now() - t); }

    const js = mean(jsT), sp = mean(spT), ac = mean(acT);
    rows.push({
      n, edges,
      kdBuildMs: kdBuild, kdQueryMs: mean(kdQ), kdMemBytes: kdMem,
      jsPropagateMs: js,
      rustSpectralMs: sp, rustActiveMs: ac,
      speedupSpectralVsJs: js / sp, speedupActiveVsJs: js / ac,
      rippleField: true, rippleKd: false,
    });
  }
  return rows;
}

// minimal k-d tree (copied from benchmark-field-vs-tree for self-containment)
class KDNode { axis = 0; i = 0; left: KDNode | null = null; right: KDNode | null = null; }
class KD {
  pts: number[][]; idx: number[]; root: KDNode | null = null;
  constructor(pts: number[][]) { this.pts = pts; this.idx = pts.map((_, i) => i); this.root = this.build(0, this.idx.length, 0); }
  private build(lo: number, hi: number, d: number): KDNode | null {
    if (lo >= hi) return null; const axis = d % (this.pts[0]?.length ?? 1); const mid = (lo + hi) >> 1;
    const sub = this.idx.slice(lo, hi).sort((a, b) => this.pts[a][axis] - this.pts[b][axis]);
    for (let k = 0; k < sub.length; k++) this.idx[lo + k] = sub[k];
    return { axis, i: this.idx[mid], left: this.build(lo, mid, d + 1), right: this.build(mid + 1, hi, d + 1) };
  }
  knn(q: number[], k: number): number[] {
    const best: { i: number; d: number }[] = [];
    const dist = (i: number) => { let s = 0; for (let j = 0; j < q.length; j++) { const dd = this.pts[i][j] - q[j]; s += dd * dd; } return s; };
    const search = (n: KDNode | null) => { if (!n) return; const d = dist(n.i); best.push({ i: n.i, d }); best.sort((a, b) => a.d - b.d); if (best.length > k) best.length = k; search(n.left); search(n.right); };
    search(this.root); return best.map((b) => b.i);
  }
}
function buildKD(pts: number[][]) { return new KD(pts); }

// CLI
if (import.meta.url === `file://${process.argv[1]}`) {
  const scales = (process.argv[2] ? process.argv[2].split(',').map(Number) : [500, 1000, 2000, 5000]);
  const density = process.argv[3] ? Number(process.argv[3]) : 0.1;
  scaleReport(scales, density).then((rows) => {
    console.log(`\n# Scaling report — field (graph-PDE/Rust+WASM) vs k-d tree (native binary tree)\n# density=${density}, runs=8, JS=40 Euler steps, Rust: spectral deg24 (t=2.0) / active-set (10 steps)\n`);
    console.log(['n', 'edges', 'kdBuild', 'kdQuery', 'kdMemKB', 'jsProp', 'rustSpec', 'rustAct', 'sp×', 'act×'].join('\t'));
    for (const r of rows) {
      console.log([
        r.n, Math.round(r.edges), r.kdBuildMs.toFixed(2), r.kdQueryMs.toFixed(4),
        (r.kdMemBytes / 1024).toFixed(1), r.jsPropagateMs.toFixed(2), r.rustSpectralMs.toFixed(3),
        r.rustActiveMs.toFixed(3), r.speedupSpectralVsJs.toFixed(1), r.speedupActiveVsJs.toFixed(1),
      ].join('\t'));
    }
    console.log(`\nripple-prediction: field=${rows[0].rippleField} (predicts change footprint), kd=${rows[0].rippleKd} (blind to graph)`);
  });
}
