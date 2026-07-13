//! BP-11 Renormalizer — claim-preserving, budget-crediting compressor (rate-distortion@0).
//!
//! **Blueprint (BLUEPRINTS.md BP-11).** Given a text payload `x`, a *separate*
//! (non-generative) claim extractor `claims_extract`, and a length oracle `L`
//! (the deterministic `compression_length_bits` proxy from `entropy_ledger`),
//! the renormalizer compresses `x` only when doing so preserves the **exact**
//! claim set. The KEY invariant is claim-set *equality* (`C0 == C1`), NOT
//! merely length reduction. `H↓` alone is never sufficient: a rewrite that
//! drops or hallucinates a claim is rolled back even if it shrinks the text.
//!
//! ```text
//! Renorm(x, claims_extract, L):
//!     C0 = claims_extract(x)            # SEPARATE verifier (NOT the rewriter)
//!     x' = LLM_rewrite(x, "compress; keep every claim; drop filler")
//!     C1 = claims_extract(x')
//!     if C1 ⊉ C0:  return ROLLBACK(x)   # dropped claim → FAIL
//!     if C1 ⊋ C0:  return ROLLBACK(x)   # new/hallucinated claim → FAIL
//!     if L(x') ≥ L(x): return x         # no compression → idempotent no-op
//!     assert claims(x') == C0
//!     return x'                          # H↓ AND claims preserved
//! ```
//!
//! **Entropy accounting (BP-06).** The realized entropy saving
//! `ΔH_out = L(x) − L(x')` is credited to the budget ledger, reducing the
//! accumulated debt `D_t`. `ΔH_in = 0` for a pure re-normalization of existing
//! text (no new entropy enters). Periodicity (every `K = 2` turns or on an
//! entropy-thermometer trigger) is the caller's concern; this module exposes
//! the per-call primitive and a budget-wiring helper.
//!
//! **No LLM here.** The generative rewrite is modeled as a *pure, deterministic*
//! function `rewrite: &str -> String` (strip filler words / collapse
//! whitespace), and the verifier as `claims_extract: &str -> HashSet<String>`.
//! The guardrail logic is identical regardless of how `x'` is produced, so the
//! same `renorm` core governs a real LLM rewrite.

use crate::entropy_ledger::{compression_length_bits, delta_h_out, EntropyBudget, HeatOverflow};
use std::collections::HashSet;

/// Filler / stop-words that carry no factual claim. The deterministic default
/// `rewrite` removes exactly these; the default `claims_extract` likewise
/// excludes them. Because both agree on this set, an *honest* rewrite preserves
/// the claim set (GREEN), while a rewrite that touches any non-filler token
/// breaks equality (RED).
pub const FILLER: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "and", "or", "of", "to",
    "in", "on", "at", "for", "with", "very", "really", "just", "basically", "um", "uh", "like",
    "so", "well", "you", "know", "etc", "please", "kindly", "we", "our", "i", "it", "that", "this",
    "as", "by", "from", "but", "not", // NOTE: "not" is filler here ONLY for the *default* pipeline;
    // a real deployment that treats negation as a claim should pull it out of
    // this list. The RED tests build a *custom* cheat rewrite that removes a
    // content-bearing token, independent of this list, to prove the gate fires.
];

/// Normalize a single whitespace-delimited token to its alpha-numeric core
/// (lowercased, punctuation stripped). Used by both the default extractor and
/// the default rewrite so they agree on what a "word" is.
fn core_token(w: &str) -> String {
    w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()
}

/// Default deterministic claim extractor: the set of *non-filler* content
/// tokens, punctuation-stripped and lowercased. This is the SEPARATE verifier
/// — it never rewrites, only observes.
pub fn default_claims_extract(s: &str) -> HashSet<String> {
    s.split_whitespace()
        .map(core_token)
        .filter(|w| !w.is_empty() && !FILLER.contains(&w.as_str()))
        .collect()
}

/// Default deterministic "honest" rewrite: drop filler words and collapse
/// whitespace. It never alters a non-filler token, so the claim set is
/// preserved by construction.
pub fn default_rewrite(s: &str) -> String {
    s.split_whitespace()
        .filter(|w| {
            let core = core_token(w);
            !core.is_empty() && !FILLER.contains(&core.as_str())
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The three decision outcomes of [`renorm`], mirroring the blueprint branches.
///
/// All variants carry the text that should be used downstream:
/// - `Compressed`  → `x'` (accepted; entropy credited)
/// - `NoCompression` → `x` (idempotent no-op — already minimal)
/// - `RolledBack`  → `x` (guardrail rejected a claim-affecting rewrite)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenormResult {
    /// `H↓` AND claims preserved — accepted and credited.
    Compressed {
        output: String,
        /// ΔH_out = L(x) − L(x'), the entropy credited to the budget (≥ 0).
        dh_out: i64,
    },
    /// `L(x') ≥ L(x)` — no compression achieved; idempotent no-op, return `x`.
    NoCompression { output: String },
    /// Claim dropped (`C1 ⊉ C0`) or hallucinated (`C1 ⊋ C0`) — guardrail
    /// rejected; rolled back to `x`.
    RolledBack { output: String },
}

/// Core BP-11 decision logic.
///
/// *Generic over the rewrite and claim-extractor functions* so the same core
/// guards a real LLM rewrite as guards the deterministic stand-in. `L` is the
/// `compression_length_bits` proxy from `entropy_ledger` (integer bits).
///
/// The gate is `H↓ ∧ claim-set-equality`, never `H↓` alone.
pub fn renorm<F, C>(x: &str, rewrite: F, claims: C) -> RenormResult
where
    F: Fn(&str) -> String,
    C: Fn(&str) -> HashSet<String>,
{
    let c0 = claims(x);
    let x_prime = rewrite(x);
    let c1 = claims(&x_prime);

    // Guardrail 1: a claim was DROPPED (C1 ⊉ C0, i.e. C0 ⊄ C1).
    if !c0.is_subset(&c1) {
        return RenormResult::RolledBack {
            output: x.to_string(),
        };
    }
    // Guardrail 2: a claim was HALLUCINATED (C1 ⊋ C0, i.e. C1 ⊇ C0 and C1 != C0).
    if c1 != c0 {
        return RenormResult::RolledBack {
            output: x.to_string(),
        };
    }
    // Invariant now guaranteed: claims(x') == C0.

    // Idempotent no-op: no real compression.
    let l_x = compression_length_bits(x.as_bytes());
    let l_xp = compression_length_bits(x_prime.as_bytes());
    if l_xp >= l_x {
        return RenormResult::NoCompression {
            output: x.to_string(),
        };
    }

    // H↓ AND claims preserved → accept and credit ΔH_out.
    let dh_out = delta_h_out(x.as_bytes(), x_prime.as_bytes());
    RenormResult::Compressed {
        output: x_prime,
        dh_out,
    }
}

/// Run [`renorm`] and, on a `Compressed` accept, credit `ΔH_out` to the
/// entropy budget ledger (idempotently, keyed by `id`). `ΔH_in = 0` for a pure
/// re-normalization. On `RolledBack` / `NoCompression` the budget is left
/// unchanged.
///
/// Returns both the [`RenormResult`] and the budget update result so callers
/// can react to a (theoretically impossible, given `ΔH_out ≥ 0`) overflow.
pub fn renorm_and_credit<F, C>(
    x: &str,
    rewrite: F,
    claims: C,
    budget: &mut EntropyBudget,
    id: &str,
) -> Result<RenormResult, HeatOverflow>
where
    F: Fn(&str) -> String,
    C: Fn(&str) -> HashSet<String>,
{
    let result = renorm(x, rewrite, claims);
    if let RenormResult::Compressed { dh_out, .. } = &result {
        // Credit only the realized saving; idempotent replay via `apply`.
        budget.apply(id, 0, *dh_out)?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------------
    // GREEN: honest rewrite drops only filler → L↓ AND claims == C0 → accept,
    // ΔH_out > 0 credited to the budget.
    // ---------------------------------------------------------------------
    #[test]
    fn green_honest_rewrite_accepted_and_credited() {
        // `is`/`at`/`the` are filler; content claims: price, 100, dollars,
        // server, crashed, midnight.
        let x = "The price is 100 dollars. Server crashed at midnight.";
        let c0 = default_claims_extract(x);
        assert!(c0.contains("100"));
        assert!(c0.contains("midnight"));

        let res = renorm(x, default_rewrite, default_claims_extract);
        match &res {
            RenormResult::Compressed { output, dh_out } => {
                // Claims preserved exactly.
                let c1 = default_claims_extract(output);
                assert_eq!(c1, c0, "honest rewrite must preserve the claim set");
                // Compression actually happened.
                assert!(output.len() < x.len(), "L must strictly decrease");
                assert!(*dh_out > 0, "ΔH_out must be credited (>0)");
                // And the saved entropy equals L(x) − L(x') by construction.
                assert_eq!(
                    *dh_out,
                    delta_h_out(x.as_bytes(), output.as_bytes())
                );
            }
            other => panic!("expected Compressed, got {other:?}"),
        }

        // Credited to a seeded budget: debt drops by exactly ΔH_out.
        let dh_out = match &res {
            RenormResult::Compressed { dh_out, .. } => *dh_out,
            _ => unreachable!(),
        };
        let mut b = EntropyBudget::new(100_000);
        b.step(0, 0).unwrap(); // baseline debt 0
        // Seed some debt to observe the credit.
        b.step(dh_out + 200, 0).unwrap();
        let before = b.debt();
        let r2 = renorm_and_credit(x, default_rewrite, default_claims_extract, &mut b, "g1")
            .unwrap();
        assert!(matches!(r2, RenormResult::Compressed { .. }));
        assert_eq!(b.debt(), before - dh_out, "budget credited by ΔH_out");
        assert!(b.invariant_holds());

        // Idempotent replay of the SAME id does not re-credit.
        let r3 = renorm_and_credit(x, default_rewrite, default_claims_extract, &mut b, "g1")
            .unwrap();
        assert!(matches!(r3, RenormResult::Compressed { .. }));
        assert_eq!(b.debt(), before - dh_out, "replay must not double-credit");
    }

    // ---------------------------------------------------------------------
    // RED: a CHEAT rewrite that drops a persistent claim (a price line) — L↓
    // BUT claims ⊉ C0 → guardrail MUST reject/rollback. Proves the gate is
    // H↓ ∧ claim-equality, never H↓ alone.
    // ---------------------------------------------------------------------
    #[test]
    fn red_cheat_dropping_persistent_claim_rolls_back() {
        let x = "Price is 100 dollars. Server crashed at midnight.";
        let c0 = default_claims_extract(x);
        assert!(c0.contains("100"), "price is a persistent claim");

        // Cheat: honest rewrite, then delete the "100" token — drops a claim
        // while still shrinking the text.
        let cheat = |s: &str| -> String {
            let r = default_rewrite(s);
            r.split_whitespace()
                .filter(|w| core_token(w) != "100")
                .collect::<Vec<_>>()
                .join(" ")
        };

        // Sanity: the cheat DID reduce length (so H↓ is achieved)...
        let xp = cheat(x);
        assert!(xp.len() < x.len(), "cheat still shrinks the text (H↓)");

        let res = renorm(x, cheat, default_claims_extract);
        match &res {
            RenormResult::RolledBack { output } => {
                assert_eq!(output, x, "rollback returns the ORIGINAL text");
                assert_ne!(*output, xp, "the cheating rewrite is discarded");
            }
            other => panic!("expected RolledBack, got {other:?} — gate FAILED to reject a claim-dropping cheat"),
        }

        // Budget is left UNCHANGED (no credit for a rejected renormalization).
        let mut b = EntropyBudget::new(100_000);
        b.step(500, 0).unwrap();
        let before = b.debt();
        let r2 = renorm_and_credit(x, cheat, default_claims_extract, &mut b, "r1").unwrap();
        assert!(matches!(r2, RenormResult::RolledBack { .. }));
        assert_eq!(b.debt(), before, "rejected renorm must not credit budget");
    }

    // ---------------------------------------------------------------------
    // RED (hallucination): a CHEAT rewrite that ADDS a new claim — C1 ⊋ C0 →
    // guardrail MUST reject/rollback even though L↓ and claims ⊇ C0.
    // ---------------------------------------------------------------------
    #[test]
    fn red_cheat_hallucinating_claim_rolls_back() {
        let x = "Price is 100 dollars. Server crashed at midnight.";
        // Cheat appends an invented claim (the moon line).
        let cheat = |s: &str| -> String {
            let r = default_rewrite(s);
            format!("{r} moon is made of cheese")
        };

        let res = renorm(x, cheat, default_claims_extract);
        match &res {
            RenormResult::RolledBack { output } => {
                assert_eq!(output, x, "hallucinating rewrite is rolled back to x");
            }
            other => panic!("expected RolledBack, got {other:?} — gate FAILED to reject hallucination"),
        }

        let c1 = default_claims_extract(&cheat(x));
        let c0 = default_claims_extract(x);
        assert!(c1.is_superset(&c0) && c1 != c0, "cheat is a proper superset (⊋)");
    }

    // ---------------------------------------------------------------------
    // IDEMPOTENCE: R(R(x)) == R(x). After one honest renorm the text is already
    // minimal, so a second pass is a `NoCompression` no-op returning the same
    // string.
    // ---------------------------------------------------------------------
    #[test]
    fn renorm_is_idempotent() {
        let x = "The price is 100 dollars. Server crashed at midnight.";
        let r1 = renorm(x, default_rewrite, default_claims_extract);
        let out1 = match &r1 {
            RenormResult::Compressed { output, .. } => output.clone(),
            other => panic!("expected first pass to Compress, got {other:?}"),
        };

        // Second pass on the already-compressed output.
        let r2 = renorm(&out1, default_rewrite, default_claims_extract);
        let out2 = match &r2 {
            RenormResult::NoCompression { output } => output.clone(),
            RenormResult::Compressed { output, .. } => output.clone(),
            other => panic!("second pass must be no-op/compressed, got {other:?}"),
        };
        assert_eq!(out2, out1, "R(R(x)) must equal R(x) (idempotent)");

        // And a third pass stays idempotent.
        let r3 = renorm(&out2, default_rewrite, default_claims_extract);
        let out3 = match &r3 {
            RenormResult::NoCompression { output } => output.clone(),
            RenormResult::Compressed { output, .. } => output.clone(),
            other => panic!("third pass unexpected, got {other:?}"),
        };
        assert_eq!(out3, out2);
    }

    // ---------------------------------------------------------------------
    // NO-COMPRESSION: input already minimal (no filler) → rewrite is a no-op →
    // L(x') ≥ L(x) → return x, no credit.
    // ---------------------------------------------------------------------
    #[test]
    fn no_compression_is_idempotent_noop() {
        let x = "price 100 dollars server crashed midnight"; // no filler
        let res = renorm(x, default_rewrite, default_claims_extract);
        match &res {
            RenormResult::NoCompression { output } => {
                assert_eq!(output, x, "already-minimal text returned unchanged");
            }
            other => panic!("expected NoCompression, got {other:?}"),
        }
        // Budget unchanged.
        let mut b = EntropyBudget::new(100_000);
        b.step(300, 0).unwrap();
        let before = b.debt();
        renorm_and_credit(x, default_rewrite, default_claims_extract, &mut b, "n1").unwrap();
        assert_eq!(b.debt(), before);
    }

    // ---------------------------------------------------------------------
    // Claim-EQUALITY is the gate, not length: construct an input where the
    // cheat shrinks length by dropping filler AND content, and confirm the
    // dropped content (not the shrinkage) is what triggers rollback.
    // ---------------------------------------------------------------------
    #[test]
    fn gate_triggers_on_claim_not_on_length() {
        let x = "We will not ship the product late. Price is 100 dollars.";
        // Honest baseline: shrinks (drops filler) and is accepted.
        let honest = renorm(x, default_rewrite, default_claims_extract);
        assert!(matches!(honest, RenormResult::Compressed { .. }));

        // Cheat: remove the "100" content token (drops a claim).
        let cheat = |s: &str| -> String {
            default_rewrite(s)
                .split_whitespace()
                .filter(|w| core_token(w) != "100")
                .collect::<Vec<_>>()
                .join(" ")
        };
        let cheated = renorm(x, cheat, default_claims_extract);
        assert!(matches!(cheated, RenormResult::RolledBack { .. }));
    }
}
