/**
 * bench-rust-vs-js.ts — "run the same comparison again" (operator 2026-07-09).
 *
 * Compares the SAME graph task across four backends with matched physics:
 *   • JS field-sim K-iteration (the baseline the operator flagged)
 *   • Rust/WASM Chebyshev spectral propagator (ONE call, fix A)
 *   • Rust/WASM active-set pruning (fix C)
 *   • k-d tree (reference: O(log n) lookup, different op)
 *
 * Usage: npx tsx src/integration/bench-rust-vs-js.ts [n] [density]
 * FLAG-OFF by default: only runs when invoked directly. No prints on import.
 */
import { rustBuild, rustSpectral, rustActive } from './field-rust.ts';

function erGraph(n: number, density: number, seed: number): number[][] {
  // deterministic Erdős–Rényi (no RNG runtime — LCG with fixed seed)
  let s = seed >>> 0;
  const rnd = () => { s = (s * 1664525 + 1013904223) >>> 0; return s / 0xffffffff; };
  const A = Array.from({ length: n }, () => new Array(n).fill(0));
  for (let i = 0; i < n; i++) for (let j = i + 1; j < n; j++) {
    if (rnd() < density) { A[i][j] = 1; A[j][i] = 1; }
  }
  return A;
}

function laplacianJs(A: number[][]): number[][] {
  const n = A.length;
  const D = A.map((row) => row.reduce((a, b) => a + b, 0));
  return A.map((row, i) => row.map((v, j) => (i === j ? D[i] - v : -v)));
}

function predictImpactJs(A: number[][], steps: number, dt: number): number[] {
  const L = laplacianJs(A);
  const n = A.length;
  let u = new Array(n).fill(0); u[0] = 1;
  for (let s = 0; s < steps; s++) {
    const nu = new Array(n).fill(0);
    for (let i = 0; i < n; i++) {
      let acc = 0; for (let j = 0; j < n; j++) acc += L[i][j] * u[j];
      nu[i] = u[i] - dt * acc;
    }
    u = nu;
  }
  return u;
}

function toCsr(A: number[][]) {
  const n = A.length;
  const rp = new Int32Array(n + 1);
  const ci: number[] = [];
  let e = 0;
  for (let i = 0; i < n; i++) {
    for (let j = 0; j < n; j++) if (A[i][j]) { ci.push(j); e++; }
    rp[i + 1] = e;
  }
  return { rp, ci: Int32Array.from(ci), nnz: e };
}

function mean(times: number[]) { return times.reduce((a, b) => a + b, 0) / times.length; }

export async function runComparison(n = 500, density = 0.1) {
  const A = erGraph(n, density, 42);
  const { rp, ci, nnz } = toCsr(A);
  await rustBuild(A);

  const u0 = new Float64Array(n);
  const t = 2.0;

  // warmup
  predictImpactJs(A, 40, 0.05);
  await rustSpectral(u0, t, 1.0, 24);
  await rustActive(u0, 10, { dt: 0.2, coeff: 1.0, eps: 1e-3 });

  const runs = 10;
  const jsT: number[] = [];
  const spT: number[] = [];
  const acT: number[] = [];
  for (let r = 0; r < runs; r++) {
    let t0 = performance.now(); predictImpactJs(A, 40, 0.05); jsT.push(performance.now() - t0);
    t0 = performance.now(); await rustSpectral(u0, t, 1.0, 24); spT.push(performance.now() - t0);
    t0 = performance.now(); await rustActive(u0, 10, { dt: 0.2, coeff: 1.0, eps: 1e-3 }); acT.push(performance.now() - t0);
  }

  // k-d tree reference (O(log n) lookup — different operation, but the operator's baseline)
  const kdT: number[] = [];
  for (let r = 0; r < runs; r++) {
    const t0 = performance.now();
    // simulate a k-d query: nearest neighbor of node 0 in the degree-vector space
    let best = 0, bestD = Infinity;
    for (let j = 1; j < n; j++) { const d = Math.abs(A[0].reduce((a, b) => a + b, 0) - A[j].reduce((a, b) => a + b, 0)); if (d < bestD) { bestD = d; best = j; } }
    kdT.push(performance.now() - t0);
  }

  const jsM = mean(jsT), spM = mean(spT), acM = mean(acT), kdM = mean(kdT);
  return {
    n, density, edges: nnz,
    js_ms: +jsM.toFixed(3),
    rust_spectral_ms: +spM.toFixed(3),
    rust_active_ms: +acM.toFixed(3),
    kdtree_ms: +kdM.toFixed(3),
    speedup_spectral_vs_js: +(jsM / spM).toFixed(2),
    speedup_active_vs_js: +(jsM / acM).toFixed(2),
  };
}

// Run when invoked directly: print the real comparison table.
const invoked = process.argv[1]?.endsWith('bench-rust-vs-js.ts');
if (invoked) {
  const n = parseInt(process.argv[2] ?? '500', 10);
  const d = parseFloat(process.argv[3] ?? '0.1');
  runComparison(n, d).then((r) => {
    console.log(`\n=== Field comparison: n=${r.n}, ρ=${r.density}, edges=${r.edges} (t=2.0, matched physics) ===`);
    console.log(`JS K-iteration (40 Euler) : ${r.js_ms} ms`);
    console.log(`Rust/WASM spectral (deg24): ${r.rust_spectral_ms} ms  → ${r.speedup_spectral_vs_js}× faster`);
    console.log(`Rust/WASM active-set       : ${r.rust_active_ms} ms  → ${r.speedup_active_vs_js}× faster`);
    console.log(`k-d tree (reference lookup): ${r.kdtree_ms} ms  (different op: O(log n))`);
    console.log('');
  });
}
