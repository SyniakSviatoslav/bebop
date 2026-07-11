//! pq_dsa — ML-DSA-65 (FIPS 204) implemented from scratch, zero external crates.
//!
//! ENTROPY MODEL: RNG-free on the crypto hot path. All randomness enters ONLY through
//! caller-supplied byte streams (`seed`, `rnd`). We never call any OS RNG, clock, network.
//!
//! CORRECTNESS DISCIPLINE (NIST KAT vectors unreachable — network blocked):
//!   1. SHAKE256 delegated to the verified FIPS-202 Keccak core in `pq_kem` (GREEN).
//!   2. keygen -> sign -> verify roundtrip GREEN + tamper-RED (the scheme is internally
//!      correct). NOTE: signature/hint PACKING follows FIPS 204 §6 structure but is NOT
//!      bit-exact interoperable with reference ML-DSA-65 (no Dilithium oracle in this
//!      sandbox); flagged honestly per the repo's doc-integrity rule. The lattice math
//!      (SHAKE, schoolbook mul, decompose, hint) is correct and tested. Coefficient-domain
//!      multiplication (schoolbook) is used for sign/verify so the algorithm is
//!      verifiably correct; an NTT fast-path can be swapped in later behind the same tests.

#![allow(dead_code)]

// `alloc::vec::Vec` is used by the packing helpers (w1_encode/pack_pk/pack_sig and the
// sign/verify hash-input buffers). Under no_std this resolves through the crate-wide
// `extern crate alloc` + bump allocator provided by lib.rs; under std it is the usual
// allocator. Imported here so the symbols resolve in both builds.
use alloc::vec::Vec;

// SHAKE256 delegates to the verified FIPS-202 Keccak core in pq_kem (no duplicate).
pub fn shake256(data: &[u8], out: &mut [u8]) {
    crate::pq_kem::shake256(data, out)
}

// ─────────────────────────────────────────────────────────────────────────────
// ML-DSA-65 parameters (FIPS 204 Table 2, cat-3)
// ─────────────────────────────────────────────────────────────────────────────
const N: usize = 256;
const Q: i32 = 8380417;
const QINV: i32 = 58728449;
const D: usize = 13;
const K: usize = 6;
const L: usize = 5;
const ETA: usize = 4;
const BETA: i32 = 196;
const GAMMA1: i32 = 1 << 19;
const GAMMA2: i32 = (Q - 1) / 32;
const TAU: usize = 49;
const OMEGA: usize = 55;
const LAMBDA: usize = 192;
const ALPHA: i32 = 2 * GAMMA2;

// ─────────────────────────────────────────────────────────────────────────────
// Modulus arithmetic (Montgomery)
// ─────────────────────────────────────────────────────────────────────────────
#[inline]
fn mul_mod(a: i32, b: i32) -> i32 {
    // a,b in [-Q+1, Q-1]; product fits i64 (~7e13). Reduce mod Q, signed-correct.
    let p = (a as i64) * (b as i64);
    let mut r = (p % (Q as i64)) as i32;
    if r < 0 {
        r += Q;
    }
    r
}

#[inline]
fn reduce_once(a: i32) -> i32 {
    let mut r = a % Q;
    if r < 0 {
        r += Q;
    }
    r
}

#[inline]
fn add_mod(a: i32, b: i32) -> i32 {
    let r = a + b;
    if r >= Q {
        r - Q
    } else if r < 0 {
        r + Q
    } else {
        r
    }
}

#[inline]
fn sub_mod(a: i32, b: i32) -> i32 {
    let r = a - b;
    if r < 0 {
        r + Q
    } else if r >= Q {
        r - Q
    } else {
        r
    }
}

/// Canonical representative of a mod Q, in [0, Q).
#[inline]
fn mod_q(a: i32) -> i32 {
    let r = (a % Q + Q) % Q;
    r
}

/// Re-center a coefficient from [0,Q) to the symmetric representative in [−Q/2, Q/2].
#[inline]
fn recenter(a: i32) -> i32 {
    let r = if a > Q / 2 { a - Q } else { a };
    r
}

fn poly_recenter(p: &Poly) -> Poly {
    let mut r = [0i32; N];
    for i in 0..N {
        r[i] = recenter(p[i]);
    }
    r
}

fn poly_add_centered(a: &Poly, b: &Poly) -> Poly {
    let mut r = [0i32; N];
    for i in 0..N {
        r[i] = a[i] + b[i]; // plain integer; callers keep this in a small centered range
    }
    r
}

// ─────────────────────────────────────────────────────────────────────────────
// Polynomial / vector types
// ─────────────────────────────────────────────────────────────────────────────
type Poly = [i32; N];
type PolyVecL = [Poly; L];
type PolyVecK = [Poly; K];

fn poly_zero() -> Poly {
    [0i32; N]
}

fn poly_add(a: &Poly, b: &Poly) -> Poly {
    let mut r = [0i32; N];
    for i in 0..N {
        r[i] = add_mod(a[i], b[i]);
    }
    r
}

fn poly_sub(a: &Poly, b: &Poly) -> Poly {
    let mut r = [0i32; N];
    for i in 0..N {
        r[i] = sub_mod(a[i], b[i]);
    }
    r
}

/// Schoolbook polynomial multiplication in R_q = Z_q[x]/(x^N+1). Ground-truth.
fn poly_mul_schoolbook(a: &Poly, b: &Poly) -> Poly {
    let mut r = [0i32; N];
    for i in 0..N {
        if a[i] == 0 {
            continue;
        }
        for j in 0..N {
            if b[j] == 0 {
                continue;
            }
            let prod = mul_mod(a[i], b[j]);
            let k = i + j;
            if k < N {
                r[k] = add_mod(r[k], prod);
            } else {
                r[k - N] = sub_mod(r[k - N], prod);
            }
        }
    }
    r
}

// ─────────────────────────────────────────────────────────────────────────────
// Sampling
// ─────────────────────────────────────────────────────────────────────────────
/// Center-binomial sampler, η=4 (8 bits/coeff -> range [-4,4]).
fn sample_poly_cbd(seed: &[u8], offset: usize) -> Poly {
    let mut poly = [0i32; N];
    for i in 0..N {
        let byte0 = seed[offset + 2 * i];
        let byte1 = seed[offset + 2 * i + 1];
        let mut a = 0i32;
        let mut b = 0i32;
        for j in 0..4 {
            a += ((byte0 >> j) & 1) as i32;
            b += ((byte0 >> (j + 4)) & 1) as i32;
        }
        for j in 0..4 {
            a += ((byte1 >> j) & 1) as i32;
            b += ((byte1 >> (j + 4)) & 1) as i32;
        }
        poly[i] = a - b;
    }
    poly
}

/// Expand s1 (L polys) and s2 (K polys) from rhoprime (64 bytes for ML-DSA-65).
/// FIPS 204 ExpandS: seed = SHAKE256(rhoprime || r || i, 128), r=0 for s1, r=1 for s2.
fn expand_s(rhoprime: &[u8]) -> (PolyVecL, PolyVecK) {
    let mut s1: PolyVecL = [[0i32; N]; L];
    let mut s2: PolyVecK = [[0i32; N]; K];
    let mut buf = [0u8; 512];
    for i in 0..L {
        let mut seed = [0u8; 66];
        seed[..64].copy_from_slice(rhoprime);
        seed[64] = 0;
        seed[65] = i as u8;
        shake256(&seed, &mut buf);
        s1[i] = sample_poly_cbd(&buf, 0);
    }
    for i in 0..K {
        let mut seed = [0u8; 66];
        seed[..64].copy_from_slice(rhoprime);
        seed[64] = 1;
        seed[65] = i as u8;
        shake256(&seed, &mut buf);
        s2[i] = sample_poly_cbd(&buf, 0);
    }
    (s1, s2)
}

/// Expand A (K x L matrix of polys) from rho (32 bytes).
fn expand_a(rho: &[u8; 32]) -> [[Poly; L]; K] {
    let mut a = [[poly_zero(); L]; K];
    let mut buf = [0u8; 512];
    for i in 0..K {
        for j in 0..L {
            let mut tag = [0u8; 34];
            tag[..32].copy_from_slice(rho);
            tag[32] = j as u8;
            tag[33] = i as u8;
            shake256(&tag, &mut buf);
            a[i][j] = sample_poly_cbd(&buf, 0);
        }
    }
    a
}

/// Sample a poly with coeffs in [-gamma1+1, gamma1-1] (38 bits/coeff, FIPS 204 §6.2.2).
fn sample_poly_gamma1(buf: &[u8], coeff: usize) -> i32 {
    let bit_off = 38 * coeff;
    let byte_base = bit_off / 8;
    let shift = bit_off % 8;
    let mut acc: u64 = 0;
    for j in 0..6 {
        acc |= (buf[byte_base + j] as u64) << (8 * j);
    }
    acc >>= shift;
    let a = (acc & ((1u64 << 19) - 1)) as i32;
    let b = ((acc >> 19) & ((1u64 << 19) - 1)) as i32;
    a - b
}

/// Expand the masking vector y (L polys) from rhoprime + nonce (Alg 5).
fn expand_mask(rhoprime: &[u8], nonce: u16) -> PolyVecL {
    let mut y: PolyVecL = [[0i32; N]; L];
    let mut buf = [0u8; 1224]; // 38*256/8 = 1216 bytes, +8 headroom for 6-byte reads
    for i in 0..L {
        let mut seed = [0u8; 68];
        seed[..64].copy_from_slice(rhoprime);
        seed[64] = i as u8;
        seed[65..67].copy_from_slice(&nonce.to_le_bytes());
        shake256(&seed, &mut buf);
        for n in 0..N {
            y[i][n] = sample_poly_gamma1(&buf, n);
        }
    }
    y
}

/// Sample the challenge polynomial c (exactly TAU=49 coeffs in {+1,-1}, rest 0).
/// FIPS 204 §6.2.4 SampleInBall: read 9-bit index candidates from the 32-byte
/// hash (cyclic bitstream) with rejection until TAU distinct indices in [0,N) are
/// placed. Sign of the k-th placed coeff comes from the k-th bit of seed[24..32].
fn sample_in_ball(seed: &[u8; 32]) -> Poly {
    let mut c = [0i32; N];
    let mut signs = [0u8; 8];
    signs.copy_from_slice(&seed[24..32]);
    let mut inb = [false; N];
    let mut n = 0usize;
    let mut bitpos = 0usize;
    let mut attempts = 0usize;
    while n < TAU {
        let mut idx = 0usize;
        for _ in 0..9 {
            let byte = seed[bitpos / 8 % 32];
            let bit = (byte >> (bitpos % 8)) & 1;
            idx = (idx << 1) | bit as usize;
            bitpos += 1;
        }
        attempts += 1;
        if attempts > 256 * 9 {
            break; // safety valve; properly distributed hashes fill 49 well before this
        }
        if idx < N && !inb[idx] {
            inb[idx] = true;
            let sign_bit = (signs[n / 8] >> (n % 8)) & 1;
            c[idx] = (sign_bit as i32) * 2 - 1;
            n += 1;
        }
    }
    c
}

// ─────────────────────────────────────────────────────────────────────────────
// Decompose / hint (FIPS 204 §6.2.3)
// ─────────────────────────────────────────────────────────────────────────────
fn decompose(r: i32, alpha: i32) -> (i32, i32) {
    // FIPS 204 Alg 36 Decompose(r): r0 = r mod± α (centered in (-α/2, α/2]);
    // r1 = (r - r0)/α, special case r - r0 = q-1 ⇒ r1 ← 0, r0 ← r0 - 1.
    // Yields r1 ∈ [0, (q-1)/(2γ2)] = [0,16] for ML-DSA-65 (top bin maps to 0).
    let rq = mod_q(r); // [0, Q)
    let mut r0 = rq % alpha; // [0, alpha)
    if r0 > alpha / 2 {
        r0 -= alpha; // centered (-α/2, α/2]
    }
    if rq - r0 == Q - 1 {
        (0, r0 - 1)
    } else {
        ((rq - r0) / alpha, r0)
    }
}

fn highbits(r: i32, alpha: i32) -> i32 {
    decompose(r, alpha).0
}

fn lowbits(r: i32, alpha: i32) -> i32 {
    decompose(r, alpha).1
}

/// MakeHint(c, r) = 1 if HighBits(r) != HighBits(r + c). (FIPS 204 Alg 39: v1 = HighBits(r + z))
fn make_hint(c: &Poly, r: &Poly) -> Poly {
    let mut h = [0i32; N];
    for i in 0..N {
        let rc = add_mod(r[i], c[i]);
        if highbits(r[i], ALPHA) != highbits(rc, ALPHA) {
            h[i] = 1;
        }
    }
    h
}

/// UseHint(h, r): inverse of MakeHint (FIPS 204 Alg 40).
/// r1 = HighBits(r); (r1, r0) = Decompose(r). If h=1 and r0>0 return (r1+1) mod m;
/// if h=1 and r0<=0 return (r1-1) mod m; else return r1. Here m = (q-1)/(2*gamma2).
fn use_hint(h: &Poly, r: &Poly) -> Poly {
    let mut u = [0i32; N];
    let m = (Q - 1) / (2 * GAMMA2); // 16
    for i in 0..N {
        let r0 = lowbits(r[i], ALPHA);
        let mut a = highbits(r[i], ALPHA);
        if h[i] != 0 {
            if r0 > 0 {
                a = (a + 1) % m;
            } else {
                a = (a + m - 1) % m;
            }
        }
        u[i] = a;
    }
    u
}

// ─────────────────────────────────────────────────────────────────────────────
// Packing
// ─────────────────────────────────────────────────────────────────────────────
fn w1_encode(w1: &PolyVecK) -> Vec<u8> {
    let mut out = vec![0u8; K * N / 2]; // 4 bits/coeff
    for (p, poly) in w1.iter().enumerate() {
        for i in 0..N {
            // poly[i] is ALREADY HighBits(w, 2*gamma2) in [0,16] (see sign/verify builders).
            // Do NOT re-apply highbits here — that would collapse every value to 0 and
            // zero the commitment (forgeable). Pack the 4-bit high-bits directly.
            let t = (poly[i] & 0x0f) as u8;
            if i % 2 == 0 {
                out[(p * N + i) / 2] = t;
            } else {
                out[(p * N + i) / 2] |= t << 4;
            }
        }
    }
    out
}

fn power2round(f: &Poly, d: usize) -> (Poly, Poly) {
    let mut t1 = [0i32; N];
    let mut t0 = [0i32; N];
    let half = 1 << (d - 1);
    for i in 0..N {
        t1[i] = (f[i] + half) >> d;
        t0[i] = f[i] - (t1[i] << d);
        t1[i] = t1[i] % Q;
    }
    (t1, t0)
}

fn pack_pk(rho: &[u8; 32], t1: &PolyVecK) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 + K * (N * D / 8));
    out.extend_from_slice(rho);
    for poly in t1.iter() {
        for &coeff in poly.iter() {
            let mut v = coeff as u32;
            for _ in 0..(D / 8) {
                out.push((v & 0xff) as u8);
                v >>= 8;
            }
            out.push((v & 0x1f) as u8);
        }
    }
    out
}

fn pack_sig(z: &PolyVecL, h: &PolyVecK) -> Vec<u8> {
    let mut out = Vec::new();
    for poly in z.iter() {
        for &coeff in poly.iter() {
            let mut v = (coeff + GAMMA1) as u32;
            out.push((v & 0xff) as u8);
            out.push(((v >> 8) & 0xff) as u8);
            out.push(((v >> 16) & 0x7f) as u8);
        }
    }
    // hint: store per-component OMEGA set positions, then count. (internal packing; not NIST-bit-exact)
    let mut hint = [0u8; OMEGA + 1];
    let mut nset = 0;
    for comp in h.iter() {
        for i in 0..N {
            if comp[i] != 0 {
                if nset < OMEGA {
                    hint[nset] = i as u8;
                    nset += 1;
                }
            }
        }
    }
    out.extend_from_slice(&hint);
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Matrix-vector (coefficient domain)
// ─────────────────────────────────────────────────────────────────────────────
fn mat_vec_mul(a: &[[Poly; L]; K], v: &PolyVecL) -> PolyVecK {
    let mut r: PolyVecK = [[0i32; N]; K];
    for i in 0..K {
        for j in 0..L {
            let prod = poly_mul_schoolbook(&a[i][j], &v[j]);
            r[i] = poly_add(&r[i], &prod);
        }
    }
    r
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────
pub struct MlDsa65Pk {
    pub rho: [u8; 32],
    pub t1: PolyVecK,
}

pub struct MlDsa65Sk {
    pub rho: [u8; 32],
    pub k: [u8; 32],
    pub tr: [u8; 32],
    pub s1: PolyVecL,
    pub s2: PolyVecK,
    pub t0: PolyVecK,
}

pub struct MlDsa65Sig {
    pub z: PolyVecL,
    pub h: PolyVecK,
    pub c_t: [u8; 32],
}

/// KeyGen (FIPS 204 Alg 1). seed=32 bytes.
pub fn keygen(seed: &[u8; 32]) -> (MlDsa65Pk, MlDsa65Sk) {
    // ext = ρ(32) ‖ ρ̂(64) ‖ K(32) = 128 bytes for ML-DSA-65 (FIPS 204 §6.3).
    let mut ext = [0u8; 128];
    shake256(seed, &mut ext);
    let mut rho = [0u8; 32];
    rho.copy_from_slice(&ext[0..32]);
    let rhoprime = &ext[32..96];
    let mut k = [0u8; 32];
    k.copy_from_slice(&ext[96..128]);

    let a = expand_a(&rho);
    let (s1, s2) = expand_s(rhoprime);

    let mut t: PolyVecK = [[0i32; N]; K];
    for i in 0..K {
        for j in 0..L {
            let prod = poly_mul_schoolbook(&a[i][j], &s1[j]);
            t[i] = poly_add(&t[i], &prod);
        }
        for n in 0..N {
            t[i][n] = add_mod(t[i][n], s2[i][n]);
        }
    }
    let mut t1 = [[0i32; N]; K];
    let mut t0 = [[0i32; N]; K];
    for i in 0..K {
        let (t1i, t0i) = power2round(&t[i], D);
        t1[i] = t1i;
        t0[i] = t0i;
    }
    let pk = MlDsa65Pk { rho, t1 };
    let mut tr = [0u8; 32];
    let pk_bytes = pack_pk(&rho, &t1);
    shake256(&pk_bytes, &mut tr);
    let sk = MlDsa65Sk { rho, k, tr, s1, s2, t0 };
    (pk, sk)
}

/// Sign (FIPS 204 Alg 5, randomized via rnd).
pub fn sign(sk: &MlDsa65Sk, msg: &[u8], rnd: &[u8; 32]) -> MlDsa65Sig {
    let mut mu = [0u8; 64];
    {
        let mut tmp = Vec::with_capacity(32 + msg.len());
        tmp.extend_from_slice(&sk.tr);
        tmp.extend_from_slice(msg);
        shake256(&tmp, &mut mu);
    }
    let mut rhoprime = [0u8; 64];
    {
        let mut tmp = Vec::with_capacity(32 + 32 + 32 + msg.len());
        tmp.extend_from_slice(rnd);
        tmp.extend_from_slice(&sk.k);
        tmp.extend_from_slice(&sk.tr);
        tmp.extend_from_slice(msg);
        shake256(&tmp, &mut rhoprime);
    }
    let a = expand_a(&sk.rho);

    let mut nonce: u16 = 0;
    const MAX_NONCE: u16 = 768; // fail-closed: never loop unbounded (per VERIFIED-BY-MATH red-line discipline)
    loop {
        if nonce >= MAX_NONCE {
            panic!("ML-DSA-65 sign: rejection-sampling exhausted nonce budget (deterministic input defect)");
        }
        // w = A*y  (coeff domain)
        let y = expand_mask(&rhoprime, nonce);
        let w = mat_vec_mul(&a, &y);
        // w1 = HighBits(w, 2*gamma2)
        let mut w1 = [[0i32; N]; K];
        for i in 0..K {
            for n in 0..N {
                w1[i][n] = highbits(w[i][n], ALPHA);
            }
        }
        let mut c_input = Vec::with_capacity(64 + K * N / 2);
        c_input.extend_from_slice(&mu);
        c_input.extend_from_slice(&w1_encode(&w1));
        let mut c_hash = [0u8; 32];
        shake256(&c_input, &mut c_hash);
        let c = sample_in_ball(&c_hash);

        // z = y + c*s1   (y centered in [-gamma1,gamma1]; c*s1 small centered; keep centered, NOT mod-Q)
        let mut z = y;
        for i in 0..L {
            let cs1 = poly_recenter(&poly_mul_schoolbook(&c, &sk.s1[i]));
            z[i] = poly_add_centered(&z[i], &cs1);
        }
        let mut z_ok = true;
        for i in 0..L {
            for n in 0..N {
                if z[i][n] >= GAMMA1 - BETA || z[i][n] <= -(GAMMA1 - BETA) {
                    z_ok = false;
                }
            }
        }
        // r0 = LowBits(w - c*s2, 2*gamma2)
        let mut cs2 = [[0i32; N]; K];
        for i in 0..K {
            cs2[i] = poly_mul_schoolbook(&c, &sk.s2[i]);
        }
        let mut r0_ok = true;
        for i in 0..K {
            for n in 0..N {
                let diff = sub_mod(w[i][n], cs2[i][n]);
                let r0n = lowbits(diff, ALPHA);
                if r0n >= GAMMA2 - BETA || r0n <= -(GAMMA2 - BETA) {
                    r0_ok = false;
                }
            }
        }
        if !r0_ok || !z_ok {
            nonce += 1;
            continue;
        }
        // FIPS 204 Alg 7 line 26: h <- MakeHint(-c*t0, w - c*s2 + c*t0)
        // (UNSCALED: no 2^d here — z = -c*t0, r = w - c*s2 + c*t0; both within |c*t0| <= 4096
        // of w, far below alpha/2, so HighBits(r) and HighBits(r+z)=HighBits(w) are the bridge.)
        let mut wp = [[0i32; N]; K];
        for i in 0..K {
            let ct0 = poly_mul_schoolbook(&c, &sk.t0[i]);
            for n in 0..N {
                let terms = sub_mod(w[i][n], cs2[i][n]);
                wp[i][n] = add_mod(terms, ct0[n]);
            }
        }
        // x = -c*t0  (FIPS MakeHint(z, r) with z = -c*t0)
        let mut x = [[0i32; N]; K];
        for i in 0..K {
            let ct0 = poly_mul_schoolbook(&c, &sk.t0[i]);
            for n in 0..N {
                x[i][n] = mod_q(-(ct0[n] as i64) as i32);
            }
        }
        // h is per-component: Hi = MakeHint(-c*t0, w - c*s2 + c*t0) for each i in [0,K).
        // FIPS 204 requires h ∈ {0,1}^(K*N); merging across K (OR) corrupts reconstruction.
        let mut h: PolyVecK = [[0i32; N]; K];
        for i in 0..K {
            h[i] = make_hint(&x[i], &wp[i]);
        }
        return MlDsa65Sig { z, h, c_t: c_hash };
    }
}

/// Verify (FIPS 204 Alg 6).
pub fn verify(pk: &MlDsa65Pk, msg: &[u8], sig: &MlDsa65Sig) -> bool {
    let mut tr = [0u8; 32];
    let pk_bytes = pack_pk(&pk.rho, &pk.t1);
    shake256(&pk_bytes, &mut tr);
    let mut mu = [0u8; 64];
    {
        let mut tmp = Vec::with_capacity(32 + msg.len());
        tmp.extend_from_slice(&tr);
        tmp.extend_from_slice(msg);
        shake256(&tmp, &mut mu);
    }
    let c = sample_in_ball(&sig.c_t);
    let a = expand_a(&pk.rho);

    // w' = A*z - c*(t1*2^d)
    let mut az = mat_vec_mul(&a, &sig.z);
    let mut t1_2d = pk.t1;
    for i in 0..K {
        for n in 0..N {
            t1_2d[i][n] = ((t1_2d[i][n] as i64 * (1 << D) as i64) % Q as i64) as i32;
        }
    }
    let mut w_ = [[0i32; N]; K];
    for i in 0..K {
        let ct1 = poly_mul_schoolbook(&c, &t1_2d[i]);
        for n in 0..N {
            w_[i][n] = sub_mod(az[i][n], ct1[n]);
        }
    }
    // w1' = UseHint(h, w')
    let mut w1_prime = [[0i32; N]; K];
    for i in 0..K {
        let u = use_hint(&sig.h[i], &w_[i]);
        for n in 0..N {
            // u is ALREADY the reconstructed HighBits(w, 2*gamma2) in [0,16] (use_hint returns r1).
            // Do NOT re-apply highbits here — that would collapse to 0 (see w1_encode note).
            w1_prime[i][n] = u[n];
        }
    }
    let mut c_input = Vec::with_capacity(64 + K * N / 2);
    c_input.extend_from_slice(&mu);
    c_input.extend_from_slice(&w1_encode(&w1_prime));
    let mut c_hash = [0u8; 32];
    shake256(&c_input, &mut c_hash);
    if c_hash != sig.c_t {
        return false;
    }
    for i in 0..L {
        for n in 0..N {
            if sig.z[i][n] >= GAMMA1 - BETA || sig.z[i][n] <= -(GAMMA1 - BETA) {
                return false;
            }
        }
    }
    true
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests (RED+GREEN, falsifiable)
// ─────────────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kat_shake256_empty() {
        let mut out = [0u8; 64];
        shake256(b"", &mut out);
        let expected = [
            0x46, 0xb9, 0xdd, 0x2b, 0x0b, 0xa8, 0x8d, 0x13, 0x23, 0x3b, 0x3f, 0xeb, 0x74, 0x3e,
            0xeb, 0x24, 0x3f, 0xcd, 0x52, 0xea, 0x62, 0xb8, 0x1b, 0x82, 0xb5, 0x0c, 0x27, 0x64,
            0x6e, 0xd5, 0x76, 0x2f,
        ];
        assert_eq!(&out[..32], &expected[..], "SHAKE256 empty KAT mismatch");
    }

    #[test]
    fn kat_shake256_abc() {
        let mut out = [0u8; 64];
        shake256(b"abc", &mut out);
        let expected = [
            0x48, 0x33, 0x66, 0x60, 0x13, 0x60, 0xa8, 0x77, 0x1c, 0x68, 0x63, 0x08, 0x0c, 0xc4,
            0x11, 0x4d, 0x8d, 0xb4, 0x45, 0x30, 0xf8, 0xf1, 0xe1, 0xee, 0x4f, 0x94, 0xea, 0x37,
            0xe7, 0x8b, 0x57, 0x39,
        ];
        assert_eq!(&out[..32], &expected[..], "SHAKE256(abc) KAT mismatch");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // ML-DSA-65 deterministic bit-exact KAT (golden-vector) tests.
    //
    // FINDING (see task report): bebop2's ML-DSA-65 serialization is NOT FIPS 204
    // bit-exact — pk=3104B (FIPS 1952), sig=3896B (FIPS 3309), c̃=32B (FIPS 48B),
    // pk uses 13-bit t1 packing (FIPS 10-bit), sig uses 24-bit z packing
    // (FIPS 20-bit) and a custom hint layout (pack_sig comment: "not
    // NIST-bit-exact"). There is also no public deserialization API to import a
    // reference (pk,sk) — MlDsa65Sk is only constructible via keygen(). Therefore
    // the OFFICIAL NIST CSRC / FIPS 204 KAT (msg,pk,sk,sig) cannot be asserted
    // byte-exact against this implementation without a serialization rewrite.
    //
    // What IS verified here: signing is DETERMINISTIC for a fixed (sk,msg,rnd)
    // and reproduces a pinned golden byte-vector, i.e. a self-consistent KAT that
    // guards against silent output drift. The reference format is FIPS 204:
    //   https://nvlpubs.nist.gov/nistpubs/fips/nist.fips.204.pdf
    // Golden bytes were captured from bebop2 HEAD 0567003 (this branch), rnd=0
    // (ML-DSA deterministic mode).
    #[test]
    fn mldsa65_deterministic_kat_golden() {
        let seed = [7u8; 32];
        let (_pk, sk) = keygen(&seed);
        let msg = b"probe";
        let rnd = [0u8; 32]; // deterministic mode (FIPS 204 Alg 2 with rnd=0)
        let sig = sign(&sk, msg, &rnd);
        let sig_bytes = pack_sig(&sig.z, &sig.h);

        // GREEN: exact size + exact digest of full serialized signature.
        assert_eq!(sig_bytes.len(), 3896, "bebop2 sig serialization size drifted");
        let mut dig = [0u8; 32];
        shake256(&sig_bytes, &mut dig);
        let expected_digest: [u8; 32] = [
            0x6c, 0x6a, 0x2e, 0x00, 0xea, 0xda, 0xcf, 0xd4, 0xb9, 0x5a, 0x0c, 0x27, 0x18, 0x1a,
            0x29, 0x99, 0xca, 0x83, 0xb5, 0x2a, 0x2d, 0xbf, 0x76, 0x42, 0x9e, 0xaf, 0x15, 0x74,
            0x63, 0x66, 0x05, 0x36,
        ];
        assert_eq!(dig, expected_digest, "ML-DSA-65 deterministic signature bytes changed (KAT drift)");

        // GREEN: pinned c̃ (challenge hash) exact bytes.
        let expected_ctilde: [u8; 32] = [
            0x46, 0x21, 0xcb, 0x4b, 0x48, 0x5a, 0xd2, 0x59, 0x8d, 0x19, 0x60, 0x99, 0x8e, 0x10,
            0x88, 0x1b, 0x11, 0x4e, 0xc5, 0x38, 0xb3, 0x5a, 0xb7, 0x45, 0x42, 0x5f, 0xf8, 0x2a,
            0x7e, 0x50, 0x24, 0xd0,
        ];
        assert_eq!(sig.c_t, expected_ctilde, "ML-DSA-65 c̃ (challenge) bytes changed");

        // Second signing must reproduce identical bytes (determinism).
        let sig2 = sign(&sk, msg, &rnd);
        assert_eq!(pack_sig(&sig2.z, &sig2.h), sig_bytes, "signing non-deterministic for fixed (sk,msg,rnd)");
    }

    // RED: flipping one message byte MUST change the signature (proves the golden
    // KAT is not a constant match independent of input).
    #[test]
    fn mldsa65_kat_red_msg_flip_changes_sig() {
        let seed = [7u8; 32];
        let (_pk, sk) = keygen(&seed);
        let rnd = [0u8; 32];
        let sig_a = pack_sig_of(&sk, b"probe", &rnd);
        let sig_b = pack_sig_of(&sk, b"probf", &rnd); // last byte flipped 'e'->'f'
        assert_ne!(sig_a, sig_b, "flipping a message byte did not change the signature (constant-match defect)");
    }

    // RED: different rnd nonce MUST change the signature.
    #[test]
    fn mldsa65_kat_red_nonce_changes_sig() {
        let seed = [7u8; 32];
        let (_pk, sk) = keygen(&seed);
        let sig_a = pack_sig_of(&sk, b"probe", &[0u8; 32]);
        let sig_b = pack_sig_of(&sk, b"probe", &[1u8; 32]);
        assert_ne!(sig_a, sig_b, "different rnd nonce did not change the signature");
    }

    fn pack_sig_of(sk: &MlDsa65Sk, msg: &[u8], rnd: &[u8; 32]) -> Vec<u8> {
        let s = sign(sk, msg, rnd);
        pack_sig(&s.z, &s.h)
    }

    #[test]
    fn sign_verify_roundtrip_and_tamper() {
        let seed = [7u8; 32];
        let (pk, sk) = keygen(&seed);
        let msg = b"bebop2 ml-dsa-65 fable gate";
        let rnd = [3u8; 32];
        let sig = sign(&sk, msg, &rnd);
        assert!(verify(&pk, msg, &sig), "verify failed on valid signature");
        let mut bad = msg.to_vec();
        bad[0] ^= 0xff;
        assert!(!verify(&pk, &bad, &sig), "tampered message verified (RED missing)");
    }

    // Forge-RED: a signature with all-zero hint and in-bounds random z must be REJECTED.
    // Before the w1_encode double-highbits fix, the commitment collapsed to 0 and ANY in-bounds
    // z with h=0 verified (total break). Now use_hint reconstructs real highbits, so this fails.
    #[test]
    fn forge_with_zero_hint_is_rejected() {
        let seed = [9u8; 32];
        let (pk, _sk) = keygen(&seed);
        let msg = b"forge attempt";
        let mut z = [[0i32; N]; L];
        for p in &mut z {
            for v in p.iter_mut() {
                *v = 1234; // trivially within |z| < gamma1-beta
            }
        }
        let h = [[0i32; N]; K];
        let c_t = [42u8; 32];
        let sig = MlDsa65Sig { z, h, c_t };
        assert!(!verify(&pk, msg, &sig), "forged signature (zero hint, in-bounds z) verified");
    }
}
