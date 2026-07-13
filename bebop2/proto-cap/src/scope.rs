//! Scope — resource/action namespace the capability system understands.
//!
//! A closed enum so the gate is exhaustively checkable. Scope describes
//! OBJECTS and VERBS (route, ledger entry, delivery intent, …), never ratings.
//!
//! CI GUARD: NO-COURIER-SCORING — scope describes objects/verbs, not trust.

use serde::{Deserialize, Serialize};

/// A protocol resource a capability may target. Closed set so the gate is total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resource {
    /// A transport route / channel.
    Route,
    /// A ledger entry (append / read).
    Ledger,
    /// A delivery intent (drop / query).
    DeliveryIntent,
    /// A generic mesh heartbeat / presence message.
    Presence,
    /// A restaurant / courier menu (catalog read).
    Menu,
    /// A customer order (create / read / mutate).
    Order,
    /// An analytics / reporting projection.
    Analytics,
    /// A customer / account record.
    Customer,
    /// A knowledge / embedding corpus (RAG).
    Corpus,
    /// A backup / snapshot artifact.
    Backup,
    /// A loyalty / rewards program record.
    Loyalty,
}

/// An action permitted on a [`Resource`]. Closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Action {
    /// Authorize a send on the resource.
    Send,
    /// Authorize a read/query of the resource.
    Read,
    /// Authorize an append/write to the resource.
    Append,
    /// Render a view / template (read-only presentation).
    Render,
    /// Create a new order (mutation).
    CreateOrder,
    /// Read a precomputed projection (read-only).
    ReadProjection,
    /// Upload a conversion event / telemetry (write).
    UploadConversion,
    /// Push a notification (write).
    Notify,
    /// Synchronize a catalog (write).
    SyncCatalog,
    /// Export a dataset (read/write boundary).
    Export,
    /// Take / restore a backup (write).
    Backup,
}

/// `(resource, action)` pair a capability authorizes. No score, no subject rating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scope {
    pub resource: Resource,
    pub action: Action,
}

impl Scope {
    /// Construct a scope. Placeholder until Tier-4 wiring enumerates the full
    /// resource/action matrix.
    pub fn new(resource: Resource, action: Action) -> Self {
        Scope { resource, action }
    }

    /// Fixed-layout canonical encoding of a scope, 2 bytes: `[resource_u8, action_u8]`.
    ///
    /// **No serde.** Discriminants are explicitly assigned so the byte mapping is
    /// stable across compiler versions and independent of Rust's enum
    /// representation (which is why this is hand-written, not `#[repr(u8)]` +
    /// `transmute` — we pin the exact byte values, not whatever the optimizer
    /// chooses). Consumed by `Capability::canonical_bytes_tlv` for signing.
    pub fn to_tlv_bytes(&self) -> [u8; 2] {
        [self.resource.discriminant(), self.action.discriminant()]
    }
}

impl Resource {
    /// Explicit discriminant byte (pinned; not compiler-chosen).
    pub fn discriminant(&self) -> u8 {
        match self {
            Resource::Route => 0x01,
            Resource::Ledger => 0x02,
            Resource::DeliveryIntent => 0x03,
            Resource::Presence => 0x04,
            Resource::Menu => 0x05,
            Resource::Order => 0x06,
            Resource::Analytics => 0x07,
            Resource::Customer => 0x08,
            Resource::Corpus => 0x09,
            Resource::Backup => 0x0A,
            Resource::Loyalty => 0x0B,
        }
    }

    /// Inverse of [`Resource::discriminant`]. Returns `None` for unknown bytes so
    /// decoding is fail-closed (no default/panic on a malformed scope).
    pub fn from_discriminant(b: u8) -> Option<Resource> {
        match b {
            0x01 => Some(Resource::Route),
            0x02 => Some(Resource::Ledger),
            0x03 => Some(Resource::DeliveryIntent),
            0x04 => Some(Resource::Presence),
            0x05 => Some(Resource::Menu),
            0x06 => Some(Resource::Order),
            0x07 => Some(Resource::Analytics),
            0x08 => Some(Resource::Customer),
            0x09 => Some(Resource::Corpus),
            0x0A => Some(Resource::Backup),
            0x0B => Some(Resource::Loyalty),
            _ => None,
        }
    }
}

impl Action {
    /// Explicit discriminant byte (pinned; not compiler-chosen).
    pub fn discriminant(&self) -> u8 {
        match self {
            Action::Send => 0x01,
            Action::Read => 0x02,
            Action::Append => 0x03,
            Action::Render => 0x04,
            Action::CreateOrder => 0x05,
            Action::ReadProjection => 0x06,
            Action::UploadConversion => 0x07,
            Action::Notify => 0x08,
            Action::SyncCatalog => 0x09,
            Action::Export => 0x0A,
            Action::Backup => 0x0B,
        }
    }

    /// Inverse of [`Action::discriminant`]. Returns `None` for unknown bytes so
    /// decoding is fail-closed.
    pub fn from_discriminant(b: u8) -> Option<Action> {
        match b {
            0x01 => Some(Action::Send),
            0x02 => Some(Action::Read),
            0x03 => Some(Action::Append),
            0x04 => Some(Action::Render),
            0x05 => Some(Action::CreateOrder),
            0x06 => Some(Action::ReadProjection),
            0x07 => Some(Action::UploadConversion),
            0x08 => Some(Action::Notify),
            0x09 => Some(Action::SyncCatalog),
            0x0A => Some(Action::Export),
            0x0B => Some(Action::Backup),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability::Capability;
    use crate::error::CapError;
    use crate::roster::{verify_chain, AnchorRoster, Delegation, Effect};

    fn key(seed_byte: u8) -> ([u8; 32], [u8; 32]) {
        let seed = [seed_byte; 32];
        let (pk, _) = bebop2_core::sign::keygen(&seed);
        (seed, pk)
    }

    #[test]
    fn scope_roundtrip_fixed_layout() {
        for r in [
            Resource::Route,
            Resource::Ledger,
            Resource::DeliveryIntent,
            Resource::Presence,
            Resource::Menu,
            Resource::Order,
            Resource::Analytics,
            Resource::Customer,
            Resource::Corpus,
            Resource::Backup,
            Resource::Loyalty,
        ] {
            for a in [
                Action::Send,
                Action::Read,
                Action::Append,
                Action::Render,
                Action::CreateOrder,
                Action::ReadProjection,
                Action::UploadConversion,
                Action::Notify,
                Action::SyncCatalog,
                Action::Export,
                Action::Backup,
            ] {
                let s = Scope::new(r, a);
                let bytes = s.to_tlv_bytes();
                assert_eq!(bytes[0], r.discriminant());
                assert_eq!(bytes[1], a.discriminant());
                assert_eq!(Resource::from_discriminant(bytes[0]), Some(r));
                assert_eq!(Action::from_discriminant(bytes[1]), Some(a));
            }
        }
    }

    #[test]
    fn scope_discriminants_are_stable() {
        // These byte values are part of the wire/signing contract — changing them
        // is a breaking wire change. Pin them explicitly.
        assert_eq!(Resource::Route.discriminant(), 0x01);
        assert_eq!(Resource::Ledger.discriminant(), 0x02);
        assert_eq!(Resource::DeliveryIntent.discriminant(), 0x03);
        assert_eq!(Resource::Presence.discriminant(), 0x04);
        assert_eq!(Resource::Menu.discriminant(), 0x05);
        assert_eq!(Resource::Order.discriminant(), 0x06);
        assert_eq!(Resource::Analytics.discriminant(), 0x07);
        assert_eq!(Resource::Customer.discriminant(), 0x08);
        assert_eq!(Resource::Corpus.discriminant(), 0x09);
        assert_eq!(Resource::Backup.discriminant(), 0x0A);
        assert_eq!(Resource::Loyalty.discriminant(), 0x0B);
        assert_eq!(Action::Send.discriminant(), 0x01);
        assert_eq!(Action::Read.discriminant(), 0x02);
        assert_eq!(Action::Append.discriminant(), 0x03);
        assert_eq!(Action::Render.discriminant(), 0x04);
        assert_eq!(Action::CreateOrder.discriminant(), 0x05);
        assert_eq!(Action::ReadProjection.discriminant(), 0x06);
        assert_eq!(Action::UploadConversion.discriminant(), 0x07);
        assert_eq!(Action::Notify.discriminant(), 0x08);
        assert_eq!(Action::SyncCatalog.discriminant(), 0x09);
        assert_eq!(Action::Export.discriminant(), 0x0A);
        assert_eq!(Action::Backup.discriminant(), 0x0B);
    }

    // ── R4 (IP-02): an attenuated capability requesting a Resource/Action
    // outside its own subtree MUST fail verify. Reuses the existing
    // `Delegation::sign` + `verify_chain` attenuation logic (narrow-only) — no
    // new attenuation scheme is invented here.
    //
    // The anchor grants `Order::CreateOrder` to a leaf. The leaf then forges a
    // capability requesting `Ledger::Append` (a DIFFERENT resource / action than
    // the granted subtree). `verify_chain` must reject it as ScopeViolation,
    // because the requested effect is not a subset of the tail scope.
    #[test]
    fn r4_attenuated_capability_outside_subtree_is_rejected() {
        // Anchor grants a *narrow* subtree: Order::CreateOrder.
        let (anchor_seed, anchor_pk) = key(0x21);
        let (leaf_seed, leaf_pk) = key(0x22);

        let granted = Scope::new(Resource::Order, Action::CreateOrder);
        let link = Delegation::sign(
            anchor_pk,
            leaf_pk,
            granted,
            Effect::new(Resource::Order, Action::CreateOrder),
            9999,
            [0x23u8; 8],
            &anchor_seed,
        )
        .unwrap();

        // (a) A capability requesting a DIFFERENT resource (Ledger) is out of
        // subtree -> ScopeViolation.
        let out_of_resource =
            Capability::new(leaf_pk, Resource::Ledger, Action::Append, [0x24u8; 8], 9999);
        // (b) A capability requesting a DIFFERENT action on the same resource
        // (Order::Read) is also out of subtree -> ScopeViolation.
        let out_of_action =
            Capability::new(leaf_pk, Resource::Order, Action::Read, [0x25u8; 8], 9999);
        // (c) A capability requesting a brand-new enum variant pair outside the
        // granted subtree (Analytics::Export) is likewise rejected.
        let out_of_subtree = Capability::new(
            leaf_pk,
            Resource::Analytics,
            Action::Export,
            [0x26u8; 8],
            9999,
        );

        let mut roster = AnchorRoster::new();
        roster.enroll(&anchor_pk);
        let chain = vec![link];

        for cap in [out_of_resource, out_of_action, out_of_subtree] {
            let err = verify_chain(&roster, &chain, &cap, 0);
            assert!(
                matches!(err, Err(CapError::ScopeViolation)),
                "attenuated cap outside subtree must be ScopeViolation, got {:?}",
                err
            );
        }
        let _ = leaf_seed;
    }
}
