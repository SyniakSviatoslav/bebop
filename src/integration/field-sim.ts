/**
 * field-sim.ts — coupled graph-Laplacian FIELD EVOLUTION (the dynamical layer the ∇·F/∇×F
 * diagnostic in field.ts was missing).
 *
 * THEORY (operator 2026-07-09, probed + corrected): "simulate physics a bit with math; a tensor+graph
 * structure for BOTH the living memory AND the project, so changes propagate as multidimensional waves
 * simulating memory + time with minimal latency."
 *
 *   PROBE: the repo already has a STATIC field law (∇·F/∇×F over the embedding plane — read field.ts).
 *   What is missing is TIME. The honest physical model for "memory + time change" is field evolution
 *   on a GRAPH:
 *     • HEAT / diffusion:  ∂u/∂t = D · L u        (L = graph Laplacian)
 *       — this IS spreading-activation made rigorous: activation diffuses along edges, decaying.
 *     • WAVE (2nd order):  ∂²u/∂t² = c² · L u       (momentum → overshoot / oscillation = physical
 *       "reconsider" cycle; the curl/rotate signal from field.ts has a dynamical origin).
 *   CORRECTION: "simulate physics" ≠ Newtonian mechanics; it is graph-structured field theory. A single
 *   explicit Euler step is O(|E|), deterministic, no LLM/network — that is the "minimal latency".
 *
 *   COUPLED LATTICES (tensor+graph): living-memory graph (concepts) + project graph (files) are two
 *   layers of ONE field. Inter-layer coupling κ links a file to its concept node, so a code change
 *   perturbs the associated memory and vice-versa. The combined operator is the BLOCK LAPLACIAN:
 *       L_block = [ L_mem    κ·C ]
 *                 [ κ·Cᵀ   L_proj ]
 *   Each node carries C channels (activation, recency, risk, version, …) → u is a (nodes × channels)
 *   TENSOR; channels may cross-couple via a small c×c matrix. That is the "multidimensional wave".
 *
 * All math PURE + DETERMINISTIC (no RNG / Date / network). Falsifiable RED+GREEN. FLAG-OFF seam:
 * construct a FieldSim explicitly; nothing runs unless you call step().
 */

export type SimMode = 'diffuse' | 'wave';

export interface FieldSimConfig {
  /** number of channels per node (the tensor width). ≥1. */
  channels?: number;
  /** explicit-step stability: dt · D (diffuse) or dt · c² (wave) must stay < ~0.5 for the graph. */
  dt?: number;
  /** diffusion coefficient D (diffuse) or wave speed² c² (wave). */
  coeff?: number;
  /** per-channel cross-coupling matrix (c×c), symmetric — how channels bleed into each other. */
  channelCoupling?: number[][];
  mode?: SimMode;
}

export interface FieldStepResult {
  /** mean absolute change across all nodes/channels this step (the "latency-free" delta). */
  delta: number;
  /** max absolute change (the leading edge of the wave). */
  maxDelta: number;
  step: number;
}

/** Build an UNNORMALIZED graph Laplacian L = D − A from an adjacency matrix A (symmetric). */
export function laplacian(A: number[][]): number[][] {
  const n = A.length;
  const L: number[][] = Array.from({ length: n }, () => new Array(n).fill(0));
  for (let i = 0; i < n; i++) {
    let deg = 0;
    for (let j = 0; j < n; j++) {
      if (i !== j) { L[i][j] = -A[i][j]; deg += A[i][j]; }
    }
    L[i][i] = deg;
  }
  return L;
}

/** Build the BLOCK Laplacian coupling `layers` (adjacencies) with inter-layer edges `coupling[a][b]` (a matrix). */
export function blockLaplacian(layers: number[][][], coupling: number[][][][]): number[][] {
  const sizes = layers.map((L) => L.length);
  const N = sizes.reduce((a, b) => a + b, 0);
  const L: number[][] = Array.from({ length: N }, () => new Array(N).fill(0));
  // intra-layer Laplacians
  let off = 0;
  for (let k = 0; k < layers.length; k++) {
    const Lk = laplacian(layers[k]);
    for (let i = 0; i < sizes[k]; i++) for (let j = 0; j < sizes[k]; j++) L[off + i][off + j] = Lk[i][j];
    off += sizes[k];
  }
  // inter-layer coupling (symmetric): κ·C between layer a node i and layer b node j
  off = 0;
  const starts = layers.map((_, k) => { const s = off; off += sizes[k]; return s; });
  for (let a = 0; a < layers.length; a++) {
    for (let b = a + 1; b < layers.length; b++) {
      const C = coupling[a]?.[b];
      if (!C) continue;
      for (let i = 0; i < sizes[a]; i++) for (let j = 0; j < sizes[b]; j++) {
        const w = (C[i]?.[j] ?? 0);
        if (w === 0) continue;
        L[starts[a] + i][starts[b] + j] -= w;
        L[starts[b] + j][starts[a] + i] -= w;
        // keep diagonals = degree (row-sum to zero for a proper Laplacian)
        L[starts[a] + i][starts[a] + i] += w;
        L[starts[b] + j][starts[b] + j] += w;
      }
    }
  }
  return L;
}

/**
 * A coupled tensor+graph field simulator. `u[channel][node]` is the state; for wave mode `v` is the
 * velocity. `L` is the (block) Laplacian. step() advances ONE explicit Euler step — O(|E|·channels),
 * the "minimal latency" evolution.
 */
export class FieldSim {
  readonly n: number;
  readonly c: number;
  readonly mode: SimMode;
  private L: number[][];
  private dt: number;
  private coeff: number;
  private cc: number[][]; // channel coupling (c×c)
  u: number[][]; // [c][n]
  v: number[][]; // [c][n] velocity (wave mode)
  stepCount = 0;

  constructor(L: number[][], cfg: FieldSimConfig = {}) {
    this.n = L.length;
    this.c = cfg.channels ?? 1;
    this.mode = cfg.mode ?? 'diffuse';
    this.dt = cfg.dt ?? 0.1;
    this.coeff = cfg.coeff ?? 0.25;
    this.cc = cfg.channelCoupling ?? identity(this.c);
    if (this.n === 0) throw new Error('FieldSim: empty Laplacian');
    this.L = L;
    this.u = Array.from({ length: this.c }, () => new Array(this.n).fill(0));
    this.v = Array.from({ length: this.c }, () => new Array(this.n).fill(0));
  }

  /** Seed channel `ch` at node `i` with amplitude `amp` (an impulse — a "change" entering the field). */
  impulse(node: number, amp: number, ch = 0): void {
    if (ch < 0 || ch >= this.c || node < 0 || node >= this.n) return;
    this.u[ch][node] = amp;
  }

  /** Advance one step. diffuse: heat equation (contractive). wave: velocity-Verlet (symplectic). */
  step(): FieldStepResult {
    const { dt, coeff, L, c, n, cc } = this;
    const uBefore = this.u.map((row) => row.slice()); // for the delta measurement
    const Lu = applyLaplacian(L, this.u, c, n);
    if (this.mode === 'diffuse') {
      for (let ch = 0; ch < c; ch++) {
        for (let i = 0; i < n; i++) {
          // heat equation ∂u/∂t = −coeff·L u → contractive; plus cross-channel coupling
          let d = dt * coeff * Lu[ch][i];
          for (let k = 0; k < c; k++) d += dt * coeff * cc[ch][k] * this.u[k][i];
          this.u[ch][i] -= d;
        }
      }
    } else {
      // wave: ∂²u/∂t² = −coeff·L u. VELOCITY-VERLET (symplectic → energy conserved; explicit Euler
      // on the wave equation injects energy every step, so it is the wrong integrator here).
      //   half-kick(old) → drift → half-kick(new)
      for (let ch = 0; ch < c; ch++) {
        for (let i = 0; i < n; i++) {
          let accOld = coeff * Lu[ch][i];
          for (let k = 0; k < c; k++) accOld += coeff * cc[ch][k] * this.u[k][i];
          this.v[ch][i] += (dt / 2) * -accOld; // half-kick with old Laplacian
        }
      }
      for (let ch = 0; ch < c; ch++) for (let i = 0; i < n; i++) this.u[ch][i] += dt * this.v[ch][i]; // drift
      const LuFin = applyLaplacian(L, this.u, c, n);
      for (let ch = 0; ch < c; ch++) {
        for (let i = 0; i < n; i++) {
          let accNew = coeff * LuFin[ch][i];
          for (let k = 0; k < c; k++) accNew += coeff * cc[ch][k] * this.u[k][i];
          this.v[ch][i] += (dt / 2) * -accNew; // half-kick with new Laplacian → symplectic close
        }
      }
    }
    let delta = 0, maxDelta = 0;
    for (let ch = 0; ch < c; ch++) for (let i = 0; i < n; i++) {
      const ad = Math.abs(this.u[ch][i] - uBefore[ch][i]);
      delta += ad; if (ad > maxDelta) maxDelta = ad;
    }
    this.stepCount++;
    return { delta: delta / (c * n), maxDelta, step: this.stepCount };
  }

  /** Run `steps` steps, return the final overlay tensor [c][n]. */
  run(steps: number): number[][] {
    for (let s = 0; s < steps; s++) this.step();
    return this.u;
  }

  /**
   * Field energy. For `diffuse` this is Σ u² (decays — the memory fade). For `wave` this is the
   * HAMILTONIAN ½vᵀv + ½uᵀ(L u) — the quantity velocity-Verlet actually conserves (NOT just Σu²,
   * which oscillates). A wave's energy sloshes between kinetic (v) and potential (uᵀLu); the sum is
   * the conserved total.
   */
  energy(): number {
    if (this.mode === 'diffuse') {
      let e = 0;
      for (let ch = 0; ch < this.c; ch++) for (let i = 0; i < this.n; i++) e += this.u[ch][i] * this.u[ch][i];
      return e;
    }
    // wave: Hamiltonian ½vᵀv + ½·coeff·uᵀ(L u)  (coeff is the coupling strength in ∂²u/∂t² = −coeff·L u)
    let e = 0;
    for (let ch = 0; ch < this.c; ch++) for (let i = 0; i < this.n; i++) e += 0.5 * this.v[ch][i] * this.v[ch][i];
    const Lu = applyLaplacian(this.L, this.u, this.c, this.n);
    for (let ch = 0; ch < this.c; ch++) for (let i = 0; i < this.n; i++) e += 0.5 * this.coeff * this.u[ch][i] * Lu[ch][i];
    return e;
  }
}

function identity(c: number): number[][] {
  return Array.from({ length: c }, (_, i) => Array.from({ length: c }, (_, j) => (i === j ? 0 : 0)));
}

/** L u for a (c×n) tensor field: per-channel graph Laplacian (L is n×n). */
function applyLaplacian(L: number[][], u: number[][], c: number, n: number): number[][] {
  const out: number[][] = Array.from({ length: c }, () => new Array(n).fill(0));
  for (let ch = 0; ch < c; ch++) {
    for (let i = 0; i < n; i++) {
      let s = 0;
      for (let j = 0; j < n; j++) s += L[i][j] * u[ch][j];
      out[ch][i] = s;
    }
  }
  return out;
}
