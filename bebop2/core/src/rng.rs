//! rng — from-scratch, zero-dependency ChaCha20-based CSPRNG (bebop2-core).
//!
//! # Design
//! - `chacha20_block` — RFC 8439 §2.3.1 primitive (also reused by `aead`).
//! - `hchacha20`     — draft-irtf-cfrg-xchacha §2.2 (XChaCha20 subkey derivation).
//! - `ChaCha20Rng`   — a counter-mode CSPRNG seeded from a 32-byte key + 12-byte nonce.
//!   Same seed → same stream (deterministic, verifiable). Useful for tests and for
//!   callers that already hold real entropy.
//! - `EntropyRng`    — a **ChaCha20 DRBG seeded from real platform entropy**. This is the
//!   production generator. It is *fail-closed*: if no entropy provider is wired for the
//!   target, the crate does not compile (`compile_error!`), and on a wired target
//!   `EntropyRng::new()` returns `Err` if entropy cannot be obtained (never a constant
//!   fallback).
//!
//! # Entropy sources (honest platform matrix)
//! - **unix / Linux** — `getrandom(2)` syscall (raw, no libc dependency), blocking until
//!   the kernel CSPRNG is seeded. No std/`libc` dependency.
//! - **wasm32** — the single sanctioned wasm import `crypto.getRandomValues` (the only
//!   host import this crate is allowed to carry; everything else stays empty-import).
//! - **x86 / x86_64** without an OS RNG (bare metal, no `getrandom`) — `RDRAND`
//!   (blocking-until-init, retried). Used only when no OS syscall is available.
//! - **Any other target** — intentionally fails to compile. Production keygen must not
//!   ship on a platform with no wired entropy source.
//!
//! # Fail-closed contract (REMEDIATION-BLUEPRINT-2026-07-12 §3B)
//! - The deterministic `ChaCha20Rng::from_seed` and `sign::keygen(seed)` /
//!   `pq_dsa::keygen(seed)` are gated behind `#[cfg(any(test, feature =
//!   "dangerous_deterministic"))]`. A normal (non-test, feature-off) build CANNOT
//!   construct a predictable generator or call the constant-seed keygen — the symbols
//!   do not exist.
//! - Production keygen (`*::keygen_from_entropy()`) draws a fresh 32-byte seed from
//!   `EntropyRng` every call and returns `Err` if entropy is unavailable.
//!
//! All ChaCha20 vectors below are canonical (RFC 8439 / draft-irtf-cfrg-xchacha) — see
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
/// Deterministic: identical (key, nonce) yields an identical stream.
///
/// **This primitive is deterministic by design.** The 32-bit block counter wraps after
/// 2^32 blocks (~256 GiB) of output under a single (key, nonce); re-seed (fresh key)
/// well before that volume. Seed it ONLY from real entropy (see [`EntropyRng`]); the
/// `from_seed` constructor is test/feature-gated so production cannot build a
/// predictable generator.
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
        ChaCha20Rng {
            key,
            nonce,
            counter: 0,
            buf: [0u8; 64],
            pos: 64,
        }
    }

    /// Convenience: seed the whole generator from a 44-byte seed
    /// (32-byte key || 12-byte nonce), the canonical "seed" shape.
    ///
    /// **TEST-ONLY / `dangerous_deterministic`.** In a normal (non-test, feature-off)
    /// build this symbol does not exist, so production code cannot construct a
    /// predictable generator from a hardcoded constant. Use [`EntropyRng`] for prod.
    #[cfg(any(test, feature = "dangerous_deterministic", feature = "test_keygen"))]
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

// ─────────────────────────────────────────────────────────────────────────────
// Fail-closed entropy source (REMEDIATION-BLUEPRINT-2026-07-12 §3B)
// ─────────────────────────────────────────────────────────────────────────────

/// Error returned when entropy cannot be obtained. Never a constant fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntropyError {
    /// No entropy provider is wired for this target (crate fails to compile instead).
    Unavailable,
    /// A draw returned fewer bytes than requested and could not be completed.
    Partial,
    /// The platform call returned a system error code.
    System(i64),
}

/// A platform entropy source: fill `dest` with cryptographically-usable randomness.
pub trait Entropy {
    /// Fill `dest` entirely from platform randomness. Returns `Err` on any failure
    /// (never leaves `dest` partially filled with a constant).
    fn fill(&self, dest: &mut [u8]) -> Result<(), EntropyError>;
}

// ── Platform impl: Linux `getrandom(2)` (raw syscall, no libc) ─────────────────
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
struct LinuxGetrandom;

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
impl Entropy for LinuxGetrandom {
    fn fill(&self, dest: &mut [u8]) -> Result<(), EntropyError> {
        getrandom_syscall(dest)
            .map(|_| ())
            .map_err(EntropyError::System)
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn getrandom_syscall(buf: &mut [u8]) -> Result<usize, i64> {
    use core::arch::asm;
    const NR_GETRANDOM: u64 = 318; // __NR_getrandom on x86_64
    let mut total = 0usize;
    let mut ptr = buf.as_mut_ptr() as u64;
    let mut len = buf.len() as u64;
    while len > 0 {
        // getrandom(2) caps a single call at 0x7fffffff bytes.
        let chunk = len.min(0x7fff_ffff);
        let ret: i64;
        unsafe {
            asm!(
                "syscall",
                in("rax") NR_GETRANDOM,
                in("rdi") ptr,
                in("rsi") chunk,
                in("rdx") 0u64, // flags = 0 → blocking until the CSPRNG is seeded
                lateout("rax") ret,
                lateout("rcx") _,
                lateout("r11") _,
                options(nomem, nostack)
            );
        }
        if ret < 0 {
            return Err(ret);
        }
        let got = ret as u64;
        if got == 0 {
            return Err(-1);
        }
        ptr += got;
        len -= got;
        total += got as usize;
    }
    Ok(total)
}

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
struct LinuxGetrandom;

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
impl Entropy for LinuxGetrandom {
    fn fill(&self, dest: &mut [u8]) -> Result<(), EntropyError> {
        getrandom_syscall(dest)
            .map(|_| ())
            .map_err(EntropyError::System)
    }
}

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
fn getrandom_syscall(buf: &mut [u8]) -> Result<usize, i64> {
    use core::arch::asm;
    const NR_GETRANDOM: u64 = 278; // __NR_getrandom on aarch64
    let mut total = 0usize;
    let mut ptr = buf.as_mut_ptr() as u64;
    let mut len = buf.len() as u64;
    while len > 0 {
        let chunk = len.min(0x7fff_ffff);
        let ret: i64;
        unsafe {
            asm!(
                "svc #0",
                in("x8") NR_GETRANDOM,
                in("x0") ptr,
                in("x1") chunk,
                in("x2") 0u64,
                lateout("x0") ret,
                options(nomem, nostack)
            );
        }
        if ret < 0 {
            return Err(ret);
        }
        let got = ret as u64;
        if got == 0 {
            return Err(-1);
        }
        ptr += got;
        len -= got;
        total += got as usize;
    }
    Ok(total)
}

// ── Platform impl: x86 / x86_64 `RDRAND` (used only without an OS RNG) ─────────
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
struct RdRand;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
impl Entropy for RdRand {
    fn fill(&self, dest: &mut [u8]) -> Result<(), EntropyError> {
        let mut out = [0u8; 8];
        let mut i = 0;
        while i < dest.len() {
            let word = rdran_u64().ok_or(EntropyError::Unavailable)?;
            out.copy_from_slice(&word.to_le_bytes());
            let take = core::cmp::min(8, dest.len() - i);
            dest[i..i + take].copy_from_slice(&out[..take]);
            i += take;
        }
        Ok(())
    }
}

/// Draw one 64-bit word from RDRAND, retrying while the carry flag is clear.
#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
fn rdran_u64() -> Option<u64> {
    #[cfg(target_arch = "x86_64")]
    {
        for _ in 0..128 {
            let mut val: u64 = 0;
            let cf: u8;
            unsafe {
                core::arch::asm!(
                    "rdrand {0}",
                    "setc {1}",
                    out(reg) val,
                    out(reg_byte) cf,
                    options(nomem, nostack)
                );
            }
            if cf != 0 {
                return Some(val);
            }
        }
        None
    }
    #[cfg(target_arch = "x86")]
    {
        for _ in 0..128 {
            let mut lo: u32 = 0;
            let mut hi: u32 = 0;
            let cf1: u8;
            let cf2: u8;
            unsafe {
                core::arch::asm!(
                    "rdrand {0}", "setc {1}",
                    out(reg) lo, out(reg_byte) cf1, options(nomem, nostack)
                );
                core::arch::asm!(
                    "rdrand {0}", "setc {1}",
                    out(reg) hi, out(reg_byte) cf2, options(nomem, nostack)
                );
            }
            if cf1 != 0 && cf2 != 0 {
                return Some((hi as u64) << 32 | lo as u64);
            }
        }
        None
    }
}

// ── Platform impl: wasm32 `crypto.getRandomValues` (sole sanctioned wasm import) ─
#[cfg(target_arch = "wasm32")]
struct WasmCrypto;

#[cfg(target_arch = "wasm32")]
#[link(wasm_import_module = "crypto")]
extern "C" {
    /// The single sanctioned wasm host import: `crypto.getRandomValues(view)`.
    /// `buf` must point at a wasm-linear-memory buffer that the JS side wraps in a
    /// `Uint8Array`. Called with raw pointer + length (glue provided by the host).
    fn getRandomValues(buf: *mut u8, len: usize);
}

#[cfg(target_arch = "wasm32")]
impl Entropy for WasmCrypto {
    fn fill(&self, dest: &mut [u8]) -> Result<(), EntropyError> {
        if dest.is_empty() {
            return Ok(());
        }
        unsafe { getRandomValues(dest.as_mut_ptr(), dest.len()) };
        Ok(())
    }
}

/// Return the platform entropy provider for this target.
///
/// On any target with no wired provider this function is absent and the crate
/// fails to compile (fail-closed), so production keygen cannot be built.
#[cfg(target_arch = "wasm32")]
pub fn entropy_provider() -> &'static dyn Entropy {
    static P: WasmCrypto = WasmCrypto;
    &P
}

#[cfg(all(target_os = "linux"))]
pub fn entropy_provider() -> &'static dyn Entropy {
    static P: LinuxGetrandom = LinuxGetrandom;
    &P
}

#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "linux"),
    any(target_arch = "x86", target_arch = "x86_64")
))]
pub fn entropy_provider() -> &'static dyn Entropy {
    static P: RdRand = RdRand;
    &P
}

#[cfg(all(
    not(target_arch = "wasm32"),
    not(target_os = "linux"),
    not(any(target_arch = "x86", target_arch = "x86_64"))
))]
compile_error!(
    "bebop2-core has NO wired entropy provider for this target. Production keygen is \
     fail-closed and must not compile here. Wire a platform entropy source (getrandom / \
     RDRAND / crypto.getRandomValues) or, for tests only, build with the \
     `dangerous_deterministic` feature."
);

/// A ChaCha20 DRBG seeded from real platform entropy. This is the production
/// generator. Fail-closed: [`EntropyRng::new`] returns `Err` if entropy cannot be
/// drawn (never a constant fallback).
pub struct EntropyRng {
    inner: ChaCha20Rng,
    /// Bytes emitted since the last reseed (drives volume-based reseed, §3B).
    since_reseed: usize,
}

impl EntropyRng {
    /// Volume threshold (bytes) after which the DRBG reseeds from fresh entropy.
    const RESEED_BYTES: usize = 1 << 20; // ~1 MiB

    /// Construct a DRBG seeded from 32 bytes of real platform entropy.
    ///
    /// Returns `Err` if entropy is unavailable — production callers must handle this
    /// and MUST NOT substitute a constant seed.
    pub fn new() -> Result<Self, EntropyError> {
        let mut seed = [0u8; 32];
        // Nonce is all-zero: the key already carries full entropy, and ChaCha20's
        // 96-bit nonce is not a secret. A fresh key per `new()` gives unique streams.
        entropy_provider().fill(&mut seed)?;
        Ok(Self {
            inner: ChaCha20Rng::new(seed, [0u8; 12]),
            since_reseed: 0,
        })
    }

    /// Reseed the DRBG from fresh platform entropy. Fail-closed: returns `Err` if
    /// entropy cannot be obtained (the existing state is left intact).
    pub fn reseed(&mut self) -> Result<(), EntropyError> {
        let mut seed = [0u8; 32];
        entropy_provider().fill(&mut seed)?;
        self.inner = ChaCha20Rng::new(seed, [0u8; 12]);
        self.since_reseed = 0;
        Ok(())
    }

    /// Fill `dest` with keystream bytes, reseeding from entropy after a volume threshold.
    pub fn fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), EntropyError> {
        if self.since_reseed + dest.len() > Self::RESEED_BYTES {
            self.reseed()?;
        }
        self.inner.fill_bytes(dest);
        self.since_reseed += dest.len();
        Ok(())
    }

    /// Next 32-bit word.
    pub fn next_u32(&mut self) -> Result<u32, EntropyError> {
        let mut b = [0u8; 4];
        self.fill_bytes(&mut b)?;
        Ok(u32::from_le_bytes(b))
    }

    /// Next 64-bit word.
    pub fn next_u64(&mut self) -> Result<u64, EntropyError> {
        let mut b = [0u8; 8];
        self.fill_bytes(&mut b)?;
        Ok(u64::from_le_bytes(b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use core::convert::TryInto;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
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
            0x8795_31e0,
            0xc5ec_f37d,
            0x5164_61b1,
            0xc9a6_2f8a,
            0x44c2_0ef3,
            0x3390_af7f,
            0xd9fc_690b,
            0x2a5f_714c,
            0x5337_2767,
            0xb00a_5631,
            0x974c_541a,
            0x359e_9963,
            0x5c97_1061,
            0x3d63_1689,
            0x2098_d9d6,
            0x91db_d320,
        ];
        quarter_round(&mut s, 2, 7, 8, 13);
        let exp: [u32; 16] = [
            0x8795_31e0,
            0xc5ec_f37d,
            0xbdb8_86dc,
            0xc9a6_2f8a,
            0x44c2_0ef3,
            0x3390_af7f,
            0xd9fc_690b,
            0xcfac_afd2,
            0xe46b_ea80,
            0xb00a_5631,
            0x974c_541a,
            0x359e_9963,
            0x5c97_1061,
            0xccc0_7c79,
            0x2098_d9d6,
            0x91db_d320,
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
        let exp = hex("10f1e7e4d13b5915500fdd1fa32071c4\
             c7d1f4c733c068030422aa9ac3d46c4e\
             d2826446079faa0914c2d705d98b02a2\
             b5129cd1de164eb9cbd083e8a2503c4e");
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
        assert!(
            diffs >= 32,
            "seed bit-flip should flip ~half the block, got {diffs}/64 diffs"
        );
    }

    // ── Fail-closed entropy tests (RED→GREEN) ──────────────────────────────────

    #[test]
    fn entropy_rng_new_succeeds_on_wired_host_and_is_random() {
        // GREEN: this target HAS a wired entropy provider (Linux getrandom / wasm
        // crypto.getRandomValues / x86 RDRAND), so construction succeeds. On a target
        // with NO provider, the crate FAILS TO COMPILE (compile_error!) — that is the
        // fail-closed guarantee; there is no constant fallback.
        let mut rng = EntropyRng::new().expect("entropy provider must be wired on this target");
        let mut a = [0u8; 32];
        let mut b = [0u8; 32];
        rng.fill_bytes(&mut a).expect("entropy draw");
        rng.fill_bytes(&mut b).expect("entropy draw");
        assert_ne!(a, b, "two independent entropy draws must differ");
        assert_ne!(a, [0u8; 32], "entropy draw must not be all-zero");
    }

    #[test]
    fn prod_keygen_requires_entropy_red_green() {
        // RED→GREEN (bebop2-core §3B).
        //
        // RED (before this fix): production keygen consumed a CALLER-SUPPLIED constant
        // seed and `ChaCha20Rng::from_seed` was public, so every key/nonce/KEM ephemeral
        // was predictable. A release build without entropy wiring still compiled.
        //
        // GREEN (now): the production entry points draw a FRESH seed from `EntropyRng`
        // every call and return `Err` if entropy is unavailable — never a constant.
        // We assert (a) they succeed on this wired host and (b) two successive calls
        // produce DIFFERENT keys, proving the seed is real entropy, not a constant.
        //
        // Compile-time guarantee: the constant-seed entry points
        // (`ChaCha20Rng::from_seed`, `sign::keygen(seed)`, `pq_dsa::keygen(seed)`) are
        // gated behind `#[cfg(any(test, feature = \"dangerous_deterministic\"))]`. In a
        // normal (non-test, feature-off) build those symbols do NOT exist, so a prod
        // crate calling `sign::keygen(&[0u8; 32])` FAILS TO COMPILE. That is the
        // fail-closed property this test documents.

        // Ed25519
        let (pk1, _sk1) = crate::sign::keygen_from_entropy().expect("entropy for Ed25519");
        let (pk2, _sk2) = crate::sign::keygen_from_entropy().expect("entropy for Ed25519");
        assert_ne!(
            pk1, pk2,
            "two prod Ed25519 keygens must differ (non-constant seed)"
        );

        // ML-DSA-65
        let (pkd1, _skd1) = crate::pq_dsa::keygen_from_entropy().expect("entropy for ML-DSA");
        let (pkd2, _skd2) = crate::pq_dsa::keygen_from_entropy().expect("entropy for ML-DSA");
        assert_ne!(
            pkd1.bytes, pkd2.bytes,
            "two prod ML-DSA keygens must differ (non-constant seed)"
        );

        // ML-KEM-768
        let (ek1, _dk1) = crate::pq_kem::keygen_from_entropy().expect("entropy for ML-KEM");
        let (ek2, _dk2) = crate::pq_kem::keygen_from_entropy().expect("entropy for ML-KEM");
        assert_ne!(
            ek1, ek2,
            "two prod ML-KEM keygens must differ (non-constant seed)"
        );
    }

    #[test]
    fn from_seed_remains_usable_under_test_cfg() {
        // Documents that the test/feature-gated deterministic constructor is still
        // reachable inside the crate's own test build (so the determinism tests above
        // keep compiling and passing). In a non-test, feature-off build this symbol is
        // absent — that absence is the production fail-closed guarantee.
        let seed = [0xCDu8; 44];
        let mut a = ChaCha20Rng::from_seed(&seed);
        let mut b = [0u8; 32];
        a.fill_bytes(&mut b);
        assert_ne!(b, [0u8; 32]);
    }
}
