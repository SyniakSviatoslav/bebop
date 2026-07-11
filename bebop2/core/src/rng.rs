//! rng — from-scratch, zero-dependency ChaCha20-based CSPRNG (bebop2-core).
//!
//! Design (core-only, no std::time / OS RNG / network, wasm-empty-import safe):
//! - `chacha20_block` — RFC 8439 §2.3.1 primitive (also reused by `aead`).
//! - `hchacha20`     — draft-irtf-cfrg-xchacha §2.2 (XChaCha20 subkey derivation).
//! - `ChaCha20Rng`   — a counter-mode CSPRNG seeded from a 32-byte key + 12-byte nonce.
//!   Same seed → same stream (deterministic, verifiable). The "CSPRNG" property comes
//!   from ChaCha20's PRF security: an adversary who sees output cannot recover the seed
//!   or predict unobserved blocks without the key. In a real deployment the seed is drawn
//!   from hardware entropy (out of tree, since no OS RNG is reachable under wasm); here
//!   the API accepts the seed explicitly so the stream is testable.
//!
//! All vectors below are canonical (RFC 8439 / draft-irtf-cfrg-xchacha) — see
//! `kat::vectors_long::CHACHA20` and `kat::vectors::HCHACHA20`.

use core::convert::TryInto;

const CHACHA_CONSTANTS: [u32; 4] = [0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574];

/// ChaCha quarter round (RFC 8439 §2.1). Operates in place on `state[a,b,c,d]`.
#[inline]
fn quarter_round(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    s[a] = s[a].wrapping_add(s[b]);
    s[d] ^= s[a];
    s[d] = s[d].rotate_left(16);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] ^= s[c];
    s[b] = s[b].rotate_left(12);
    s[a] = s[a].wrapping_add(s[b]);
    s[d] ^= s[a];
    s[d] = s[d].rotate_left(8);
    s[c] = s[c].wrapping_add(s[d]);
    s[b] ^= s[c];
    s[b] = s[b].rotate_left(7);
}

/// RFC 8439 §2.3.1 — produce one 64-byte ChaCha20 block.
/// `key` = 32 bytes, `counter` = 32-bit block counter, `nonce` = 12 bytes.
pub fn chacha20_block(key: &[u8; 32], counter: u32, nonce: &[u8; 12]) -> [u8; 64] {
    let mut state = [0u32; 16];
    state[0..4].copy_from_slice(&CHACHA_CONSTANTS);
    for i in 0..8 {
        state[4 + i] = u32::from_le_bytes(key[4 * i..4 * i + 4].try_into().unwrap());
    }
    state[12] = counter;
    for i in 0..3 {
        state[13 + i] = u32::from_le_bytes(nonce[4 * i..4 * i + 4].try_into().unwrap());
    }

    let mut working = state;
    for _ in 0..10 {
        // column rounds
        quarter_round(&mut working, 0, 4, 8, 12);
        quarter_round(&mut working, 1, 5, 9, 13);
        quarter_round(&mut working, 2, 6, 10, 14);
        quarter_round(&mut working, 3, 7, 11, 15);
        // diagonal rounds
        quarter_round(&mut working, 0, 5, 10, 15);
        quarter_round(&mut working, 1, 6, 11, 12);
        quarter_round(&mut working, 2, 7, 8, 13);
        quarter_round(&mut working, 3, 4, 9, 14);
    }

    let mut out = [0u8; 64];
    for i in 0..16 {
        let word = working[i].wrapping_add(state[i]);
        out[4 * i..4 * i + 4].copy_from_slice(&word.to_le_bytes());
    }
    out
}

/// draft-irtf-cfrg-xchacha-03 §2.2 — HChaCha20.
/// `key` = 32 bytes, `nonce` = 16 bytes. Returns a 32-byte subkey
/// (first 128 bits + last 128 bits of the post-round state, little-endian).
pub fn hchacha20(key: &[u8; 32], nonce: &[u8; 16]) -> [u8; 32] {
    let mut state = [0u32; 16];
    state[0..4].copy_from_slice(&CHACHA_CONSTANTS);
    for i in 0..8 {
        state[4 + i] = u32::from_le_bytes(key[4 * i..4 * i + 4].try_into().unwrap());
    }
    for i in 0..4 {
        state[12 + i] = u32::from_le_bytes(nonce[4 * i..4 * i + 4].try_into().unwrap());
    }

    for _ in 0..10 {
        quarter_round(&mut state, 0, 4, 8, 12);
        quarter_round(&mut state, 1, 5, 9, 13);
        quarter_round(&mut state, 2, 6, 10, 14);
        quarter_round(&mut state, 3, 7, 11, 15);
        quarter_round(&mut state, 0, 5, 10, 15);
        quarter_round(&mut state, 1, 6, 11, 12);
        quarter_round(&mut state, 2, 7, 8, 13);
        quarter_round(&mut state, 3, 4, 9, 14);
    }

    let mut out = [0u8; 32];
    // first 128 bits (words 0..4) + last 128 bits (words 12..16)
    for i in 0..4 {
        out[4 * i..4 * i + 4].copy_from_slice(&state[i].to_le_bytes());
    }
    for i in 0..4 {
        out[16 + 4 * i..16 + 4 * i + 4].copy_from_slice(&state[12 + i].to_le_bytes());
    }
    out
}

/// A ChaCha20 counter-mode CSPRNG (RFC 8439 construction).
/// Deterministic: identical (key, nonce) yields an identical stream. Seed from
/// hardware entropy out of tree; the stream is fully specified by the 44-byte seed.
///
/// Deployment caveat: the 32-bit block counter wraps after 2^32 blocks (~256 GiB)
/// of output under a single (key, nonce), after which the keystream repeats. Re-seed
/// (fresh seed) well before that volume. This matches RFC 8439's 96-bit-nonce / 32-bit
/// counter design and is not a defect.
pub struct ChaCha20Rng {
    key: [u8; 32],
    nonce: [u8; 12],
    counter: u32,
    buf: [u8; 64],
    pos: usize,
}

impl ChaCha20Rng {
    /// Build a keystream generator from a 32-byte key and 12-byte nonce.
    pub fn new(key: [u8; 32], nonce: [u8; 12]) -> Self {
        ChaCha20Rng { key, nonce, counter: 0, buf: [0u8; 64], pos: 64 }
    }

    /// Convenience: seed the whole generator from a 44-byte seed
    /// (32-byte key || 12-byte nonce), the canonical "seed" shape.
    pub fn from_seed(seed: &[u8; 44]) -> Self {
        let mut key = [0u8; 32];
        let mut nonce = [0u8; 12];
        key.copy_from_slice(&seed[0..32]);
        nonce.copy_from_slice(&seed[32..44]);
        Self::new(key, nonce)
    }

    fn refill(&mut self) {
        self.buf = chacha20_block(&self.key, self.counter, &self.nonce);
        self.counter = self.counter.wrapping_add(1);
        self.pos = 0;
    }

    /// Fill `dest` with keystream bytes.
    pub fn fill_bytes(&mut self, dest: &mut [u8]) {
        let mut i = 0;
        while i < dest.len() {
            if self.pos == 64 {
                self.refill();
            }
            let take = core::cmp::min(64 - self.pos, dest.len() - i);
            dest[i..i + take].copy_from_slice(&self.buf[self.pos..self.pos + take]);
            self.pos += take;
            i += take;
        }
    }

    /// Next 32-bit word (little-endian word of the keystream).
    pub fn next_u32(&mut self) -> u32 {
        let mut b = [0u8; 4];
        self.fill_bytes(&mut b);
        u32::from_le_bytes(b)
    }

    /// Next 64-bit word.
    pub fn next_u64(&mut self) -> u64 {
        let mut b = [0u8; 8];
        self.fill_bytes(&mut b);
        u64::from_le_bytes(b)
    }
}

/// Stream-encrypt `plaintext` in place (XOR with the ChaCha20 keystream).
/// `counter` is the starting block counter (0 for a fresh message).
pub fn chacha20_xor(key: &[u8; 32], counter: u32, nonce: &[u8; 12], text: &mut [u8]) {
    let mut c = counter;
    let mut off = 0;
    while off < text.len() {
        let block = chacha20_block(key, c, nonce);
        let take = core::cmp::min(64, text.len() - off);
        for i in 0..take {
            text[off + i] ^= block[i];
        }
        off += take;
        c = c.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use core::convert::TryInto;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn chacha_quarter_round_rfc8439_2_1_1() {
        // RFC 8439 §2.1.1
        let mut s = [0u32; 16];
        s[0] = 0x1111_1111;
        s[1] = 0x0102_0304;
        s[2] = 0x9b8d_6f43;
        s[3] = 0x0123_4567;
        quarter_round(&mut s, 0, 1, 2, 3);
        assert_eq!(s[0], 0xea2a_92f4);
        assert_eq!(s[1], 0xcb1c_f8ce);
        assert_eq!(s[2], 0x4581_472e);
        assert_eq!(s[3], 0x5881_c4bb);
    }

    #[test]
    fn chacha_quarter_round_on_state_rfc8439_2_2_1() {
        // RFC 8439 §2.2.1 — QUARTERROUND(2,7,8,13) on a random state.
        let mut s: [u32; 16] = [
            0x8795_31e0, 0xc5ec_f37d, 0x5164_61b1, 0xc9a6_2f8a,
            0x44c2_0ef3, 0x3390_af7f, 0xd9fc_690b, 0x2a5f_714c,
            0x5337_2767, 0xb00a_5631, 0x974c_541a, 0x359e_9963,
            0x5c97_1061, 0x3d63_1689, 0x2098_d9d6, 0x91db_d320,
        ];
        quarter_round(&mut s, 2, 7, 8, 13);
        let exp: [u32; 16] = [
            0x8795_31e0, 0xc5ec_f37d, 0xbdb8_86dc, 0xc9a6_2f8a,
            0x44c2_0ef3, 0x3390_af7f, 0xd9fc_690b, 0xcfac_afd2,
            0xe46b_ea80, 0xb00a_5631, 0x974c_541a, 0x359e_9963,
            0x5c97_1061, 0xccc0_7c79, 0x2098_d9d6, 0x91db_d320,
        ];
        assert_eq!(s, exp);
    }

    #[test]
    fn chacha20_block_rfc8439_vectors_long() {
        // All canonical keystream blocks from kat::vectors_long (RFC 8439 Appendix A).
        for v in crate::kat::vectors_long::CHACHA20 {
            let key: [u8; 32] = hex(v.key).try_into().unwrap();
            let nonce: [u8; 12] = hex(v.nonce).try_into().unwrap();
            let got = chacha20_block(&key, v.counter, &nonce);
            assert_eq!(
                got.to_vec(),
                hex(v.keystream),
                "chacha20_block counter={} mismatch",
                v.counter
            );
        }
    }

    #[test]
    fn chacha20_block_section_2_3_2() {
        // RFC 8439 §2.3.2 (counter=1, nonce 09 00 00 00 4a 00 00 00).
        let key = hex("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f");
        let nonce = hex("000000090000004a00000000");
        let got = chacha20_block(&key.try_into().unwrap(), 1, &nonce.try_into().unwrap());
        let exp = hex(
            "10f1e7e4d13b5915500fdd1fa32071c4\
             c7d1f4c733c068030422aa9ac3d46c4e\
             d2826446079faa0914c2d705d98b02a2\
             b5129cd1de164eb9cbd083e8a2503c4e",
        );
        assert_eq!(got.to_vec(), exp);
    }

    #[test]
    fn hchacha20_draft_xchacha_2_2_1() {
        // draft-irtf-cfrg-xchacha-03 §2.2.1 (canonical; the committed vector was
        // corrected — prior nonce had a corrupted tail).
        let v = &crate::kat::vectors::HCHACHA20;
        let key: [u8; 32] = hex(v.key).try_into().unwrap();
        let nonce: [u8; 16] = hex(v.nonce).try_into().unwrap();
        let got = hchacha20(&key, &nonce);
        assert_eq!(got.to_vec(), hex(v.out), "hchacha20 mismatch");
    }

    #[test]
    fn chacha20_xor_roundtrip_red_green() {
        // RED+GREEN: XOR-encrypt then XOR-decrypt with the same (key,nonce) recovers plaintext.
        // GREEN path asserts recovery; RED path (wrong key) must NOT recover.
        let key = [7u8; 32];
        let nonce = [9u8; 12];
        let pt = b"the cosmo-noir helm turns by starlight, never by panic.";
        let mut ct = pt.to_vec();
        chacha20_xor(&key, 0, &nonce, &mut ct);
        assert_ne!(ct, pt.to_vec(), "encryption must change the plaintext");

        let mut dec = ct.clone();
        chacha20_xor(&key, 0, &nonce, &mut dec);
        assert_eq!(dec, pt.to_vec(), "decrypt must recover plaintext");

        // RED: a different key must produce a different (non-recovering) stream.
        let mut wrong = ct.clone();
        chacha20_xor(&[0u8; 32], 0, &nonce, &mut wrong);
        assert_ne!(wrong, pt.to_vec(), "wrong key must NOT recover plaintext");
    }

    #[test]
    fn rng_deterministic_same_seed_same_stream() {
        // The CSPRNG is deterministic: same seed => identical 256-byte stream.
        let seed = [0xABu8; 44];
        let mut a = ChaCha20Rng::from_seed(&seed);
        let mut b = ChaCha20Rng::from_seed(&seed);
        let mut sa = [0u8; 256];
        let mut sb = [0u8; 256];
        a.fill_bytes(&mut sa);
        b.fill_bytes(&mut sb);
        assert_eq!(sa, sb, "same seed must yield identical stream");
    }

    #[test]
    fn rng_different_seed_different_stream() {
        // Different seed => different stream (property, not a fixed vector).
        let mut s1 = [0u8; 44];
        let mut s2 = [0u8; 44];
        s1[0] = 1;
        s2[0] = 2;
        let mut a = ChaCha20Rng::from_seed(&s1);
        let mut b = ChaCha20Rng::from_seed(&s2);
        let mut x = [0u8; 64];
        let mut y = [0u8; 64];
        a.fill_bytes(&mut x);
        b.fill_bytes(&mut y);
        assert_ne!(x, y, "different seeds must yield different streams");
    }

    #[test]
    fn rng_stream_matches_block_function_block_stitching() {
        // Verifies ChaCha20Rng.fill_bytes stitches chacha20_block(counter=0,1,2,...)
        // correctly across exact-block and partial-block boundaries (200 bytes spans
        // 3 full blocks + 8 bytes of block 3). This is an IN-CRATE consistency check;
        // the independent Python oracle for chacha20_block lives at /tmp/chacha_ref.py.
        let seed = [0x5Au8; 44];
        let mut key = [0u8; 32];
        let mut nonce = [0u8; 12];
        key.copy_from_slice(&seed[0..32]);
        nonce.copy_from_slice(&seed[32..44]);
        let mut rng = ChaCha20Rng::from_seed(&seed);
        let mut stream = [0u8; 200];
        rng.fill_bytes(&mut stream);

        let mut off = 0u32;
        let mut blk = 0u32;
        while off < 200 {
            let take = core::cmp::min(64, 200 - off as usize);
            let block = chacha20_block(&key, blk, &nonce);
            assert_eq!(
                &stream[off as usize..off as usize + take],
                &block[..take],
                "rng stream [{},{}) must match chacha20_block({})",
                off,
                off + take as u32,
                blk
            );
            off += take as u32;
            blk += 1;
        }
        assert_eq!(blk, 4, "200 bytes must consume 3 full + 1 partial block");
    }

    #[test]
    fn rng_avalanche_seed_bit_flip_changes_stream() {
        // A one-bit seed change must flip the entire first block (avalanche / PRF property).
        let mut base = [0u8; 44];
        for (i, b) in base.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(37).wrapping_add(11);
        }
        let mut flipped = base;
        flipped[10] ^= 0x80; // flip a high bit in the key region
        let mut a = ChaCha20Rng::from_seed(&base);
        let mut b = ChaCha20Rng::from_seed(&flipped);
        let mut xa = [0u8; 64];
        let mut xb = [0u8; 64];
        a.fill_bytes(&mut xa);
        b.fill_bytes(&mut xb);
        let diffs = xa.iter().zip(xb.iter()).filter(|(x, y)| x != y).count();
        assert!(diffs >= 32, "seed bit-flip should flip ~half the block, got {diffs}/64 diffs");
    }
}
