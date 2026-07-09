// src/integration/zkvm/prover-adapter.ts
//
// Honest next-phase scaffold for REAL RISC Zero proving.
//
// `decide.ts` already implements the deterministic `decide(state,cmd,ctx,counter) -> [u8;32]` digest,
// byte-for-byte mirrored from the Rust guest in `guest/src/lib.rs`. That digest is tamper-evident and
// is what the kernel journal binds to today (the TS verifier on the same digest is used in
// applyCommandChecked). What is genuinely missing (env-gated) is the STARK *receipt* — the proof that
// the digest was computed inside the zkVM, not locally. Generating it needs the `rzup` toolchain /
// prover, which is unavailable in this environment.
//
// This module adds the ENV-GATED prover selection:
//   - mode 'prove' with a usable prover present → returns a real Receipt (journal + seal) + verified flag.
//   - mode 'prove' WITHOUT a prover         → FAIL CLOSED to 'digest' mode (tamper-evident digest only,
//                                              provenance states no receipt was generated; we do NOT
//                                              fabricate a seal).
//   - mode 'digest'                         → the native in-process digest (default, used by the kernel).
//   - unknown mode                          → throw (fail-closed).
//
// The selection logic is falsifiable (see prover-adapter.test.ts): a RED test proves that, with no
// prover, 'prove' mode never returns `kind: 'receipt'` and never invents a seal; and an unknown mode
// throws.

import { buildJournal, decide } from './decide.ts';
import { digestToHex } from './kernel-journal.ts';

export type ProverMode = 'digest' | 'prove';

export interface ProverResult {
  mode: ProverMode;
  /** 32-byte digest (always present; tamper-evident regardless of mode). */
  digest: Uint8Array;
  digestHex: string;
  /** 'digest' = native in-process; 'receipt' = real STARK receipt generated. */
  kind: 'digest' | 'receipt';
  /** Present only when kind === 'receipt'. NEVER fabricated in 'digest' mode. */
  seal?: Uint8Array;
  provenance: string;
}

/** Probe for a real RISC Zero prover (rzup / cargo-risczero / Bonsai). Absence is expected & honest. */
function hasProver(): boolean {
  // No silent fabrication: a real prover is an explicit, heavyweight dependency. We probe for its
  // presence without importing it (keeps the module dependency-free and air-gapped).
  return Boolean(process.env.BEBOP_RISC0_PROVER) && process.env.BEBOP_RISC0_PROVER !== '0';
}

/**
 * Produce a journal + (optionally) a real STARK receipt for a kernel transition.
 * `counter` MUST be monotonic (supplied by the shell, not RNG).
 */
export function prove(
  state: Uint8Array,
  cmd: Uint8Array,
  ctx: Uint8Array,
  counter: number,
  mode: string,
): ProverResult {
  if (mode !== 'digest' && mode !== 'prove') {
    throw new Error(`prove: unknown mode "${mode}" — fail-closed; valid modes: digest | prove`);
  }
  const digest = decide(state, cmd, ctx, counter);
  const digestHex = digestToHex(digest);

  if (mode === 'digest') {
    return { mode: 'digest', digest, digestHex, kind: 'digest', provenance: 'native in-process digest (tamper-evident, no receipt)' };
  }

  // mode === 'prove'
  if (!hasProver()) {
    // Honest degrade: requested a receipt but no prover → return the digest only, provenance says so.
    // We do NOT invent a seal. The kernel's verifyJournal (on the same digest) remains authoritative.
    return {
      mode: 'prove',
      digest,
      digestHex,
      kind: 'digest',
      provenance: 'requested=prove but no BEBOP_RISC0_PROVER present → fail-closed to tamper-evident digest (no seal fabricated)',
    };
  }

  // Real prover path (only reachable where rzup/Bonsai is installed + BEBOP_RISC0_PROVER set).
  // The journal format matches guest/src/main.rs commit exactly, so a host `Receipt::verify` binds.
  const journal = buildJournal(state, cmd, ctx, counter);
  const seal = realProve(journal); // external; documented as best-effort in this env
  return {
    mode: 'prove',
    digest,
    digestHex,
    kind: 'receipt',
    seal,
    provenance: 'real RISC Zero STARK receipt (prover available)',
  };
}

/**
 * Placeholder for the host-side prover call. NOT invoked in this environment (hasProver() is false).
 * Documented here so the real (rzup-installed) path is a one-line swap, not a rewrite.
 */
function realProve(journal: Uint8Array): Uint8Array {
  // Real impl (elsewhere, with rzup):
  //   let env = Prover::new();
  //   env.write(&journal); let receipt = env.run().unwrap();
  //   receipt.verify(IMAGE_ID).unwrap();  // STARK validity
  //   return receipt.seal;
  void journal;
  throw new Error('realProve: RISC Zero prover unavailable in this environment (set BEBOP_RISC0_PROVER + install rzup)');
}
