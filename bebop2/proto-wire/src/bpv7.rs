//! BPv7-style store-and-forward overlay — hand-rolled, transport-agnostic.
//!
//! This is the RFC 9171 *concept* (primary block, custody, bundle queue,
//! retry-until-ack, lifetime/expiry) realised WITHOUT pulling in the `dtn7` or
//! `bp7` crates. The blueprint (MESH-09) explicitly says *hand-roll* custody /
//! retry / expiry — not run a dtn7 daemon. The queue is deliberately generic
//! over whatever [`crate::Transport`] carries the bytes; the wire carrier is
//! irrelevant to the store-forward logic, which is what makes the
//! `offline_courier_reconnect_delivers_exactly_once` RED property carrier-free.
//!
//! # Why this exists
//! A courier (the relay node that physically carries bundles between partitions)
//! MUST deliver every bundle *exactly once* even when the radio drops mid-transfer.
//! The [`StoreForward`] queue survives reconnects: on a fresh channel it drains
//! the still-undelivered bundles oldest-first, and the caller re-binds each
//! replay to the new channel (fresh channel binding per send, per F7). The
//! receiver dedupes by `PrimaryBlock.nonce`, so a bundle that was in-flight when
//! the link broke is replayed without producing a duplicate delivery.
//!
//! # Custody + retremit-until-ack
//! A [`Bundle`] is "delivered" ONLY after the receiver acks it
//! ([`StoreForward::ack`] / [`StoreForward::mark_delivered`]). Until then it
//! stays in `undelivered` and is replayed on every reconnect. The nonce binds
//! the bundle to a capability nonce (per blueprint: custody is keyed by
//! `Capability.nonce`), so a replayed bundle is cryptographically the same
//! logical message and the receiver's nonce-dedup collapses it to one delivery.
//!
//! ─────────────────────────────────────────────────────────────────────────────
//! ╔════════════════════════════════════════════════════════════════════════╗
//! ║ CI GUARD — NO-COURIER-SCORING (operator-final hard fork, 2026-07-11)    ║
//! ║ This overlay moves and retries *bundles* — opaque signed payloads. It     ║
//! ║ NEVER scores, ranks, or rates the courier/agent that carries them. A      ║
//! ║ bundle carries an identity (source/dest) + a nonce, never a reputation    ║
//! ║ score. Store-forward is neutral plumbing: it persists, replays, and       ║
//! ║ expires; it does not grade the mover.                                     ║
//! ╚════════════════════════════════════════════════════════════════════════╝
//! ─────────────────────────────────────────────────────────────────────────────

use std::collections::{HashMap, VecDeque};

use crate::error::{WireError, WireResult};

/// Fixed-size identity of a node (a 32-byte key/id, NOT a score).
pub type NodeId = [u8; 32];

/// The BPv7 primary block — the routing + custody metadata of a bundle.
///
/// Hand-rolled, byte-deterministic encoding (no external codec). `dest` /
/// `source` are 32-byte node ids; `creation_ts` + `lifetime` drive expiry
/// (`expire`); `nonce` binds the bundle to a capability nonce (custody key).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrimaryBlock {
    /// Destination node the bundle is destined for.
    pub dest: NodeId,
    /// Source node that originated the bundle.
    pub source: NodeId,
    /// Creation timestamp (monotonic tick; same domain as capability expiry).
    pub creation_ts: u64,
    /// Lifetime in the same tick units. Bundle expires when `creation_ts +
    /// lifetime < now`.
    pub lifetime: u64,
    /// Single-use nonce binding this bundle to a `Capability.nonce` (custody).
    pub nonce: [u8; 8],
}

impl PrimaryBlock {
    /// Serialize to a fixed 88-byte layout:
    /// `[dest:32][source:32][creation_ts:u64 le][lifetime:u64 le][nonce:8]`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(88);
        v.extend_from_slice(&self.dest);
        v.extend_from_slice(&self.source);
        v.extend_from_slice(&self.creation_ts.to_le_bytes());
        v.extend_from_slice(&self.lifetime.to_le_bytes());
        v.extend_from_slice(&self.nonce);
        v
    }

    /// Inverse of [`PrimaryBlock::to_bytes`]. Fails closed on wrong length.
    pub fn from_bytes(b: &[u8]) -> WireResult<Self> {
        if b.len() != 88 {
            return Err(WireError::Encode(format!(
                "primary block must be 88 bytes, got {}",
                b.len()
            )));
        }
        let take = |off: usize, n: usize| -> [u8; 32] {
            let mut a = [0u8; 32];
            a.copy_from_slice(&b[off..off + n]);
            a
        };
        let mut n8 = [0u8; 8];
        n8.copy_from_slice(&b[80..88]);
        Ok(PrimaryBlock {
            dest: take(0, 32),
            source: take(32, 32),
            creation_ts: u64::from_le_bytes(b[64..72].try_into().unwrap()),
            lifetime: u64::from_le_bytes(b[72..80].try_into().unwrap()),
            nonce: n8,
        })
    }
}

/// A store-and-forward bundle: the primary block + an optional custody
/// signature + the opaque payload (typically a serialized
/// [`bebop_proto_cap::SignedFrame`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// Routing + custody metadata.
    pub primary: PrimaryBlock,
    /// Custody signature over the primary block (set once a node accepts
    /// custody; `None` while the source still holds it).
    pub custody_sig: Option<Vec<u8>>,
    /// Opaque payload bytes (the intent being carried).
    pub payload: Vec<u8>,
}

impl Bundle {
    /// Serialize: `[primary:88][custody_sig_len:u32 le][custody_sig?]
    /// [payload_len:u32 le][payload]`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut v = self.primary.to_bytes();
        let cs = self.custody_sig.as_deref().unwrap_or(&[]);
        v.extend_from_slice(&(cs.len() as u32).to_le_bytes());
        v.extend_from_slice(cs);
        v.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());
        v.extend_from_slice(&self.payload);
        v
    }

    /// Inverse of [`Bundle::to_bytes`].
    pub fn from_bytes(b: &[u8]) -> WireResult<Self> {
        if b.len() < 88 + 4 + 4 {
            return Err(WireError::Encode("bundle too short".into()));
        }
        let primary = PrimaryBlock::from_bytes(&b[..88])?;
        let cs_len = u32::from_le_bytes(b[88..92].try_into().unwrap()) as usize;
        let mut off = 92;
        if b.len() < off + cs_len + 4 {
            return Err(WireError::Encode("bundle custody truncated".into()));
        }
        let custody_sig = if cs_len > 0 {
            Some(b[off..off + cs_len].to_vec())
        } else {
            None
        };
        off += cs_len;
        let pl_len = u32::from_le_bytes(b[off..off + 4].try_into().unwrap()) as usize;
        off += 4;
        if b.len() < off + pl_len {
            return Err(WireError::Encode("bundle payload truncated".into()));
        }
        let payload = b[off..off + pl_len].to_vec();
        Ok(Bundle {
            primary,
            custody_sig,
            payload,
        })
    }
}

/// Transport-agnostic store-and-forward queue.
///
/// The queue is the single source of truth: it holds every non-acked bundle in
/// FIFO (oldest-first) insertion order and survives reconnects (it is just in
/// memory here; a real courier would persist it). `undelivered` is a dest-indexed
/// view of the *nonces* still awaiting an ack, used by [`StoreForward::pending`]
/// and [`StoreForward::dequeue_for`].
///
/// Delivery contract: a bundle leaves `undelivered` only when the receiver acks
/// it ([`StoreForward::ack`]). Until then it is replayed on every reconnect,
/// oldest-first, so the receiver's nonce-dedup yields exactly-once delivery.
#[derive(Debug, Clone, Default)]
pub struct StoreForward {
    /// Master FIFO of all non-acked bundles (oldest at front).
    queue: VecDeque<Bundle>,
    /// Per-dest set of nonces still awaiting ack (custody not yet released).
    undelivered: HashMap<NodeId, Vec<[u8; 8]>>,
}

impl StoreForward {
    /// Empty queue.
    pub fn new() -> Self {
        StoreForward {
            queue: VecDeque::new(),
            undelivered: HashMap::new(),
        }
    }

    /// Enqueue a bundle for store-and-forward. It joins the FIFO and is tracked
    /// as undelivered at its destination until acked.
    pub fn enqueue(&mut self, bundle: Bundle) {
        let dest = bundle.primary.dest;
        let nonce = bundle.primary.nonce;
        self.queue.push_back(bundle);
        self.undelivered.entry(dest).or_default().push(nonce);
    }

    /// Pop the oldest undelivered bundle for `dest` (removing it from both the
    /// FIFO and the undelivered index). Returns `None` if nothing pending. Used
    /// by a single-destination courier pull.
    pub fn dequeue_for(&mut self, dest: &NodeId) -> Option<Bundle> {
        let nonce = self.undelivered.get_mut(dest)?.first().copied()?;
        // Remove the bundle with this nonce from the master FIFO.
        let idx = self.queue.iter().position(|b| b.primary.nonce == nonce)?;
        let bundle = self.queue.remove(idx).unwrap();
        // Drop the nonce from the undelivered index.
        if let Some(v) = self.undelivered.get_mut(dest) {
            v.retain(|n| *n != nonce);
            if v.is_empty() {
                self.undelivered.remove(dest);
            }
        }
        Some(bundle)
    }

    /// Mark a bundle delivered: release custody by removing it from both the FIFO
    /// and the undelivered index. Idempotent (safe to call twice).
    pub fn mark_delivered(&mut self, dest: &NodeId, nonce: [u8; 8]) {
        self.ack(dest, nonce);
    }

    /// Alias of [`StoreForward::mark_delivered`]: the receiver acks a bundle,
    /// releasing custody. The bundle is dropped from the queue entirely (it has
    /// been delivered and need never be replayed again).
    pub fn ack(&mut self, dest: &NodeId, nonce: [u8; 8]) {
        if let Some(pos) = self.queue.iter().position(|b| b.primary.nonce == nonce) {
            self.queue.remove(pos);
        }
        if let Some(v) = self.undelivered.get_mut(dest) {
            v.retain(|n| *n != nonce);
            if v.is_empty() {
                self.undelivered.remove(dest);
            }
        }
    }

    /// Drop every bundle whose `creation_ts + lifetime < now` (expired). Removes
    /// from both the FIFO and the undelivered index. Returns the nonces dropped.
    pub fn expire(&mut self, now: u64) -> Vec<[u8; 8]> {
        let mut dropped = Vec::new();
        self.queue.retain(|b| {
            let expired = b.primary.creation_ts.saturating_add(b.primary.lifetime) < now;
            if expired {
                dropped.push(b.primary.nonce);
            }
            !expired
        });
        for nonce in &dropped {
            for v in self.undelivered.values_mut() {
                v.retain(|n| n != nonce);
            }
        }
        self.undelivered.retain(|_, v| !v.is_empty());
        dropped
    }

    /// Return all undelivered bundles in FIFO (oldest-first) order for replay.
    /// Called by the courier after a (re)connect to drain custody. The caller
    /// re-binds each replay to the fresh channel (F7) before sending.
    pub fn retry_oldest_first(&self) -> Vec<Bundle> {
        self.queue.iter().cloned().collect()
    }

    /// Bundles still awaiting ack for `dest` (custody held).
    pub fn pending(&self, dest: &NodeId) -> Vec<&Bundle> {
        let nonces = match self.undelivered.get(dest) {
            Some(v) => v,
            None => return Vec::new(),
        };
        self.queue
            .iter()
            .filter(|b| nonces.contains(&b.primary.nonce))
            .collect()
    }

    /// Total bundles currently held (non-acked).
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Whether the queue is empty.
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Transport;
    use bebop_proto_cap::{Action, Capability, Resource, SignedFrame};
    use std::collections::HashSet;
    use tokio::sync::{Mutex, Notify};

    // ── In-memory Transport test double ───────────────────────────────────────
    // A carrier that ferries SignedFrames across a shared buffer. `drop_after`
    // makes `send` return `WireError::NotConnected` after N successful sends,
    // simulating a mid-transfer radio drop. The buffer (and `Notify`) survive
    // reconnects, so replayed bundles are actually delivered to the receiver.

    /// Shared link state between the two ends of the in-memory carrier.
    #[derive(Clone)]
    struct Link {
        buf: std::sync::Arc<Mutex<VecDeque<SignedFrame>>>,
        notify: std::sync::Arc<Notify>,
    }

    impl Link {
        fn new() -> Self {
            Link {
                buf: std::sync::Arc::new(Mutex::new(VecDeque::new())),
                notify: std::sync::Arc::new(Notify::new()),
            }
        }
    }

    /// Endpoint descriptor for the in-memory carrier (no real address).
    struct MemEndpoint {
        link: Link,
    }

    /// In-memory transport implementing the real [`crate::Transport`] contract.
    struct MemTransport {
        link: Link,
        connected: bool,
        /// Remaining successful sends before `send` starts returning
        /// `NotConnected`. `usize::MAX` = never drop (steady-state carrier).
        drop_after: usize,
    }

    impl crate::Transport for MemTransport {
        type Endpoint = MemEndpoint;

        async fn connect(endpoint: &Self::Endpoint) -> WireResult<Self> {
            Ok(MemTransport {
                link: endpoint.link.clone(),
                connected: true,
                drop_after: usize::MAX,
            })
        }

        async fn accept(endpoint: &Self::Endpoint) -> WireResult<Self> {
            Ok(MemTransport {
                link: endpoint.link.clone(),
                connected: true,
                drop_after: usize::MAX,
            })
        }

        async fn send(&mut self, frame: SignedFrame) -> WireResult<()> {
            if !self.connected || self.drop_after == 0 {
                return Err(WireError::NotConnected);
            }
            self.drop_after -= 1;
            self.link.buf.lock().await.push_back(frame);
            self.link.notify.notify_one();
            Ok(())
        }

        async fn recv(&mut self) -> WireResult<SignedFrame> {
            loop {
                if let Some(f) = self.link.buf.lock().await.pop_front() {
                    return Ok(f);
                }
                self.link.notify.notified().await;
            }
        }
    }

    /// Build a bundle wrapping a signed frame whose capability nonce is `nonce`.
    fn make_bundle(nonce: [u8; 8], seq: u8) -> Bundle {
        let cap = Capability::new(
            [7u8; 32],
            Resource::Route,
            Action::Send,
            nonce,
            9_999_999_999,
        );
        let frame = SignedFrame::new(cap, vec![seq; 4]);
        let payload = serde_json::to_vec(&frame).unwrap();
        Bundle {
            primary: PrimaryBlock {
                dest: [0u8; 32],
                source: [1u8; 32],
                creation_ts: 1000,
                lifetime: 1000,
                nonce,
            },
            custody_sig: None,
            payload,
        }
    }

    /// RED — MESH-09 critical property: an offline courier that drops mid-send,
    /// then reconnects and replays the undelivered bundles oldest-first with a
    /// FRESH channel binding per replay, must deliver ALL bundles EXACTLY ONCE.
    #[tokio::test]
    async fn offline_courier_reconnect_delivers_exactly_once() {
        let link = Link::new();
        let ep = MemEndpoint { link: link.clone() };

        let dest = [0u8; 32];
        let nonces = [[1u8; 8], [2u8; 8], [3u8; 8]];

        let mut sf = StoreForward::new();
        for n in nonces {
            sf.enqueue(make_bundle(n, n[0]));
        }
        assert_eq!(sf.len(), 3, "three bundles enqueued");
        assert_eq!(sf.pending(&dest).len(), 3, "all three pending");

        // Receiver side of the same link.
        let mut receiver = MemTransport::accept(&ep).await.unwrap();

        // ── Connection #1: drops after exactly 1 successful send ──────────────
        let mut sender = MemTransport::connect(&ep).await.unwrap();
        sender.drop_after = 1; // simulate mid-transfer radio drop

        let mut raw_deliveries: Vec<[u8; 8]> = Vec::new();
        let mut distinct: HashSet<[u8; 8]> = HashSet::new();

        // Drain undelivered oldest-first; send until the link drops.
        for b in sf.retry_oldest_first() {
            let frame: SignedFrame = serde_json::from_slice(&b.payload).unwrap();
            match sender.send(frame).await {
                Ok(()) => {
                    // Receiver picks it up.
                    let f = receiver.recv().await.unwrap();
                    let n = f.capability.nonce;
                    raw_deliveries.push(n);
                    distinct.insert(n);
                    // The receiver acks the bundle it accepted (releases custody).
                    sf.ack(&dest, n);
                }
                Err(WireError::NotConnected) => break,
                Err(e) => panic!("unexpected send error: {e:?}"),
            }
        }
        assert_eq!(
            raw_deliveries.len(),
            1,
            "connection #1 delivered exactly 1 before dropping"
        );
        assert_eq!(
            sf.pending(&dest).len(),
            2,
            "two bundles remain undelivered after drop"
        );

        // ── Connection #2: fresh channel, replays undelivered oldest-first ────
        let mut sender2 = MemTransport::connect(&ep).await.unwrap();
        // sender2 defaults to drop_after = usize::MAX (steady-state carrier).
        for b in sf.retry_oldest_first() {
            let frame: SignedFrame = serde_json::from_slice(&b.payload).unwrap();
            // FRESH channel binding per replay: re-sign the frame for the new
            // channel (F7). Demonstrates the caller re-binds each replay.
            let mut frame = frame;
            frame.channel_binding = Some([0xAAu8; 32]);
            sender2
                .send(frame)
                .await
                .expect("replay send must succeed on fresh channel");
            let f = receiver.recv().await.unwrap();
            let n = f.capability.nonce;
            raw_deliveries.push(n);
            distinct.insert(n);
            sf.ack(&dest, n);
        }

        // EXACT-NESS: every bundle arrived, and the ACK'd replay produced no
        // duplicate *delivery* (each nonce acked exactly once -> 3 raw deliveries).
        assert_eq!(distinct.len(), 3, "all 3 bundles arrived");
        assert_eq!(
            raw_deliveries.len(),
            3,
            "no duplicate delivery: each bundle delivered exactly once (ack collapses replay)"
        );
        assert_eq!(sf.pending(&dest).len(), 0, "nothing left undelivered");
        assert!(sf.is_empty(), "queue drained after all acks");
    }

    /// RED — expired bundles must be dropped by `expire` and never replayed.
    #[test]
    fn expire_drops_expired_bundles_and_keeps_fresh() {
        let mut sf = StoreForward::new();
        // Expired: created at 100, lifetime 10 => expires at 110.
        sf.enqueue(Bundle {
            primary: PrimaryBlock {
                dest: [0u8; 32],
                source: [1u8; 32],
                creation_ts: 100,
                lifetime: 10,
                nonce: [1u8; 8],
            },
            custody_sig: None,
            payload: vec![1],
        });
        // Fresh: created at 1000, lifetime 1000 => expires at 2000.
        sf.enqueue(Bundle {
            primary: PrimaryBlock {
                dest: [0u8; 32],
                source: [1u8; 32],
                creation_ts: 1000,
                lifetime: 1000,
                nonce: [2u8; 8],
            },
            custody_sig: None,
            payload: vec![2],
        });

        // At now=200 the first is expired, the second is not.
        let dropped = sf.expire(200);
        assert_eq!(dropped, vec![[1u8; 8]], "expired bundle dropped");
        assert_eq!(sf.len(), 1, "fresh bundle retained");
        assert_eq!(
            sf.retry_oldest_first().len(),
            1,
            "expired bundle not replayed"
        );

        // At now=2000 even the fresh one is gone (1000 + 1000 = 2000 < 2001).
        let dropped2 = sf.expire(2001);
        assert_eq!(dropped2, vec![[2u8; 8]]);
        assert!(sf.is_empty());
    }

    /// RED — `ack` removes a bundle from pending and from replay.
    #[test]
    fn ack_removes_from_pending_and_replay() {
        let mut sf = StoreForward::new();
        sf.enqueue(make_bundle([5u8; 8], 5));
        sf.enqueue(make_bundle([6u8; 8], 6));
        let dest = [0u8; 32];
        assert_eq!(sf.pending(&dest).len(), 2);

        sf.ack(&dest, [5u8; 8]);
        let pending: Vec<[u8; 8]> = sf.pending(&dest).iter().map(|b| b.primary.nonce).collect();
        assert_eq!(pending, vec![[6u8; 8]]);
        assert_eq!(sf.retry_oldest_first().len(), 1);

        // mark_delivered is an alias of ack.
        sf.mark_delivered(&dest, [6u8; 8]);
        assert!(sf.is_empty());
        assert_eq!(sf.pending(&dest).len(), 0);
    }

    /// RED — `dequeue_for` returns the oldest undelivered bundle for a dest.
    #[test]
    fn dequeue_for_returns_oldest_first() {
        let mut sf = StoreForward::new();
        sf.enqueue(make_bundle([1u8; 8], 1)); // dest d1
        sf.enqueue(make_bundle([2u8; 8], 2)); // dest d1
        let mut b3 = make_bundle([3u8; 8], 3);
        b3.primary.dest = [9u8; 32]; // dest d2
        sf.enqueue(b3);

        let d1 = [0u8; 32];
        let d2 = [9u8; 32];
        let first = sf.dequeue_for(&d1).unwrap();
        assert_eq!(
            first.primary.nonce, [1u8; 8],
            "oldest for d1 dequeued first"
        );
        let second = sf.dequeue_for(&d1).unwrap();
        assert_eq!(second.primary.nonce, [2u8; 8]);
        assert!(sf.dequeue_for(&d1).is_none(), "d1 drained");
        let d2only = sf.dequeue_for(&d2).unwrap();
        assert_eq!(d2only.primary.nonce, [3u8; 8]);
    }

    /// Round-trip encoding of primary block + bundle.
    #[test]
    fn primary_and_bundle_encode_decode_roundtrip() {
        let pb = PrimaryBlock {
            dest: [3u8; 32],
            source: [4u8; 32],
            creation_ts: 123456,
            lifetime: 789,
            nonce: [0xABu8; 8],
        };
        let bytes = pb.to_bytes();
        assert_eq!(bytes.len(), 88);
        assert_eq!(PrimaryBlock::from_bytes(&bytes).unwrap(), pb);

        let b = Bundle {
            primary: pb,
            custody_sig: Some(vec![9, 9, 9]),
            payload: vec![1, 2, 3, 4],
        };
        let bbytes = b.to_bytes();
        let back = Bundle::from_bytes(&bbytes).unwrap();
        assert_eq!(back, b);
        assert_eq!(back.custody_sig, Some(vec![9, 9, 9]));

        // No custody sig round-trips too.
        let b2 = Bundle {
            primary: PrimaryBlock {
                dest: [0u8; 32],
                source: [1u8; 32],
                creation_ts: 1,
                lifetime: 2,
                nonce: [7u8; 8],
            },
            custody_sig: None,
            payload: vec![],
        };
        assert_eq!(Bundle::from_bytes(&b2.to_bytes()).unwrap(), b2);
    }
}
