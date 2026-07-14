//! Event payload dictionary — the wire shape of every delivery event.
//!
//! Maps each `(Resource, Action)` delivery pair to a canonical, fixed-layout
//! TLV payload carried inside a [`crate::signed_frame::SignedFrame`]. The codec
//! is **no serde on the signed path**: pure byte push (mirrors [`crate::tlv`]).
//!
//! Three payload families (per the MESH choreography):
//! - `place_order`   → `OrderPlaced` (`Resource::Order`)
//! - `claim_machine` → `ClaimOffered`/`ClaimAccepted`/`ClaimReleased` (`Resource::Claim`)
//! - `ledger i64`    → `SettlementRecorded` (`Resource::Ledger`) / `OrderStatusChanged`
//!
//! # Receiver-side Law (forged-transition rejection)
//! Every receiver validates an `OrderStatusChanged` payload against the order
//! state-machine transition table BEFORE applying it. A forged
//! `Pending → Delivered` (skipping the legal lifecycle) is rejected on **every**
//! node, because all nodes share the same fixed-layout payload AND the same
//! transition table. `assert_status_transition` is that receiver-side check.
//!
//! CI GUARD: NO-COURIER-SCORING — payloads carry order/claim/ledger data only.
//! No score, rating, or trust field anywhere.

use crate::scope::{Action, Resource, Scope};

/// A 32-byte courier public key (Ed25519). Plain bytes; never a score.
pub type CourierKey = [u8; 32];

/// Pinned wire byte for a delivery status (matches `DeliveryStatus` in
/// `bebop-delivery-domain`; kept local so proto-cap stays kernel-free).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryStatus {
    Pending,
    Confirmed,
    Preparing,
    Ready,
    InDelivery,
    Delivered,
    Rejected,
    Cancelled,
    PickedUp,
}

impl DeliveryStatus {
    pub fn discriminant(&self) -> u8 {
        match self {
            DeliveryStatus::Pending => 0x10,
            DeliveryStatus::Confirmed => 0x11,
            DeliveryStatus::Preparing => 0x12,
            DeliveryStatus::Ready => 0x13,
            DeliveryStatus::InDelivery => 0x14,
            DeliveryStatus::Delivered => 0x15,
            DeliveryStatus::Rejected => 0x16,
            DeliveryStatus::Cancelled => 0x17,
            DeliveryStatus::PickedUp => 0x18,
        }
    }
    pub fn from_discriminant(b: u8) -> Option<DeliveryStatus> {
        Some(match b {
            0x10 => DeliveryStatus::Pending,
            0x11 => DeliveryStatus::Confirmed,
            0x12 => DeliveryStatus::Preparing,
            0x13 => DeliveryStatus::Ready,
            0x14 => DeliveryStatus::InDelivery,
            0x15 => DeliveryStatus::Delivered,
            0x16 => DeliveryStatus::Rejected,
            0x17 => DeliveryStatus::Cancelled,
            0x18 => DeliveryStatus::PickedUp,
            _ => return None,
        })
    }
}

/// Receiver-side legality table (1:1 with `dowiz_kernel::order_machine`). The
/// malware class "forged status skip" is rejected by this identical table on
/// every receiver.
fn allowed_next(from: DeliveryStatus) -> &'static [DeliveryStatus] {
    use DeliveryStatus::*;
    match from {
        Pending => &[Confirmed, Rejected, Cancelled],
        Confirmed => &[Preparing, InDelivery],
        Preparing => &[Ready],
        Ready => &[InDelivery, PickedUp],
        InDelivery => &[Delivered],
        Delivered | Rejected | Cancelled | PickedUp => &[],
    }
}

/// Receiver-side Law: a status transition is legal iff `to ∈ allowed_next(from)`
/// and `from != to`. This is what every receiver runs on an `OrderStatusChanged`
/// payload — a forged skip fails here on ALL nodes.
pub fn assert_status_transition(
    from: DeliveryStatus,
    to: DeliveryStatus,
) -> Result<(), &'static str> {
    if from == to {
        return Err("same status");
    }
    if allowed_next(from).contains(&to) {
        Ok(())
    } else {
        Err("illegal transition")
    }
}

/// `place_order` payload (`Resource::Order`, `Action::OrderPlaced`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrderPlacedPayload {
    pub order_id: u64,
    /// Integer minor units (the order subtotal). Money is i64, never float.
    pub amount_i64: i64,
    pub src: String,
    pub dst: String,
}

/// `claim_machine` payload (`Resource::Claim`, `ClaimOffered`/`ClaimAccepted`/`ClaimReleased`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimPayload {
    pub claim_id: u64,
    pub order_id: u64,
    pub courier: CourierKey,
}

/// `ledger i64` payload (`Resource::Ledger`, `Action::SettlementRecorded`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerPayload {
    pub order_id: u64,
    /// Settlement amount in integer minor units.
    pub amount_i64: i64,
}

/// `OrderStatusChanged` payload (`Resource::Order`, `Action::OrderStatusChanged`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusChangedPayload {
    pub order_id: u64,
    pub from: DeliveryStatus,
    pub to: DeliveryStatus,
}

// ── canonical encoders (fixed-layout) ─────────────────────────────────────────

fn put_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn put_i64(buf: &mut Vec<u8>, v: i64) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn put_str(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(&(s.len() as u32).to_le_bytes());
    buf.extend_from_slice(s.as_bytes());
}
fn put_key(buf: &mut Vec<u8>, k: &CourierKey) {
    buf.extend_from_slice(k);
}

fn get_u64(b: &[u8], p: &mut usize) -> Result<u64, &'static str> {
    if *p + 8 > b.len() {
        return Err("truncated u64");
    }
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[*p..*p + 8]);
    *p += 8;
    Ok(u64::from_le_bytes(a))
}
fn get_i64(b: &[u8], p: &mut usize) -> Result<i64, &'static str> {
    Ok(get_u64(b, p)? as i64)
}
fn get_str(b: &[u8], p: &mut usize) -> Result<String, &'static str> {
    if *p + 4 > b.len() {
        return Err("truncated str-len");
    }
    let mut l = [0u8; 4];
    l.copy_from_slice(&b[*p..*p + 4]);
    *p += 4;
    let n = u32::from_le_bytes(l) as usize;
    if *p + n > b.len() {
        return Err("truncated str");
    }
    let s = String::from_utf8(b[*p..*p + n].to_vec()).map_err(|_| "bad utf8")?;
    *p += n;
    Ok(s)
}
fn get_key(b: &[u8], p: &mut usize) -> Result<CourierKey, &'static str> {
    if *p + 32 > b.len() {
        return Err("truncated key");
    }
    let mut k = [0u8; 32];
    k.copy_from_slice(&b[*p..*p + 32]);
    *p += 32;
    Ok(k)
}

impl OrderPlacedPayload {
    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::new();
        put_u64(&mut v, self.order_id);
        put_i64(&mut v, self.amount_i64);
        put_str(&mut v, &self.src);
        put_str(&mut v, &self.dst);
        v
    }
    pub fn decode(b: &[u8]) -> Result<Self, &'static str> {
        let mut p = 0;
        let order_id = get_u64(b, &mut p)?;
        let amount_i64 = get_i64(b, &mut p)?;
        let src = get_str(b, &mut p)?;
        let dst = get_str(b, &mut p)?;
        Ok(Self {
            order_id,
            amount_i64,
            src,
            dst,
        })
    }
}

impl ClaimPayload {
    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::new();
        put_u64(&mut v, self.claim_id);
        put_u64(&mut v, self.order_id);
        put_key(&mut v, &self.courier);
        v
    }
    pub fn decode(b: &[u8]) -> Result<Self, &'static str> {
        let mut p = 0;
        let claim_id = get_u64(b, &mut p)?;
        let order_id = get_u64(b, &mut p)?;
        let courier = get_key(b, &mut p)?;
        Ok(Self {
            claim_id,
            order_id,
            courier,
        })
    }
}

impl LedgerPayload {
    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::new();
        put_u64(&mut v, self.order_id);
        put_i64(&mut v, self.amount_i64);
        v
    }
    pub fn decode(b: &[u8]) -> Result<Self, &'static str> {
        let mut p = 0;
        let order_id = get_u64(b, &mut p)?;
        let amount_i64 = get_i64(b, &mut p)?;
        Ok(Self {
            order_id,
            amount_i64,
        })
    }
}

impl StatusChangedPayload {
    pub fn encode(&self) -> Vec<u8> {
        let mut v = Vec::new();
        put_u64(&mut v, self.order_id);
        v.push(self.from.discriminant());
        v.push(self.to.discriminant());
        v
    }
    pub fn decode(b: &[u8]) -> Result<Self, &'static str> {
        let mut p = 0;
        let order_id = get_u64(b, &mut p)?;
        if p + 2 > b.len() {
            return Err("truncated status");
        }
        let from = DeliveryStatus::from_discriminant(b[p]).ok_or("bad from-status")?;
        let to = DeliveryStatus::from_discriminant(b[p + 1]).ok_or("bad to-status")?;
        Ok(Self { order_id, from, to })
    }
}

/// Given a `(Resource, Action)` and a raw payload, return the decoded event
/// payload as an opaque enum so callers can dispatch. Fails closed on unknown
/// scope or malformed bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryEvent {
    OrderPlaced(OrderPlacedPayload),
    StatusChanged(StatusChangedPayload),
    Claim(ClaimPayload),
    Settlement(LedgerPayload),
}

impl DeliveryEvent {
    pub fn decode(scope: Scope, payload: &[u8]) -> Result<Self, &'static str> {
        match scope.grants.first() {
            Some(&(Resource::Order, Action::OrderPlaced)) => Ok(DeliveryEvent::OrderPlaced(
                OrderPlacedPayload::decode(payload)?,
            )),
            Some(&(Resource::Order, Action::OrderStatusChanged)) => Ok(
                DeliveryEvent::StatusChanged(StatusChangedPayload::decode(payload)?),
            ),
            Some(&(Resource::Claim, Action::ClaimOffered))
            | Some(&(Resource::Claim, Action::ClaimAccepted))
            | Some(&(Resource::Claim, Action::ClaimReleased)) => {
                Ok(DeliveryEvent::Claim(ClaimPayload::decode(payload)?))
            }
            Some(&(Resource::Ledger, Action::SettlementRecorded)) => {
                Ok(DeliveryEvent::Settlement(LedgerPayload::decode(payload)?))
            }
            _ => Err("unsupported (resource, action) for delivery event"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── R-MESH03a: a forged OrderStatusChanged{Pending -> Delivered} is rejected
    // on EVERY receiver. Two nodes decode the SAME payload; both run the
    // receiver-side Law and both reject the illegal skip.
    #[test]
    fn r_mesh03_forge_pending_to_delivered_rejected_everywhere() {
        let forged = StatusChangedPayload {
            order_id: 99,
            from: DeliveryStatus::Pending,
            to: DeliveryStatus::Delivered, // forged skip
        };
        let bytes = forged.encode();

        // Node A and Node B fold the same event -> identical decode.
        let a = StatusChangedPayload::decode(&bytes).unwrap();
        let b = StatusChangedPayload::decode(&bytes).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.from, DeliveryStatus::Pending);
        assert_eq!(a.to, DeliveryStatus::Delivered);

        // Both receivers reject the forged transition via the shared Law.
        assert!(assert_status_transition(a.from, a.to).is_err());
        assert!(assert_status_transition(b.from, b.to).is_err());
    }

    // ── R-MESH03b: two nodes fold a LEGAL lifecycle to identical state.
    #[test]
    fn r_mesh03_legal_lifecycle_folds_identical() {
        let steps = [
            StatusChangedPayload {
                order_id: 1,
                from: DeliveryStatus::Pending,
                to: DeliveryStatus::Confirmed,
            },
            StatusChangedPayload {
                order_id: 1,
                from: DeliveryStatus::Confirmed,
                to: DeliveryStatus::Preparing,
            },
            StatusChangedPayload {
                order_id: 1,
                from: DeliveryStatus::Preparing,
                to: DeliveryStatus::Ready,
            },
            StatusChangedPayload {
                order_id: 1,
                from: DeliveryStatus::Ready,
                to: DeliveryStatus::InDelivery,
            },
            StatusChangedPayload {
                order_id: 1,
                from: DeliveryStatus::InDelivery,
                to: DeliveryStatus::Delivered,
            },
        ];
        let mut node_a = DeliveryStatus::Pending;
        let mut node_b = DeliveryStatus::Pending;
        for s in &steps {
            let dec = StatusChangedPayload::decode(&s.encode()).unwrap();
            // Receiver Law passes.
            assert!(assert_status_transition(dec.from, dec.to).is_ok());
            node_a = dec.to;
            node_b = dec.to;
        }
        assert_eq!(node_a, node_b);
        assert_eq!(node_a, DeliveryStatus::Delivered);
    }

    // ── R-MESH03c: DeliveryEvent::decode dispatches each (Resource,Action) and
    // fails closed on an unsupported scope.
    #[test]
    fn r_mesh03_decode_dispatch_and_fail_closed() {
        let op = OrderPlacedPayload {
            order_id: 5,
            amount_i64: 1300,
            src: "R".into(),
            dst: "C".into(),
        };
        let ev = DeliveryEvent::decode(
            Scope::single(Resource::Order, Action::OrderPlaced),
            &op.encode(),
        )
        .unwrap();
        match ev {
            DeliveryEvent::OrderPlaced(p) => assert_eq!(p.amount_i64, 1300),
            _ => panic!("wrong variant"),
        }

        let cl = ClaimPayload {
            claim_id: 2,
            order_id: 5,
            courier: [7u8; 32],
        };
        let ev = DeliveryEvent::decode(
            Scope::single(Resource::Claim, Action::ClaimOffered),
            &cl.encode(),
        )
        .unwrap();
        match ev {
            DeliveryEvent::Claim(p) => assert_eq!(p.claim_id, 2),
            _ => panic!("wrong variant"),
        }

        let led = LedgerPayload {
            order_id: 5,
            amount_i64: -50,
        };
        let ev = DeliveryEvent::decode(
            Scope::single(Resource::Ledger, Action::SettlementRecorded),
            &led.encode(),
        )
        .unwrap();
        match ev {
            DeliveryEvent::Settlement(p) => assert_eq!(p.amount_i64, -50),
            _ => panic!("wrong variant"),
        }

        // Unsupported scope -> fail closed.
        assert!(
            DeliveryEvent::decode(Scope::single(Resource::Route, Action::Send), &[0u8; 4],)
                .is_err()
        );
    }

    // ── GREEN: roundtrip of every payload shape.
    #[test]
    fn green_mesh03_payload_roundtrips() {
        let op = OrderPlacedPayload {
            order_id: 11,
            amount_i64: 42,
            src: "a".into(),
            dst: "b".into(),
        };
        assert_eq!(OrderPlacedPayload::decode(&op.encode()).unwrap(), op);
        let cl = ClaimPayload {
            claim_id: 3,
            order_id: 11,
            courier: [9u8; 32],
        };
        assert_eq!(ClaimPayload::decode(&cl.encode()).unwrap(), cl);
        let led = LedgerPayload {
            order_id: 11,
            amount_i64: 777,
        };
        assert_eq!(LedgerPayload::decode(&led.encode()).unwrap(), led);
        let st = StatusChangedPayload {
            order_id: 11,
            from: DeliveryStatus::Ready,
            to: DeliveryStatus::InDelivery,
        };
        assert_eq!(StatusChangedPayload::decode(&st.encode()).unwrap(), st);
    }
}
