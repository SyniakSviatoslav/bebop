# Living memory (Vector Symbolic Architecture)

`src/memory.ts` is a real **Vector Symbolic Architecture (VSA)** engine — not a lookup table.
Meaning is *composition*, and memory can choose to forget.

## Core idea

- Concepts are high-dimensional **bipolar vectors** (e.g. ±1 × D).
- Composition operators: `bind` (element-wise ×), `bundle` (sum/normalize), `permute` (rotate).
- A complex concept = a bundle of bound atomic vectors. Similarity = cosine similarity.

## Token-level insert and forget

The distinctive feature: you can **remove a single token's contribution** from a concept and
re-derive the vector, without retraining.

```ts
remember(concept, payload)   // bind token → concept, store contribution
forget(token)                // subtract that token's bound vector; re-normalize
```

This gives memory that can *choose* to forget — human-like, and falsifiable (assert the vector
changed in a defined direction).

## Associative recall

```ts
nearest(query, k)  // top-k concepts by cosine similarity, with a score
recall(query, k)   // same, higher-level
```

Recall is falsifiable: `memory.test.ts` asserts that the nearest concept to a known query is the
expected one, and that `forget` actually moves the vector away.

## Layered, forgetting clock

Memory is organized in layers (`working` / `short` / `long`). `tick()` applies decay + eviction
so stale tokens fade — a forgetting clock you can advance with `bebop memory tick`.

## Graceful fallback

If the optional `spikes/` / `tools/vsa/` knowledge scripts are absent, memory falls back to
in-process storage. It never crashes on a missing external tool — verified by `memory.test.ts`.

## ▶ Live CLI

> Real `bebop` output, recorded with [asciinema](https://asciinema.org) → [agg](https://github.com/asciinema/agg) (no staging, no post-editing).

**bebop remember — write a concept into living memory**

![bebop remember — write a concept into living memory](../footage/feat-remember.gif)

**bebop recall — query in-process memory (VSA retriever not bundled; honest)**

![bebop recall — query in-process memory (VSA retriever not bundled; honest)](../footage/feat-recall.gif)

**bebop memory — inspect the memory store**

![bebop memory — inspect the memory store](../footage/feat-memory.gif)

