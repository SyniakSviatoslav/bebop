//! KernelFacade — anti-corruption layer (IP-01 / IP-04).
//!
//! The facade is the *only* boundary `proto-cap` exposes to the host kernel. It
//! runs the hybrid gate (authorization / Law) and, **only** on a clean pass,
//! delegates the effect to a host-supplied [`EventSink`]. It contains **no**
//! `decide` / `money` / `fold` logic — those live in the host kernel, which is
//! **not** reachable from this crate. That is the compile-firewall: `proto-cap`
//! can *verify*, but it can never itself *exercise* the kernel's money
//! semantics. The ordering is explicit — **wire → Law → money**:
//!
//! 1. `HybridGate::check` (the Law gate) runs FIRST and must pass.
//! 2. On failure the kernel is never touched (the sink is never called).
//! 3. On success the sink applies the event and returns the produced events.
//!
//! CI GUARD: NO-COURIER-SCORING — the facade gates on signature + scope only;
//! it never derives, consults, or encodes a courier/agent score.

use crate::error::CapError;
use crate::hybrid_gate::{HybridGate, HybridPolicy};
use crate::revocation::RevocationSet;
use crate::roster::AnchorRoster;
use crate::scope::Scope;
use crate::signed_frame::SignedFrame;

/// A host-kernel event produced by applying an intent. Minimal and opaque — the
/// host kernel interprets events; `proto-cap` only carries them. No score.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// Opaque event id assigned by the host kernel.
    pub id: u64,
    /// Opaque event payload (the kernel's own encoding).
    pub payload: Vec<u8>,
}

/// Rejection returned by the facade when the gate (or a read-scope check)
/// refuses an operation. Wraps the underlying authorization fault; the facade
/// never invents a reason of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reject {
    /// The underlying authorization fault that caused the rejection.
    pub reason: CapError,
}

/// A read-only projection the facade is allowed to surface. Minimal newtype so
/// the host kernel cannot mistake an opaque byte bag for a live mutation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Projection(pub Vec<u8>);

/// Host-supplied sink that *applies* a verified intent and returns the events
/// the kernel produced. The facade calls this **only** after the hybrid gate
/// passes — it is the single seam through which the host kernel is reached.
///
/// The facade never inspects or modifies the returned events; it is pure
/// pass-through. All `decide`/`money`/`fold` semantics live behind this trait,
/// in the host kernel.
pub trait EventSink {
    /// Apply a gate-verified frame and return the resulting events.
    fn apply(&self, frame: &SignedFrame) -> Vec<Event>;
}

/// The anti-corruption layer between `proto-cap` (authorization) and the host
/// kernel (money semantics). Owns the gate, the anchor roster, the host sink,
/// and a monotonic clock. Exposes **exactly two** public methods:
/// [`KernelFacade::submit_intent`] and [`KernelFacade::read_projection`].
pub struct KernelFacade {
    /// The hybrid authorization gate (Law). Runs before any kernel touch.
    gate: HybridGate,
    /// Enrolled trust anchors (root of trust for delegation chains).
    roster: AnchorRoster,
    /// UCAN-style invalidation set (MESH-11); folded into the gate check so a
    /// revoked key/capability is refused even with valid signatures.
    revocations: RevocationSet,
    /// Host-supplied application sink (the only kernel seam).
    sink: Box<dyn EventSink>,
    /// Monotonic clock used for the gate's expiry check.
    clock: Box<dyn Fn() -> u64>,
    /// The set of read-only scopes this facade is permitted to surface via
    /// [`KernelFacade::read_projection`]. Anything else is rejected.
    allowed_reads: Vec<Scope>,
}

impl KernelFacade {
    /// Build the facade. `policy` is the hybrid gate policy (e.g.
    /// [`HybridPolicy::RequireBoth`]); `roster` is the enrolled anchor set;
    /// `revocations` is the MESH-11 invalidation set (pass
    /// [`RevocationSet::new`] if none yet); `sink` is the host-kernel
    /// application seam; `clock` supplies the current monotonic tick for expiry
    /// checks.
    pub fn new(
        policy: HybridPolicy,
        roster: AnchorRoster,
        revocations: RevocationSet,
        sink: Box<dyn EventSink>,
        clock: Box<dyn Fn() -> u64>,
    ) -> Self {
        KernelFacade {
            gate: HybridGate::new(policy),
            roster,
            revocations,
            sink,
            clock,
            allowed_reads: Vec::new(),
        }
    }

    /// Configure the read-only scopes the facade may surface via
    /// [`KernelFacade::read_projection`]. Defaults to empty (everything
    /// rejected) until configured by the host.
    pub fn with_allowed_reads(mut self, reads: Vec<Scope>) -> Self {
        self.allowed_reads = reads;
        self
    }

    /// Submit a signed intent. **Wire → Law → money ordering:**
    ///
    /// 1. Run the hybrid gate first. On any authorization fault, return
    ///    `Err(Reject { reason })` and **never** call the sink — the kernel is
    ///    not touched (this is the authority boundary: no valid gate, no money
    ///    effect).
    /// 2. On a clean pass, delegate to the host-kernel [`EventSink`] and return
    ///    the produced events. The facade contains no `decide`/`money`/`fold`
    ///    logic; it only verifies then delegates.
    pub fn submit_intent(&self, frame: &SignedFrame) -> Result<Vec<Event>, Reject> {
        let now = (self.clock)();
        self.gate
            .check(
                frame,
                &self.roster,
                &frame.delegation_chain,
                &self.revocations,
                now,
            )
            .map_err(|e| Reject { reason: e })?;
        // Gate passed: the kernel is reached ONLY through the sink seam.
        Ok(self.sink.apply(frame))
    }

    /// Return a read-only projection for `scope`. The scope must be one of the
    /// facade's configured allowed-read scopes; otherwise it is rejected as a
    /// [`CapError::ScopeViolation`]. The facade holds no kernel state, so the
    /// projection is a minimal placeholder bag — real reads are served by the
    /// host kernel through its own projection API.
    pub fn read_projection(&self, scope: Scope) -> Result<Projection, Reject> {
        if !self.allowed_reads.contains(&scope) {
            return Err(Reject {
                reason: CapError::ScopeViolation,
            });
        }
        Ok(Projection(Vec::new()))
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::Capability;
    use crate::revocation::RevocationSet;
    use crate::roster::{Delegation, Effect};
    use crate::scope::{Action, Resource};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    fn key(seed_byte: u8) -> ([u8; 32], [u8; 32]) {
        let seed = [seed_byte; 32];
        let (pk, _) = bebop2_core::sign::keygen(&seed);
        (seed, pk)
    }

    /// Counts how many times `apply` is invoked, via a shared `Arc<AtomicU64>`
    /// so the test can read the count AFTER the sink has been moved into the
    /// facade. Interior-mutable so it satisfies the `&self` `EventSink` contract.
    struct MockSink {
        calls: Arc<AtomicU64>,
    }

    impl MockSink {
        fn new() -> (Self, Arc<AtomicU64>) {
            let counter = Arc::new(AtomicU64::new(0));
            (
                MockSink {
                    calls: counter.clone(),
                },
                counter,
            )
        }
    }

    impl EventSink for MockSink {
        fn apply(&self, _frame: &SignedFrame) -> Vec<Event> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            vec![Event {
                id: 1,
                payload: b"applied".to_vec(),
            }]
        }
    }

    /// Build an anchor-rooted, signed, hybrid frame whose delegation chain
    /// grants exactly `(res, act)` to a leaf, with a capability carrying the
    /// SAME scope. When `bad_pq` is true, the PQ leg is signed with an
    /// UNRELATED ML-DSA-65 secret key whose public key does NOT match the
    /// capability's `subject_key_pq`, so `verify_pq` fails under `RequireBoth`.
    fn build_frame(
        anchor_seed: &[u8; 32],
        anchor_pk: &[u8; 32],
        leaf_seed: &[u8; 32],
        leaf_pk: &[u8; 32],
        cap_resource: Resource,
        cap_action: Action,
        bad_pq: bool,
    ) -> (SignedFrame, AnchorRoster, Vec<Delegation>) {
        // Real ML-DSA-65 keypair for the leaf's PQ identity.
        let pq_seed = [0xABu8; 32];
        let (pq_pk, pq_sk) = bebop2_core::pq_dsa::keygen(&pq_seed);

        // For a non-verifying PQ leg, sign with a second, unrelated PQ keypair.
        // (The structs are not Clone; derive fresh keypairs per branch.)
        let (sign_pk, sign_sk) = if bad_pq {
            bebop2_core::pq_dsa::keygen(&[0xCDu8; 32])
        } else {
            bebop2_core::pq_dsa::keygen(&pq_seed)
        };

        let cap = Capability::new_hybrid(
            *leaf_pk,
            pq_pk.bytes.clone(),
            cap_resource,
            cap_action,
            [7u8; 8],
            9999,
        );
        let mut f = SignedFrame::new(cap, b"intent-bytes".to_vec());
        f.sign_classical(leaf_seed).unwrap();
        let link = Delegation::sign(
            *anchor_pk,
            *leaf_pk,
            Scope::single(cap_resource, cap_action),
            Effect::single(cap_resource, cap_action),
            9999,
            [7u8; 8],
            anchor_seed,
        )
        .unwrap();
        f.sign_pq(&sign_sk.bytes.clone().try_into().unwrap(), &[0u8; 32])
            .unwrap();
        let _ = sign_pk;
        let mut roster = AnchorRoster::new();
        roster.enroll(anchor_pk);
        (f, roster, vec![link])
    }

    /// Build a frame whose capability scope differs from the granted subtree.
    fn build_frame_with_cap_scope(
        anchor_seed: &[u8; 32],
        anchor_pk: &[u8; 32],
        leaf_seed: &[u8; 32],
        leaf_pk: &[u8; 32],
        granted_resource: Resource,
        granted_action: Action,
        cap_resource: Resource,
        cap_action: Action,
    ) -> (SignedFrame, AnchorRoster, Vec<Delegation>) {
        let pq_seed = [0xABu8; 32];
        let (pq_pk, pq_sk) = bebop2_core::pq_dsa::keygen(&pq_seed);
        let link = Delegation::sign(
            *anchor_pk,
            *leaf_pk,
            Scope::single(granted_resource, granted_action),
            Effect::single(granted_resource, granted_action),
            9999,
            [7u8; 8],
            anchor_seed,
        )
        .unwrap();
        let mut f = SignedFrame::new(
            Capability::new_hybrid(
                *leaf_pk,
                pq_pk.bytes.clone(),
                cap_resource,
                cap_action,
                [7u8; 8],
                9999,
            ),
            b"intent".to_vec(),
        );
        f.delegation_chain = vec![link];
        f.sign_classical(leaf_seed).unwrap();
        f.sign_pq(&pq_sk.bytes.clone().try_into().unwrap(), &[0u8; 32])
            .unwrap();
        let mut roster = AnchorRoster::new();
        roster.enroll(anchor_pk);
        (f, roster, vec![])
    }

    fn facade_for(roster: AnchorRoster, sink: Box<dyn EventSink>) -> KernelFacade {
        KernelFacade::new(
            HybridPolicy::RequireBoth,
            roster,
            RevocationSet::new(),
            sink,
            Box::new(|| 0), // frozen clock: now = 0, all expiries (9999) fresh
        )
    }

    // ── R0 (authority boundary): a capability scoped to a READ-ONLY action
    // (`Order::Notify`) must NOT be able to drive an order-mutation intent. The
    // delegation chain grants a WRITE subtree (`Order::CreateOrder`), but the
    // frame's capability scope (`Order::Notify`) is NOT within that subtree, so
    // the gate rejects (scope attenuation). Critically, the sink is NEVER called
    // — assert call count 0.
    #[test]
    fn r0_read_only_notify_cannot_submit_order_mutation() {
        let (a_seed, a_pk) = key(0x31);
        let (l_seed, l_pk) = key(0x32);

        // Chain grants a WRITE subtree (Order::CreateOrder) to the leaf.
        let pq_seed = [0xABu8; 32];
        let (pq_pk, pq_sk) = bebop2_core::pq_dsa::keygen(&pq_seed);
        let link = Delegation::sign(
            a_pk,
            l_pk,
            Scope::single(Resource::Order, Action::CreateOrder),
            Effect::single(Resource::Order, Action::CreateOrder),
            9999,
            [7u8; 8],
            &a_seed,
        )
        .unwrap();

        // The frame's capability is READ-ONLY (Order::Notify) — outside the
        // granted Order::CreateOrder subtree.
        let mut f = SignedFrame::new(
            Capability::new_hybrid(
                l_pk,
                pq_pk.bytes.clone(),
                Resource::Order,
                Action::Notify,
                [7u8; 8],
                9999,
            ),
            b"order-mutation-payload".to_vec(),
        );
        f.delegation_chain = vec![link];
        f.sign_classical(&l_seed).unwrap();
        f.sign_pq(&pq_sk.bytes.clone().try_into().unwrap(), &[0u8; 32])
            .unwrap();

        let (sink, counter) = MockSink::new();
        let facade = facade_for(anchor_roster_with(&a_pk), Box::new(sink));
        let res = facade.submit_intent(&f);
        assert!(
            res.is_err(),
            "read-only Notify cap must be rejected for an order mutation"
        );
        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "sink MUST NOT be called when the gate rejects"
        );
    }

    // ── R2 (hybrid gate): a frame with a VALID classical sig but a
    // NON-verifying ML-DSA-65 leg under `RequireBoth` must be rejected. The
    // sink is never reached, evidencing the gate runs BEFORE delegation.
    #[test]
    fn r2_valid_classical_bad_pq_rejected_under_require_both() {
        let (a_seed, a_pk) = key(0x41);
        let (l_seed, l_pk) = key(0x42);
        // bad_pq = true => PQ signed with a mismatched key -> verify_pq fails.
        let (f, roster, chain) = build_frame(
            &a_seed,
            &a_pk,
            &l_seed,
            &l_pk,
            Resource::Order,
            Action::CreateOrder,
            true,
        );
        let mut f = f;
        f.delegation_chain = chain;

        let (sink, counter) = MockSink::new();
        let facade = facade_for(roster, Box::new(sink));
        let res = facade.submit_intent(&f);
        assert!(
            matches!(
                res,
                Err(Reject {
                    reason: CapError::PqVerifyFailed
                })
            ),
            "RequireBoth with bad PQ must be PqVerifyFailed, got {:?}",
            res.err()
        );
        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "sink MUST NOT be called when PQ fails"
        );
    }

    // ── R4 (attenuation): a capability requesting a Resource/Action outside its
    // granted subtree must be rejected by the facade (it never reaches the sink,
    // and the rejection is specifically ScopeViolation).
    #[test]
    fn r4_attenuated_capability_outside_subtree_rejected_by_facade() {
        let (a_seed, a_pk) = key(0x51);
        let (l_seed, l_pk) = key(0x52);

        // Anchor grants a NARROW subtree: Order::CreateOrder.
        // Capability requests a DIFFERENT resource/action (Ledger::Append) that
        // is outside the granted subtree. Classical + PQ sigs are VALID, so the
        // rejection comes specifically from the scope-attentuation check.
        let (f, roster, _chain) = build_frame_with_cap_scope(
            &a_seed,
            &a_pk,
            &l_seed,
            &l_pk,
            Resource::Order,
            Action::CreateOrder,
            Resource::Ledger,
            Action::Append,
        );

        let (sink, counter) = MockSink::new();
        let facade = facade_for(roster, Box::new(sink));
        let res = facade.submit_intent(&f);
        assert!(
            matches!(
                res,
                Err(Reject {
                    reason: CapError::ScopeViolation
                })
            ),
            "attenuated cap outside subtree must be ScopeViolation, got {:?}",
            res.err()
        );
        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "sink MUST NOT be called when scope is out of subtree"
        );
    }

    // ── GREEN sanity: a properly authorized, hybrid-verified frame DOES reach
    // the sink exactly once and returns the produced events. (Confirms the
    // facade is not *always* rejecting, and that the sink seam works.)
    #[test]
    fn green_authorized_frame_reaches_sink_once() {
        let (a_seed, a_pk) = key(0x61);
        let (l_seed, l_pk) = key(0x62);
        let (f, roster, chain) = build_frame(
            &a_seed,
            &a_pk,
            &l_seed,
            &l_pk,
            Resource::Order,
            Action::CreateOrder,
            false, // good PQ
        );
        let mut f = f;
        f.delegation_chain = chain;

        let (sink, counter) = MockSink::new();
        let facade = facade_for(roster, Box::new(sink));
        let res = facade.submit_intent(&f);
        assert!(
            res.is_ok(),
            "authorized hybrid frame must pass: {:?}",
            res.err()
        );
        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "sink MUST be called once"
        );
        let events = res.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].payload, b"applied");
    }

    // ── read_projection: configured read scopes pass; everything else rejected.
    #[test]
    fn read_projection_rejects_unconfigured_scope() {
        let mut roster = AnchorRoster::new();
        roster.enroll(&[0u8; 32]);
        let (sink, _counter) = MockSink::new();
        let facade = facade_for(roster, Box::new(sink))
            .with_allowed_reads(vec![Scope::single(Resource::Order, Action::ReadProjection)]);
        // Configured read scope -> Ok.
        assert!(facade
            .read_projection(Scope::single(Resource::Order, Action::ReadProjection))
            .is_ok());
        // Anything else -> Reject (ScopeViolation).
        assert!(matches!(
            facade.read_projection(Scope::single(Resource::Ledger, Action::Read)),
            Err(Reject {
                reason: CapError::ScopeViolation
            })
        ));
    }

    /// Helper: an `AnchorRoster` enrolled with `anchor_pk`.
    fn anchor_roster_with(anchor_pk: &[u8; 32]) -> AnchorRoster {
        let mut roster = AnchorRoster::new();
        roster.enroll(anchor_pk);
        roster
    }
}
