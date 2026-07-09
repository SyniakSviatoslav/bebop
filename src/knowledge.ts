// Bebop knowledge seam — reuse the repo's existing intelligence, do not reinvent it.
//
// This is a STANDALONE repo. It ships its own, dependency-free living memory in src/memory.ts
// (VSA hypervectors + graph spreading-activation + forgetting + persistence). That memory is
// ALWAYS consulted by recall().
//
// OPTIONALLY, if this repo also vendors the richer §0·GP retriever (spikes/living-knowledge) and
// VSA codec (tools/vsa) — which in the canonical setup live in the dowiz monorepo, not here — recall
// shells out to them too. When they are absent (the default for the standalone repo), recall degrades
// HONESTLY to the bundled in-process memory and says so. No fabrication.
//
// PLUS: the in-process livingMemory() singleton (src/memory.ts) — ONE always-on memory shared by
// every agentic CLI, this Hermes session included. recall() consults it FIRST.

import { execFileSync } from 'node:child_process';
import path from 'node:path';
import os from 'node:os';
import fs from 'node:fs';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { livingMemory, type LivingMemory } from './memory.ts';
import { opticalRecall } from './integration/optical/field-recall.ts';
import { thinLensMask } from './integration/optical/optic.ts';

const HERE = path.dirname(fileURLToPath(import.meta.url));
// This is a standalone repo: src/ → bebop-repo root is ONE level up.
const REPO_ROOT = path.resolve(HERE, '..');

export interface Recall {
  found: boolean;
  hits: { id: string; text: string; score?: number }[];
  note: string;
}

// Query the in-process living memory (ONE memory, this session included).
// PRIMARY: graph spreading-activation (deterministic concept match + edge traversal) — trustworthy.
// FALLBACK: bundled VSA vector recall (associative, by meaning) — a WEAKER signal; only used when the
// graph finds nothing, and gated at a high noise floor so random/gibberish queries return nothing
// (we never present a noisy vector match as confident). Merged, de-duplicated, scored.
export interface RecallOpts {
  /** Advisory optical field recall (SVETlANNa/Meep primitive) re-ranks hits by field correlation.
   *  Off by default — the graph/vector score stays the source of truth; optical only re-orders
   *  weak/equal-score bands as a third associative signal. */
  opticalRecall?: boolean;
  /** D5 RAG noise-cleaning (flag-OFF): PCA the candidate hit vectors, then DEMOTE hits whose PCA
   *  reconstruction residual is an outlier (> mean + k·std of residuals) — an off-manifold hit is
   *  semantic noise dragged in by the weak vector fallback. Pure/deterministic (matrix.ts PCA, no
   *  RNG/training). Off by default: the graph/vector score stays the source of truth; denoise only
   *  RE-RANKS (never drops) so a false-positive can't erase a real hit. */
  denoise?: boolean;
}

// Deterministic text → n×n real vector (char-bucketed). Same spirit as the bundled VSA codebook;
// gives opticalRecall a stable projection to rank by field correlation.
function projectText(text: string, n: number): number[] {
  const v = new Array<number>(n * n).fill(0);
  for (let i = 0; i < text.length; i++) {
    const code = text.charCodeAt(i);
    const idx = (code * 31 + i) % (n * n);
    v[idx] += ((code % 7) - 3) / 4; // small bipolar value, deterministic
  }
  return v;
}

function recallLocal(query: string, k = 5, opts: RecallOpts = {}): { id: string; text: string; score: number }[] {
  const mem: LivingMemory = livingMemory();
  const byId = new Map<string, { id: string; text: string; score: number }>();

  // 1) graph spreading-activation (concept match + edge traversal) — deterministic, the source of truth.
  //    Use recallScored: each node carries its REAL activation energy as `score` (exact match = 1,
  //    one-hop = <=decay), so the graph itself ranks the set — optical is only ever a tie-breaker.
  for (const r of mem.recallScored(query, 3)) {
    const node = mem.node(r.id);
    if (node) byId.set(r.id, { id: r.id, text: node.payload, score: Number(r.score.toFixed(4)) });
  }

  // 2) fallback vector recall — ONLY when the graph found nothing, and only above a high floor.
  //    The bundled char-codebook VSA is a weak associative signal (bipolar noise floor is high), so we
  //    refuse to surface it as confident. The richer §0·GP retriever (vendored from dowiz) is the real
  //    embedding-based recall; without it, we stay humble.
  if (byId.size === 0) {
    for (const n of mem.nearest(query, k)) {
      if (n.sim <= 0.85) continue; // high floor: exclude noise/gibberish
      const node = mem.node(n.id);
      if (node) byId.set(n.id, { id: n.id, text: node.payload, score: Number(n.sim.toFixed(3)) });
    }
  }

  const hits = [...byId.values()];

  // 3) ADVISORY optical field recall — re-rank by optical correlation when enabled. The primary score
  //    (graph=1 / vector sim) is preserved; optical supplies a secondary key so two hits with the same
  //    primary score are ordered by field similarity to the query. Never overrides the graph truth.
  if (opts.opticalRecall && hits.length > 1) {
    const n = 8; // optical field grid (n×n = 64 vec dim)
    const mask = thinLensMask(n, 0.5, 2); // passive mask, |t|<=1
    const qVec = projectText(query, n);
    const cands = hits.map((h) => projectText(h.text, n));
    const order = opticalRecall(qVec, cands, mask); // indices into hits, DESC by optical correlation
    // Optical is a TERTIARY signal: it may only re-order hits that share the SAME primary score.
    // To do that safely we capture the stable original index BEFORE sorting (the old code used
    // hits.indexOf(a) inside the comparator, which reads the live, already-reshuffling array and
    // can mis-order equal-score hits). Map: originalIndex -> optical rank.
    const opticalRank = new Map<number, number>();
    order.forEach((idx, rank) => opticalRank.set(idx, rank));
    const withOrig = hits.map((h, orig) => ({ h, orig }));
    withOrig.sort((a, b) => {
      if (b.h.score !== a.h.score) return b.h.score - a.h.score; // primary (graph/vector) dominates
      const ra = opticalRank.get(a.orig) ?? Number.MAX_SAFE_INTEGER;
      const rb = opticalRank.get(b.orig) ?? Number.MAX_SAFE_INTEGER;
      return ra - rb; // equal primary → optical correlation wins (only within the band)
    });
    hits.length = 0;
    for (const { h } of withOrig) hits.push(h);
  }

  // 4) D5 RAG NOISE-CLEANING (flag-OFF) — demote off-manifold outlier hits (see denoiseHits).
  if (opts.denoise) denoiseHits(hits);

  return hits.sort((a, b) => b.score - a.score);
}

/**
 * D5 RAG noise-cleaning (pure, deterministic). Project each hit's text to a fixed vector, then DEMOTE
 * (never drop) hits whose distance to the cluster CENTROID is an outlier (> mean + 1σ of distances) —
 * an off-manifold hit is semantic noise dragged in by the weak vector fallback. Centroid-distance is
 * used deliberately instead of PCA-reconstruction residual: with only a handful of hits the outlier
 * would dominate the top principal axis and reconstruct *well*, inverting the signal (the cluster,
 * not the outlier, would look anomalous). Centroid distance is robust at this sample size. Mutates
 * `hits[i].score` in place; a mis-flagged real hit is only halved, so it stays recoverable. No-op for
 * <3 hits (no stable centroid). Returns the demoted indices (for falsifiable assertions).
 */
export function denoiseHits(hits: { text: string; score: number }[]): number[] {
  if (hits.length < 3) return [];
  const n = 8;
  const vecs = hits.map((h) => projectText(h.text, n));
  const dim = vecs[0].length;
  const centroid = new Array<number>(dim).fill(0);
  for (const v of vecs) for (let i = 0; i < dim; i++) centroid[i] += v[i] / vecs.length;
  const dist = vecs.map((v) => {
    let s = 0;
    for (let i = 0; i < dim; i++) { const d = v[i] - centroid[i]; s += d * d; }
    return Math.sqrt(s);
  });
  const mean = dist.reduce((a, b) => a + b, 0) / dist.length;
  const std = Math.sqrt(dist.reduce((a, r) => a + (r - mean) ** 2, 0) / dist.length);
  const cutoff = mean + std; // > 1σ from centroid ⇒ off-manifold noise
  const demoted: number[] = [];
  hits.forEach((h, i) => {
    if (std > 1e-9 && dist[i] > cutoff) { h.score = Number((h.score * 0.5).toFixed(4)); demoted.push(i); }
  });
  return demoted;
}

// Call the living-knowledge §0·GP retriever (optional, vendored from dowiz). Returns ranked
// {id,text} hits. Degrades honestly to in-process memory when the retriever isn't present here.
export function recall(query: string, opts: RecallOpts = {}): Recall {
  const trimmed = (query ?? '').trim();
  // HONEST DEGRADATION (RED-TEAM fix 2026-07-09): an empty/whitespace query cannot match a concept,
  // so it must never surface hits or claim `found=true`. Previously recallScored("") substring-matched
  // every node ("" is a substring of all concepts) and fabricated a confident recall. Empty query →
  // honest "nothing found".
  if (trimmed.length === 0) {
    return {
      found: false,
      hits: [],
      note: 'in-process livingMemory (VSA + graph) only — empty query matches nothing (honest degradation)',
    };
  }
  const hits = recallLocal(query, 5, opts).map((h) => ({ id: h.id, text: h.text, score: h.score }));
  const script = path.join(REPO_ROOT, 'spikes', 'living-knowledge', 'search.mjs');
  if (!fs.existsSync(script)) {
    return {
      found: hits.length > 0,
      hits,
      note: `in-process livingMemory (VSA + graph) only — §0·GP retriever not bundled in this standalone repo`,
    };
  }
  try {
    const out = execFileSync('node', [script, query], { encoding: 'utf8', timeout: 20000, stdio: ['ignore', 'pipe', 'ignore'] });
    const remote = parseRecall(out);
    return {
      found: true,
      hits: [...hits, ...remote],
      note: `in-process livingMemory (VSA + graph) + living-knowledge §0·GP recall`,
    };
  } catch (e: any) {
    // local memory still works even if the repo retriever errors — degrade honestly
    return {
      found: hits.length > 0,
      hits,
      note: `in-process livingMemory only (living-knowledge unavailable: ${String(e.message ?? e).split('\n')[0]})`,
    };
  }
}

export function rememberLocal(concept: string, payload: string, linkTo?: string[]): string {
  return livingMemory().remember(concept, payload, linkTo);
}

// VSA token estimate via tools/vsa/cli.mjs tokens. Returns null if vsa absent.
export function estimateTokens(text: string): number | null {
  const cli = path.join(REPO_ROOT, 'tools', 'vsa', 'cli.mjs');
  if (!fs.existsSync(cli)) return null; // VSA not bundled in this standalone repo
  try {
    // content-addressed temp name (F5 fix): identical text → identical path, collision-safe,
    // no pid/Date.now nondeterminism.
    const digest = crypto.createHash('sha256').update(text).digest('hex').slice(0, 24);
    const tmp = path.join(os.tmpdir(), `.bebop-recall-${digest}.json`);
    fs.writeFileSync(tmp, text);
    const out = execFileSync('node', [cli, 'tokens', tmp], { encoding: 'utf8', timeout: 10000, stdio: ['ignore', 'pipe', 'ignore'] });
    fs.unlinkSync(tmp);
    const m = out.match(/(\d+)/);
    return m ? Number(m[1]) : null;
  } catch {
    return null;
  }
}

function parseRecall(out: string): { id: string; text: string; score?: number }[] {
  // tolerate JSON array or line-delimited; best-effort parse
  try {
    const j = JSON.parse(out);
    if (Array.isArray(j)) return j.map((h: any) => ({ id: String(h.id ?? ''), text: String(h.text ?? ''), score: h.score }));
  } catch { /* fall through */ }
  return out
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean)
    .slice(0, 5)
    .map((l, i) => ({ id: `hit-${i}`, text: l }));
}
