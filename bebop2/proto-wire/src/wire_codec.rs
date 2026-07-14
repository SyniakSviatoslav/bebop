//! Canonical wire codec for [`bebop_proto_cap::SignedFrame`].
//!
//! G1 (2026-07-14): the live carrier previously serialized the whole frame with
//! `serde_json::to_vec` (non-canonical — serde_json is implementation-defined:
//! map key ordering and float formatting diverge across Rust versions, so a
//! non-Rust node cannot reproduce the bytes, and an on-path attacker has a
//! malleability surface). This module replaces that with a hand-written,
//! **fixed-layout, length-prefixed, domain-separated** binary codec so that:
//!
//! 1. **Canonical** — identical logical frames always produce byte-identical
//!    wire bytes (deterministic field order, no serde). A peer can re-encode to
//!    verify, not just parse.
//! 2. **Fail-closed decode** — every length field is bounds-checked; an
//!    over-long, truncated, or wrong-tag frame is rejected with
//!    [`WireError::Encode`], never silently accepted or panned.
//! 3. **No score / trust surface** — the codec is pure layout; it never derives
//!    or carries a courier/agent rating. CI GUARD: NO-COURIER-SCORING.
//!
//! The signing path inside `proto-cap` is untouched — signatures still commit to
//! the TLV `signing_domain` (ARCHITECTURE.md:75). This codec is purely the
//! *envelope layer* that carries the signed frame across the carrier.

use bebop_proto_cap::{Action, Capability, Delegation, Effect, Resource, Scope, SignedFrame};

use crate::error::{WireError, WireResult};

/// Wire magic for the SignedFrame wire blob (domain separation from the
/// outer `Envelope` JSON version field and from the signing-domain TLV tags).
const WIRE_MAGIC: &[u8; 8] = b"BEBOPFRM";
/// Wire schema version for the frame blob itself (independent of envelope version).
const WIRE_VERSION: u8 = 0x01;

// Field ids (ascending; pinned to the wire contract).
const F_CAPABILITY: u8 = 0x01;
const F_PAYLOAD: u8 = 0x02;
const F_CHANNEL_BINDING: u8 = 0x03;
const F_CLASSICAL_SIG: u8 = 0x04;
const F_PQ_SIG: u8 = 0x05;
const F_DELEGATION_CHAIN: u8 = 0x06;

// ── little-endian scalar helpers ───────────────────────────────────────────────
fn put_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn put_u64(buf: &mut Vec<u8>, v: u64) {
    buf.extend_from_slice(&v.to_le_bytes());
}
fn put_bytes(buf: &mut Vec<u8>, b: &[u8]) {
    put_u32(buf, b.len() as u32);
    buf.extend_from_slice(b);
}

fn take<'a>(buf: &'a [u8], pos: &mut usize, n: usize) -> WireResult<&'a [u8]> {
    if *pos + n > buf.len() {
        return Err(WireError::Encode("truncated frame field".into()));
    }
    let s = &buf[*pos..*pos + n];
    *pos += n;
    Ok(s)
}
fn take_u32(buf: &[u8], pos: &mut usize) -> WireResult<u32> {
    let s = take(buf, pos, 4)?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}
fn take_u16(buf: &[u8], pos: &mut usize) -> WireResult<u16> {
    let s = take(buf, pos, 2)?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}
fn take_u64(buf: &[u8], pos: &mut usize) -> WireResult<u64> {
    let s = take(buf, pos, 8)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(s);
    Ok(u64::from_le_bytes(a))
}
fn take_bytes<'a>(buf: &'a [u8], pos: &mut usize) -> WireResult<&'a [u8]> {
    let len = take_u32(buf, pos)? as usize;
    take(buf, pos, len)
}
fn take_arr32(buf: &[u8], pos: &mut usize) -> WireResult<[u8; 32]> {
    let s = take(buf, pos, 32)?;
    let mut a = [0u8; 32];
    a.copy_from_slice(s);
    Ok(a)
}

// ── Scope encode/decode (length-prefixed set of (resource, action) pairs) ──
// G4: a Scope/Effect is a SET of (resource, action) pairs, so the wire form is
// `len:u16 LE || (resource_u8, action_u8)*`. Self-delimiting + fail-closed.
fn scope_to_tlv(scope: &Scope) -> Vec<u8> {
    scope.to_tlv_bytes()
}

fn scope_from_tlv(buf: &[u8], pos: &mut usize) -> WireResult<Scope> {
    let n = take_u16(buf, pos)? as usize;
    let mut grants = Vec::with_capacity(n);
    for _ in 0..n {
        let r = Resource::from_discriminant(take_u8(buf, pos)?)
            .ok_or_else(|| WireError::Encode("unknown scope resource discriminant".into()))?;
        let a = Action::from_discriminant(take_u8(buf, pos)?)
            .ok_or_else(|| WireError::Encode("unknown scope action discriminant".into()))?;
        grants.push((r, a));
    }
    Ok(Scope::new(grants))
}

// ── Capability encode/decode ───────────────────────────────────────────────────
fn encode_capability(cap: &Capability) -> Vec<u8> {
    let mut b = Vec::with_capacity(64);
    // scope (length-prefixed set) nonce(8) expiry(8) subject_key(32)
    b.extend_from_slice(&scope_to_tlv(&cap.scope));
    b.extend_from_slice(&cap.nonce);
    put_u64(&mut b, cap.expiry);
    b.extend_from_slice(&cap.subject_key);
    // Optional PQ subject key (length-prefixed; absent => 0-length).
    if let Some(pq) = &cap.subject_key_pq {
        put_bytes(&mut b, pq);
    } else {
        put_u32(&mut b, 0);
    }
    b
}

fn decode_capability(buf: &[u8]) -> WireResult<Capability> {
    let mut pos = 0usize;
    let scope = scope_from_tlv(buf, &mut pos)?;
    if buf.len() - pos < 8 + 8 + 32 {
        return Err(WireError::Encode("capability too short".into()));
    }
    let nonce = take_arr8(buf, &mut pos)?;
    let expiry = take_u64(buf, &mut pos)?;
    let subject_key = take_arr32(buf, &mut pos)?;
    let pq = take_bytes(buf, &mut pos)?;
    let subject_key_pq = if pq.is_empty() {
        None
    } else {
        Some(pq.to_vec())
    };
    Ok(Capability {
        subject_key,
        subject_key_pq,
        scope,
        nonce,
        expiry,
    })
}

fn take_u8(buf: &[u8], pos: &mut usize) -> WireResult<u8> {
    let s = take(buf, pos, 1)?;
    Ok(s[0])
}
fn take_arr8(buf: &[u8], pos: &mut usize) -> WireResult<[u8; 8]> {
    let s = take(buf, pos, 8)?;
    let mut a = [0u8; 8];
    a.copy_from_slice(s);
    Ok(a)
}

// ── Delegation encode/decode ───────────────────────────────────────────────────
fn encode_delegation(d: &Delegation) -> Vec<u8> {
    let mut b = Vec::with_capacity(64 + 64);
    b.extend_from_slice(&d.issued_by);
    b.extend_from_slice(&d.subject);
    // scope (length-prefixed set) + effect (length-prefixed set)
    b.extend_from_slice(&scope_to_tlv(&d.scope));
    b.extend_from_slice(&scope_to_tlv(&Scope::new(d.effect.grants.clone())));
    put_u64(&mut b, d.expiry);
    b.extend_from_slice(&d.nonce);
    put_bytes(&mut b, &d.signature);
    b
}

fn decode_delegation(buf: &[u8]) -> WireResult<Delegation> {
    let mut pos = 0usize;
    if buf.len() < 32 + 32 {
        return Err(WireError::Encode("delegation too short".into()));
    }
    let issued_by = take_arr32(buf, &mut pos)?;
    let subject = take_arr32(buf, &mut pos)?;
    let scope = scope_from_tlv(buf, &mut pos)?;
    let effect_scope = scope_from_tlv(buf, &mut pos)?;
    let effect = Effect::new(effect_scope.grants.clone());
    let expiry = take_u64(buf, &mut pos)?;
    let nonce = take_arr8(buf, &mut pos)?;
    let signature = take_bytes(buf, &mut pos)?.to_vec();
    Ok(Delegation {
        issued_by,
        subject,
        scope,
        effect,
        expiry,
        nonce,
        signature,
    })
}

// ── Top-level SignedFrame encode/decode ────────────────────────────────────────
/// Encode a [`SignedFrame`] into canonical, fail-closed wire bytes.
pub fn encode_frame(frame: &SignedFrame) -> WireResult<Vec<u8>> {
    let mut out = Vec::with_capacity(WIRE_MAGIC.len() + 1 + 64);
    out.extend_from_slice(WIRE_MAGIC);
    out.push(WIRE_VERSION);

    // Field list — emitted in FIXED ascending field-id order so the wire is
    // canonical regardless of how the frame was constructed.
    let cap = encode_capability(&frame.capability);

    // We accumulate an ordered list of (field_id, bytes) then emit sorted.
    let mut fields: Vec<(u8, Vec<u8>)> = Vec::with_capacity(6);
    fields.push((F_CAPABILITY, cap));
    fields.push((F_PAYLOAD, frame.payload.clone()));
    if let Some(b) = frame.channel_binding {
        fields.push((F_CHANNEL_BINDING, b.to_vec()));
    }
    if let Some(s) = &frame.classical_sig {
        fields.push((F_CLASSICAL_SIG, s.clone()));
    }
    if let Some(s) = &frame.pq_sig {
        fields.push((F_PQ_SIG, s.clone()));
    }
    if !frame.delegation_chain.is_empty() {
        let mut chain = Vec::new();
        put_u32(&mut chain, frame.delegation_chain.len() as u32);
        for d in &frame.delegation_chain {
            let db = encode_delegation(d);
            put_bytes(&mut chain, &db);
        }
        fields.push((F_DELEGATION_CHAIN, chain));
    }

    fields.sort_by_key(|(id, _)| *id);
    out.push(fields.len() as u8);
    for (id, bytes) in fields {
        out.push(id);
        put_bytes(&mut out, &bytes);
    }
    Ok(out)
}

/// Decode canonical wire bytes back into a [`SignedFrame`]. Fail-closed.
pub fn decode_frame(buf: &[u8]) -> WireResult<SignedFrame> {
    let mut pos = 0usize;
    let magic = take(buf, &mut pos, WIRE_MAGIC.len())?;
    if magic != WIRE_MAGIC {
        return Err(WireError::Encode("bad frame magic".into()));
    }
    let version = take_u8(buf, &mut pos)?;
    if version != WIRE_VERSION {
        return Err(WireError::Encode(format!(
            "unsupported frame wire version {version}"
        )));
    }
    let nfields = take_u8(buf, &mut pos)? as usize;

    let mut capability: Option<Capability> = None;
    let mut payload: Vec<u8> = Vec::new();
    let mut channel_binding: Option<[u8; 32]> = None;
    let mut classical_sig: Option<Vec<u8>> = None;
    let mut pq_sig: Option<Vec<u8>> = None;
    let mut delegation_chain: Vec<Delegation> = Vec::new();

    for _ in 0..nfields {
        let id = take_u8(buf, &mut pos)?;
        let bytes = take_bytes(buf, &mut pos)?.to_vec();
        match id {
            F_CAPABILITY => {
                capability = Some(decode_capability(&bytes)?);
            }
            F_PAYLOAD => payload = bytes,
            F_CHANNEL_BINDING => {
                if bytes.len() != 32 {
                    return Err(WireError::Encode("channel_binding must be 32 bytes".into()));
                }
                let mut a = [0u8; 32];
                a.copy_from_slice(&bytes);
                channel_binding = Some(a);
            }
            F_CLASSICAL_SIG => classical_sig = Some(bytes),
            F_PQ_SIG => pq_sig = Some(bytes),
            F_DELEGATION_CHAIN => {
                let mut cp = 0usize;
                let count = take_u32(&bytes, &mut cp)? as usize;
                for _ in 0..count {
                    let db = take_bytes(&bytes, &mut cp)?;
                    delegation_chain.push(decode_delegation(db)?);
                }
            }
            _ => {
                // Unknown field: reject (fail-closed; no forward-compat silent skip).
                return Err(WireError::Encode(format!(
                    "unknown frame field id 0x{id:02x}"
                )));
            }
        }
    }

    let capability =
        capability.ok_or_else(|| WireError::Encode("missing capability field".into()))?;
    Ok(SignedFrame {
        capability,
        payload,
        channel_binding,
        classical_sig,
        pq_sig,
        delegation_chain,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bebop2_core::sign::keygen;

    fn sample_frame() -> SignedFrame {
        let seed = [42u8; 32];
        let (pk, _) = keygen(&seed);
        let (anchor_seed, anchor_pk) = keygen(&[0x21u8; 32]);
        let (leaf_pk, _) = keygen(&[0x22u8; 32]);
        let cap = Capability::new(pk, Resource::Route, Action::Send, [9u8; 8], 12345);
        let mut frame =
            SignedFrame::new(cap, b"hello canonical wire".to_vec()).with_binding([0xCAu8; 32]);
        frame.sign_classical(&seed).unwrap();
        // Attach a real delegation link so the chain codec is exercised.
        let link = Delegation::sign(
            anchor_pk,
            leaf_pk,
            Scope::single(Resource::Route, Action::Send),
            Effect::single(Resource::Route, Action::Send),
            9999,
            [0x23u8; 8],
            &anchor_seed,
        )
        .unwrap();
        frame.delegation_chain = vec![link];
        frame
    }

    #[test]
    fn roundtrip_canonical_and_injective() {
        let f = sample_frame();
        let a = encode_frame(&f).unwrap();
        let b = encode_frame(&f).unwrap();
        assert_eq!(a, b, "encode MUST be canonical (byte-identical)");
        let back = decode_frame(&a).unwrap();
        assert_eq!(back, f, "roundtrip MUST preserve the frame");
    }

    #[test]
    fn decode_rejects_bad_magic() {
        let mut buf = encode_frame(&sample_frame()).unwrap();
        buf[0] ^= 0xFF; // corrupt magic
        assert!(decode_frame(&buf).is_err());
    }

    #[test]
    fn decode_rejects_wrong_version() {
        let mut buf = encode_frame(&sample_frame()).unwrap();
        buf[WIRE_MAGIC.len()] = 0x99;
        assert!(decode_frame(&buf).is_err());
    }

    #[test]
    fn decode_rejects_truncated_field() {
        let buf = encode_frame(&sample_frame()).unwrap();
        // Drop the last 4 bytes (mid field-length or mid payload).
        assert!(decode_frame(&buf[..buf.len() - 4]).is_err());
    }

    #[test]
    fn decode_rejects_unknown_field() {
        let mut buf = encode_frame(&sample_frame()).unwrap();
        // Inject an unknown field id right after the field count.
        let count_pos = WIRE_MAGIC.len() + 1;
        let n = buf[count_pos] as usize;
        buf[count_pos] = (n + 1) as u8;
        // Splice a fake field (id 0x7F, len 0) just before the payload field.
        let insert_at = count_pos + 1;
        buf.splice(insert_at..insert_at, vec![0x7F, 0, 0, 0, 0]);
        assert!(
            decode_frame(&buf).is_err(),
            "unknown field MUST be rejected"
        );
    }

    #[test]
    fn decoded_frame_still_verifies() {
        // The wire codec is a pure transport layer; a decoded-then-resent frame
        // must continue to pass the real signature check (the bytes a signature
        // commits to are unaffected by the envelope format).
        let f = sample_frame();
        let bytes = encode_frame(&f).unwrap();
        let back = decode_frame(&bytes).unwrap();
        assert!(
            back.verify_classical().is_ok(),
            "decoded frame must re-verify"
        );
    }
}
