# Bebop — architecture narration transcript

> Spoken version of `docs/ARCHITECTURE.md`. Recorded from `docs/narration/architecture-narration.mp3`.

Bebop is built in two layers. The outside layer is a TypeScript program you run in your terminal.
The inside layer is a tiny, fast guard written in the Rust language and compiled into a
WebAssembly module — a small file called bebop_core.wasm that lives right inside the project.
Bebop does not need Rust installed to run; the guard file is already there.

The TypeScript shell handles the cross-cutting rules: the guard that blocks dangerous paths, the
router that picks the cheapest good-enough model, the copilot that makes a plan then checks it, the
memory that remembers what worked, and the governor that watches quality and turns autonomy up or
down like a thermostat.

The agents — Claude, Codex, OpenCode, and the rest — are thin adapters. Bebop treats them as dumb
executors; the brains stay in Bebop. When the guard runs, it delegates to the Rust core when present,
and falls back to a faithful TypeScript copy otherwise, so the rules are the same either way.

You can see the whole thing as a picture: run `bebop map` to draw the real module graph, or
`bebop diagrams` to redraw every schema.
