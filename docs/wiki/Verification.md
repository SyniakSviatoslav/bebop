# Verification

Every claim in Bebop is **falsifiable** (Verified-by-Math): an assertion that goes RED on bad input,
shipped alongside the GREEN.

## Counts (0.4.0, 2026-07-09c)
- **Rust kernel:** 16 tests (`cargo test -p bebop-core`), wasm32 build clean.
- **TS suite:** 547 tests (`npm test`), 0 fail.
- **Typecheck:** `npm run typecheck` → 0 errors.
- **Doc-gate:** `node scripts/verify-doc-claims.mjs` → all doc claims backed by live proof.
- **Falsifiable-proof:** `node scripts/guardrail-falsifiable-proof.mjs` → every test file has a RED case.

## Principles
- **Constant Doubt:** no verification, no statement.
- **Better less than sorry:** never state what isn't fact-checked.
- **Ground truth over proxy:** deterministic math truth may delete rotten processes.
- **Red-line globs** (auth / money / RLS / migrations / bulk-edit) need a human gate before change.

## Pre-commit gates
`verify` runs: typecheck → tests → doc-claim honesty → falsifiable-proof. All must pass before a commit
lands (the repo enforces this in `.husky`/pre-commit hook).
