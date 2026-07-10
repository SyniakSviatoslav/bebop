# Bebop — Home

> **Bebop** is a local-first coding-agent CLI with its own deterministic Rust/WASM guard kernel, a
> living (VSA) memory, and a node identity built on a **hybrid post-quantum vault** —
> that drives any agent you already use (Claude Code, Codex, OpenCode, Aider, Goose) behind one
> auditable, free-by-default, offline control plane, and self-evolves via a "freestyle bebop soul" loop.
>
> Version **1.0.0** (2026-07-09) · native Rust core (no TypeScript at runtime). License AGPL-3.0.

## Start here
- [[Field-Sim-Comparison]] — the **unique feature**: planner reads a deterministic graph-PDE field as cost. Visual explainer.
- [[Multipilot]] — "copilot is now a multipilot": fan a task to N specialist pilots + distinct synthesizer, field arbiter veto.
- [[Outfit]] — the cosmo-noir identity contract (the "new outfit").
- [[Verification]] — how every claim is falsifiable (RED+GREEN), test counts.
- [[Research]] — the agent/research layer: Hermes parity + the L5 applied-research roadmap.

## Quick start
```bash
git clone https://github.com/SyniakSviatoslav/bebop
cd bebop
cargo run -p bebop -- boot        # guard self-test (must go RED to be trusted)
cargo run -p bebop -- dispatch "fix the red ship animation"
bebop                               # interactive TUI with the sun-warm launch (on a TTY)
```

## Identity reality (native Rust core, 2026-07-10)
- **Node identity** is a **hybrid post-quantum** vault (`src/vault.rs`): **ML-KEM-768 ⊕ X25519**
  KEM + **ML-DSA-65 ⊕ Ed25519** signature, **Argon2id** KDF, **XChaCha20-Poly1305** AEAD. The PQ
  half closes the harvest-now-decrypt-later threat; the classical half is a fallback if a PQ
  primitive regresses. The vault is **not** symmetric-only — the PQ half is wired and tested
  (`vault::tests`, `doc_claims::claim_vault_roundtrip_real`).
- **119 Rust tests** green (103 `bebop` + 16 `bebop-core`). No TypeScript in the runtime path.
