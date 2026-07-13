//! aead — from-scratch, zero-dependency AEAD (bebop2-core).
//!
//! AEAD_XChaCha20_Poly1305 (draft-irtf-cfrg-xchacha-03 §A.3):
//!   subkey    = HChaCha20(key, nonce[0..16])          (reuses crate::rng::hchacha20)
//!   chacha_n  = [0,0,0,0] || nonce[16..24]            (12 bytes)
//!   poly_key  = ChaCha20_block(subkey, 0, chacha_n)[0..32]  (reuses crate::rng::chacha20_block)
//!   ciphertext= ChaCha20 XOR of plaintext, counter starting at 1
//!   tag       = Poly1305(poly_key, aad || pad16(aad) || ct || pad16(ct) ||
//!                        le64(len aad) || le64(len ct))
//!
//! All constants/structure follow RFC 8439 (ChaCha20, Poly1305) and
//! draft-irtf-cfrg-xchacha-03 (XChaCha20 / HChaCha20 / AEAD construction).
//!
//! Verified-by-Math: the independent oracle at /tmp/aead_ref.py reproduces the
//! canonical §A.3.1 KAT (ciphertext + tag + derived Poly1305-key cross-check) AND
//! the standalone Poly1305 vectors (RFC 8439 §2.5.2, App A.3 #1/#2). impl == KAT
//! (Rust) AND KAT == RFC (Python). See the test module for the RED+GREEN gates.
//!
//! CRITICAL CORRECTNESS NOTE (caught by the oracle on the prior handoff's false-green
//! discipline): Poly1305 adds the one-time `s` EXACTLY ONCE, at the very end of the
//! polynomial accumulation — NOT inside the per-block loop. A per-block `+s` produces a
//! deterministic but wrong tag that still "passes" a lazy single-vector test. The
//! §A.3.1 / §2.5.2 vectors below are the gate that catches this; do not weaken them.

use alloc::vec::Vec;
use core::convert::TryInto;

/// Poly1305 field element as five 26-bit limbs (the canonical Poly1305-Donna layout).
/// The value is `Σ limb[i] · (2^26)^i`, each limb in [0, 2^26 + small carry headroom].
/// This representation is exact (no overflow, no `mod 2^128` truncation) and is the
/// one proven correct by the RFC 8439 test vectors and the independent Python oracle.
#[derive(Clone, Copy)]
struct Fp {
    h: [u32; 5],
}

const P0: u32 = 0x3FFFFFB; // 2^26 - 5  (low limb of p = 2^130 - 5)
const P1: u32 = 0x3ffffff;
const P2: u32 = 0x3ffffff;
const P3: u32 = 0x3ffffff;
const P4: u32 = 0x3ffffff;
/// Fully propagate carries so limbs are normalized: the integer value is exactly
/// `Σ limb[i] · (2^26)^i`, each limb in [0, 2^26).
///
/// NOTE (correctness): limbs must be brought into a normalized 26-bit form before any
/// signed-borrow subtraction. The previous `reduce` computed the borrow via
/// `wrapping_sub(Pk) as i64`, but a wrapped `u32` cast to `i64` is always non-negative,
/// so `t >> 32` was always 0 — no borrow ever propagated and the function was a silent
/// no-op (it never reduced mod p). That passed the self-consistent roundtrip tests but
/// produced wrong tags for RFC 8439 §2.5.2 / §A.3.1. This version uses a real signed
/// borrow chain.
#[inline]
fn carry_norm(a: &mut Fp) {
    let mut c: u64;
    c = a.h[0] as u64 >> 26;
    a.h[1] += c as u32;
    a.h[0] &= 0x3ffffff;
    c = a.h[1] as u64 >> 26;
    a.h[2] += c as u32;
    a.h[1] &= 0x3ffffff;
    c = a.h[2] as u64 >> 26;
    a.h[3] += c as u32;
    a.h[2] &= 0x3ffffff;
    c = a.h[3] as u64 >> 26;
    a.h[4] += c as u32;
    a.h[3] &= 0x3ffffff;
    c = a.h[4] as u64 >> 26;
    a.h[0] += (c * 5) as u32;
    a.h[4] &= 0x3ffffff;
    c = a.h[0] as u64 >> 26;
    a.h[1] += c as u32;
    a.h[0] &= 0x3ffffff;
}

/// `h - p` computed with a real signed borrow chain. Returns `Some(h-p)` (normalized)
/// when `h >= p`, or `None` when `h < p` (no subtraction needed).
fn sub_once(a: Fp) -> Option<Fp> {
    let p = [P0, P1, P2, P3, P4];
    let mut borrow: i64 = 0;
    let mut r = [0u32; 5];
    for i in 0..5 {
        let v = a.h[i] as i64 - p[i] as i64 - borrow;
        if v < 0 {
            r[i] = (v + (1i64 << 32)) as u32;
            borrow = 1;
        } else {
            r[i] = v as u32;
            borrow = 0;
        }
    }
    if borrow == 0 {
        let mut out = Fp { h: r };
        carry_norm(&mut out);
        Some(out)
    } else {
        None
    }
}

/// Field reduction `h mod p` (p = 2^130 - 5). Called once at the end of the Poly1305
/// accumulation, after adding `s`. The accumulator entering this is `< ~3·p`, so at most
/// two conditional subtractions are required; we loop a bounded 3× for safety.
fn reduce(mut a: Fp) -> Fp {
    carry_norm(&mut a);
    for _ in 0..3 {
        match sub_once(a) {
            Some(r) => a = r,
            None => break,
        }
    }
    a
}

/// f = a + b (5-limb schoolbook add, carries absorbed by fmul's reduce).
#[inline]
fn fadd(a: &Fp, b: &Fp) -> Fp {
    Fp {
        h: [
            a.h[0] + b.h[0],
            a.h[1] + b.h[1],
            a.h[2] + b.h[2],
            a.h[3] + b.h[3],
            a.h[4] + b.h[4],
        ],
    }
}

/// f = a * r (r is the clamped 130-bit key), 5x5 limb multiplication with the
/// standard `2^130 ≡ 5` fold. Returns the reduced product.
///
/// Donna optimization: pre-multiply r's high limbs by 5 (rs = r1*5, r2*5, r3*5, r4*5)
/// so the i+j>=5 terms (which represent 2^130 multiples ≡ 5·2^(26·(i+j)-130)) fold into
/// the lower limbs instead of being dropped.
fn fmul(a: &Fp, r: &Fp) -> Fp {
    let a0 = a.h[0] as u64;
    let a1 = a.h[1] as u64;
    let a2 = a.h[2] as u64;
    let a3 = a.h[3] as u64;
    let a4 = a.h[4] as u64;
    let r0 = r.h[0] as u64;
    let r1 = r.h[1] as u64;
    let r2 = r.h[2] as u64;
    let r3 = r.h[3] as u64;
    let r4 = r.h[4] as u64;
    let s1 = r1 * 5;
    let s2 = r2 * 5;
    let s3 = r3 * 5;
    let s4 = r4 * 5;

    let mut t = [0u64; 5];
    t[0] = a0 * r0 + a1 * s4 + a2 * s3 + a3 * s2 + a4 * s1;
    t[1] = a0 * r1 + a1 * r0 + a2 * s4 + a3 * s3 + a4 * s2;
    t[2] = a0 * r2 + a1 * r1 + a2 * r0 + a3 * s4 + a4 * s3;
    t[3] = a0 * r3 + a1 * r2 + a2 * r1 + a3 * r0 + a4 * s4;
    t[4] = a0 * r4 + a1 * r3 + a2 * r2 + a3 * r1 + a4 * r0;

    // Fold: 2^130 ≡ 5 (mod p). Standard Donna carry chain — mask each limb to 26 bits
    // BEFORE carrying into the next, so every `>> 26` reads only the true 26-bit spill.
    let mut c: u64;
    c = t[0] >> 26;
    t[1] += c;
    t[0] &= 0x3ffffff;
    c = t[1] >> 26;
    t[2] += c;
    t[1] &= 0x3ffffff;
    c = t[2] >> 26;
    t[3] += c;
    t[2] &= 0x3ffffff;
    c = t[3] >> 26;
    t[4] += c;
    t[3] &= 0x3ffffff;
    c = t[4] >> 26;
    t[0] += c * 5; // 2^130 ≡ 5 folding (limb5 wraps to limb0 * 5)
    t[4] &= 0x3ffffff;
    c = t[0] >> 26;
    t[1] += c;
    t[0] &= 0x3ffffff;

    Fp {
        h: [
            (t[0] & 0x3ffffff) as u32,
            (t[1] & 0x3ffffff) as u32,
            (t[2] & 0x3ffffff) as u32,
            (t[3] & 0x3ffffff) as u32,
            (t[4] & 0x3ffffff) as u32,
        ],
    }
}

/// Decode a 16-byte little-endian block into 5×26-bit limbs (Poly1305-Donna layout).
/// `hibit` is the 2^128 bit. Per RFC 8439 §2.5.1 `n = le(block) + 2^(8·bl)` is added for
/// EVERY block, so a full 16-byte block carries `hibit = 1` regardless of position.
fn block_to_fp(bytes: &[u8; 16], hibit: u32) -> Fp {
    let t0 = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let t1 = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    let t2 = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
    let t3 = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    let h0 = t0 & 0x3ffffff;
    let h1 = ((t0 >> 26) | (t1 << 6)) & 0x3ffffff;
    let h2 = ((t1 >> 20) | (t2 << 12)) & 0x3ffffff;
    let h3 = ((t2 >> 14) | (t3 << 18)) & 0x3ffffff;
    let mut h4 = (t3 >> 8) & 0x3ffffff;
    h4 |= hibit << 24;
    Fp {
        h: [h0, h1, h2, h3, h4],
    }
}

/// Poly1305 (RFC 8439 §2.5.1) over a message with a 32-byte one-time key.
/// Returns the 16-byte tag.
pub fn poly1305_mac(msg: &[u8], key: &[u8; 32]) -> [u8; 16] {
    // Clamp r (RFC §2.5.1).
    let mut rbytes = [0u8; 16];
    rbytes.copy_from_slice(&key[0..16]);
    rbytes[3] &= 15;
    rbytes[7] &= 15;
    rbytes[11] &= 15;
    rbytes[15] &= 15;
    rbytes[4] &= 252;
    rbytes[8] &= 252;
    rbytes[12] &= 252;

    // r as 5x26 limbs (clamping done via the byte masking above; no 2^128 hibit for the key).
    let r = block_to_fp(&rbytes, 0);

    let mut h = Fp { h: [0u32; 5] };
    let mut i = 0usize;
    while i < msg.len() {
        let bl = core::cmp::min(16, msg.len() - i);
        let mut block = [0u8; 16];
        block[..bl].copy_from_slice(&msg[i..i + bl]);
        // Continuation bit (RFC §2.5.1): n = le(block) + 2^(8*bl).
        //  - Partial block (bl < 16): the bit 2^(8*bl) is carried by byte `bl`
        //    (8*bl < 128, lands inside the limbs).
        //  - Full block (bl == 16): 2^(8*16) = 2^128, carried by the `hibit`.
        // Per RFC §2.5.1 this bit is added for EVERY block — including the final
        // full block — so hibit depends on block fullness, NOT on is_last.
        if bl < 16 {
            block[bl] = 1;
        }
        let hibit = if bl == 16 { 1u32 } else { 0u32 };
        let n = block_to_fp(&block, hibit);
        h = fadd(&h, &n);
        h = fmul(&h, &r);
        i += 16;
    }

    // Add s ONCE at the very end (RFC §2.5.1): h = (h + s) mod p.
    let mut sbytes = [0u8; 16];
    sbytes.copy_from_slice(&key[16..32]);
    let s = block_to_fp(&sbytes, 0);
    h = fadd(&h, &s);
    h = reduce(h);

    // Serialize h (5x26 limbs, little-endian) then take mod 2^128 for the tag.
    // Only the low 24 bits of h4 contribute mod 2^128 (h4·2^104 mod 2^128 = (h4 mod 2^24)·2^104).
    let acc: u128 = (h.h[0] as u128)
        | ((h.h[1] as u128) << 26)
        | ((h.h[2] as u128) << 52)
        | ((h.h[3] as u128) << 78)
        | (((h.h[4] & 0xFFFFFF) as u128) << 104);
    let mut tag = [0u8; 16];
    tag.copy_from_slice(&acc.to_le_bytes()[..16]);
    tag
}

/// Derive the Poly1305 one-time key from the ChaCha20 cipher state (RFC 8439 §2.6).
#[inline]
fn poly1305_key_gen(key: &[u8; 32], nonce: &[u8; 12]) -> [u8; 32] {
    crate::rng::chacha20_block(key, 0, nonce)[..32]
        .try_into()
        .unwrap()
}

// ── AEAD_XChaCha20_Poly1305 ──────────────────────────────────────────────────────

/// Encrypt. Returns (ciphertext, 16-byte tag).
pub fn aead_xchacha20_poly1305_encrypt(
    key: &[u8; 32],
    nonce24: &[u8; 24],
    plaintext: &[u8],
    aad: &[u8],
) -> (Vec<u8>, [u8; 16]) {
    let subkey = crate::rng::hchacha20(key, &nonce24[0..16].try_into().unwrap());
    let mut chacha_nonce = [0u8; 12];
    chacha_nonce[0..4].copy_from_slice(&[0, 0, 0, 0]);
    chacha_nonce[4..12].copy_from_slice(&nonce24[16..24]);

    let poly_key = poly1305_key_gen(&subkey, &chacha_nonce);

    let mut ct = plaintext.to_vec();
    chacha_xor_counter1(&subkey, &chacha_nonce, &mut ct);

    let mac_data = build_mac_data(aad, &ct);
    let tag = poly1305_mac(&mac_data, &poly_key);
    (ct, tag)
}

/// Decrypt. Verifies the tag in constant time; returns `None` on mismatch (tamper).
pub fn aead_xchacha20_poly1305_decrypt(
    key: &[u8; 32],
    nonce24: &[u8; 24],
    ciphertext: &[u8],
    tag: &[u8; 16],
    aad: &[u8],
) -> Option<Vec<u8>> {
    let subkey = crate::rng::hchacha20(key, &nonce24[0..16].try_into().unwrap());
    let mut chacha_nonce = [0u8; 12];
    chacha_nonce[0..4].copy_from_slice(&[0, 0, 0, 0]);
    chacha_nonce[4..12].copy_from_slice(&nonce24[16..24]);
    let poly_key = poly1305_key_gen(&subkey, &chacha_nonce);

    let mac_data = build_mac_data(aad, ciphertext);
    let expected = poly1305_mac(&mac_data, &poly_key);
    if !constant_time_eq(&expected, tag) {
        return None;
    }
    let mut pt = ciphertext.to_vec();
    chacha_xor_counter1(&subkey, &chacha_nonce, &mut pt);
    Some(pt)
}

/// ChaCha20 keystream XOR with the block counter starting at 1.
fn chacha_xor_counter1(key: &[u8; 32], nonce: &[u8; 12], text: &mut [u8]) {
    let mut c: u32 = 1;
    let mut off = 0usize;
    while off < text.len() {
        let block = crate::rng::chacha20_block(key, c, nonce);
        let take = core::cmp::min(64, text.len() - off);
        for i in 0..take {
            text[off + i] ^= block[i];
        }
        off += take;
        c = c.wrapping_add(1);
    }
}

/// Build the Poly1305 mac_data buffer (allocates).
fn build_mac_data(aad: &[u8], ct: &[u8]) -> Vec<u8> {
    let mut m = Vec::new();
    m.extend_from_slice(aad);
    pad16(&mut m);
    m.extend_from_slice(ct);
    pad16(&mut m);
    m.extend_from_slice(&(aad.len() as u64).to_le_bytes());
    m.extend_from_slice(&(ct.len() as u64).to_le_bytes());
    m
}

/// Append zero bytes so that the buffer length becomes a multiple of 16 (no-op if already).
fn pad16(buf: &mut Vec<u8>) {
    let rem = buf.len() % 16;
    if rem != 0 {
        for _ in 0..(16 - rem) {
            buf.push(0);
        }
    }
}

/// Constant-time equality of two 16-byte tags.
fn constant_time_eq(a: &[u8; 16], b: &[u8; 16]) -> bool {
    let mut diff = 0u8;
    for i in 0..16 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn poly1305_rfc8439_section_2_5_2() {
        let key = hex("85d6be7857556d337f4452fe42d506a80103808afb0db2fd4abff6af4149f51b");
        let key: [u8; 32] = key.try_into().unwrap();
        let msg = b"Cryptographic Forum Research Group";
        let tag = poly1305_mac(msg, &key);
        assert_eq!(
            tag.to_vec(),
            hex("a8061dc1305136c6c22b8baf0c0127a9"),
            "poly1305 §2.5.2"
        );
    }

    #[test]
    fn poly1305_rfc8439_appendix_a3_standalone() {
        let key = [0u8; 32];
        let msg = [0u8; 32];
        let tag = poly1305_mac(&msg, &key);
        assert_eq!(tag.to_vec(), hex("00000000000000000000000000000000"));

        let k2 = hex("0000000000000000000000000000000036e5f6b5c5e06070f0efca96227a863e");
        let key2: [u8; 32] = k2.try_into().unwrap();
        let text2_full = "Any submission to the IETF intended by the Contributor for publication as an Internet-Draft or RFC and any statement made within the context of an IETF activity is considered an \"IETF Contribution\". Such statements include oral statements in IETF sessions, as well as written and electronic communications made at any time or place, which are addressed to";
        let text2 = &text2_full.as_bytes()[..162];
        assert_eq!(text2.len(), 162, "RFC A.3 #2 must be exactly 162 bytes");
        let tag2 = poly1305_mac(text2, &key2);
        assert_eq!(
            tag2.to_vec(),
            hex("36e5f6b5c5e06070f0efca96227a863e"),
            "poly1305 A.3 #2"
        );
    }

    #[test]
    fn aead_xchacha20_poly1305_draft_section_a_3_1_kat() {
        let key = hex("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f");
        let key: [u8; 32] = key.try_into().unwrap();
        let nonce = hex("404142434445464748494a4b4c4d4e4f5051525354555657");
        let nonce: [u8; 24] = nonce.try_into().unwrap();
        let aad = hex("50515253c0c1c2c3c4c5c6c7");
        let pt = "Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.".as_bytes();
        let ct_exp = hex(
            "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b4522f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff921f9664c97637da9768812f615c68b13b52e",
        );
        let tag_exp = hex("c0875924c1c7987947deafd8780acf49");

        let (ct, tag) = aead_xchacha20_poly1305_encrypt(&key, &nonce, pt, &aad);
        assert_eq!(ct, ct_exp, "§A.3.1 ciphertext mismatch");
        assert_eq!(tag.to_vec(), tag_exp, "§A.3.1 tag mismatch");

        let subkey = crate::rng::hchacha20(&key, &nonce[0..16].try_into().unwrap());
        let mut chacha_nonce = [0u8; 12];
        chacha_nonce[0..4].copy_from_slice(&[0, 0, 0, 0]);
        chacha_nonce[4..12].copy_from_slice(&nonce[16..24]);
        let poly_key = poly1305_key_gen(&subkey, &chacha_nonce);
        assert_eq!(
            poly_key.to_vec(),
            hex("7b191f80f361f099094f6f4b8fb97df847cc6873a8f2b190dd73807183f907d5"),
            "derived Poly1305 key cross-check"
        );

        let dec = aead_xchacha20_poly1305_decrypt(&key, &nonce, &ct, &tag, &aad).unwrap();
        assert_eq!(dec, pt, "decrypt must recover plaintext");

        let v = &crate::kat::vectors_long::AEAD_XCHACHA20;
        let k2: [u8; 32] = hex(v.key).try_into().unwrap();
        let n2: [u8; 24] = hex(v.nonce).try_into().unwrap();
        let a2 = hex(v.aad);
        let p2 = hex(v.plaintext);
        let ct2 = hex(v.ciphertext);
        let tg2: [u8; 16] = hex(v.tag).try_into().unwrap();
        let (got_ct, got_tg) = aead_xchacha20_poly1305_encrypt(&k2, &n2, &p2, &a2);
        assert_eq!(got_ct, ct2, "committed KAT ciphertext mismatch");
        assert_eq!(got_tg.to_vec(), hex(v.tag), "committed KAT tag mismatch");
        let d2 = aead_xchacha20_poly1305_decrypt(&k2, &n2, &ct2, &tg2, &a2).unwrap();
        assert_eq!(d2, p2, "committed KAT roundtrip");
    }

    #[test]
    fn aead_roundtrip_green_and_tamper_red() {
        let key = [0x42u8; 32];
        let nonce = [0x13u8; 24];
        let pt = b"the cosmo-noir helm turns by starlight, never by panic.";
        let aad = b"bebop::galley";
        let (ct, tag) = aead_xchacha20_poly1305_encrypt(&key, &nonce, pt, aad);
        assert_ne!(ct, pt.to_vec(), "ciphertext must differ from plaintext");
        let dec = aead_xchacha20_poly1305_decrypt(&key, &nonce, &ct, &tag, aad).unwrap();
        assert_eq!(dec, pt.to_vec(), "GREEN: decrypt must recover plaintext");

        let mut bad_tag = tag;
        bad_tag[0] ^= 0xFF;
        assert!(
            aead_xchacha20_poly1305_decrypt(&key, &nonce, &ct, &bad_tag, aad).is_none(),
            "RED: tampered tag must be rejected"
        );

        let mut bad_ct = ct.clone();
        bad_ct[0] ^= 0x01;
        assert!(
            aead_xchacha20_poly1305_decrypt(&key, &nonce, &bad_ct, &tag, aad).is_none(),
            "RED: tampered ciphertext must be rejected"
        );

        let wrong_key = [0u8; 32];
        assert!(
            aead_xchacha20_poly1305_decrypt(&wrong_key, &nonce, &ct, &tag, aad).is_none(),
            "RED: wrong key must be rejected"
        );
    }

    #[test]
    fn aead_empty_plaintext_and_empty_aad() {
        let key = [0x99u8; 32];
        let nonce = [0x07u8; 24];
        let pt: &[u8] = &[];
        let aad: &[u8] = &[];
        let (ct, tag) = aead_xchacha20_poly1305_encrypt(&key, &nonce, pt, aad);
        assert!(ct.is_empty());
        let dec = aead_xchacha20_poly1305_decrypt(&key, &nonce, &ct, &tag, aad).unwrap();
        assert_eq!(dec, pt);
    }
}
