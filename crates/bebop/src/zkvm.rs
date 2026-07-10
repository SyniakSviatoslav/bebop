//! zkVM boundary — deterministic, verifiable state-transition commitment.
//!
//! Replaces the Research-slot "zkVM boundary" as real, tested Rust. This is an
//! HONEST prototype: a deterministic boundary that, given (prev_state, input),
//! produces (next_state, receipt) where `receipt = H(prev_state || input ||
//! next_state || meta)`. `verify(receipt, prev, input, next)` recomputes the
//! hash and checks equality — a falsifiable integrity claim over a boundary
//! crossing (e.g. "this state change was authorized and is tamper-evident").
//!
//! It is NOT a full zero-knowledge proof system (no RISC Zero / no circuit). It
//! is the *shape* of the boundary: commit → cross → verify, with a RED case that
//! fails on tampered output. The seam where a real zkVM proof would slot in is
//! `verify()` — swap the hash check for a proof verification. NO rng, NO clock.

use sha2::{Digest, Sha256};

/// A state is an opaque byte blob (e.g. a serialized ledger snapshot).
pub type State = Vec<u8>;

/// A receipt commits to a boundary crossing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    pub prev: Vec<u8>,
    pub input: Vec<u8>,
    pub next: Vec<u8>,
    pub meta: Vec<u8>,
    pub seal: String, // = H(prev || input || next || meta)
}

fn seal(prev: &[u8], input: &[u8], next: &[u8], meta: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(prev);
    h.update(input);
    h.update(next);
    h.update(meta);
    let d = h.finalize();
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// Apply a pure transition `f` at the boundary, returning the next state and a
/// receipt that commits to (prev, input, next, meta).
pub fn cross<F>(prev: &[u8], input: &[u8], meta: &[u8], f: F) -> (State, Receipt)
where
    F: Fn(&[u8], &[u8]) -> State,
{
    let next = f(prev, input);
    let seal = seal(prev, input, &next, meta);
    let r = Receipt {
        prev: prev.to_vec(),
        input: input.to_vec(),
        next: next.clone(),
        meta: meta.to_vec(),
        seal,
    };
    (next, r)
}

/// Verify a receipt: recompute the seal and check it binds prev/input/next/meta.
/// Returns false if any field was tampered (RED case).
pub fn verify(r: &Receipt) -> bool {
    seal(&r.prev, &r.input, &r.next, &r.meta) == r.seal
}

/// Convenience: verify AND bind a specific expected `next` (caller knows the
/// post-condition they require of the boundary).
pub fn verify_expect(r: &Receipt, expect_next: &[u8]) -> bool {
    verify(r) && r.next == expect_next
}

#[cfg(test)]
mod tests {
    use super::*;

    // a trivial deterministic transition: append input to prev
    fn append(prev: &[u8], input: &[u8]) -> State {
        let mut v = prev.to_vec();
        v.extend_from_slice(input);
        v
    }

    #[test]
    fn cross_then_verify_green() {
        // GREEN: a legit crossing verifies.
        let (next, r) = cross(b"ledger-v1", b"+100", b"credit", append);
        assert_eq!(next, b"ledger-v1+100".to_vec());
        assert!(verify(&r), "valid receipt failed verification");
        assert!(verify_expect(&r, b"ledger-v1+100"), "expected next mismatch");
    }

    #[test]
    fn tampered_next_fails() {
        // RED: if the recorded `next` is changed after the fact, verify fails.
        let (_next, mut r) = cross(b"ledger-v1", b"+100", b"credit", append);
        r.next = b"ledger-v1-999".to_vec(); // tamper
        assert!(!verify(&r), "tampered receipt verified (should fail)");
    }

    #[test]
    fn tampered_seal_fails() {
        // RED: a forged seal (without knowing the transition) fails.
        let (next, mut r) = cross(b"ledger-v1", b"+100", b"credit", append);
        // attacker tries to claim a different input but keeps old seal
        r.input = b"-999".to_vec();
        assert!(!verify(&r), "forged seal verified");
        // and the legit next (from the original crossing) is independent of the forged input
        assert_eq!(next, b"ledger-v1+100".to_vec());
    }

    #[test]
    fn determinism_same_in_same_out() {
        // GREEN: same inputs → same receipt seal (deterministic, replayable).
        let (_, r1) = cross(b"x", b"y", b"m", append);
        let (_, r2) = cross(b"x", b"y", b"m", append);
        assert_eq!(r1.seal, r2.seal);
    }
}
