//! Handshake — mutual transport setup (key confirmation, routing bootstrap).
//!
//! The handshake authenticates endpoints via the signed capability in each frame
//! (see `bebop-proto-cap`), never via a bearer token or accumulated score. The
//! WSS carrier's HTTP Upgrade is handled by `tokio-tungstenite`; iroh's ticket /
//! peer-id exchange is a TODO in `iroh_transport`.
//!
//! This module currently exposes the neutral bootstrap types shared by carriers.
//! No scoring surface.
//!
//! CI GUARD: NO-COURIER-SCORING — handshake authenticates endpoints via signed
//! capability, never via accumulated score.

use serde::{Deserialize, Serialize};

/// Compute the channel-binding hash from the handshake transcript.
///
/// `transcript` is the concatenation of the handshake records exchanged on the
/// channel (ClientHello..ServerFinished, or whatever the carrier records for the
/// session). The binding is the SHA3-256 of that transcript (from
/// `bebop2_core::hash`). A captured frame whose signature was made over
/// `channel_binding_hash(transcript_A)` will not verify on channel B, because B
/// produced a different `transcript` and therefore a different hash. This is the
/// cross-channel replay defense (F7).
///
/// ponytail: `transcript` MUST be the *authenticated* handshake bytes (the exact
/// bytes the peers exchanged and verified), not a re-derived summary — otherwise
/// a MITM that reselects ciphersuites could collide the binding.
pub fn channel_binding_hash(transcript: &[u8]) -> [u8; 32] {
    bebop2_core::hash::sha3_256(transcript)
}

/// A bootstrap greeting exchanged (inside the first signed envelope) at connect
/// time. Carries endpoint identity + protocol version only — never a rating.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Handshake {
    /// Protocol envelope version the peer speaks.
    pub version: u8,
    /// Opaque endpoint identity (e.g. a node id / public key). Not a score.
    pub peer_id: Vec<u8>,
}

impl Handshake {
    pub fn new(version: u8, peer_id: Vec<u8>) -> Self {
        Handshake { version, peer_id }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_binding_hash_is_sha3_256_of_transcript() {
        let transcript_a = b"ClientHello|ServerHello|..|ServerFinished-A";
        let transcript_b = b"ClientHello|ServerHello|..|ServerFinished-B";
        let hash_a = channel_binding_hash(transcript_a);
        let hash_b = channel_binding_hash(transcript_b);
        // Matches the core primitive directly.
        assert_eq!(hash_a, bebop2_core::hash::sha3_256(transcript_a));
        // Different transcripts => different bindings (collision resistance).
        assert_ne!(hash_a, hash_b, "different transcripts must produce different bindings");
    }
}
