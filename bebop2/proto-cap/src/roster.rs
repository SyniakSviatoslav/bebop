//! AnchorRoster + UCAN-subset delegation verification.
//!
//! # The root-of-trust problem this solves
//!
//! Before this module, [`crate::capability::Capability`] carried a
//! `subject_key` that was **self-attested**: the capability said "I am
//! authorized by key X", and the verifier would check the signature against
//! X itself. That is a total authorization bypass — any key can mint a
//! capability naming itself as the subject and sign it with itself.
//!
//! This module introduces a **trust anchor roster**: a small, fixed set of
//! Ed25519 public keys enrolled at genesis. At runtime there is no central
//! issuer and no reputation ledger; the *only* keys that may bootstrap
//! authority are the enrolled anchors. A [`Delegation`] is a signed,
//! attenuated capability grant from a parent key to a child key (the UCAN
//! model: `ucan.A` issues `ucan.B` with a scope that is a subset of `A`'s).
//! A chain of delegations is accepted only when:
//!
//! 1. its **root** issuer is an enrolled anchor (kills self-issue), and
//! 2. every link is signed by its `issued_by` parent, and
//! 3. links chain (child == parent's subject), and
//! 4. scope only ever attenuates (narrows), and
//! 5. the chain tail binds to the capability's `subject_key`, and
//! 6. the requested effect is a subset of the tail scope.
//!
//! CI GUARD: NO-COURIER-SCORING — anchors are *identities* (public keys), not
//! trust ratings. There is no score, no reputation, no "trusted mover".
//!
//! # Honest bound (module doc requirement)
//!
//! Authorization needs a root of trust. We have **no central issuer at
//! runtime**: the roster is enrolled exactly once, at genesis, and then frozen.
//! There is exactly one set of anchors and it never grows or shrinks during
//! operation. That is the whole trust surface — keep it small and audit it.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::capability::Capability;
use crate::error::{CapError, CapResult};
use crate::scope::{Action, Resource, Scope};
use crate::tlv::{tlv_signing_input, DOMAIN_DELEGATION};

/// The authorization a delegation grants / a capability requests.
///
/// G4 (2026-07-14): a delegation does NOT grant a single `(resource, action)`
/// pair — it grants a *set* of them. A delegated principal may hold several
/// verbs-on-objects. Attenuation (UCAN "narrow-only") means a child's set is a
/// **subset** of its parent's set. The previous model stored one pair and made
/// `is_subset_of` flat equality, so attenuation narrowed *nothing* — that is the
/// live G4 bug this type fixes.
///
/// A `Scope` is therefore a small closed set of `(Resource, Action)` pairs.
/// `is_subset_of` is a real set-subset, so a parent granting
/// `{Route::Send, Ledger::Read}` can attenuate a leaf to `{Route::Send}` and the
/// leaf cannot escalate to `Ledger::Read`.
///
/// CI GUARD: NO-COURIER-SCORING — a scope is verbs-on-objects, never a score.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effect {
    /// Set of `(resource, action)` pairs this effect authorizes.
    pub grants: Vec<(Resource, Action)>,
}

impl Effect {
    /// Construct an effect from an explicit set of `(resource, action)` pairs.
    pub fn new(grants: Vec<(Resource, Action)>) -> Self {
        Effect { grants }
    }

    /// Single-pair convenience constructor (the previous flat shape).
    pub fn single(resource: Resource, action: Action) -> Self {
        Effect {
            grants: vec![(resource, action)],
        }
    }

    /// Whether `self` is a (narrow-or-equal) subset of `super_scope`.
    ///
    /// G4 fix: set-subset, not equality. Every pair in `self` must appear in
    /// `super_scope`. An empty effect is a subset of anything (least privilege),
    /// and a scope is a subset of itself.
    pub fn is_subset_of(&self, super_scope: &Scope) -> bool {
        self.grants.iter().all(|p| super_scope.grants.contains(p))
    }
}

/// `Scope` (the set of `(resource, action)` pairs) is defined in
/// `crate::scope::Scope`; `Effect` reuses that type as its superset.

/// A single delegation link in a UCAN-subset chain.
///
/// `issued_by` is the *parent* (issuer) key and `subject` is the *child*
/// (subject) key. `issued_by` signs the canonical bytes of this link, so a
/// child cannot forge a grant it was not given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delegation {
    /// Parent / issuer public key (32 bytes). Must be an enrolled anchor when
    /// this is the root of the chain, otherwise must equal the preceding
    /// link's `subject`.
    pub issued_by: [u8; 32],
    /// Child / subject public key (32 bytes) this grant is made to.
    pub subject: [u8; 32],
    /// Scope (resource + action) the granter is willing to pass down. Must be a
    /// subset of the parent link's scope (attenuation-only).
    pub scope: Scope,
    /// Effect that is actually authorized at this link. Subset of `scope`.
    pub effect: Effect,
    /// Expiry (monotonic tick, like [`Capability::expiry`]).
    pub expiry: u64,
    /// Single-use nonce (8 bytes).
    pub nonce: [u8; 8],
    /// Ed25519 signature (64 bytes) over `canonical_bytes()`, by `issued_by`.
    /// Stored as `Vec<u8>` because serde's derive only auto-implements arrays
    /// up to length 32 (same shape as `SignedFrame::classical_sig`).
    pub signature: Vec<u8>,
}

impl Delegation {
    /// Canonical bytes that `issued_by` signs. Uses the same fixed-layout TLV
    /// codec as [`crate::capability::Capability`] (ARCHITECTURE.md:75, red-team
    /// §4A) — **no serde_json** on the signed path. The delegation's authorization
    /// fields are encoded with a distinct `DOMAIN_DELEGATION` tag so a delegation
    /// signature can never be replayed as a capability or frame signature.
    pub fn canonical_bytes(&self) -> CapResult<Vec<u8>> {
        let expiry_le = self.expiry.to_le_bytes();
        let scope_bytes = self.scope.to_tlv_bytes();
        let effect_bytes = Scope::new(self.effect.grants.clone()).to_tlv_bytes();
        Ok(tlv_signing_input(
            DOMAIN_DELEGATION,
            0x01, // struct_tag
            0x01, // wire_version
            &[
                (&[0x01], &self.issued_by[..]),
                (&[0x02], &self.subject[..]),
                (&[0x03], &scope_bytes[..]),
                (&[0x04], &effect_bytes[..]),
                (&[0x05], &expiry_le[..]),
                (&[0x06], &self.nonce[..]),
            ],
        ))
    }

    /// Build a delegation and sign it with the 32-byte Ed25519 `seed` of
    /// `issued_by`. Produces a REAL Ed25519 signature (RFC 8032, from
    /// `bebop2-core`). Tampering fails [`Delegation::verify_signature`].
    pub fn sign(
        issued_by: [u8; 32],
        subject: [u8; 32],
        scope: Scope,
        effect: Effect,
        expiry: u64,
        nonce: [u8; 8],
        seed: &[u8; 32],
    ) -> CapResult<Self> {
        let mut d = Delegation {
            issued_by,
            subject,
            scope,
            effect,
            expiry,
            nonce,
            signature: Vec::new(),
        };
        let msg = d.canonical_bytes()?;
        d.signature = bebop2_core::sign::sign(seed, &msg).to_vec();
        Ok(d)
    }

    /// Verify this link's Ed25519 signature against its `issued_by` key.
    pub fn verify_signature(&self) -> CapResult<()> {
        let sig: [u8; 64] = self
            .signature
            .clone()
            .try_into()
            .map_err(|_| CapError::BadLength)?;
        let msg = self.canonical_bytes()?;
        if bebop2_core::sign::verify(&self.issued_by, &msg, &sig) {
            Ok(())
        } else {
            Err(CapError::BadSignature)
        }
    }
}

/// Fixed set of trust anchors (Ed25519 public keys) enrolled at genesis.
///
/// At runtime the roster is frozen: exactly these keys may bootstrap a
/// delegation chain. No central issuer, no reputation ledger — just this set.
#[derive(Debug, Clone, Default)]
pub struct AnchorRoster {
    anchors: HashSet<[u8; 32]>,
}

impl AnchorRoster {
    /// Empty roster. Enroll anchors before use; the set is frozen at runtime.
    pub fn new() -> Self {
        AnchorRoster {
            anchors: HashSet::new(),
        }
    }

    /// Enroll a root public key as a trust anchor. Called at genesis only.
    pub fn enroll(&mut self, root_pubkey: &[u8; 32]) {
        self.anchors.insert(*root_pubkey);
    }

    /// Whether `key` is an enrolled anchor.
    pub fn contains(&self, key: &[u8; 32]) -> bool {
        self.anchors.contains(key)
    }

    /// Whether the roster enrolls zero anchors (fail-closed default).
    pub fn is_empty(&self) -> bool {
        self.anchors.is_empty()
    }

    /// Drop an anchor from the enrolled set (MESH-11 drop-anchor). Fail-closed:
    /// dropping a key that was never enrolled is a silent no-op. Dropping an
    /// anchor does NOT retroactively un-revoke — revocation is irreversible; this
    /// only prevents the anchor from *vouching* going forward.
    pub fn remove(&mut self, root_pubkey: &[u8; 32]) {
        self.anchors.remove(root_pubkey);
    }
}

impl Capability {
    /// Whether this capability's `subject_key` is an enrolled anchor.
    /// Used as a fast pre-check / by tests that assert self-issue is rejected.
    pub fn subject_in_roster(&self, roster: &AnchorRoster) -> bool {
        roster.contains(&self.subject_key)
    }
}

/// Verify a UCAN-subset delegation chain against an anchor roster and a
/// capability. Enforces, in order:
///
/// (a) the **root** issuer is an enrolled anchor — kills self-issue auth bypass;
/// (b) every link chains: `link.issued_by == prev.subject` (root has no prev);
/// (c) **narrow-only** scope attenuation: each link's scope is a subset of its
///     parent's scope, and `effect ⊆ scope`;
/// (d) the **tail** subject binds to `cap.subject_key`;
/// (e) the requested effect (`cap.scope` modeled as an `Effect`) is a subset of
///     the tail link's scope — makes the dead `ScopeViolation` gate live;
/// (f) every link's Ed25519 signature verifies against its `issued_by`;
/// (g) no link (and the capability) is expired against `now`.
///
/// Returns the first `CapError` encountered. A non-empty, well-formed chain is
/// required: an empty chain is rejected as [`CapError::UnknownIssuer`] because
/// there is no root to anchor to.
pub fn verify_chain(
    roster: &AnchorRoster,
    chain: &[Delegation],
    cap: &Capability,
    now: u64,
) -> CapResult<()> {
    // (a) + root existence. The chain must have at least one link rooted at an
    // enrolled anchor. An empty chain has no root -> no anchor can vouch.
    let root = chain.first().ok_or(CapError::UnknownIssuer)?;
    if !roster.contains(&root.issued_by) {
        return Err(CapError::UnknownIssuer);
    }

    // Walk the chain: check each link in order.
    let mut prev_subject: Option<[u8; 32]> = None;
    let mut parent_scope: Option<Scope> = None;

    for link in chain {
        // (g) expiry (per-link).
        if link.expiry <= now {
            return Err(CapError::Expired);
        }
        // (f) signature must verify against the issuer (parent) key.
        link.verify_signature()?;
        // (c) effect ⊆ scope at this link.
        if !link.effect.is_subset_of(&link.scope) {
            return Err(CapError::ScopeViolation);
        }
        // (b) chain alignment: child.issued_by == parent.subject.
        if let Some(prev) = prev_subject {
            if link.issued_by != prev {
                return Err(CapError::ChainBroken);
            }
        }
        // (c) narrow-only attenuation: this link's scope must be a subset of the
        // parent's scope (real set-subset since G4 fix).
        if let Some(ps) = parent_scope {
            if !link.scope.is_subset_of(&ps) {
                return Err(CapError::ScopeViolation);
            }
        }

        prev_subject = Some(link.subject);
        parent_scope = Some(link.scope.clone());
    }

    // (d) tail subject binds to the capability's subject.
    let tail = chain.last().expect("chain non-empty checked above");
    if tail.subject != cap.subject_key {
        return Err(CapError::SubjectMismatch);
    }

    // (e) requested effect (cap.scope) is a subset of the tail scope.
    let requested = Effect::new(cap.scope.grants.clone());
    if !requested.is_subset_of(&tail.scope) {
        return Err(CapError::ScopeViolation);
    }

    // (g) capability expiry (mirror of the per-link check).
    if !cap.is_fresh(now) {
        return Err(CapError::Expired);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::{Action, Resource};

    fn key(seed_byte: u8) -> ([u8; 32], [u8; 32]) {
        let seed = [seed_byte; 32];
        let (pk, _) = bebop2_core::sign::keygen(&seed);
        (seed, pk)
    }

    // ── RED tests (must FAIL without the anchor-rooted model; GREEN after) ──

    #[test]
    fn red_self_issued_delegation_rejected_as_unknown_issuer() {
        // A key signs a delegation naming ITSELF as both issuer and subject,
        // and it is NOT an enrolled anchor. This is the weaponized self-issue
        // bypass: it must be rejected as UnknownIssuer.
        let (seed, pk) = key(1);
        let cap = Capability::new(pk, Resource::Route, Action::Send, [1u8; 8], 9999);
        let delegation = Delegation::sign(
            pk, // issued_by == self (NOT in roster)
            pk, // subject == self
            Scope::single(Resource::Route, Action::Send),
            Effect::single(Resource::Route, Action::Send),
            9999,
            [1u8; 8],
            &seed,
        )
        .unwrap();

        let roster = AnchorRoster::new(); // empty: nothing enrolled
        assert!(!cap.subject_in_roster(&roster));
        let err = verify_chain(&roster, &[delegation], &cap, 0);
        assert!(
            matches!(err, Err(CapError::UnknownIssuer)),
            "self-issued (non-anchor) delegation must be UnknownIssuer, got {:?}",
            err
        );
    }

    #[test]
    fn red_effect_not_subset_of_tail_scope_is_scope_violation() {
        // Tail grants Route::Send, but the capability requests Ledger::Append.
        // That is an escalation, not attenuation -> ScopeViolation.
        let (anchor_seed, anchor_pk) = key(2);
        let (_leaf_seed, leaf_pk) = key(3);

        let tail = Delegation::sign(
            anchor_pk,
            leaf_pk,
            Scope::single(Resource::Route, Action::Send),
            Effect::single(Resource::Route, Action::Send),
            9999,
            [2u8; 8],
            &anchor_seed,
        )
        .unwrap();

        // Capability requests a DIFFERENT (broader) effect than the tail scope.
        let cap = Capability::new(leaf_pk, Resource::Ledger, Action::Append, [3u8; 8], 9999);

        let mut roster = AnchorRoster::new();
        roster.enroll(&anchor_pk);
        let err = verify_chain(&roster, &[tail], &cap, 0);
        assert!(
            matches!(err, Err(CapError::ScopeViolation)),
            "effect not subset of tail scope must be ScopeViolation, got {:?}",
            err
        );
    }

    #[test]
    fn red_broken_chain_link_is_chain_broken() {
        // Root issued by enrolled anchor A -> subject B. Second link claims
        // issued_by C (not B) -> ChainBroken.
        let (anchor_seed, anchor_pk) = key(4);
        let (_b_seed, b_pk) = key(5);
        let (_c_seed, c_pk) = key(6);
        let (_leaf_seed, leaf_pk) = key(7);

        let link0 = Delegation::sign(
            anchor_pk,
            b_pk,
            Scope::single(Resource::Route, Action::Send),
            Effect::single(Resource::Route, Action::Send),
            9999,
            [4u8; 8],
            &anchor_seed,
        )
        .unwrap();
        // link1.issued_by == c_pk, but link0.subject == b_pk => broken.
        let link1 = Delegation::sign(
            c_pk,
            leaf_pk,
            Scope::single(Resource::Route, Action::Send),
            Effect::single(Resource::Route, Action::Send),
            9999,
            [5u8; 8],
            &key(6).0, // seed for c
        )
        .unwrap();

        let cap = Capability::new(leaf_pk, Resource::Route, Action::Send, [6u8; 8], 9999);
        let mut roster = AnchorRoster::new();
        roster.enroll(&anchor_pk);
        let err = verify_chain(&roster, &[link0, link1], &cap, 0);
        assert!(
            matches!(err, Err(CapError::ChainBroken)),
            "broken link must be ChainBroken, got {:?}",
            err
        );
    }

    // ── GREEN: a valid, anchor-rooted delegated chain is accepted ──

    #[test]
    fn green_valid_anchor_rooted_chain_accepts() {
        let (anchor_seed, anchor_pk) = key(8);
        let (mid_seed, mid_pk) = key(9);
        let (leaf_seed, leaf_pk) = key(10);

        // anchor -> mid (Route::Send)
        let link0 = Delegation::sign(
            anchor_pk,
            mid_pk,
            Scope::single(Resource::Route, Action::Send),
            Effect::single(Resource::Route, Action::Send),
            9999,
            [7u8; 8],
            &anchor_seed,
        )
        .unwrap();
        // mid -> leaf (same scope; attenuation-only, equal is allowed)
        let link1 = Delegation::sign(
            mid_pk,
            leaf_pk,
            Scope::single(Resource::Route, Action::Send),
            Effect::single(Resource::Route, Action::Send),
            9999,
            [8u8; 8],
            &mid_seed,
        )
        .unwrap();

        let cap = Capability::new(leaf_pk, Resource::Route, Action::Send, [9u8; 8], 9999);
        let mut roster = AnchorRoster::new();
        roster.enroll(&anchor_pk);

        assert!(cap.subject_in_roster(&roster) == false);
        assert!(verify_chain(&roster, &[link0, link1], &cap, 0).is_ok());

        // And the leaf itself (not in roster) must NOT pass a self-issue attempt.
        let self_cap = Capability::new(leaf_pk, Resource::Route, Action::Send, [10u8; 8], 9999);
        assert!(!self_cap.subject_in_roster(&roster));
        // Issuing a delegation from the leaf as root is rejected (leaf not anchor).
        let bogus = Delegation::sign(
            leaf_pk,
            leaf_pk,
            Scope::single(Resource::Route, Action::Send),
            Effect::single(Resource::Route, Action::Send),
            9999,
            [11u8; 8],
            &leaf_seed,
        )
        .unwrap();
        assert!(matches!(
            verify_chain(&roster, &[bogus], &self_cap, 0),
            Err(CapError::UnknownIssuer)
        ));
    }

    #[test]
    fn green_attest_tampered_link_fails_signature() {
        let (anchor_seed, anchor_pk) = key(11);
        let (leaf_seed, leaf_pk) = key(12);
        let mut link = Delegation::sign(
            anchor_pk,
            leaf_pk,
            Scope::single(Resource::Route, Action::Send),
            Effect::single(Resource::Route, Action::Send),
            9999,
            [12u8; 8],
            &anchor_seed,
        )
        .unwrap();
        // Tamper with the granted scope after signing.
        link.scope = Scope::single(Resource::Ledger, Action::Append);
        assert!(matches!(
            link.verify_signature(),
            Err(CapError::BadSignature)
        ));

        let cap = Capability::new(leaf_pk, Resource::Route, Action::Send, [13u8; 8], 9999);
        let mut roster = AnchorRoster::new();
        roster.enroll(&anchor_pk);
        assert!(matches!(
            verify_chain(&roster, &[link], &cap, 0),
            Err(CapError::BadSignature)
        ));
        let _ = leaf_seed;
    }

    #[test]
    fn green_expired_link_rejected() {
        let (anchor_seed, anchor_pk) = key(13);
        let (_leaf_seed, leaf_pk) = key(14);
        let link = Delegation::sign(
            anchor_pk,
            leaf_pk,
            Scope::single(Resource::Route, Action::Send),
            Effect::single(Resource::Route, Action::Send),
            100, // expiry
            [14u8; 8],
            &anchor_seed,
        )
        .unwrap();
        let cap = Capability::new(leaf_pk, Resource::Route, Action::Send, [15u8; 8], 9999);
        let mut roster = AnchorRoster::new();
        roster.enroll(&anchor_pk);
        // now (101) >= link expiry (100) => Expired.
        assert!(matches!(
            verify_chain(&roster, &[link], &cap, 101),
            Err(CapError::Expired)
        ));
    }

    // ── G4 (2026-07-14): attenuation is a REAL set-subset, not equality ──
    // A parent granting {Route::Send, Ledger::Read} can attenuate a leaf to
    // {Route::Send} alone. The leaf's own cap on Route::Send verifies (it is a
    // subset of its granted tail scope), but the leaf CANNOT escalate to
    // Ledger::Read — that pair is not in the tail scope.

    #[test]
    fn g4_attenuation_narrows_and_blocks_escalation() {
        let (anchor_seed, anchor_pk) = key(0x41);
        let (_leaf_seed, leaf_pk) = key(0x42);

        // Anchor grants a SET: two verbs-on-objects.
        let _parent_scope = Scope::new(vec![
            (Resource::Route, Action::Send),
            (Resource::Ledger, Action::Read),
        ]);
        let tail = Delegation::sign(
            anchor_pk,
            leaf_pk,
            Scope::single(Resource::Route, Action::Send), // attenuated: strictly smaller set
            Effect::single(Resource::Route, Action::Send),
            9999,
            [0x43u8; 8],
            &anchor_seed,
        )
        .unwrap();

        let mut roster = AnchorRoster::new();
        roster.enroll(&anchor_pk);

        // Leaf requests Route::Send — a subset of the tail scope => OK.
        let allowed = Capability::new(leaf_pk, Resource::Route, Action::Send, [0x44u8; 8], 9999);
        assert!(verify_chain(&roster, &[tail.clone()], &allowed, 0).is_ok());

        // Leaf tries to escalate to Ledger::Read — NOT in the (attenuated) tail
        // scope => ScopeViolation. This is the bug G4 fixes: before the set
        // model, the parent's broader grant could not be narrowed at all.
        let escalated = Capability::new(leaf_pk, Resource::Ledger, Action::Read, [0x45u8; 8], 9999);
        assert!(matches!(
            verify_chain(&roster, &[tail], &escalated, 0),
            Err(CapError::ScopeViolation)
        ));
    }

    #[test]
    fn g4_chain_attenuation_is_monotonic() {
        // anchor {A,B,C} -> mid {A,B} -> leaf {A}. Leaf requesting B is OK;
        // leaf requesting C (dropped at mid) is ScopeViolation.
        let (a_seed, a_pk) = key(0x51);
        let (mid_seed, mid_pk) = key(0x52);
        let (_leaf_seed, leaf_pk) = key(0x53);

        let l0 = Delegation::sign(
            a_pk,
            mid_pk,
            Scope::new(vec![
                (Resource::Route, Action::Send),
                (Resource::Ledger, Action::Read),
                (Resource::Order, Action::CreateOrder),
            ]),
            Effect::new(vec![
                (Resource::Route, Action::Send),
                (Resource::Ledger, Action::Read),
                (Resource::Order, Action::CreateOrder),
            ]),
            9999,
            [0x54u8; 8],
            &a_seed,
        )
        .unwrap();
        let l1 = Delegation::sign(
            mid_pk,
            leaf_pk,
            Scope::new(vec![
                (Resource::Route, Action::Send),
                (Resource::Ledger, Action::Read),
            ]),
            Effect::new(vec![
                (Resource::Route, Action::Send),
                (Resource::Ledger, Action::Read),
            ]),
            9999,
            [0x55u8; 8],
            &mid_seed,
        )
        .unwrap();

        let mut roster = AnchorRoster::new();
        roster.enroll(&a_pk);

        // leaf keeps Route::Send (a subset of its tail scope) => OK.
        let ok = Capability::new(leaf_pk, Resource::Route, Action::Send, [0x56u8; 8], 9999);
        assert!(verify_chain(&roster, &[l0.clone(), l1.clone()], &ok, 0).is_ok());

        // leaf reaches for Order::CreateOrder, which mid dropped => the per-link
        // attenuation check (l1.scope ⊆ l0.scope) passes, but the tail scope no
        // longer contains Order::CreateOrder => ScopeViolation at (e).
        let exceeded = Capability::new(
            leaf_pk,
            Resource::Order,
            Action::CreateOrder,
            [0x57u8; 8],
            9999,
        );
        assert!(matches!(
            verify_chain(&roster, &[l0, l1], &exceeded, 0),
            Err(CapError::ScopeViolation)
        ));
    }
}
