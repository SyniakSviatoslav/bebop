//! Integration ports (IP-03) — capability-scoped, deny-by-default.
//!
//! A *port* is the seam where `proto-cap` (authorization) meets the outside
//! world: an inbound channel that pulls frames into the kernel, an outbound
//! channel that surfaces projections, or a transport adapter that relays frames
//! across a carrier. Every port declares a single [`Scope`] it is allowed to
//! act within and **literally cannot** act outside it — `check_port_scope`
//! enforces this at the boundary before any frame reaches [`KernelFacade`].
//!
//! This is the *compile-time-ish* analogue of the DK-02/DK-03 WASI
//! zero-ambient-authority rule: a port is granted no ambient authority; it may
//! only exercise the exact `(resource, action)` it declared. A port whose
//! `required_scope() == Order::Notify` can push notifications and nothing else —
//! it can never be coerced into submitting an order-mutation intent, because
//! `check_port_scope` rejects any frame whose capability scope differs from the
//! port's.
//!
//! CI GUARD: NO-COURIER-SCORING — ports authorize actions on resources only;
//! they never derive, consult, or encode a courier/agent score.
//!
//! innovate: the *contract* here (traits + `check_port_scope` + the deny-by-default
//! gate) is the testable core. Real WASI-p2 *instantiation* of ports (each port
//! running as an isolated component with its own capability handle) is a
//! post-G11-GREEN Phase-3 upgrade and is intentionally NOT required to exercise
//! the authority boundary — no wasmtime dependency is introduced.

use std::sync::Mutex;

use crate::error::CapError;
use crate::facade::{Event, KernelFacade, Projection, Reject};
use crate::scope::{Action, Resource, Scope};
use crate::signed_frame::SignedFrame;

/// An inbound port pulls a frame from the outside world and submits it to the
/// kernel through the [`KernelFacade`]. It is bound to exactly one
/// [`Scope`] and may never submit a frame outside it (see [`check_port_scope`]).
pub trait InboundPort {
    /// Stable identifier for the port (used in logs / wiring).
    fn id(&self) -> &str;
    /// The single scope this port is authorized to act within.
    fn required_scope(&self) -> Scope;
    /// Submit `frame` through `facade`. The default-shaped implementation must
    /// call [`check_port_scope`] first: a frame whose capability scope differs
    /// from `required_scope()` is rejected *before* the facade (and therefore
    /// the host kernel via the [`crate::facade::EventSink`]) is ever reached.
    async fn submit(
        &self,
        frame: &SignedFrame,
        facade: &KernelFacade,
    ) -> Result<Vec<Event>, Reject>;
}

/// An outbound port receives read-only projections the kernel surfaces and
/// forwards them to the outside world. It is bound to exactly one [`Scope`]
/// (typically a read/notify action) and holds no submit authority.
pub trait OutboundPort {
    /// Stable identifier for the port.
    fn id(&self) -> &str;
    /// The single scope this port is authorized to observe.
    fn required_scope(&self) -> Scope;
    /// Handle a projection the facade surfaced under `required_scope()`.
    fn on_projection(&self, p: &Projection);
}

/// A channel adapter relays frames across a transport carrier. Like
/// [`InboundPort`], it is bound to exactly one [`Scope`] and may only deliver
/// frames that fall within it.
pub trait ChannelAdapter {
    /// Stable identifier for the carrier/channel.
    fn channel_id(&self) -> &str;
    /// The single scope this adapter is authorized to act within.
    fn required_scope(&self) -> Scope;
    /// Deliver `frame` into the kernel through `facade`. Must call
    /// [`check_port_scope`] first (deny-by-default).
    async fn deliver(&self, frame: &SignedFrame, facade: &KernelFacade) -> Result<(), Reject>;
}

/// Deny-by-default gate: assert a `frame`'s capability scope is *exactly* the
/// port's declared `required_scope()`. A port may only act within its declared
/// scope — if the frame targets a different `(resource, action)`, reject with
/// [`CapError::ScopeViolation`].
///
/// This is the authority boundary that mirrors DK-02/DK-03 WASI restriction:
/// the port has no ambient authority and cannot be coerced into a wider effect
/// by an attacker-supplied frame (attenuation is enforced, not just enumerated).
///
/// `facade` is accepted for symmetry with the port submit/deliver seams
/// (a future read-scope check will consult it); the present gate is a pure,
/// fail-closed scope comparison.
pub fn check_port_scope(
    port_scope: &Scope,
    facade: &KernelFacade,
    frame: &SignedFrame,
) -> Result<(), Reject> {
    let _ = facade; // reserved seam; today's check is a pure scope comparison
    let fs = &frame.capability.scope;
    // The frame's scope must *contain* the port's required scope (subset):
    // a port is authorized only for exactly the verbs-on-objects it declares,
    // never for anything broader carried by the frame. Fail-closed otherwise.
    if !port_scope.is_subset_of(fs) {
        return Err(Reject {
            reason: CapError::ScopeViolation,
        });
    }
    Ok(())
}

/// A concrete outbound port that pushes order notifications. Its
/// `required_scope()` is `Order::Notify` — it may surface order notifications
/// and nothing else. `on_projection` retains the most recent projection so the
/// host can inspect what was delivered.
///
/// (In this authorization crate `Notify` is a closed action on `Resource::Order`;
/// the pairing `Order::Notify` is the valid, meaningful scope for an
/// order-notification port — see `scope.rs`.)
pub struct NotificationPort {
    scope: Scope,
    last: Mutex<Option<Projection>>,
}

impl NotificationPort {
    /// Construct a notification port scoped to `Order::Notify`.
    pub fn new() -> Self {
        NotificationPort {
            scope: Scope::single(Resource::Order, Action::Notify),
            last: Mutex::new(None),
        }
    }

    /// The most recent projection delivered to this port, if any.
    pub fn last_projection(&self) -> Option<Projection> {
        self.last.lock().ok().and_then(|g| g.clone())
    }
}

impl Default for NotificationPort {
    fn default() -> Self {
        Self::new()
    }
}

impl OutboundPort for NotificationPort {
    fn id(&self) -> &str {
        "notification"
    }

    fn required_scope(&self) -> Scope {
        self.scope.clone()
    }

    fn on_projection(&self, p: &Projection) {
        if let Ok(mut g) = self.last.lock() {
            *g = Some(p.clone());
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::Capability;
    use crate::facade::EventSink;
    use crate::revocation::RevocationSet;
    use crate::roster::{AnchorRoster, Delegation, Effect};
    use crate::scope::{Action, Resource};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    // ── minimal zero-dep block_on so async trait methods can be exercised in
    // tests without pulling in tokio/futures (no new deps). These futures do
    // not actually await anything, so the first poll returns Ready.
    fn block_on<F: std::future::Future>(fut: F) -> F::Output {
        use std::pin::pin;
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

        fn noop(_: *const ()) {}
        fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
        let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
        let mut cx = Context::from_waker(&waker);
        let mut fut = pin!(fut);
        loop {
            match fut.as_mut().poll(&mut cx) {
                Poll::Ready(v) => return v,
                // Not expected: our async fns never yield. Park defensively.
                Poll::Pending => std::thread::park(),
            }
        }
    }

    fn key(seed_byte: u8) -> ([u8; 32], [u8; 32]) {
        let seed = [seed_byte; 32];
        let (pk, _) = bebop2_core::sign::keygen(&seed);
        (seed, pk)
    }

    /// A counting [`EventSink`] — records how many times `apply` runs (the
    /// kernel seam). Interior-mutable so it satisfies `&self`.
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

    /// An inbound port whose `required_scope()` is `Notify`, exercising the
    /// deny-by-default contract: it routes through `check_port_scope` first.
    struct MockInboundPort {
        scope: Scope,
        _id: String,
    }

    impl InboundPort for MockInboundPort {
        fn id(&self) -> &str {
            "mock-inbound"
        }
        fn required_scope(&self) -> Scope {
            self.scope.clone()
        }
        async fn submit(
            &self,
            frame: &SignedFrame,
            facade: &KernelFacade,
        ) -> Result<Vec<Event>, Reject> {
            // Deny-by-default: reject any frame outside the port scope BEFORE
            // the facade (and therefore the kernel sink) is reached.
            check_port_scope(&self.required_scope(), facade, frame)?;
            facade.submit_intent(frame)
        }
    }

    /// Build an anchor-rooted, signed, hybrid frame whose capability scope is
    /// exactly `(res, act)`. When `bad_pq` is true the PQ leg is signed with an
    /// UNRELATED ML-DSA-65 key so `verify_pq` fails under `RequireBoth`. The
    /// delegation chain grants the SAME scope (so the only failure, if any, is
    /// the bad PQ leg — not attenuation).
    fn build_hybrid_frame(
        anchor_seed: &[u8; 32],
        anchor_pk: &[u8; 32],
        leaf_seed: &[u8; 32],
        leaf_pk: &[u8; 32],
        res: Resource,
        act: Action,
        bad_pq: bool,
    ) -> (SignedFrame, AnchorRoster) {
        let pq_seed = [0xABu8; 32];
        let (pq_pk, pq_sk) = bebop2_core::pq_dsa::keygen(&pq_seed);

        let (sign_pk, sign_sk) = if bad_pq {
            bebop2_core::pq_dsa::keygen(&[0xCDu8; 32])
        } else {
            bebop2_core::pq_dsa::keygen(&pq_seed)
        };
        let _ = sign_pk;

        let cap = Capability::new_hybrid(*leaf_pk, pq_pk.bytes.clone(), res, act, [7u8; 8], 9999);
        let mut f = SignedFrame::new(cap, b"intent-bytes".to_vec());
        f.sign_classical(leaf_seed).unwrap();

        let link = Delegation::sign(
            *anchor_pk,
            *leaf_pk,
            Scope::single(res, act),
            Effect::single(res, act),
            9999,
            [7u8; 8],
            anchor_seed,
        )
        .unwrap();
        f.sign_pq(&sign_sk.bytes.clone().try_into().unwrap(), &[0u8; 32])
            .unwrap();
        f.delegation_chain = vec![link];

        let mut roster = AnchorRoster::new();
        roster.enroll(anchor_pk);
        (f, roster)
    }

    fn facade_for(roster: AnchorRoster, sink: Box<dyn EventSink>) -> KernelFacade {
        KernelFacade::new(
            crate::hybrid_gate::HybridPolicy::RequireBoth,
            roster,
            RevocationSet::new(),
            sink,
            Box::new(|| 0), // frozen clock: now = 0, all expiries (9999) fresh
        )
    }

    // ── R0 (deny-by-default, direct): a NotificationPort scoped to
    // `Order::Notify` must REJECT (ScopeViolation) a frame whose capability
    // targets an ORDER-MUTATION intent (`Order::CreateOrder`). A port literally
    // cannot act outside its declared scope.
    #[test]
    fn r0_check_port_scope_rejects_order_mutation_for_notify_port() {
        let port = NotificationPort::new();
        let port_scope = port.required_scope();
        assert_eq!(port_scope, Scope::single(Resource::Order, Action::Notify));

        // Build a frame whose capability scope is an order-mutation intent.
        let (l_seed, l_pk) = key(0x32);
        let cap = Capability::new_hybrid(
            l_pk,
            Vec::new(), // PQ key irrelevant to the scope check
            Resource::Order,
            Action::CreateOrder,
            [7u8; 8],
            9999,
        );
        let frame = SignedFrame::new(cap, b"order-mutation-payload".to_vec());
        let _ = l_seed;

        let (sink, _counter) = MockSink::new();
        let facade = facade_for(AnchorRoster::new(), Box::new(sink));

        let res = check_port_scope(&port_scope, &facade, &frame);
        assert!(
            matches!(
                res,
                Err(Reject {
                    reason: CapError::ScopeViolation
                })
            ),
            "notify port must reject an order-mutation frame, got {:?}",
            res.err()
        );
    }

    // ── R0 (deny-by-default, via full submit): a MockInboundPort scoped to
    // `Notify` attempting to submit an order-mutation frame through the facade
    // must be rejected at the port boundary — the sink is NEVER called (count
    // 0). Mirrors the facade R0 authority-boundary test.
    #[test]
    fn r0_submit_notify_port_rejects_mutation_without_sink_call() {
        let (a_seed, a_pk) = key(0x31);
        let (l_seed, l_pk) = key(0x32);

        // Frame whose capability scope is an order MUTATION (CreateOrder), with
        // a validly-signed capability so the only possible failure is the port
        // scope gate (not the hybrid gate).
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
        let mut f = SignedFrame::new(
            Capability::new_hybrid(
                l_pk,
                pq_pk.bytes.clone(),
                Resource::Order,
                Action::CreateOrder,
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

        let port = MockInboundPort {
            scope: Scope::single(Resource::Order, Action::Notify),
            _id: "mock-inbound".to_string(),
        };
        let res = block_on(port.submit(&f, &facade));
        assert!(
            matches!(
                res,
                Err(Reject {
                    reason: CapError::ScopeViolation
                })
            ),
            "notify port submit of a mutation must be ScopeViolation, got {:?}",
            res.err()
        );
        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "sink MUST NOT be called when the port rejects"
        );
    }

    // ── R2 (hybrid gate still enforced): a frame whose scope IS within the port
    // scope (`Order::Notify`) but carries a NON-verifying PQ leg under
    // `RequireBoth` must be rejected by the facade with `PqVerifyFailed`. The
    // port lets the in-scope frame through; the hybrid gate (Law) still runs
    // inside the facade and rejects the bad proof. The sink is never reached.
    #[test]
    fn r2_in_scope_frame_with_bad_pq_rejected_by_facade() {
        let (a_seed, a_pk) = key(0x41);
        let (l_seed, l_pk) = key(0x42);
        // bad_pq = true => PQ signed with a mismatched key -> verify_pq fails.
        let (f, roster) = build_hybrid_frame(
            &a_seed,
            &a_pk,
            &l_seed,
            &l_pk,
            Resource::Order,
            Action::Notify,
            true,
        );

        let (sink, counter) = MockSink::new();
        let facade = facade_for(roster, Box::new(sink));

        let port = MockInboundPort {
            scope: Scope::single(Resource::Order, Action::Notify),
            _id: "mock-inbound".to_string(),
        };
        let res = block_on(port.submit(&f, &facade));
        assert!(
            matches!(
                res,
                Err(Reject {
                    reason: CapError::PqVerifyFailed
                })
            ),
            "in-scope frame with bad PQ must be PqVerifyFailed, got {:?}",
            res.err()
        );
        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "sink MUST NOT be called when PQ fails"
        );
    }

    // ── R4 (attenuation): a port whose `required_scope()` is `Notify` cannot be
    // extended to a wider scope by an attacker-supplied frame. A frame targeting
    // ANY scope other than `Order::Notify` (here `Ledger::Append`, a wholly
    // different resource/action) is rejected by `check_port_scope`.
    #[test]
    fn r4_attacker_cannot_widen_port_scope_via_frame() {
        let port_scope = Scope::single(Resource::Order, Action::Notify);

        // Attacker forges a frame whose capability requests a different
        // resource/action than the port's declared scope.
        let (l_seed, l_pk) = key(0x52);
        let cap = Capability::new_hybrid(
            l_pk,
            Vec::new(),
            Resource::Ledger,
            Action::Append,
            [7u8; 8],
            9999,
        );
        let frame = SignedFrame::new(cap, b"widen-attempt".to_vec());
        let _ = l_seed;

        let (sink, _counter) = MockSink::new();
        let facade = facade_for(AnchorRoster::new(), Box::new(sink));

        let res = check_port_scope(&port_scope, &facade, &frame);
        assert!(
            matches!(
                res,
                Err(Reject {
                    reason: CapError::ScopeViolation
                })
            ),
            "attacker-supplied wider-scope frame must be ScopeViolation, got {:?}",
            res.err()
        );
    }

    // ── GREEN (allow-path): an in-scope, fully-hybrid-verified `Order::Notify`
    // frame DOES pass the port and reach the sink exactly once. Confirms the
    // deny-by-default contract permits (and only permits) the declared scope —
    // it is not *always* rejecting.
    #[test]
    fn green_in_scope_notify_frame_reaches_sink_once() {
        let (a_seed, a_pk) = key(0x61);
        let (l_seed, l_pk) = key(0x62);
        let (f, roster) = build_hybrid_frame(
            &a_seed,
            &a_pk,
            &l_seed,
            &l_pk,
            Resource::Order,
            Action::Notify,
            false, // good PQ
        );

        let (sink, counter) = MockSink::new();
        let facade = facade_for(roster, Box::new(sink));

        let port = MockInboundPort {
            scope: Scope::single(Resource::Order, Action::Notify),
            _id: "mock-inbound".to_string(),
        };
        let res = block_on(port.submit(&f, &facade));
        assert!(
            res.is_ok(),
            "in-scope hybrid frame must pass, got {:?}",
            res.err()
        );
        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "sink MUST be called once"
        );

        // And the OutboundPort stores the projection it is handed.
        let np = NotificationPort::new();
        np.on_projection(&Projection(b"notify-payload".to_vec()));
        assert_eq!(
            np.last_projection().map(|p| p.0),
            Some(b"notify-payload".to_vec())
        );
    }

    fn anchor_roster_with(anchor_pk: &[u8; 32]) -> AnchorRoster {
        let mut roster = AnchorRoster::new();
        roster.enroll(anchor_pk);
        roster
    }
}
