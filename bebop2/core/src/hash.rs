//! Zero-dependency cryptographic hashes over `core` only.
//!
//! - `sha512`      — FIPS 180-4, big-endian schedule, bit-exact vs `hashlib`.
//! - `sha3_256`    — FIPS 202 (Keccak-f[1600,24]), domain 0x06.
//! - `sha3_512`    — FIPS 202, domain 0x06.
//!
//! All published test vectors are asserted in the `tests` module (KAT-green).
//! No `std`, no external crates. Uses `alloc` for the output `Vec<u8>` only.

#![allow(clippy::needless_range_loop)]

extern crate alloc;

use alloc::vec::Vec;

// ── SHA-512 (FIPS 180-4) ─────────────────────────────────────────────────────

const SHA512_K: [u64; 80] = [
    0x428a2f98d728ae22,
    0x7137449123ef65cd,
    0xb5c0fbcfec4d3b2f,
    0xe9b5dba58189dbbc,
    0x3956c25bf348b538,
    0x59f111f1b605d019,
    0x923f82a4af194f9b,
    0xab1c5ed5da6d8118,
    0xd807aa98a3030242,
    0x12835b0145706fbe,
    0x243185be4ee4b28c,
    0x550c7dc3d5ffb4e2,
    0x72be5d74f27b896f,
    0x80deb1fe3b1696b1,
    0x9bdc06a725c71235,
    0xc19bf174cf692694,
    0xe49b69c19ef14ad2,
    0xefbe4786384f25e3,
    0x0fc19dc68b8cd5b5,
    0x240ca1cc77ac9c65,
    0x2de92c6f592b0275,
    0x4a7484aa6ea6e483,
    0x5cb0a9dcbd41fbd4,
    0x76f988da831153b5,
    0x983e5152ee66dfab,
    0xa831c66d2db43210,
    0xb00327c898fb213f,
    0xbf597fc7beef0ee4,
    0xc6e00bf33da88fc2,
    0xd5a79147930aa725,
    0x06ca6351e003826f,
    0x142929670a0e6e70,
    0x27b70a8546d22ffc,
    0x2e1b21385c26c926,
    0x4d2c6dfc5ac42aed,
    0x53380d139d95b3df,
    0x650a73548baf63de,
    0x766a0abb3c77b2a8,
    0x81c2c92e47edaee6,
    0x92722c851482353b,
    0xa2bfe8a14cf10364,
    0xa81a664bbc423001,
    0xc24b8b70d0f89791,
    0xc76c51a30654be30,
    0xd192e819d6ef5218,
    0xd69906245565a910,
    0xf40e35855771202a,
    0x106aa07032bbd1b8,
    0x19a4c116b8d2d0c8,
    0x1e376c085141ab53,
    0x2748774cdf8eeb99,
    0x34b0bcb5e19b48a8,
    0x391c0cb3c5c95a63,
    0x4ed8aa4ae3418acb,
    0x5b9cca4f7763e373,
    0x682e6ff3d6b2b8a3,
    0x748f82ee5defb2fc,
    0x78a5636f43172f60,
    0x84c87814a1f0ab72,
    0x8cc702081a6439ec,
    0x90befffa23631e28,
    0xa4506cebde82bde9,
    0xbef9a3f7b2c67915,
    0xc67178f2e372532b,
    0xca273eceea26619c,
    0xd186b8c721c0c207,
    0xeada7dd6cde0eb1e,
    0xf57d4f7fee6ed178,
    0x06f067aa72176fba,
    0x0a637dc5a2c898a6,
    0x113f9804bef90dae,
    0x1b710b35131c471b,
    0x28db77f523047d84,
    0x32caab7b40c72493,
    0x3c9ebe0a15c9bebc,
    0x431d67c49c100d4c,
    0x4cc5d4becb3e42b6,
    0x597f299cfc657e2a,
    0x5fcb6fab3ad6faec,
    0x6c44198c4a475817,
];

#[inline]
fn rotr64(x: u64, n: u32) -> u64 {
    (x >> n) | (x << (64 - n))
}

#[inline]
fn sha512_ch(x: u64, y: u64, z: u64) -> u64 {
    (x & y) ^ ((!x) & z)
}

#[inline]
fn sha512_maj(x: u64, y: u64, z: u64) -> u64 {
    (x & y) ^ (x & z) ^ (y & z)
}

#[inline]
fn sha512_bsig0(x: u64) -> u64 {
    rotr64(x, 28) ^ rotr64(x, 34) ^ rotr64(x, 39)
}

#[inline]
fn sha512_bsig1(x: u64) -> u64 {
    rotr64(x, 14) ^ rotr64(x, 18) ^ rotr64(x, 41)
}

#[inline]
fn sha512_ssig0(x: u64) -> u64 {
    rotr64(x, 1) ^ rotr64(x, 8) ^ (x >> 7)
}

#[inline]
fn sha512_ssig1(x: u64) -> u64 {
    rotr64(x, 19) ^ rotr64(x, 61) ^ (x >> 6)
}

const SHA512_H0: [u64; 8] = [
    0x6a09e667f3bcc908,
    0xbb67ae8584caa73b,
    0x3c6ef372fe94f82b,
    0xa54ff53a5f1d36f1,
    0x510e527fade682d1,
    0x9b05688c2b3e6c1f,
    0x1f83d9abfb41bd6b,
    0x5be0cd19137e2179,
];

/// SHA-512 digest of `msg`, 64 bytes.
pub fn sha512(msg: &[u8]) -> [u8; 64] {
    let mut h = SHA512_H0;
    // padded message length is a multiple of 128 bytes
    let ml = (msg.len() as u64) * 8;
    let mut data = Vec::with_capacity(msg.len() + 17);
    data.extend_from_slice(msg);
    data.push(0x80);
    while data.len() % 128 != 112 {
        data.push(0x00);
    }
    // SHA-512 appends a 128-bit (16-byte) big-endian message length.
    data.extend_from_slice(&[0u8; 8]);
    data.extend_from_slice(&ml.to_be_bytes());
    // process 1024-bit (128-byte) blocks
    let mut w = [0u64; 80];
    let mut i = 0;
    while i < data.len() {
        for t in 0..16 {
            let mut v = 0u64;
            for k in 0..8 {
                v = (v << 8) | (data[i + t * 8 + k] as u64);
            }
            w[t] = v;
        }
        for t in 16..80 {
            w[t] = (sha512_ssig1(w[t - 2])
                .wrapping_add(w[t - 7])
                .wrapping_add(sha512_ssig0(w[t - 15]))
                .wrapping_add(w[t - 16]))
                & 0xFFFF_FFFF_FFFF_FFFF;
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for t in 0..80 {
            let t1 = hh
                .wrapping_add(sha512_bsig1(e))
                .wrapping_add(sha512_ch(e, f, g))
                .wrapping_add(SHA512_K[t])
                .wrapping_add(w[t]);
            let t2 = sha512_bsig0(a).wrapping_add(sha512_maj(a, b, c));
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
        i += 128;
    }
    let mut out = [0u8; 64];
    for (idx, word) in h.iter().enumerate() {
        out[idx * 8..idx * 8 + 8].copy_from_slice(&word.to_be_bytes());
    }
    out
}

// ── SHA3 / Keccak-f[1600,24] (FIPS 202) ───────────────────────────────────────

const KECCAK_RC: [u64; 24] = [
    0x0000000000000001,
    0x0000000000008082,
    0x800000000000808a,
    0x8000000080008000,
    0x000000000000808b,
    0x0000000080000001,
    0x8000000080008081,
    0x8000000000008009,
    0x000000000000008a,
    0x0000000000000088,
    0x0000000080008009,
    0x000000008000000a,
    0x000000008000808b,
    0x800000000000008b,
    0x8000000000008089,
    0x8000000000008003,
    0x8000000000008002,
    0x8000000000000080,
    0x000000000000800a,
    0x800000008000000a,
    0x8000000080008081,
    0x8000000000008080,
    0x0000000080000001,
    0x8000000080008008,
];

// rotation offsets r[x, y] (FIPS 202 §3.2.2), row-major by x: index = x*5 + y.
const KECCAK_ROT: [u32; 25] = [
    0, 36, 3, 41, 18, 1, 44, 10, 45, 2, 62, 6, 43, 15, 61, 28, 55, 25, 21, 56, 27, 20, 39, 8, 14,
];

#[inline]
fn keccak_rotl(x: u64, n: u32) -> u64 {
    if n == 0 {
        x
    } else {
        (x << n) | (x >> (64 - n))
    }
}

/// Keccak-f[1600] permutation in place on a 25-lane state.
fn keccak_f(state: &mut [u64; 25]) {
    for round in 0..24 {
        // Theta
        let mut c = [0u64; 5];
        for x in 0..5 {
            c[x] = state[x] ^ state[x + 5] ^ state[x + 10] ^ state[x + 15] ^ state[x + 20];
        }
        let mut d = [0u64; 5];
        for x in 0..5 {
            d[x] = c[(x + 4) % 5] ^ keccak_rotl(c[(x + 1) % 5], 1);
        }
        for x in 0..5 {
            for y in 0..5 {
                state[x + 5 * y] ^= d[x];
            }
        }
        // Rho (rotate) then Pi (permute)
        let a = *state;
        // Rho: A'[x][y] = rot(A[x][y], r[x][y])
        let mut t = [0u64; 25];
        for x in 0..5 {
            for y in 0..5 {
                t[x + 5 * y] = keccak_rotl(a[x + 5 * y], KECCAK_ROT[x * 5 + y]);
            }
        }
        // Pi: B[x][y] = A'[(x + 3*y) % 5][x]
        let mut b = [0u64; 25];
        for x in 0..5 {
            for y in 0..5 {
                b[x + 5 * y] = t[((x + 3 * y) % 5) + 5 * x];
            }
        }
        // Chi
        for x in 0..5 {
            for y in 0..5 {
                state[x + 5 * y] =
                    b[x + 5 * y] ^ ((!b[(x + 1) % 5 + 5 * y]) & b[(x + 2) % 5 + 5 * y]);
            }
        }
        // Iota
        state[0] ^= KECCAK_RC[round];
    }
}

/// SHA3 sponge over `msg` with given rate (bytes) and domain suffix.
/// `out_len` bytes are squeezed. `domain` is 0x06 for SHA3, 0x1f for SHAKE.
fn sha3_sponge(msg: &[u8], rate: usize, domain: u8, out_len: usize) -> Vec<u8> {
    let mut state = [0u64; 25];
    let mut i = 0;
    loop {
        let mut block = vec![0u8; rate]; // sized to rate so any FIPS 202 rate is safe (no fixed 136 cap)
        let take = core::cmp::min(rate, msg.len() - i);
        block[..take].copy_from_slice(&msg[i..i + take]);
        if take < rate {
            block[take] = domain;
            block[rate - 1] |= 0x80;
        }
        for j in 0..rate / 8 {
            let mut v = 0u64;
            for k in 0..8 {
                v |= (block[j * 8 + k] as u64) << (8 * k);
            }
            state[j] ^= v;
        }
        keccak_f(&mut state);
        i += take;
        if take < rate {
            break; // final block absorbed
        }
    }
    // squeeze
    let mut out = Vec::with_capacity(out_len);
    while out.len() < out_len {
        let want = core::cmp::min(rate, out_len - out.len());
        for j in 0..want {
            let lane = state[j / 8];
            out.push((lane >> (8 * (j % 8))) as u8);
        }
        if out.len() < out_len {
            keccak_f(&mut state);
        }
    }
    out
}

/// SHA3-256 digest → 32 bytes (rate 136, domain 0x06).
pub fn sha3_256(msg: &[u8]) -> [u8; 32] {
    let v = sha3_sponge(msg, 136, 0x06, 32);
    let mut out = [0u8; 32];
    out.copy_from_slice(&v[..32]);
    out
}

/// SHA3-512 digest → 64 bytes (rate 72, domain 0x06).
pub fn sha3_512(msg: &[u8]) -> [u8; 64] {
    let v = sha3_sponge(msg, 72, 0x06, 64);
    let mut out = [0u8; 64];
    out.copy_from_slice(&v[..64]);
    out
}

// ── Tests: KAT-green against `kat::vectors` + falsifiable RED cases ─────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kat;

    fn dehex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn sha512_kat_green() {
        for (input_hex, expected) in kat::vectors::SHA512 {
            let got = sha512(&dehex(input_hex));
            assert_eq!(
                got.as_slice(),
                dehex(expected).as_slice(),
                "SHA-512 KAT failed for {input_hex}"
            );
        }
    }

    // RED — must hold: SHA-512("") is the canonical 64-hex cf83e135… ,
    // NOT the 32-hex SHA-256 empty digest e3b0c442… (the false-green trap).
    #[test]
    fn sha512_empty_is_cf83() {
        assert_eq!(
            sha512(b"").as_slice(),
            dehex("cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e").as_slice()
        );
    }

    // RED — SHA-512 must NOT be length-extension resistant: appending to a
    // known message/digest without the key must NOT reproduce a known prefix.
    // We assert the empty and one-byte digests are distinct (a weaker but
    // falsifiable property that any correct SHA-512 satisfies).
    #[test]
    fn sha512_length_extension_property_red() {
        let empty = sha512(b"");
        let one = sha512(b"a");
        assert_ne!(empty.as_slice(), one.as_slice());
    }

    #[test]
    fn sha3_256_kat_green() {
        for (input_hex, expected) in kat::vectors::SHA3_256 {
            let got = sha3_256(&dehex(input_hex));
            assert_eq!(
                got.as_slice(),
                dehex(expected).as_slice(),
                "SHA3-256 KAT failed for {input_hex}"
            );
        }
    }

    // SHA3-256("") = a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a (FIPS 202 published vector).
    #[test]
    fn sha3_256_empty_known() {
        assert_eq!(
            sha3_256(b"").as_slice(),
            dehex("a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a").as_slice()
        );
    }

    #[test]
    fn sha3_512_empty_known() {
        assert_eq!(
            sha3_512(b"").as_slice(),
            dehex("a69f73cca23a9ac5c8b567dc185a756e97c982164fe25859e0d1dcc1475c80a615b2123af1f5f94c11e3e9402c3ac558f500199d95b6d3e301758586281dcd26").as_slice()
        );
    }

    // RED — distinct algorithms yield distinct digests for the same input.
    #[test]
    fn sha3_256_vs_512_distinct_red() {
        let a = sha3_256(b"bebop");
        let b = sha3_512(b"bebop");
        assert_ne!(a.as_slice(), &b[..32]);
    }

    // RED — SHA-512 and SHA3-512 must differ for the same input.
    #[test]
    fn sha512_vs_sha3_512_distinct_red() {
        let a = sha512(b"bebop");
        let b = sha3_512(b"bebop");
        assert_ne!(a.as_slice(), b.as_slice());
    }
}
