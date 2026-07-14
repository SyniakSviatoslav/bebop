//! Envelope — the carrier-neutral frame wrapper.
//!
//! The envelope is what every carrier (WSS, iroh) moves. It carries:
//! - a protocol `version`,
//! - an opaque, carrier-neutral `payload` (the already-signed [`SignedFrame`]
//!   bytes from `bebop-proto-cap`),
//! - a `trace` id for diagnostics (NOT a score — a correlation id only).
//!
//! CI GUARD: NO-COURIER-SCORING — envelope carries identity + signature only,
//! never a trust/score field.

use serde::{Deserialize, Serialize};

/// Protocol version of the envelope wire format. Bump on incompatible changes.
pub const ENVELOPE_VERSION: u8 = 1;

/// The carrier-neutral envelope. Both iroh and WSS transport this exact struct;
/// only the carrier framing (`framing`) differs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    /// Wire-format version. A peer rejecting an unknown version fails closed.
    pub version: u8,
    /// Correlation id for tracing a logical message across hops. Diagnostic only
    /// — never a reputation/score input.
    pub trace: [u8; 16],
    /// Opaque bytes: the serialized, signed [`bebop_proto_cap::SignedFrame`].
    pub payload: Vec<u8>,
}

impl Envelope {
    /// Build an envelope wrapping a signed-frame payload.
    pub fn new(trace: [u8; 16], payload: Vec<u8>) -> Self {
        Envelope {
            version: ENVELOPE_VERSION,
            trace,
            payload,
        }
    }

    /// Serialize to canonical JSON bytes (deterministic field order).
    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Deserialize from canonical JSON bytes. C7 (defense-in-depth): bound the serde parse at the
    /// SAME cap `framing::decode` already enforces on the length prefix — a single source of truth,
    /// no separate/conflicting const (a stricter local cap would make 1-8 MiB frames
    /// sendable-but-undecodable). A signed frame (capability + ML-DSA-65 sig ~3.3KB) is far under it;
    /// serde_json's default recursion limit (128) bounds nesting depth.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        if bytes.len() > crate::framing::MAX_ENVELOPE_BYTES {
            return Err(<serde_json::Error as serde::de::Error>::custom(
                "envelope exceeds framing::MAX_ENVELOPE_BYTES",
            ));
        }
        serde_json::from_slice(bytes)
    }
}
