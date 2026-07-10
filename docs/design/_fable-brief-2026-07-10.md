# FABLE ONE-SHOT ADVERSARIAL REVIEW — bebop (SCOPED, bounded read list)

You are Claude Fable. Perform a SANCTIONED one-shot, READ-ONLY adversarial review of the
`bebop` coding-agent codebase. You have never seen this project. Do NOT explore the filesystem
beyond the exact files listed below — read them directly. Do NOT run recursive searches.
Produce your full report as your final message.

## Method (§0·GP ground-truth discipline)
- For EVERY finding cite `file:line` from the files you actually read.
- End each finding with a DETERMINISTIC follow-up (a gate/test to add, a claim to retract, or a
  process to delete). A finding without a follow-up is invalid.
- Read-only. Do not edit, commit, or write files.
- Be adversarial and decorrelated: assume the project's own docs may show confirmation bias.

## Files to read (read these and ONLY these):
- /root/bebop-repo/crates/bebop/src/lib.rs
- /root/bebop-repo/crates/bebop/src/pod.rs
- /root/bebop-repo/crates/bebop/src/guard.rs
- /root/bebop-repo/crates/bebop/src/reputation.rs
- /root/bebop-repo/crates/bebop/src/matcher.rs
- /root/bebop-repo/crates/bebop/src/vault.rs
- /root/bebop-repo/crates/bebop/src/stabilizer.rs
- /root/bebop-repo/crates/bebop/src/wiring.rs
- /root/bebop-repo/crates/bebop/src/mapping.rs
- /root/bebop-repo/crates/bebop/src/cost_estimate.rs
- /root/bebop-repo/crates/bebop/src/wavefield.rs
- /root/bebop-repo/crates/bebop/src/field.rs
- /root/bebop-repo/crates/bebop/src/field_physics.rs
- /root/bebop-repo/crates/bebop/src/sandbox.rs
- /root/bebop-repo/crates/bebop/src/zenoh.rs
- /root/bebop-repo/crates/bebop/src/ledger.rs
- /root/bebop-repo/crates/bebop/src/zkvm.rs
- /root/bebop-repo/crates/bebop/src/portkey.rs
- /root/bebop-repo/rust-core/src/lib.rs
- /root/bebop-repo/scripts/guardrail-falsifiable-proof.mjs
- /root/bebop-repo/scripts/verify-doc-claims.mjs
- /root/bebop-repo/docs/design/bebop-fundamental-principles-2026-07-09.md
- /root/bebop-repo/docs/design/reverse-engineering-loop-2026-07-09.md
- /root/bebop-repo/docs/design/delivery-protocol/SYSTEM-ARCHITECTURE-AUDIT.md
- /root/bebop-repo/README.md

## The project claims these load-bearing principles (VERIFY, don't assume):
0. Constant Doubt (unverified=false; enforced by verify-doc-claims + guardrail scripts)
1. Verified-by-Math (every change ships a falsifiable RED+GREEN test)
2. Red lines (auth/money/RLS/secrets/bulk human-gated, never auto-touched)
3. Symmetrical loops (cycle consistency)
4. L5 Neuro-Symbolic Gate (advisor PROPOSES, deterministic kernel DECIDES)
5. As-above-so-below (one fail-closed verifier recurs at kernel/agent/plan/tool scale)
6. Propose-don't-execute (stochastic layer only names intents; execution deterministic)
7. Flag-OFF -> shadow -> gate (no feature goes live silently)
+ determinism-as-security-model, named-blind-spots, math-not-metaphor, RED-is-the-proof,
  deterministic-twin-for-risky-IO.

## Produce this report:
### A. Reverse-engineered principles (file:line), + any CLAIMED principle you could NOT substantiate.
### B. LOGICAL FALLACIES & COGNITIVE BIASES (core ask) — hunt specifically:
   circular self-sealing reasoning; confirmation bias (only GREEN shown); survivorship bias;
   equivocation (deterministic/verified/trust/decentralized meaning different things in different places);
   appeal-to-math/false precision; motivated reasoning (moat/investability narrative overriding truth);
   composition fallacy (each module safe => system safe); reification (field/governor/consciousness as
   mechanism, not metaphor); no-true-Scotsman on what counts as "RED". Cite file:line. Be unflinching.
### C. PATTERNS & CROSS-PATTERNS — find NEW ones or show where a NAMED pattern is INCONSISTENTLY
   applied / VIOLATED (e.g. a "deterministic" module that actually leaks RNG/time/network; a place
   where the stochastic layer IS given the actuator; a RED test that cannot actually fail).
### D. Deterministic follow-ups — for each B/C finding, a concrete checkable action.
Be concise, elitist, precise. Output the report now.
