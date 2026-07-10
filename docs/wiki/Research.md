# Research — the agent / research layer

Bebop's *research* surface is the living-knowledge retriever (`crate::knowledge::recall`):
VSA (vector-symbolic) similarity over a seeded memory. It is reachable two ways from the CLI,
both backed by the same deterministic engine (no stub, no separate "research" service):

```bash
bebop recall "<query>"     # VSA similarity recall over the seeded store
bebop research "<query>"   # alias of `recall` — same engine, same output
```

The recall path is falsifiable: a real-text query returns its hits; a gibberish query returns
an honest "no hits + noise floor" note. See `crates/bebop/src/knowledge.rs` and the
`claim_vault_roundtrip_real` / recall `#[test]`s.

## Parity with Hermes / Claude Code
Bebop is built to *host* any agent you already use behind one guard plane — it is not a fork of them.
The documented parity patterns (CLI/UX borrowing, MCP adapters) live in
`docs/integrations/agent-parity.md` and `docs/integrations/mcp.md`. Key point: Bebop supplies the
**deterministic control plane** (guard OS, field arbiter, router, vault) while the hosted agent
supplies the reasoning. That split is the whole design.

## L5 applied-research roadmap (research-only synthesis)
The L5 wave (Zenoh mesh, RISC Zero zkVM money boundary, TigerBeetle ledger, active-inference
FEP, VSA codec) was synthesized from a research dump and is **documented, not all implemented**:

- `docs/design/bebop-L5-research-roadmap-2026-07-09.md` — the synthesis + max-EV priority order.
- `docs/design/bleeding-edge-EV-2026-07-08.md` — the EV ranking (Zenoh > zkVM > TigerBeetle;
  pymdp/RxInfer = design-only; SVETlANNa/Meep = research; FinalSpark wetware = OUT).
- `docs/design/bebop-tensor-field-theory-2026-07-09.md` — the graph-PDE field math (the field
  core that *is* implemented in `rust-core`).

**Honest status:** the deterministic field core, the VSA similarity, the router, and the **hybrid
post-quantum vault** are *real and tested*. The Zenoh mesh and zkVM boundary are now wired
into the native core as **honest prototypes** (not silent losses, not faked as production):
`src/zenoh.rs` is a local broker with the same pub/sub iface as `portkey` (swap-in point for a
real Zenoh/network transport); `src/zkvm.rs` is a deterministic commit/verify state seal
(replace the hash-seal with a real zk proof system when the boundary needs crypto-grade
non-interactivity). TigerBeetle ledger integration remains a **research slot** — not yet in the
native core. All wired items are covered by `doc_claims` + the falsifiable-proof guardrail.
