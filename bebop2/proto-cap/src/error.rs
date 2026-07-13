//! Error types for the authorization line.
//!
//! `CapError` describes authentication faults only — bad signature, expired
//! nonce, scope violation, missing hybrid proof. It NEVER encodes or derives a
//! courier/agent score.
//!
//! CI GUARD: NO-COURIER-SCORING — errors describe auth faults, never scores.

use core::fmt;

/// Authentication / capability error. Neutral plumbing: a frame is accepted or
/// rejected on its signature + nonce + scope; there is no reputation surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapError {
    /// The classical (Ed25519) signature failed to verify.
    ClassicalVerifyFailed,
    /// The post-quantum (ML-DSA-65) signature failed to verify.
    PqVerifyFailed,
    /// The hybrid gate requires BOTH a classical and a PQ signature, but one or
    /// both are missing (or the PQ leg is still a TODO on this build).
    HybridIncomplete,
    /// The capability nonce has already been seen (replay) or is invalid.
    NonceRejected,
    /// The capability is past its expiry.
    Expired,
    /// Cannot (de)serialize the capability for canonical signing.
    Encode,
    /// The signature or key buffer had the wrong length.
    BadLength,
    /// The root issuer of a delegation chain is NOT an enrolled trust anchor
    /// (kills the self-issue auth-bypass: a subject cannot mint authority by
    /// signing its own delegation).
    UnknownIssuer,
    /// A delegation link does not chain: `issued_by` of a link is not the
    /// `subject` of the preceding link.
    ChainBroken,
    /// The requested effect / scope is not a subset of the tail link's scope.
    /// Makes the previously-dead `ScopeViolation` gate live: attenuation is
    /// enforced, not just enumerated.
    ScopeViolation,
    /// The tail of the delegation chain does not bind to the capability's
    /// `subject_key`.
    SubjectMismatch,
    /// A delegation link's Ed25519 signature failed to verify against its
    /// `issued_by` issuer key.
    BadSignature,
    /// The mandatory OS entropy floor (getrandom / RDRAND) was unavailable, so
    /// the fail-closed entropy port refused to produce output. Advisory sources
    /// (e.g. ANU QRNG) can NEVER substitute for this — see `entropy.rs` (IP-17/18).
    EntropyUnavailable,
    /// The replay ledger lock was poisoned (internal fault). Surfaced as a clean
    /// rejection instead of a panic — a poisoned mutex must never take down the
    /// connection (red-team B2/B3: unbounded/panic-DoS on the nonce set).
    LockPoisoned,
    /// The capability (or its subject key) has been revoked by an
    /// UCAN-style irreversible invalidation set ([`crate::revocation`]). A
    /// revoked capability MUST be rejected even if its signature and chain are
    /// otherwise valid and unexpired — revocation is the missing authz control
    /// that expiry alone could never provide (MESH-11).
    Revoked,
}

impl fmt::Display for CapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            CapError::ClassicalVerifyFailed => "classical (Ed25519) signature verification failed",
            CapError::PqVerifyFailed => "post-quantum (ML-DSA-65) signature verification failed",
            CapError::HybridIncomplete => {
                "hybrid gate requires BOTH classical + PQ signatures (one missing or PQ leg TODO)"
            }
            CapError::NonceRejected => "capability nonce rejected (replay or invalid)",
            CapError::Expired => "capability expired",
            CapError::Encode => "capability (de)serialization failed",
            CapError::BadLength => "signature or key buffer had the wrong length",
            CapError::UnknownIssuer => {
                "delegation chain root issuer is not an enrolled trust anchor"
            }
            CapError::ChainBroken => "delegation link does not chain to its parent",
            CapError::ScopeViolation => "requested effect is not a subset of the granted scope",
            CapError::SubjectMismatch => {
                "delegation chain tail does not bind to the capability subject"
            }
            CapError::BadSignature => "delegation link signature verification failed",
            CapError::EntropyUnavailable => {
                "mandatory OS entropy floor unavailable (fail-closed; advisory QRNG cannot substitute)"
            }
            CapError::LockPoisoned => "replay ledger unavailable (internal fault)",
            CapError::Revoked => "capability or subject key has been revoked",
        };
        f.write_str(s)
    }
}

impl core::error::Error for CapError {}

/// Convenience `Result` alias for the authorization line.
pub type CapResult<T> = Result<T, CapError>;
