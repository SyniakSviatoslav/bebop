//! Argon2id (RFC 9106, Argon2 v1.3, version 0x13) — from scratch, zero-dep.
//!
//! Faithful Rust port of the public-domain Argon2 reference implementation
//! (P-H-C/phc-winner-argon2, CC0 1.0 / Apache 2.0), using an in-tree from-scratch
//! BLAKE2b-512 (RFC 7693) for the internal H / H' hash functions.
//!
//! Anchored to authoritative known-answer vectors (see `#[cfg(test)]`):
//!   - BLAKE2b-512("") and BLAKE2b-512("abc") (RFC 7693 Appendix A trace).
//!   - Argon2id test vector (RFC 9106 §5.3): m=32 KiB, t=3, p=4, tag=32 B.
//!
//! No heap RNG, no std; uses `alloc::vec` for the memory matrix (native + test
//! builds link std so this is fine; the wasm32 no_std path is a SEPARATE later
//! hardening step — see AGENTS.md §0).

use alloc::vec::Vec;

// ── BLAKE2b-512 (RFC 7693) ────────────────────────────────────────────────────

const BLAKE2B_IV: [u64; 8] = [
    0x6a09e667f3bcc908,
    0xbb67ae8584caa73b,
    0x3c6ef372fe94f82b,
    0xa54ff53a5f1d36f1,
    0x510e527fade682d1,
    0x9b05688c2b3e6c1f,
    0x1f83d9abfb41bd6b,
    0x5be0cd19137e2179,
];

const BLAKE2B_SIGMA: [[usize; 16]; 10] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
];

#[inline]
fn f_bla_mka(x: u64, y: u64) -> u64 {
    let m = 0xffff_ffffu64;
    let xy = (x & m).wrapping_mul(y & m);
    x.wrapping_add(y).wrapping_add(2u64.wrapping_mul(xy))
}

#[inline]
fn rotr64(x: u64, n: u32) -> u64 {
    (x >> n) | (x << (64 - n))
}

/// One BLAKE2b round over the 16 message words `m` with schedule `s`.
#[inline]
fn blake2_round_v(v: &mut [u64; 16], m: &[u64; 16], s: &[usize; 16]) {
    macro_rules! g {
        ($a:expr, $b:expr, $c:expr, $d:expr, $x:expr, $y:expr) => {{
            v[$a] = v[$a].wrapping_add(v[$b]).wrapping_add(m[s[$x]]);
            v[$d] = rotr64(v[$d] ^ v[$a], 32);
            v[$c] = v[$c].wrapping_add(v[$d]);
            v[$b] = rotr64(v[$b] ^ v[$c], 24);
            v[$a] = v[$a].wrapping_add(v[$b]).wrapping_add(m[s[$y]]);
            v[$d] = rotr64(v[$d] ^ v[$a], 16);
            v[$c] = v[$c].wrapping_add(v[$d]);
            v[$b] = rotr64(v[$b] ^ v[$c], 63);
        }};
    }
    g!(0, 4, 8, 12, 0, 1);
    g!(1, 5, 9, 13, 2, 3);
    g!(2, 6, 10, 14, 4, 5);
    g!(3, 7, 11, 15, 6, 7);
    g!(0, 5, 10, 15, 8, 9);
    g!(1, 6, 11, 12, 10, 11);
    g!(2, 7, 8, 13, 12, 13);
    g!(3, 4, 9, 14, 14, 15);
}

/// BLAKE2b round over 16 words with a CONSTANT message (Argon2 G uses m=0).
/// The Argon2 G compression uses fBlaMka (multiply-add), NOT plain addition —
/// this is the Lyra/Poly-chacha variant that distinguishes Argon2's G from
/// standalone BLAKE2b.
fn blake2_round_const(v: &mut [u64; 16]) {
    macro_rules! g {
        ($a:expr, $b:expr, $c:expr, $d:expr) => {{
            v[$a] = f_bla_mka(v[$a], v[$b]);
            v[$d] = rotr64(v[$d] ^ v[$a], 32);
            v[$c] = f_bla_mka(v[$c], v[$d]);
            v[$b] = rotr64(v[$b] ^ v[$c], 24);
            v[$a] = f_bla_mka(v[$a], v[$b]);
            v[$d] = rotr64(v[$d] ^ v[$a], 16);
            v[$c] = f_bla_mka(v[$c], v[$d]);
            v[$b] = rotr64(v[$b] ^ v[$c], 63);
        }};
    }
    g!(0, 4, 8, 12);
    g!(1, 5, 9, 13);
    g!(2, 6, 10, 14);
    g!(3, 7, 11, 15);
    g!(0, 5, 10, 15);
    g!(1, 6, 11, 12);
    g!(2, 7, 8, 13);
    g!(3, 4, 9, 14);
}
#[inline]
fn blake2_compress(h: &mut [u64; 8], block: &[u8; 128], t: u64, last: bool) {
    let mut m = [0u64; 16];
    for i in 0..16 {
        m[i] = u64::from_le_bytes(block[i * 8..i * 8 + 8].try_into().unwrap());
    }
    let mut v = [0u64; 16];
    v[..8].copy_from_slice(h);
    v[8..].copy_from_slice(&BLAKE2B_IV);
    v[12] ^= t; // t low
    if last {
        v[14] ^= 0xffff_ffff_ffff_ffff; // last-block flag
    }
    for r in 0..12 {
        let s = &BLAKE2B_SIGMA[r % 10];
        blake2_round_v(&mut v, &m, s);
    }
    for i in 0..8 {
        h[i] ^= v[i] ^ v[i + 8];
    }
}

/// Streaming BLAKE2b-512 (RFC 7693), unkeyed, arbitrary-length input.
struct Blake2b {
    h: [u64; 8],
    t: u64,
    buf: [u8; 128],
    buflen: usize,
    outlen: usize,
}

impl Blake2b {
    fn new(outlen: usize) -> Self {
        let mut h = BLAKE2B_IV;
        h[0] ^= 0x0101_0000u64 ^ (outlen as u64);
        Blake2b {
            h,
            t: 0,
            buf: [0u8; 128],
            buflen: 0,
            outlen,
        }
    }
    fn update(&mut self, mut data: &[u8]) {
        while !data.is_empty() {
            if self.buflen == 0 && data.len() >= 128 {
                let block: [u8; 128] = data[..128].try_into().unwrap();
                self.t = self.t.wrapping_add(128);
                blake2_compress(&mut self.h, &block, self.t, false);
                data = &data[128..];
            } else {
                let take = core::cmp::min(128 - self.buflen, data.len());
                self.buf[self.buflen..self.buflen + take].copy_from_slice(&data[..take]);
                self.buflen += take;
                data = &data[take..];
                if self.buflen == 128 {
                    let block = self.buf;
                    self.t = self.t.wrapping_add(128);
                    blake2_compress(&mut self.h, &block, self.t, false);
                    self.buflen = 0;
                }
            }
        }
    }
    fn finalize(mut self) -> Vec<u8> {
        self.t = self.t.wrapping_add(self.buflen as u64);
        // pad
        let mut block = [0u8; 128];
        block[..self.buflen].copy_from_slice(&self.buf[..self.buflen]);
        blake2_compress(&mut self.h, &block, self.t, true);
        let mut out = alloc::vec![0u8; self.outlen];
        for i in 0..self.outlen {
            out[i] = (self.h[i / 8] >> (8 * (i % 8))) as u8;
        }
        out
    }
}

/// BLAKE2b-512 single-shot: digest `nn` bytes over `msg` (any length).
fn blake2b_hash(nn: usize, msg: &[u8]) -> Vec<u8> {
    let mut b = Blake2b::new(nn);
    b.update(msg);
    b.finalize()
}

/// H'^T (RFC 9106 §3.3) — variable-length BLAKE2b.
fn blake2b_long(out_len: usize, msg: &[u8]) -> Vec<u8> {
    if out_len <= 64 {
        let mut inp = alloc::vec![0u8; 4 + msg.len()];
        inp[..4].copy_from_slice(&(out_len as u32).to_le_bytes());
        inp[4..].copy_from_slice(msg);
        return blake2b_hash(out_len, &inp);
    }
    let mut out = alloc::vec![0u8; out_len];
    let mut inp = alloc::vec![0u8; 4 + msg.len()];
    inp[..4].copy_from_slice(&(out_len as u32).to_le_bytes());
    inp[4..].copy_from_slice(msg);
    let v1 = blake2b_hash(64, &inp);
    out[..32].copy_from_slice(&v1[..32]);
    let mut prev = v1;
    let mut produced = 32usize;
    let mut to_produce = out_len - produced;
    while to_produce > 64 {
        let next = blake2b_hash(64, &prev);
        out[produced..produced + 32].copy_from_slice(&next[..32]);
        produced += 32;
        to_produce -= 32;
        prev = next;
    }
    let last = blake2b_hash(to_produce, &prev);
    out[produced..produced + to_produce].copy_from_slice(&last[..to_produce]);
    out
}

// ── Argon2 block arithmetic (RFC 9106 §3.5 / ref.c) ────────────────────────────

const QWORDS: usize = 128; // 1024 bytes / 8
type Block = [u64; QWORDS];

#[inline]
fn xor_block(a: &Block, b: &Block) -> Block {
    let mut r = [0u64; QWORDS];
    for i in 0..QWORDS {
        r[i] = a[i] ^ b[i];
    }
    r
}

/// Compression function G (ref.c fill_block), returns next_block.
fn fill_block(prev: &Block, ref_blk: &Block, next: &Block, with_xor: bool) -> Block {
    let mut block_r = xor_block(ref_blk, prev); // R = ref ^ prev
    let mut block_tmp = block_r;
    if with_xor {
        block_tmp = xor_block(&block_tmp, next);
    }
    // Apply Blake2 on columns of 64-bit words: rows (16*i .. 16*i+16).
    for i in 0..8 {
        let base = i * 16;
        let mut row = [0u64; 16];
        row.copy_from_slice(&block_r[base..base + 16]);
        blake2_round_const(&mut row);
        block_r[base..base + 16].copy_from_slice(&row);
    }
    // Apply Blake2 on rows of 64-bit words: stride 2, offset pattern.
    for i in 0..8 {
        let idx = [
            2 * i,
            2 * i + 1,
            2 * i + 16,
            2 * i + 17,
            2 * i + 32,
            2 * i + 33,
            2 * i + 48,
            2 * i + 49,
            2 * i + 64,
            2 * i + 65,
            2 * i + 80,
            2 * i + 81,
            2 * i + 96,
            2 * i + 97,
            2 * i + 112,
            2 * i + 113,
        ];
        let mut row = [0u64; 16];
        for k in 0..16 {
            row[k] = block_r[idx[k]];
        }
        blake2_round_const(&mut row);
        for k in 0..16 {
            block_r[idx[k]] = row[k];
        }
    }
    let mut next_block = block_tmp;
    for i in 0..QWORDS {
        next_block[i] ^= block_r[i];
    }
    next_block
}

fn load_block(bytes: &[u8; 1024]) -> Block {
    let mut b = [0u64; QWORDS];
    for i in 0..QWORDS {
        b[i] = u64::from_le_bytes(bytes[i * 8..i * 8 + 8].try_into().unwrap());
    }
    b
}

fn store_block(b: &Block) -> [u8; 1024] {
    let mut out = [0u8; 1024];
    for i in 0..QWORDS {
        out[i * 8..i * 8 + 8].copy_from_slice(&b[i].to_le_bytes());
    }
    out
}

// ── Argon2id (RFC 9106) ───────────────────────────────────────────────────────

const SYNC_POINTS: u32 = 4;
const ADDRESSES_IN_BLOCK: u32 = 128;
const VERSION_13: u32 = 0x13;
const TYPE_ID: u32 = 2; // Argon2id

struct Position {
    pass: u32,
    lane: u32,
    slice: u32,
    index: u32,
}

struct Instance<'a> {
    memory: &'a mut [Block],
    lanes: u32,
    passes: u32,
    lane_length: u32,
    segment_length: u32,
    memory_blocks: u32,
}

/// Mirrors `index_alpha` from ref.c — u32 modular arithmetic, exactly.
fn index_alpha(inst: &Instance, pos: &Position, pseudo_rand: u32, same_lane: bool) -> u32 {
    let reference_area_size: u32;
    if pos.pass == 0 {
        if pos.slice == 0 {
            reference_area_size = pos.index.wrapping_sub(1);
        } else if same_lane {
            reference_area_size = pos
                .slice
                .wrapping_mul(inst.segment_length)
                .wrapping_add(pos.index)
                .wrapping_sub(1);
        } else {
            reference_area_size = pos
                .slice
                .wrapping_mul(inst.segment_length)
                .wrapping_add(if pos.index == 0 { 0xffff_ffff } else { 0 });
        }
    } else if same_lane {
        reference_area_size = inst
            .lane_length
            .wrapping_sub(inst.segment_length)
            .wrapping_add(pos.index)
            .wrapping_sub(1);
    } else {
        reference_area_size = inst
            .lane_length
            .wrapping_sub(inst.segment_length)
            .wrapping_add(if pos.index == 0 { 0xffff_ffff } else { 0 });
    }

    let mut relative_position = pseudo_rand as u64;
    relative_position = (relative_position * relative_position) >> 32;
    let rp2 = (reference_area_size as u64).wrapping_mul(relative_position) >> 32;
    relative_position = (reference_area_size as u64)
        .wrapping_sub(1)
        .wrapping_sub(rp2);

    let start_position: u32 = if pos.pass != 0 {
        if pos.slice == SYNC_POINTS - 1 {
            0
        } else {
            (pos.slice + 1).wrapping_mul(inst.segment_length)
        }
    } else {
        0
    };

    ((start_position as u64 + relative_position) % inst.lane_length as u64) as u32
}

fn fill_segment(inst: &mut Instance, position: Position) {
    let data_independent_addressing = position.pass == 0 && position.slice < SYNC_POINTS / 2;

    let mut zero_block = [0u64; QWORDS];
    let mut input_block = [0u64; QWORDS];
    let mut address_block = [0u64; QWORDS];

    if data_independent_addressing {
        input_block[0] = position.pass as u64;
        input_block[1] = position.lane as u64;
        input_block[2] = position.slice as u64;
        input_block[3] = inst.memory_blocks as u64;
        input_block[4] = inst.passes as u64;
        input_block[5] = TYPE_ID as u64;
    }

    let mut starting_index = 0u32;
    if position.pass == 0 && position.slice == 0 {
        starting_index = 2;
        if data_independent_addressing {
            next_addresses(&mut address_block, &mut input_block, &zero_block);
        }
    }

    let mut curr_offset =
        position.lane * inst.lane_length + position.slice * inst.segment_length + starting_index;
    let mut prev_offset = if curr_offset % inst.lane_length == 0 {
        curr_offset + inst.lane_length - 1
    } else {
        curr_offset - 1
    };

    let mut i = starting_index;
    while i < inst.segment_length {
        if curr_offset % inst.lane_length == 1 {
            prev_offset = curr_offset - 1;
        }
        let pseudo_rand = if data_independent_addressing {
            if i % ADDRESSES_IN_BLOCK == 0 {
                next_addresses(&mut address_block, &mut input_block, &zero_block);
            }
            address_block[(i % ADDRESSES_IN_BLOCK) as usize]
        } else {
            inst.memory[prev_offset as usize][0]
        };

        let mut ref_lane = (pseudo_rand >> 32) % inst.lanes as u64;
        if position.pass == 0 && position.slice == 0 {
            ref_lane = position.lane as u64;
        }

        let same_lane = ref_lane == position.lane as u64;
        let pos = Position {
            pass: position.pass,
            lane: position.lane,
            slice: position.slice,
            index: i,
        };
        let ref_index = index_alpha(inst, &pos, (pseudo_rand & 0xffff_ffff) as u32, same_lane);

        let ref_block = inst.memory[(ref_lane as u32 * inst.lane_length + ref_index) as usize];
        let prev_block = inst.memory[prev_offset as usize];
        let curr_block = inst.memory[curr_offset as usize];

        let with_xor = position.pass != 0;
        let new_block = fill_block(&prev_block, &ref_block, &curr_block, with_xor);
        inst.memory[curr_offset as usize] = new_block;

        i += 1;
        curr_offset += 1;
        prev_offset += 1;
    }
}

fn next_addresses(address_block: &mut Block, input_block: &mut Block, zero_block: &Block) {
    input_block[6] += 1;
    *address_block = fill_block(zero_block, input_block, address_block, false);
    *address_block = fill_block(zero_block, address_block, address_block, false);
}

fn fill_memory_blocks(inst: &mut Instance) {
    for r in 0..inst.passes {
        for s in 0..SYNC_POINTS {
            for l in 0..inst.lanes {
                let pos = Position {
                    pass: r,
                    lane: l,
                    slice: s,
                    index: 0,
                };
                fill_segment(inst, pos);
            }
        }
    }
}

/// Argon2id key-derivation.
///
/// `password`/`salt` required; `secret`/`ad` optional (empty allowed).
/// `m_kib` = memory in KiB (≥ 8·p), `t` = passes (≥1), `p` = lanes (≥1),
/// `out_len` = tag length in bytes (4..=64).
pub fn argon2id(
    password: &[u8],
    salt: &[u8],
    secret: &[u8],
    ad: &[u8],
    t: u32,
    m_kib: u32,
    p: u32,
    out_len: usize,
) -> Vec<u8> {
    let mut h0 = alloc::vec![0u8; 0];
    h0.extend_from_slice(&p.to_le_bytes());
    h0.extend_from_slice(&(out_len as u32).to_le_bytes());
    h0.extend_from_slice(&m_kib.to_le_bytes());
    h0.extend_from_slice(&t.to_le_bytes());
    h0.extend_from_slice(&VERSION_13.to_le_bytes());
    h0.extend_from_slice(&TYPE_ID.to_le_bytes());
    h0.extend_from_slice(&(password.len() as u32).to_le_bytes());
    h0.extend_from_slice(password);
    h0.extend_from_slice(&(salt.len() as u32).to_le_bytes());
    h0.extend_from_slice(salt);
    h0.extend_from_slice(&(secret.len() as u32).to_le_bytes());
    h0.extend_from_slice(secret);
    h0.extend_from_slice(&(ad.len() as u32).to_le_bytes());
    h0.extend_from_slice(ad);
    let h0_64 = blake2b_hash(64, &h0);

    // seed = H0 (64) || 8 zero bytes (ARGON2_PREHASH_SEED_LENGTH = 72).
    let mut seed = [0u8; 72];
    seed[..64].copy_from_slice(&h0_64);

    let m_prime = 4 * p * (m_kib / (4 * p));
    let lane_length = m_prime / p;
    let segment_length = lane_length / SYNC_POINTS;
    let mut memory: Vec<Block> = alloc::vec![[0u64; QWORDS]; m_prime as usize];

    for l in 0..p {
        let mut bh = seed;
        bh[64..68].copy_from_slice(&0u32.to_le_bytes());
        bh[68..72].copy_from_slice(&l.to_le_bytes());
        let blk = blake2b_long(1024, &bh);
        memory[(l * lane_length) as usize] = load_block(&blk.try_into().unwrap());

        let mut bh = seed;
        bh[64..68].copy_from_slice(&1u32.to_le_bytes());
        bh[68..72].copy_from_slice(&l.to_le_bytes());
        let blk = blake2b_long(1024, &bh);
        memory[(l * lane_length + 1) as usize] = load_block(&blk.try_into().unwrap());
    }

    let mut inst = Instance {
        memory: &mut memory,
        lanes: p,
        passes: t,
        lane_length,
        segment_length,
        memory_blocks: m_prime,
    };
    fill_memory_blocks(&mut inst);

    let mut c = [0u64; QWORDS];
    for l in 0..p {
        let blk = inst.memory[(l * lane_length + lane_length - 1) as usize];
        for i in 0..QWORDS {
            c[i] ^= blk[i];
        }
    }
    let c_bytes = store_block(&c);
    blake2b_long(out_len, &c_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        let mut s = String::new();
        for x in b {
            s.push_str(&format!("{:02x}", x));
        }
        s
    }

    // BLAKE2b-512("") — RFC 7693 standard KAT.
    #[test]
    fn kat_blake2b_empty() {
        let got = blake2b_hash(64, b"");
        let want = "786a02f742015903c6c6fd852552d272912f4740e15847618a86e217f71f5419d25e1031afee585313896444934eb04b903a685b1448b755d56f701afe9be2ce";
        assert_eq!(hex(&got), want, "BLAKE2b-512(\"\") must match RFC 7693 KAT");
    }

    // BLAKE2b-512("abc") — RFC 7693 Appendix A trace vector.
    #[test]
    fn kat_blake2b_abc() {
        let got = blake2b_hash(64, b"abc");
        let want = "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d17d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923";
        assert_eq!(
            hex(&got),
            want,
            "BLAKE2b-512(\"abc\") must match RFC 7693 KAT"
        );
    }

    // Argon2id (RFC 9106 §5.3): m=32 KiB, t=3, p=4, tag=32 B.
    #[test]
    fn kat_argon2id_rfc9106() {
        let pwd = [0x01u8; 32];
        let salt = [0x02u8; 16];
        let secret = [0x03u8; 8];
        let ad = [0x04u8; 12];
        let tag = argon2id(&pwd, &salt, &secret, &ad, 3, 32, 4, 32);
        let want = "0d640df58d78766c08c037a34a8b53c9d01ef0452d75b65eb52520e96b01e659";
        assert_eq!(
            hex(&tag),
            want,
            "Argon2id tag must match RFC 9106 §5.3 KAT (RED if wrong)"
        );
    }

    // Tamper-RED: changing the password must change the tag.
    #[test]
    fn argon2id_tamper_red() {
        let pwd = [0x01u8; 32];
        let pwd2 = [0x02u8; 32];
        let salt = [0x02u8; 16];
        let secret = [0x03u8; 8];
        let ad = [0x04u8; 12];
        let tag1 = argon2id(&pwd, &salt, &secret, &ad, 3, 32, 4, 32);
        let tag2 = argon2id(&pwd2, &salt, &secret, &ad, 3, 32, 4, 32);
        assert_ne!(
            tag1, tag2,
            "different password MUST produce a different Argon2id tag (tamper-RED)"
        );
    }

    // Internal consistency: identical inputs → identical tag.
    #[test]
    fn argon2id_deterministic_green() {
        let pwd = b"password";
        let salt = b"somesalt";
        let a = argon2id(pwd, salt, &[], &[], 2, 16, 4, 32);
        let b = argon2id(pwd, salt, &[], &[], 2, 16, 4, 32);
        assert_eq!(a, b, "Argon2id must be deterministic for identical inputs");
    }
}
