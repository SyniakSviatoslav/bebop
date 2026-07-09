import { test } from 'node:test';
import assert from 'node:assert/strict';
import { recall, estimateTokens, denoiseHits } from './knowledge.ts';

// The living-knowledge retriever + VSA cli are NOT bundled in this standalone repo.
// recall() must (a) use the BUNDLED in-process living memory (VSA + graph), returning REAL payload
// text + a similarity score, and (b) degrade honestly (no spawn, no fabricated hits) when the
// richer §0·GP retriever is absent.

test('GREEN: recall returns REAL payload text + VSA score from bundled memory (not truncated ids)', () => {
  const r = recall('kernel law');
  assert.ok(r.hits.length > 0, 'should find seeded corpus nodes');
  for (const h of r.hits) {
    assert.ok(h.text.length > 12, `hit text should be the real payload, got: ${JSON.stringify(h.text)}`);
    assert.ok(typeof h.score === 'number' && h.score > 0, `hit should carry a score, got: ${h.score}`);
  }
});

test('GREEN: exact concept match returns the right corpus payload (deterministic, graph path)', () => {
  const r = recall('kernel law');
  assert.ok(r.hits.some((h) => h.text.includes('decide/fold/replay is pure')),
    `exact concept should surface the kernel-law payload, got: ${JSON.stringify(r.hits)}`);
});

test('RED: gibberish (no overlap with corpus concepts) returns NO confident hits — recall does not hallucinate', () => {
  // query chosen with zero substring overlap with the seeded corpus concepts (kernel/guard/mesh/...),
  // so graph recall finds nothing and the weak vector fallback (floor 0.85) excludes noise.
  const r = recall('qwfpzm vbnm lkjh tzc');
  assert.equal(r.hits.length, 0, `gibberish must produce no hits, got: ${JSON.stringify(r.hits)}`);
});

test('RED: gibberish must never surface a REAL corpus payload as a confident association', () => {
  const r = recall('zzxqwv nonsense token qwkplm'); // contains "x" → may graph-match the stray "x" node,
  // but must NEVER surface the meaningful seeded payloads (kernel law, guard, mesh, etc.)
  const meaningful = r.hits.filter((h) =>
    /decide\/fold|guard|mesh|kernel|hypervector|SyncPort/i.test(h.text));
  assert.equal(meaningful.length, 0,
    `gibberish must not surface meaningful corpus payloads, got: ${JSON.stringify(r.hits)}`);
});

test('GREEN: recall degrades honestly when §0·GP retriever absent (no spawn, no fabricated note)', () => {
  const r = recall('guard os');
  assert.ok(r.note.includes('not bundled'), `note should say not bundled, got: ${r.note}`);
  assert.ok(!r.note.includes('/root/spikes'), `note must not reference a wrong repo path, got: ${r.note}`);
  assert.ok(!r.note.includes('Command failed'), `note must not show a spawn failure, got: ${r.note}`);
});

test('GREEN: estimateTokens returns null when VSA cli absent (no spawn)', () => {
  assert.equal(estimateTokens('hello world tokens'), null);
});

// ── optical advisory field recall (off by default; re-ranks, never filters) ──
test('GREEN: opticalRecall re-ranks but never DROPS hits (advisory, id-set preserved)', () => {
  const base = recall('kernel law');
  const opt = recall('kernel law', { opticalRecall: true });
  const baseIds = base.hits.map((h) => h.id).sort();
  const optIds = opt.hits.map((h) => h.id).sort();
  assert.deepEqual(optIds, baseIds, 'optical must not filter hits — same id-set as default');
});

test('RED: graph-score hits dominate optical re-ranking (falsifiable)', () => {
  // A query that graph-matches at least one corpus concept (score 1) and also returns weaker hits.
  const r = recall('kernel law', { opticalRecall: true });
  const s = (x: { score?: number }) => x.score ?? 0;
  const graphHits = r.hits.filter((h) => s(h) >= 1);
  const weakHits = r.hits.filter((h) => s(h) < 1);
  if (graphHits.length > 0 && weakHits.length > 0) {
    // every graph hit must precede every weak hit — optical cannot promote weak above graph truth
    const firstWeak = r.hits.findIndex((h) => s(h) < 1);
    const lastGraph = r.hits.map(s).lastIndexOf(1);
    assert.ok(lastGraph < firstWeak, 'graph-score hits must stay ranked above weak hits even with optical on');
  }
});

// ── D5: RAG noise-cleaning (flag-OFF), pure denoiseHits ──

test('GREEN: denoiseHits demotes an off-manifold noise hit amid a coherent cluster', () => {
  // A tight cluster of near-identical texts + one wildly different (off-manifold) outlier.
  const hits = [
    { text: 'the kernel decide fold replay is pure and deterministic', score: 1 },
    { text: 'the kernel decide fold replay stays pure deterministic law', score: 1 },
    { text: 'the kernel decide fold replay is a pure deterministic core', score: 1 },
    { text: 'zzzz qqqq wwww vvvv unrelated gibberish noise token', score: 1 },
  ];
  const demoted = denoiseHits(hits);
  assert.ok(demoted.includes(3), `the off-manifold hit (idx 3) must be demoted, got demoted=${JSON.stringify(demoted)}`);
  assert.equal(hits[3].score, 0.5, 'demoted hit score must be halved, never zeroed (recoverable)');
});

test('RED: denoiseHits demotes NOTHING when all hits are on the same manifold (falsifiable)', () => {
  const hits = [
    { text: 'kernel decide fold replay pure deterministic', score: 1 },
    { text: 'kernel decide fold replay pure deterministic core', score: 1 },
    { text: 'kernel decide fold replay pure deterministic law', score: 1 },
  ];
  const demoted = denoiseHits(hits);
  assert.equal(demoted.length, 0, `coherent cluster must have no outliers, got demoted=${JSON.stringify(demoted)}`);
});

test('RED: denoiseHits is a no-op for <3 hits (PCA degenerate) — never fabricates a demotion', () => {
  const hits = [{ text: 'a b c', score: 1 }, { text: 'x y z totally different', score: 1 }];
  assert.deepEqual(denoiseHits(hits), [], 'fewer than 3 hits must never be denoised');
});
