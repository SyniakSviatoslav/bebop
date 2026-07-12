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
        }
    }

    /// Inverse of [`Action::discriminant`]. Returns `None` for unknown bytes so
    /// decoding is fail-closed.
    pub fn from_discriminant(b: u8) -> Option<Action> {
        match b {
            0x01 => Some(Action::Send),
            0x02 => Some(Action::Read),
            0x03 => Some(Action::Append),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_roundtrip_fixed_layout() {
        for r in [
            Resource::Route,
            Resource::Ledger,
            Resource::DeliveryIntent,
            Resource::Presence,
        ] {
            for a in [Action::Send, Action::Read, Action::Append] {
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
        assert_eq!(Action::Send.discriminant(), 0x01);
        assert_eq!(Action::Read.discriminant(), 0x02);
        assert_eq!(Action::Append.discriminant(), 0x03);
    }
}
