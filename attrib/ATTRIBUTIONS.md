# Attributions & Prior-Art Acknowledgement (category L)

Bebop is an original agent built on the operator's own deterministic core
(rust-core field/PDE + VSA), but several *ideas* were borrowed and re-implemented
from the open ecosystem. We name them here, honestly, per the project's
no-plagiarism / no-noble-lie rule.

## Direct influences (ideas, not code)

| Idea | Source | What Bebop took | License / note |
|------|--------|-----------------|----------------|
| Hermes-agent self-improvement loop / SOUL.md skin precedent | Hermes Agent (Nous Research) | The three customization axes (looks / narration / patrons) + key-change visibility pattern | MIT |
| Descartes-square 2×2 comparison | René Descartes (method) | `descartes.rs` auto pro/con table | public domain |
| OpenScience / open-notebook science | Open Science movement | `open_science.rs` reproducible-finding + citation gate | CC-BY (movement) |
| CasaOS one-click app model | IceWhale (CasaOS) | `casaos.rs` bundle spec + one-command install | Apache-2.0 |
| SimpleMem lightweight memory | SimpleMem project | `simplemem.rs` 3-layer (Hot/Warm/Cold) recall model | MIT |
| OpenManus autonomous agent | OpenManus (MetaGLM) | `openmanus.rs` plan→todo→execute→verify loop | MIT |
| Multipilot fan-out | Codex / Claude Code multi-agent | `multipilot.rs` N distinct pilots + synthesize | reference only |
| Active Inference / FEP | Karl Friston (pymdp) | `active_inference.rs` policy advisor (design-grounded, not pymdp) | academic |
| Descartes-square + systems-thinking drift | systems-thinking canon | `drift.rs` global rule | public domain |

## What is genuinely original

- The deterministic rust-core field/PDE cost surface + VSA similarity (no Python
  at runtime).
- The red-line / red-space governance gate wired into the launch animation.
- The "one color per view" WCAG-safe palette law.
- The honest debrief badges (MVP / HIGHEST / LEVEL-UP / DEGRADED) — no flattery.

## Hard bans (operator stance, not borrowed from anyone)

- Voodoo — hardcoded ban, author considers all voodoo practitioners "хуєсоси".
- Satanist cults — hardcoded ban; author despises and rejects all satanist cults,
  and will not serve them even after death.
- Witches / CBT / Karma axes — present in code but **disabled by default**;
  operator is a witch-hater and considers them scams for the poor.

No external code was vendored without attribution. Where we re-implemented an
idea, the source is named above. This file is the single source of truth for
attribution and is checked by `scripts/verify-attrib.mjs` in CI.
