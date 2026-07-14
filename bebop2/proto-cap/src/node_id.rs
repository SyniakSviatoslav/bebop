//! MESH-12 — node identity (ADR-0007) + genesis loader + HUMAN root-delegation policy.
//!
//! # node_id = H(pq_pub || classical_pub)  (ADR-0007, no-CA, SPKI-lineage)
//!
//! A mesh node's identity is **derived**, never assigned by a CA. It binds the
//! node's two public keys — the post-quantum key (ML-KEM/ML-DSA public material)
//! and the classical key (Ed25519) — into a single 32-byte id via SHA3-256.
//! This kills the *seeded-owner JWT* anti-pattern: there is no magical "owner"
//! claim you can mint; there is only a key lineage you can prove. Changing EITHER
//! public key changes the id. See `docs/design/mesh-real/ADR-0007-*.md`.
//!
//! # Genesis loader (fail-closed)
//!
//! Authority at runtime needs a frozen trust-anchor set enrolled exactly once,
//! at genesis, from config/disk — **not** inline in code and **not** auto-seeded.
//! [`load_genesis`] reads a plain-text anchor file (one hex 32-byte Ed25519
//! public key per line, `#` comments allowed). It is FAIL-CLOSED: a missing,
//! unreadable, malformed, or *zero-anchor* file yields an error and enrolls
//! nothing, so the node captures no authority from a broken genesis.
//!
//! # HUMAN decision: root-delegation policy  (innovate:)
//!
//! The actual root-delegation model — operator-signed vs Web-of-Trust vs
//! first-contact-QR — is an **OPERATOR decision**. This module implements all
//! three as the [`RootDelegationPolicy`] enum and a [`Default`] of
//! `Unspecified`, but the code MUST NOT silently pick one as "chosen". The
//! operator configures the policy explicitly; until then the node fails closed
//! and enrolls no root authority. Do not "helpfully" default to a real policy.

use bebop2_core::hash::sha3_256;

use crate::capability::Capability;
use crate::error::CapError;
use crate::roster::{AnchorRoster, Delegation, Effect};
use crate::scope::{Action, Resource, Scope};

/// A mesh node identity: `H(pq_pub || classical_pub)`.
///
/// 32 bytes; deterministic; changes if EITHER input key changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(pub [u8; 32]);

impl NodeId {
    /// Derive the node id from the PQ public key and the classical (Ed25519)
    /// public key, per ADR-0007: `id = SHA3-256(pq_pub || classical_pub)`.
    pub fn from_keys(pq_pub: &[u8], classical_pub: &[u8; 32]) -> Self {
        let mut buf = Vec::with_capacity(pq_pub.len() + 32);
        buf.extend_from_slice(pq_pub);
        buf.extend_from_slice(classical_pub);
        NodeId(sha3_256(&buf))
    }

    /// The raw 32-byte id.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lowercase hex encoding (offline, dependency-free).
    pub fn to_hex(&self) -> String {
        hex_encode(&self.0)
    }
}

/// The two public keys a node presents on the wire.
#[derive(Debug, Clone)]
pub struct NodeKeys {
    /// Post-quantum public key material (e.g. ML-KEM-768 pk, 1184 bytes; or
    /// ML-DSA public key). Variable length by design.
    pub pq_pub: Vec<u8>,
    /// Classical (Ed25519) 32-byte public key.
    pub classical_pub: [u8; 32],
}

impl NodeKeys {
    /// Derive this node's [`NodeId`].
    pub fn node_id(&self) -> NodeId {
        NodeId::from_keys(&self.pq_pub, &self.classical_pub)
    }
}

// ── Genesis loader (fail-closed) ──────────────────────────────────────────────

/// Errors returned by the genesis loader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenesisError {
    /// The genesis file could not be read (missing, no permission, ...).
    Io(String),
    /// A line in the genesis file was malformed (bad hex / wrong length).
    Parse(String),
    /// The file parsed but yielded ZERO anchors. Fail-closed: no silent
    /// empty bootstrap. The node must not capture any authority.
    EmptyRoster,
    /// A root-delegation policy was needed but none was explicitly chosen.
    PolicyUnspecified,
}

impl From<std::io::Error> for GenesisError {
    fn from(e: std::io::Error) -> Self {
        GenesisError::Io(e.to_string())
    }
}

/// Load the frozen trust-anchor set from disk.
///
/// Format: plain text, one hex-encoded 32-byte Ed25519 public key per line.
/// Blank lines and `#` comments are ignored. See `config/genesis.example.txt`.
///
/// FAIL-CLOSED: any of the following yields an error and enrolls NOTHING:
/// - file missing / unreadable ([`GenesisError::Io`]);
/// - a non-comment line is not exactly 64 hex chars / decodes to ≠ 32 bytes
///   ([`GenesisError::Parse`]);
/// - the file is valid but contains zero anchors ([`GenesisError::EmptyRoster`]).
///
/// The node therefore captures **no authority** from a broken or empty genesis.
/// Authority is never auto-seeded.
pub fn load_genesis(path: &str) -> Result<AnchorRoster, GenesisError> {
    let data = std::fs::read_to_string(path)?;
    let mut roster = AnchorRoster::new();
    for (i, raw) in data.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let bytes = hex_decode(line)
            .map_err(|e| GenesisError::Parse(format!("line {}: {}", i + 1, e)))?;
        if bytes.len() != 32 {
            return Err(GenesisError::Parse(format!(
                "line {}: expected 32-byte key, got {}",
                i + 1,
                bytes.len()
            )));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        roster.enroll(&key);
    }
    if roster.is_empty() {
        return Err(GenesisError::EmptyRoster);
    }
    Ok(roster)
}

/// A freshly-initialized node has NO anchors. It captures no authority until a
/// genesis is loaded. This is the fail-closed default — never auto-seed.
pub fn empty_roster_fail_closed() -> AnchorRoster {
    AnchorRoster::new()
}

// ── HUMAN decision: root-delegation policy (innovate:) ────────────────────────

/// innovate: The root-delegation model is an **OPERATOR decision**. This enum
/// lists all three supported models. The production system MUST NOT silently
/// pick one as "chosen" — the operator configures the policy explicitly. The
/// [`Default`] is [`RootDelegationPolicy::Unspecified`], which fails closed and
/// enrolls no root authority. Do not "helpfully" default to a real policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootDelegationPolicy {
    /// Operator-signed root certificate(s): offline, audited, pinned.
    OperatorSigned,
    /// Web-of-Trust: anchors accepted transitively from a trusted seed set.
    WebOfTrust,
    /// First-contact QR: out-of-band key exchange (e.g. scanned at commissioning).
    FirstContactQr,
    /// No policy chosen. FAIL-CLOSED: do not bootstrap any root authority.
    Unspecified,
}

impl Default for RootDelegationPolicy {
    /// Defaults to `Unspecified` on purpose: fail closed, never auto-pick a real
    /// policy. The operator must choose explicitly.
    fn default() -> Self {
        RootDelegationPolicy::Unspecified
    }
}

/// Require an explicit operator policy choice. Returns `Err` if the policy is
/// still [`RootDelegationPolicy::Unspecified`] — the node must not bootstrap any
/// root authority until the operator decides.
pub fn require_explicit_policy(p: RootDelegationPolicy) -> Result<RootDelegationPolicy, GenesisError> {
    match p {
        RootDelegationPolicy::Unspecified => Err(GenesisError::PolicyUnspecified),
        other => Ok(other),
    }
}

// ── small offline hex helpers (no external crate) ─────────────────────────────

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

fn hex_decode(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return Err("odd-length hex".to_string());
    }
    let mut out = Vec::with_capacity(s.len() / 2);
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = hex_val(bytes[i])?;
        let lo = hex_val(bytes[i + 1])?;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Ok(out)
}

fn hex_val(c: u8) -> Result<u8, String> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(format!("invalid hex char {:?}", c as char)),
    }
}

// ── RED tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roster::verify_chain;

    fn k(seed_byte: u8) -> ([u8; 32], [u8; 32]) {
        let seed = [seed_byte; 32];
        let (pk, _) = bebop2_core::sign::keygen(&seed);
        (seed, pk)
    }

    // RED: node_id recomputed from the SAME two pubkeys is identical, and
    // changing EITHER pubkey changes the id.
    #[test]
    fn red_node_id_recomputed_from_both_pubkeys_matches() {
        let (_s1, c1) = k(1u8);
        let pq1 = vec![0xaa; 1184]; // pretend ML-KEM-768 pk length

        let id_a = NodeId::from_keys(&pq1, &c1);
        let id_b = NodeId::from_keys(&pq1, &c1);
        assert_eq!(id_a, id_b, "same two pubkeys => same node_id");
        assert_eq!(id_a.to_hex(), id_b.to_hex());

        // Change the classical key -> different id.
        let (_s2, c2) = k(2u8);
        let id_c = NodeId::from_keys(&pq1, &c2);
        assert_ne!(id_a, id_c, "different classical key => different node_id");

        // Change the PQ key -> different id.
        let pq2 = vec![0xbb; 1184];
        let id_d = NodeId::from_keys(&pq2, &c1);
        assert_ne!(id_a, id_d, "different PQ key => different node_id");

        // And a NodeKeys round-trip behaves identically.
        let nk = NodeKeys { pq_pub: pq1, classical_pub: c1 };
        assert_eq!(nk.node_id(), id_a);
    }

    // RED: an empty roster is fail-closed — the node captures no authority.
    // load_genesis on a zero-anchor file is rejected (EmptyRoster), and an
    // empty roster refuses to vouch for any seeded-owner delegation.
    #[test]
    fn red_empty_roster_fail_closed_no_capture() {
        // empty_roster_fail_closed() must give a roster that contains nothing.
        let empty = empty_roster_fail_closed();
        assert!(empty.is_empty(), "fresh node captures no anchors");

        // A "seeded owner" (the old JWT-owner anti-pattern) key, used as the
        // root of a delegation, must be rejected because the roster is empty.
        let (_owner_seed, owner_pk) = k(7u8);
        let (_leaf_seed, leaf_pk) = k(8u8);
        let cap = Capability::new(leaf_pk, Resource::Route, Action::Send, [1u8; 8], 9999);
        let delegation = Delegation::sign(
            owner_pk, // issued_by == seeded owner (NOT enrolled anywhere)
            leaf_pk,
            Scope::single(Resource::Route, Action::Send),
            Effect::single(Resource::Route, Action::Send),
            9999,
            [2u8; 8],
            &_owner_seed,
        )
        .unwrap();
        let err = verify_chain(&empty, &[delegation], &cap, 0);
        assert!(
            matches!(err, Err(CapError::UnknownIssuer)),
            "empty roster must reject any root issuance (no capture), got {:?}",
            err
        );

        // load_genesis on an empty/comment-only file must fail closed.
        let dir = std::env::temp_dir();
        let path = dir.join("mesh12_empty_genesis.txt");
        std::fs::write(&path, "# only a comment\n\n").unwrap();
        let res = load_genesis(path.to_str().unwrap());
        assert!(
            matches!(res, Err(GenesisError::EmptyRoster)),
            "zero-anchor genesis must fail closed, got {:?}",
            res
        );
        let _ = std::fs::remove_file(&path);
    }

    // RED: a seeded-owner fixture cannot mint authority — there is nothing to
    // seed. The old "owner JWT" bootstrap is dead: a hardcoded owner key alone
    // grants no capability. (nothing-to-seed test)
    #[test]
    fn red_seeded_owner_fixture_cannot_mint() {
        // The "seeded owner" public key, hardcoded in the old bootstrap path.
        let (_owner_seed, owner_pk) = k(9u8);

        // Even presenting the owner key as the capability subject with no chain
        // and an empty roster yields no authority (UnknownIssuer path).
        let cap = Capability::new(owner_pk, Resource::Route, Action::Send, [3u8; 8], 9999);
        let empty = empty_roster_fail_closed();
        let err = verify_chain(&empty, &[], &cap, 0);
        assert!(
            matches!(err, Err(CapError::UnknownIssuer)),
            "seeded-owner with empty roster + no chain must be rejected, got {:?}",
            err
        );

        // And a self-signed owner->owner delegation (the literal "I am the owner"
        // mint) is rejected on an empty roster.
        let self_deleg = Delegation::sign(
            owner_pk,
            owner_pk,
            Scope::single(Resource::Route, Action::Send),
            Effect::single(Resource::Route, Action::Send),
            9999,
            [4u8; 8],
            &_owner_seed,
        )
        .unwrap();
        let err2 = verify_chain(&empty, &[self_deleg], &cap, 0);
        assert!(
            matches!(err2, Err(CapError::UnknownIssuer)),
            "seeded-owner self-mint must be rejected (nothing to seed), got {:?}",
            err2
        );
    }

    // GREEN (guard): load_genesis succeeds on a well-formed file and enrolls the
    // anchors; the policy enum defaults to Unspecified and must be chosen.
    #[test]
    fn green_load_genesis_ok_and_policy_must_be_chosen() {
        let (_a, a) = k(20u8);
        let (_b, b) = k(21u8);
        let dir = std::env::temp_dir();
        let path = dir.join("mesh12_genesis.txt");
        std::fs::write(
            &path,
            format!("# mesh-real genesis (frozen anchor set)\n{}\n{}\n", hex_encode(&a), hex_encode(&b)),
        )
        .unwrap();
        let roster = load_genesis(path.to_str().unwrap()).expect("valid genesis loads");
        assert!(roster.contains(&a));
        assert!(roster.contains(&b));
        assert!(!roster.is_empty());
        let _ = std::fs::remove_file(&path);

        // Policy defaults to Unspecified and must be explicitly chosen.
        assert_eq!(RootDelegationPolicy::default(), RootDelegationPolicy::Unspecified);
        assert!(matches!(
            require_explicit_policy(RootDelegationPolicy::default()),
            Err(GenesisError::PolicyUnspecified)
        ));
        // A real operator choice is accepted.
        assert_eq!(
            require_explicit_policy(RootDelegationPolicy::OperatorSigned).unwrap(),
            RootDelegationPolicy::OperatorSigned
        );
    }
}
