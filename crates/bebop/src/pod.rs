//! POD — Proof-of-Delivery (the Princess Pi attribution scheme, audit 29157).
//!
//! Princess Pi attributes an artifact to an author via a key signature + SHA512
//! content hash, distributable pseudonymously: the verifier proves WHO authored
//! the bytes without learning anything else about the author. We map that
//! exactly to delivery:
//!
//!   claim = "order:<order_id>|courier:<courier_id>|at:<ts>|loc:<x,y>"
//!   digest = SHA512(claim)
//!   proof  = vault.sign(digest)          // hybrid ML-DSA-65 ⊕ Ed25519
//!
//! `courier_id` is the courier's self-certifying VAULT id (a content hash of
//! their public key) — NOT their legal name. So the network learns "this order
//! was completed by courier <id> at <ts>, signed by <id>'s key" — authorship is
//! cryptographically proven, PII stays local. That is pseudonymous attribution:
//! verifiable, non-repudiable, deanonymizing only if the courier later chooses
//! to reveal the key↔identity link.
//!
//! This closes the WEAKEST LINK from the centralization map: the physical
//! handoff now has a trustless authorship anchor (no need to trust the matcher,
//! the restaurant, or any server). Settlement can require a valid POD proof.
//!
//! Built on `vault::NodeIdentity` (real hybrid PQ+classical signatures, SHA512).
//! Deterministic, std-only. RED+GREEN falsifiable below.

use crate::vault::NodeIdentity;
use sha2::{Digest, Sha512};

/// A pseudonymous delivery claim. `courier_id` is the courier's vault id, NOT PII.
#[derive(Clone, Debug, PartialEq)]
pub struct DeliveryClaim {
    pub order_id: String,
    pub courier_id: String,
    pub timestamp: u64,
    /// Drop-off location (2-D). Included so the claim is bound to a place, not
    /// just a time — prevents "sign any order later" replay at a different spot.
    pub x: f64,
    pub y: f64,
}

impl DeliveryClaim {
    /// Canonical, stable serialization of the claim (the bytes that get hashed).
    /// Field order is fixed; locale-independent formatting.
    fn canonical(&self) -> String {
        format!(
            "order:{}|courier:{}|at:{}|loc:{:.6},{:.6}",
            self.order_id, self.courier_id, self.timestamp, self.x, self.y
        )
    }

    /// SHA512 digest of the canonical claim (the Princess Pi content hash).
    pub fn digest(&self) -> [u8; 64] {
        let mut h = Sha512::new();
        h.update(self.canonical().as_bytes());
        h.finalize().into()
    }
}

/// A signed Proof-of-Delivery: the claim + the courier's hybrid signature + the
/// courier's public id (so any verifier can check without a directory).
#[derive(Clone, Debug)]
pub struct PodProof {
    pub claim: DeliveryClaim,
    /// Hybrid signature over SHA512(claim).
    pub signature: Vec<u8>,
    /// Courier's self-certifying public id (== claim.courier_id on a valid proof).
    pub courier_id: String,
}

/// Produce a POD proof. `courier` signs the claim with its vault identity.
/// Fail-closed: requires `courier.id == claim.courier_id` (you cannot sign a
/// claim attributing it to someone else).
pub fn sign_delivery(courier: &NodeIdentity, claim: DeliveryClaim) -> Option<PodProof> {
    if courier.id != claim.courier_id {
        return None; // refuse misattribution
    }
    let sig = courier.sign(&claim.digest());
    Some(PodProof {
        claim,
        signature: sig,
        courier_id: courier.id.clone(),
    })
}

/// Verify a POD proof against a courier public identity. Returns true iff the
/// signature is valid AND the courier id binds to the claim. Pseudonymous: the
/// verifier learns only that *this* id completed *this* order — no PII.
pub fn verify_delivery(courier: &NodeIdentity, proof: &PodProof) -> bool {
    if courier.id != proof.courier_id || courier.id != proof.claim.courier_id {
        return false;
    }
    if !courier.self_certify() {
        return false; // tampered key blob
    }
    courier.verify(&proof.claim.digest(), &proof.signature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::NodeIdentity;

    fn claim_for(c: &NodeIdentity, order: &str, t: u64) -> DeliveryClaim {
        DeliveryClaim {
            order_id: order.into(),
            courier_id: c.id.clone(),
            timestamp: t,
            x: 12.0,
            y: -3.4,
        }
    }

    #[test]
    fn pod_sign_verify_roundtrip_pseudonymous() {
        // GREEN: a courier signs; any verifier with the public id confirms
        // authorship without ever seeing a name.
        let courier = NodeIdentity::create();
        let claim = claim_for(&courier, "order-42", 1_700_000_000);
        let proof = sign_delivery(&courier, claim.clone()).expect("signs own claim");
        assert!(verify_delivery(&courier, &proof), "valid POD verifies");
        // claim carries only the vault id, not PII:
        assert!(!proof.claim.canonical().contains("name"));
        assert!(!proof.claim.canonical().contains("@"));
    }

    #[test]
    fn pod_refuses_misattribution() {
        // RED+GREEN: signing a claim attributed to a DIFFERENT courier is refused.
        let alice = NodeIdentity::create();
        let bob = NodeIdentity::create();
        let claim_for_bob = claim_for(&bob, "order-7", 1);
        // alice tries to sign bob's claim:
        assert!(
            sign_delivery(&alice, claim_for_bob).is_none(),
            "cannot sign a claim attributed to another id"
        );
    }

    #[test]
    fn pod_fails_on_tampered_claim() {
        // RED+GREEN: change the order id after signing ⇒ verify fails (non-repudiable).
        let courier = NodeIdentity::create();
        let claim = claim_for(&courier, "order-A", 1);
        let mut proof = sign_delivery(&courier, claim).unwrap();
        proof.claim.order_id = "order-EVIL".into();
        assert!(
            !verify_delivery(&courier, &proof),
            "tampered claim must fail verification"
        );
    }

    #[test]
    fn pod_replay_at_wrong_location_fails() {
        // RED+GREEN: the claim is bound to (ts, loc); a replay with a different
        // location must not verify (anti-replay / wrong-drop).
        let courier = NodeIdentity::create();
        let claim = claim_for(&courier, "order-9", 123);
        let proof = sign_delivery(&courier, claim).unwrap();
        let mut replay = proof.clone();
        replay.claim.x = 999.0;
        assert!(
            !verify_delivery(&courier, &replay),
            "replay at wrong location rejected"
        );
    }
}
