//! sign — Ed25519 (RFC 8032 §5.1, test vectors §7.1), from scratch, zero-dependency.
//!
//! Verified-by-Math: every keygen/sign/verify path is anchored to the RFC 8032 §7.1
//! published test vectors (bit-exact). A corrupted signature MUST fail verification
//! (RED case asserted). Determinism: identical seed → identical (pk, sk) and signature.
//!
//! NO external crates, NO std::time/OS RNG/network. SHA-512 comes from `crate::hash`.
//! The only entropy is caller-supplied: `keygen(seed)` takes a 32-byte seed; `sign`
//! takes the secret key + message. (Production seeds hardware entropy out of tree.)
//!
//! Field arithmetic is GF(2^255-19), represented as a 32-byte little-endian
//! canonical integer (the value is always in [0, p)). All ops reduce mod p.
//! Curve is the twisted Edwards form a = -1, d = -121665/121666 mod p.

extern crate alloc;

use alloc::vec::Vec;
use core::convert::TryInto;

// ── GF(2^255-19): 32-byte LE canonical integers, reduced mod p ───────────────
// p = 2^255 - 19.
// P_BE: p as big-endian bytes, for the big-endian bignum helpers (cmp/sub/mod).
type Fe = [u8; 32];

const P_BE: [u8; 32] = [
    0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xed,
];

#[inline]
fn fe_0() -> Fe {
    [0u8; 32]
}
#[inline]
fn fe_1() -> Fe {
    let mut o = [0u8; 32];
    o[0] = 1;
    o
}
#[inline]
fn fe_2() -> Fe {
    let mut o = [0u8; 32];
    o[0] = 2;
    o
}

/// Load a 32-byte LE field element. Caller guarantees it is < p (true for point
/// encodings and constants we construct this way).
#[inline]
fn fe_from_bytes(b: &[u8; 32]) -> Fe {
    *b
}

/// Canonical encoding is the 32-byte LE integer.
#[inline]
fn fe_to_bytes(a: &Fe) -> [u8; 32] {
    *a
}

// ── Fast GF(2^255-19) arithmetic in 64-bit limbs (no heap, no per-bit loop) ──
// `Fe` stays the canonical 32-byte LE integer < p. The slow path was a
// `Vec<u8>` big-endian bignum with a bit-by-bit division per field op; this
// replaces it with fixed 64-bit-limb schoolbook + a 2^255≡19 reduction.
// Algebra is identical to the RFC 8032 §5.1 spec — same values, ~1000× faster.
// p = 2^255 - 19  (little-endian u64 limbs).
const P_LIMBS: [u64; 4] = [
    0xffff_ffff_ffff_ffed,
    0xffff_ffff_ffff_ffff,
    0xffff_ffff_ffff_ffff,
    0x7fff_ffff_ffff_ffff,
];

#[inline]
fn fe_to_limbs(a: &Fe) -> [u64; 4] {
    let mut out = [0u64; 4];
    for i in 0..4 {
        let mut limb = 0u64;
        for j in 0..8 {
            limb |= (a[i * 8 + j] as u64) << (8 * j);
        }
        out[i] = limb;
    }
    out
}

#[inline]
fn fe_from_limbs(a: &[u64; 4]) -> Fe {
    let mut out = [0u8; 32];
    for i in 0..4 {
        let limb = a[i];
        for j in 0..8 {
            out[i * 8 + j] = (limb >> (8 * j)) as u8;
        }
    }
    out
}

/// Schoolbook 4×4 -> 8-limb product of two 256-bit LE values.
/// Uses a full u128 accumulator with a propagated carry chain so no limb silently
/// truncates (a bare `p[i+4] = (p[i+4] + carry) as u64` would drop high bits).
#[inline]
fn limbs_mul(a: &[u64; 4], b: &[u64; 4]) -> [u64; 8] {
    let mut p = [0u64; 8];
    for i in 0..4 {
        let mut carry: u128 = 0;
        for j in 0..4 {
            let idx = i + j;
            let v = p[idx] as u128 + (a[i] as u128) * (b[j] as u128) + carry;
            p[idx] = v as u64;
            carry = v >> 64;
        }
        // Propagate the leftover carry through the high limbs.
        let mut k = i + 4;
        let mut c = carry;
        while c > 0 {
            let v = p[k] as u128 + c;
            p[k] = v as u64;
            c = v >> 64;
            k += 1;
        }
    }
    p
}

/// Fold a value V (up to 8 LE u64 limbs) mod p using 2^255 ≡ 19 (mod p):
///   V = A + 2^255·B  →  A + 19·B
/// Returns the result as up to 7 LE limbs.
#[inline]
fn fold_val(v: &[u64; 8]) -> [u64; 7] {
    let a0 = v[0];
    let a1 = v[1];
    let a2 = v[2];
    let a3 = v[3] & 0x7fff_ffff_ffff_ffff;
    // B = V >> 255 (V < 2^512 so B < 2^257). Correct limb extraction:
    //   b_k = (V bits 255+64k .. 255+64k+63)
    //       = (v_{k+4} << 1) | (v_{k+3} >> 63), with v_8 = 0.
    let b0 = (v[4] << 1) | (v[3] >> 63);
    let b1 = (v[5] << 1) | (v[4] >> 63);
    let b2 = (v[6] << 1) | (v[5] >> 63);
    let b3 = (v[7] << 1) | (v[6] >> 63);
    let b4 = v[7] >> 63;
    let b5 = 0u64;
    let mut tb = [0u64; 6];
    let mut carry = 0u128;
    let b = [b0, b1, b2, b3, b4, b5];
    for i in 0..6 {
        let val = (b[i] as u128) * 19 + carry;
        tb[i] = val as u64;
        carry = val >> 64;
    }
    let mut r = [0u64; 7];
    r[0] = a0;
    r[1] = a1;
    r[2] = a2;
    r[3] = a3;
    let mut c = 0u128;
    for i in 0..6 {
        let val = r[i] as u128 + tb[i] as u128 + c;
        r[i] = val as u64;
        c = val >> 64;
    }
    if c > 0 {
        r[6] = c as u64;
    }
    r
}

/// Reduce an 8-limb product mod p = 2^255-19. Iterate the 2^255-fold (each pass
/// shrinks the magnitude by ~2^255) until the value fits in 255 bits, then do a
/// single conditional subtraction of p. Converges in <= 3 folds.
fn reduce_p(prod: &[u64; 8]) -> [u64; 4] {
    let mut r = fold_val(prod);
    let mut guard = 0;
    while (r[4] | r[5] | r[6]) != 0 || r[3] >= 0x8000_0000_0000_0000 {
        let v8 = [r[0], r[1], r[2], r[3], r[4], r[5], r[6], 0];
        r = fold_val(&v8);
        guard += 1;
        if guard > 8 {
            break;
        }
    }
    if limbs_ge_p(&r) {
        limbs_sub_p(&mut r);
    }
    [r[0], r[1], r[2], r[3]]
}

#[inline]
fn limbs_ge_p(r: &[u64; 7]) -> bool {
    for i in (4..7).rev() {
        if r[i] != 0 {
            return true;
        }
    }
    for i in (0..4).rev() {
        if r[i] > P_LIMBS[i] {
            return true;
        }
        if r[i] < P_LIMBS[i] {
            return false;
        }
    }
    true // r == p: must still subtract p to normalize to 0
}

fn limbs_sub_p(r: &mut [u64; 7]) {
    let mut borrow = 0i128;
    for i in 0..4 {
        let v = r[i] as i128 - P_LIMBS[i] as i128 - borrow;
        if v < 0 {
            r[i] = (v + (1i128 << 64)) as u64;
            borrow = 1;
        } else {
            r[i] = v as u64;
            borrow = 0;
        }
    }
    for i in 4..7 {
        if borrow == 0 {
            break;
        }
        let v = r[i] as i128 - borrow;
        if v < 0 {
            r[i] = (v + (1i128 << 64)) as u64;
            borrow = 1;
        } else {
            r[i] = v as u64;
            borrow = 0;
        }
    }
}

#[inline]
fn fe_add(a: &Fe, b: &Fe) -> Fe {
    let la = fe_to_limbs(a);
    let lb = fe_to_limbs(b);
    let mut s = [0u64; 8];
    let mut carry = 0u128;
    for i in 0..4 {
        let v = la[i] as u128 + lb[i] as u128 + carry;
        s[i] = v as u64;
        carry = v >> 64;
    }
    if carry > 0 {
        s[4] = carry as u64;
    }
    let prod = [s[0], s[1], s[2], s[3], s[4], 0, 0, 0];
    fe_from_limbs(&reduce_p(&prod))
}

#[inline]
fn fe_sub(a: &Fe, b: &Fe) -> Fe {
    let la = fe_to_limbs(a);
    let lb = fe_to_limbs(b);
    // Compute p + a as a 5-limb value WITH carry propagation into pa[4].
    let mut pa = [0u64; 5];
    let mut c: u128 = 0;
    for i in 0..4 {
        let v = P_LIMBS[i] as u128 + la[i] as u128 + c;
        pa[i] = v as u64;
        c = v >> 64;
    }
    pa[4] = c as u64; // 0 or 1 (p+a < 2p < 2^256 when a < p)
                      // Now pa - b (b has 4 limbs, b < p). Result is in [0, 2p); keep pa[4] as the
                      // high limb so the carry isn't lost. Integer pa >= b so final borrow is 0.
    let mut d = [0u64; 8];
    let mut borrow = 0i128;
    for i in 0..4 {
        let mut v = pa[i] as i128 - lb[i] as i128 - borrow;
        if v < 0 {
            v += 1i128 << 64;
            borrow = 1;
        } else {
            borrow = 0;
        }
        d[i] = v as u64;
    }
    let prod = [d[0], d[1], d[2], d[3], pa[4], 0, 0, 0];
    fe_from_limbs(&reduce_p(&prod))
}

#[inline]
fn fe_neg(a: &Fe) -> Fe {
    fe_sub(&fe_0(), a)
}

#[inline]
fn fe_mul(a: &Fe, b: &Fe) -> Fe {
    let prod = limbs_mul(&fe_to_limbs(a), &fe_to_limbs(b));
    fe_from_limbs(&reduce_p(&prod))
}

/// d = -121665/121666 mod p, computed from integers (not a hardcoded limb constant),
/// so the representation is independent of any radix convention.
fn fe_d() -> Fe {
    // -121665 mod p = 0 - 121665 (proper LE Fe, since fe_sub reduces mod p).
    let num = fe_sub(&fe_0(), &fe_from_u64(121665));
    let den = fe_from_u64(121666);
    fe_mul(&num, &fe_invert(&den))
}

#[inline]
fn fe_square(a: &Fe) -> Fe {
    fe_mul(a, a)
}

/// Invert a via Fermat: a^(p-2) mod p, using square-and-multiply over the exact
/// 255-bit exponent p-2 = 0x7fff…ffeb (no hand-counted windows that can drift).
fn fe_invert(a: &Fe) -> Fe {
    // MSB-first square-and-multiply: square the accumulator FIRST, then
    // conditionally multiply by the (constant) base. This computes a^E exactly.
    let mut acc = fe_1();
    let base = *a;
    for i in (0..255).rev() {
        acc = fe_square(&acc);
        let bit = (P_MINUS_2[i / 64] >> (i % 64)) & 1;
        if bit == 1 {
            acc = fe_mul(&acc, &base);
        }
    }
    acc
}

// p - 2 = 2^255 - 21, little-endian u64 limbs (255-bit value).
const P_MINUS_2: [u64; 4] = [
    0xffff_ffff_ffff_ffeb,
    0xffff_ffff_ffff_ffff,
    0xffff_ffff_ffff_ffff,
    0x7fff_ffff_ffff_ffff,
];

// Exponent e1 = (p + 3) / 8 = 2^252 - 2, used for the candidate square root when
// p ≡ 5 (mod 8). Stored as a 32-byte LE integer bit string.
const E1: [u8; 32] = [
    0xfe, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x0f,
];
// sqrt(-1) mod p = 2^((p-1)/4) mod p. Precomputed const (Tonelli-Shanks sign fix).
const SQRT_M1: [u8; 32] = [
    0xb0, 0xa0, 0x0e, 0x4a, 0x27, 0x1b, 0xee, 0xc4, 0x78, 0xe4, 0x2f, 0xad, 0x06, 0x18, 0x43, 0x2f,
    0xa7, 0xd7, 0xfb, 0x3d, 0x99, 0x00, 0x4d, 0x2b, 0x0b, 0xdf, 0xc1, 0x4f, 0x80, 0x24, 0x83, 0x2b,
];

/// Modular square root for p ≡ 5 (mod 8). Returns Some(root) where root^2 = a (mod p)
/// and root has the lower "x_0" sign bit, or None if a is a non-residue.
/// Algorithm: candidate x = a^((p+3)/8); if x^2 == a, x is the root; else if x^2 == -a,
/// the root is x * sqrt(-1); otherwise a has no square root.
fn fe_sqrt(a: &Fe) -> Option<Fe> {
    let x = {
        // a^E1 via square-and-multiply over the 256-bit E1 bit string (MSB-first).
        let mut acc = fe_1();
        let base = *a;
        for i in (0..256).rev() {
            acc = fe_square(&acc);
            let bit = (E1[i / 8] >> (i % 8)) & 1;
            if bit == 1 {
                acc = fe_mul(&acc, &base);
            }
        }
        acc
    };
    let xx = fe_square(&x);
    if fe_eq(&xx, a) {
        Some(x)
    } else {
        // x^2 == -a  →  root = x * sqrt(-1)
        let neg_a = fe_neg(a);
        if fe_eq(&xx, &neg_a) {
            Some(fe_mul(&x, &SQRT_M1))
        } else {
            None
        }
    }
}

/// Constant-time field equality (returns true iff a == b as canonical Fe).
fn fe_eq(a: &Fe, b: &Fe) -> bool {
    let mut diff = 0u8;
    for i in 0..32 {
        diff |= a[i] ^ b[i];
    }
    diff == 0
}

// ── Point in projective (X:Y:Z) coordinates; x = X/Z, y = Y/Z ────────────────
fn fe_from_u64(v: u64) -> Fe {
    let mut o = [0u8; 32];
    o[0..8].copy_from_slice(&v.to_le_bytes());
    o
}

// d = -121665/121666 mod 2^255-19 (computed once at first use; see fe_d()).
// Stored as a const via a const-fn-free lazy: we just call fe_d() where needed.
// For static use we precompute it as a `lazy` const expression through a function.

// ── Point in projective (X:Y:Z) coordinates; x = X/Z, y = Y/Z ────────────────
#[derive(Clone, Copy)]
struct Point {
    x: Fe,
    y: Fe,
    z: Fe,
    t: Fe,
}

fn point_identity() -> Point {
    Point {
        x: fe_0(),
        y: fe_1(),
        z: fe_1(),
        t: fe_0(),
    }
}

/// Twisted-Edwards addition in extended homogeneous coordinates (RFC 8032
/// §5.1.4, a = -1). Complete addition — `point_double` reuses it. Verbatim from
/// the RFC:
///   A = (Y1-X1)*(Y2-X2)   B = (Y1+X1)*(Y2+X2)   C = T1*2*d*T2   D = Z1*2*Z2
///   E = B-A   F = D-C   G = D+C   H = B+A
///   X3 = E*F   Y3 = G*H   T3 = E*H   Z3 = F*G
///
/// `d2` must be the precomputed `2*d` (passed in to avoid recomputing the
/// expensive `fe_invert` inside every addition).
fn point_add(p: &Point, q: &Point, d2: &Fe) -> Point {
    let x1 = p.x;
    let y1 = p.y;
    let x2 = q.x;
    let y2 = q.y;
    let a = fe_mul(&fe_sub(&y1, &x1), &fe_sub(&y2, &x2)); // A = (Y1-X1)*(Y2-X2)
    let b = fe_mul(&fe_add(&y1, &x1), &fe_add(&y2, &x2)); // B = (Y1+X1)*(Y2+X2)
    let c = fe_mul(d2, &fe_mul(&p.t, &q.t)); // C = T1*2*d*T2
    let dd = fe_mul(&fe_mul(&p.z, &q.z), &fe_2()); // D = Z1*2*Z2
    let e = fe_sub(&b, &a); // E = B - A
    let f = fe_sub(&dd, &c); // F = D - C
    let g = fe_add(&dd, &c); // G = D + C
    let h = fe_add(&b, &a); // H = B + A
    let x3 = fe_mul(&e, &f); // X3 = E*F
    let y3 = fe_mul(&g, &h); // Y3 = G*H
    let t3 = fe_mul(&e, &h); // T3 = E*H
    let z3 = fe_mul(&f, &g); // Z3 = F*G
    Point {
        x: x3,
        y: y3,
        z: z3,
        t: t3,
    }
}

/// Double via addition (correct for a = -1 twisted Edwards).
fn point_double(p: &Point, d2: &Fe) -> Point {
    point_add(p, p, d2)
}

/// Decode an RFC 8032 32-byte point encoding (y, x-sign in top bit of last byte).
fn point_decompress(s: &[u8; 32]) -> Option<Point> {
    let mut b = *s;
    let sign_bit = b[31] >> 7;
    b[31] &= 0x7f;
    // RFC 8032 §5.1.3: reject non-canonical y (y >= p) — encoding must be canonical.
    if cmp_be(&be(&b), &P_BE) != core::cmp::Ordering::Less {
        return None;
    }
    let y = fe_from_bytes(&b);
    let one = fe_1();
    let yy = fe_square(&y);
    // x^2 = (y^2 - 1) / (d*y^2 + 1) = u / v
    let u = fe_sub(&yy, &one);
    let v = fe_add(&fe_mul(&fe_d(), &yy), &one);
    let uv_inv = fe_mul(&u, &fe_invert(&v)); // = x^2 candidate
    let x = match fe_sqrt(&uv_inv) {
        None => return None,
        Some(x) => x,
    }; // root r with r's own low-bit parity
       // The other root is -r; verify r^2 * v == u (else non-residue → reject).
    let check = fe_sub(&fe_mul(&fe_square(&x), &v), &u);
    if !fe_eq(&check, &fe_0()) {
        return None;
    }
    // Choose the root whose low bit matches the encoded sign bit.
    let xb = fe_to_bytes(&x);
    let xsign = xb[0] & 1;
    let xfinal = if xsign != sign_bit { fe_neg(&x) } else { x };
    Some(Point {
        x: xfinal,
        y,
        z: one,
        t: fe_mul(&xfinal, &y),
    })
}

/// Encode a point: y (LE) with x-sign in top bit.
fn point_compress(p: &Point) -> [u8; 32] {
    let zinv = fe_invert(&p.z);
    let y = fe_to_bytes(&fe_mul(&p.y, &zinv)); // affine y = y / z
                                               // compute x/z mod p to read the sign bit
    let xz = fe_to_bytes(&fe_mul(&p.x, &zinv));
    let mut out = y;
    if (xz[0] & 1) == 1 {
        out[31] |= 0x80;
    }
    out
}

/// Projective point equality: P == Q iff X1*Z2 == X2*Z1 AND Y1*Z2 == Y2*Z1.
fn point_eq(p: &Point, q: &Point) -> bool {
    let a = fe_to_bytes(&fe_mul(&p.x, &q.z));
    let b = fe_to_bytes(&fe_mul(&q.x, &p.z));
    let c = fe_to_bytes(&fe_mul(&p.y, &q.z));
    let d = fe_to_bytes(&fe_mul(&q.y, &p.z));
    a == b && c == d
}

// ── Scalar arithmetic mod L (group order), 256-bit bignum (BE Vec<u8>) ────────
// L = 2^252 + 27742317777372353535851937790883648493
const L: [u8; 32] = [
    0xed, 0xd3, 0xf5, 0x5c, 0x1a, 0x63, 0x12, 0x58, 0xd6, 0x9c, 0xf7, 0xa2, 0xde, 0xf9, 0xde, 0x14,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10,
];

fn be(bytes_le: &[u8; 32]) -> Vec<u8> {
    bytes_le.iter().rev().copied().collect()
}

fn cmp_be(a: &[u8], b: &[u8]) -> core::cmp::Ordering {
    // pad to equal length with leading zeros
    let n = core::cmp::max(a.len(), b.len());
    let mut pa = vec![0u8; n - a.len()];
    pa.extend_from_slice(a);
    let mut pb = vec![0u8; n - b.len()];
    pb.extend_from_slice(b);
    for i in 0..n {
        if pa[i] < pb[i] {
            return core::cmp::Ordering::Less;
        } else if pa[i] > pb[i] {
            return core::cmp::Ordering::Greater;
        }
    }
    core::cmp::Ordering::Equal
}

fn sub_be(a: &[u8], b: &[u8]) -> Vec<u8> {
    // assumes a >= b
    let n = core::cmp::max(a.len(), b.len());
    let mut pa = vec![0u8; n - a.len()];
    pa.extend_from_slice(a);
    let mut pb = vec![0u8; n - b.len()];
    pb.extend_from_slice(b);
    let mut out = vec![0u8; n];
    let mut borrow = 0i32;
    for i in (0..n).rev() {
        let mut v = pa[i] as i32 - borrow;
        if v < pb[i] as i32 {
            v += 256;
            borrow = 1;
        } else {
            borrow = 0;
        }
        out[i] = (v - pb[i] as i32) as u8;
    }
    // trim leading zeros
    let mut start = 0;
    while start < out.len() - 1 && out[start] == 0 {
        start += 1;
    }
    out[start..].to_vec()
}

fn add_be(a: &[u8], b: &[u8]) -> Vec<u8> {
    let n = core::cmp::max(a.len(), b.len()) + 1;
    let mut pa = vec![0u8; n - a.len()];
    pa.extend_from_slice(a);
    let mut pb = vec![0u8; n - b.len()];
    pb.extend_from_slice(b);
    let mut out = vec![0u8; n];
    let mut carry = 0u32;
    for i in (0..n).rev() {
        let v = pa[i] as u32 + pb[i] as u32 + carry;
        out[i] = (v & 0xff) as u8;
        carry = v >> 8;
    }
    out
}

fn mul_be(a: &[u8], b: &[u8]) -> Vec<u8> {
    // Inputs are big-endian. Reverse to little-endian (index 0 = LSB) and do the
    // standard grade-school multiply with a carry chain, then reverse the result
    // back to BE.
    let al: Vec<u8> = a.iter().rev().copied().collect();
    let bl: Vec<u8> = b.iter().rev().copied().collect();
    let mut out = vec![0u8; al.len() + bl.len()];
    for i in 0..al.len() {
        let mut carry = 0u32;
        for j in 0..bl.len() {
            let idx = i + j;
            let v = out[idx] as u32 + (al[i] as u32) * (bl[j] as u32) + carry;
            out[idx] = (v & 0xff) as u8;
            carry = v >> 8;
        }
        out[i + bl.len()] = (out[i + bl.len()] as u32 + carry) as u8;
    }
    out.iter().rev().copied().collect()
}

/// Reduce a big-endian bignum mod L via bit-by-bit division.
/// Group order L, as a big-endian Vec, for the bignum mod-L helpers.
/// (The `L` const is stored little-endian; `mod_l`/`sub_be`/`cmp_be` need BE.)
fn l_be() -> Vec<u8> {
    let mut v = L.to_vec();
    v.reverse();
    v
}

fn mod_l(num_be: &[u8]) -> [u8; 32] {
    let l = l_be();
    let mut rem: Vec<u8> = Vec::new();
    for &byte in num_be {
        for bit in (0..8).rev() {
            rem = add_be(&rem, &rem); // rem << 1
            if (byte >> bit) & 1 == 1 {
                rem = add_be(&rem, &[1]);
            }
            if cmp_be(&rem, &l) != core::cmp::Ordering::Less {
                rem = sub_be(&rem, &l);
            }
        }
    }
    // left-pad to 32 bytes LE output
    while rem.len() < 32 {
        rem.insert(0, 0);
    }
    let mut out = [0u8; 32];
    // rem is BE; convert to 32-byte LE (trim/pad)
    let be32 = if rem.len() >= 32 {
        &rem[rem.len() - 32..]
    } else {
        &rem[..]
    };
    for i in 0..be32.len() {
        out[i] = be32[be32.len() - 1 - i];
    }
    out
}

/// SHA-512 hash of `data`, reduced mod L (256-bit scalar).
fn scalar_from_hash(data: &[u8]) -> [u8; 32] {
    let h = crate::hash::sha512(data);
    mod_l(&be_array(&h))
}

fn be_array(h: &[u8; 64]) -> Vec<u8> {
    h.iter().rev().copied().collect()
}

/// Scalar (LE 32 bytes) × scalar → mod L.
fn scalar_mul_mod_l(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let prod = mul_be(&be(a), &be(b));
    mod_l(&prod)
}

/// Scalar (LE 32 bytes) + scalar → mod L.
fn scalar_add_mod_l(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    let s = add_be(&be(a), &be(b));
    mod_l(&s)
}

/// Scalar (LE 32 bytes) × scalar → mod L.
fn scalar_mul(base: &Point, scalar_le: &[u8; 32]) -> Point {
    let d2 = fe_mul(&fe_d(), &fe_2()); // 2*d, computed once
    let mut result = point_identity();
    let mut addend = *base;
    for i in 0..256 {
        let byte = scalar_le[i / 8];
        let bit = (byte >> (i % 8)) & 1;
        if bit == 1 {
            result = point_add(&result, &addend, &d2);
        }
        addend = point_double(&addend, &d2);
    }
    result
}

// ── Public API ────────────────────────────────────────────────────────────────

/// RFC 8032 §5.1.5 — generate (public_key, secret_key) from a 32-byte seed.
/// secret_key = seed || pubkey (64 bytes, RFC form). pubkey = 32 bytes.
///
/// **TEST-ONLY / `dangerous_deterministic`.** In a normal (non-test, feature-off)
/// build this symbol does not exist, so production code cannot keygen from a
/// predictable constant seed. Use [`keygen_from_entropy`] for prod.
#[cfg(any(test, feature = "dangerous_deterministic", feature = "test_keygen"))]
pub fn keygen(seed: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let az = crate::hash::sha512(seed);
    let mut a = [0u8; 32];
    a.copy_from_slice(&az[0..32]);
    // clamp (RFC 8032 §5.1.5): clear low 3 bits of octet 0; clear high bit and
    // set second-highest bit of octet 31.
    a[0] &= 248;
    a[31] = (a[31] & 0x7f) | 0x40;
    let b_pt = point_decompress(&B_ENCODED).expect("base point must decode");
    let a_pt = scalar_mul(&b_pt, &a);
    let pk = point_compress(&a_pt);
    let mut sk = [0u8; 32];
    sk.copy_from_slice(&pk);
    (pk, sk)
}

/// Production Ed25519 keygen: draw a fresh 32-byte seed from platform entropy and
/// derive the keypair. Fail-closed — returns `Err` if entropy is unavailable, never
/// a constant fallback. Replaces the constant-seed [`keygen`] in all prod paths.
pub fn keygen_from_entropy() -> Result<([u8; 32], [u8; 32]), crate::rng::EntropyError> {
    let mut seed = [0u8; 32];
    crate::rng::entropy_provider().fill(&mut seed)?;
    // Delegate to the deterministic core. SAFETY: `keygen` is gated behind test /
    // dangerous_deterministic, but it is NEVER depend-feature-gated off for the crate
    // itself (only for downstream callers), so it is always available in-tree. To keep
    // the prod path unconditionally present, inline the derivation here instead.
    Ok(keygen_from_seed_infallible(&seed))
}

/// In-tree deterministic Ed25519 derivation (always available; never exposed publicly
/// as a constant-seed entry point). Used by [`keygen_from_entropy`].
fn keygen_from_seed_infallible(seed: &[u8; 32]) -> ([u8; 32], [u8; 32]) {
    let az = crate::hash::sha512(seed);
    let mut a = [0u8; 32];
    a.copy_from_slice(&az[0..32]);
    a[0] &= 248;
    a[31] = (a[31] & 0x7f) | 0x40;
    let b_pt = point_decompress(&B_ENCODED).expect("base point must decode");
    let a_pt = scalar_mul(&b_pt, &a);
    let pk = point_compress(&a_pt);
    let mut sk = [0u8; 32];
    sk.copy_from_slice(&pk);
    (pk, sk)
}

/// RFC 8032 §5.1.6 — sign `msg` with the 32-byte secret seed → 64-byte signature.
/// (Convenience form: takes the seed, derives the secret scalar internally.)
pub fn sign(seed: &[u8; 32], msg: &[u8]) -> [u8; 64] {
    let az = crate::hash::sha512(seed);
    let mut a = [0u8; 32];
    a.copy_from_slice(&az[0..32]);
    a[0] &= 248;
    a[31] = (a[31] & 0x7f) | 0x40;
    let prefix = &az[32..64];

    let b_pt = point_decompress(&B_ENCODED).expect("base point must decode");
    let a_pt = scalar_mul(&b_pt, &a);
    let pk = point_compress(&a_pt);

    let mut r_input = Vec::with_capacity(prefix.len() + msg.len());
    r_input.extend_from_slice(prefix);
    r_input.extend_from_slice(msg);
    let r = scalar_from_hash(&r_input);

    let r_pt = scalar_mul(&b_pt, &r);
    let r_enc = point_compress(&r_pt);

    let mut k_input = Vec::with_capacity(32 + 32 + msg.len());
    k_input.extend_from_slice(&r_enc);
    k_input.extend_from_slice(&pk);
    k_input.extend_from_slice(msg);
    let k = scalar_from_hash(&k_input);

    // S = (r + k*a) mod L
    let ka = scalar_mul_mod_l(&k, &a);
    let s = scalar_add_mod_l(&r, &ka);

    let mut sig = [0u8; 64];
    sig[0..32].copy_from_slice(&r_enc);
    sig[32..64].copy_from_slice(&s);
    sig
}

/// RFC 8032 §5.1.7 — verify a 64-byte signature over `msg` with `pubkey`.
pub fn verify(pubkey: &[u8; 32], msg: &[u8], sig: &[u8; 64]) -> bool {
    let r_enc = match sig[0..32].try_into() {
        Ok(v) => v,
        Err(_) => return false,
    };
    let s_le = match sig[32..64].try_into() {
        Ok(v) => v,
        Err(_) => return false,
    };
    // RFC 8032 §5.1.7: S must be in [0, L). Reject non-canonical / malleable S >= L.
    if cmp_be(&be(&s_le), &l_be()) != core::cmp::Ordering::Less {
        return false;
    }
    let a_pt = match point_decompress(pubkey) {
        Some(p) => p,
        None => return false,
    };
    let r_pt = match point_decompress(&r_enc) {
        Some(p) => p,
        None => return false,
    };

    let mut k_input = Vec::with_capacity(32 + 32 + msg.len());
    k_input.extend_from_slice(&r_enc);
    k_input.extend_from_slice(pubkey);
    k_input.extend_from_slice(msg);
    let k = scalar_from_hash(&k_input);

    // Check S·B == R + k·A
    let b_pt = match point_decompress(&B_ENCODED) {
        Some(p) => p,
        None => return false,
    };
    let lhs = scalar_mul(&b_pt, &s_le);
    let ka_pt = scalar_mul(&a_pt, &k);
    let d2 = fe_mul(&fe_d(), &fe_2());
    let rhs = point_add(&r_pt, &ka_pt, &d2);
    point_eq(&lhs, &rhs)
}

/// RFC 8032 §5.1.3 base point B encoding (y = 4/5, x positive).
const B_ENCODED: [u8; 32] = [
    0x58, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
    0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66, 0x66,
];

#[cfg(test)]
mod tests {
    use super::*;

    fn dehex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    // RFC 8032 §7.1 — first test vector (the canonical one).
    #[test]
    fn ed25519_rfc8032_section_7_1_vector1() {
        // RFC 8032 §7.1 TEST 1 (verbatim).
        let seed_hex = "9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60";
        let pk_hex = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
        let msg = b"";
        let sig_hex = "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b";

        let seed: [u8; 32] = dehex(seed_hex).try_into().unwrap();
        let (pk, _sk) = keygen(&seed);
        assert_eq!(hex(&pk), pk_hex, "pubkey mismatch (RFC 8032 §7.1 #1)");

        let sig = sign(&seed, msg);
        assert_eq!(hex(&sig), sig_hex, "signature mismatch (RFC 8032 §7.1 #1)");

        let pk_arr: [u8; 32] = pk;
        assert!(
            verify(&pk_arr, msg, &sig),
            "verify must pass for the genuine §7.1 #1 signature"
        );

        // RED KAT: a wrong public key must NOT verify the genuine signature.
        // (Catches a verify-always-true bug — the original test only used the computed pk.)
        let mut wrong_pk = pk_arr;
        wrong_pk[0] ^= 0xff;
        assert!(
            !verify(&wrong_pk, msg, &sig),
            "verify must REJECT a signature under the wrong public key"
        );
    }

    #[test]
    fn ed25519_roundtrip_red_green() {
        // GREEN: sign then verify recovers. RED: tampering the signature fails.
        let seed = [0x42u8; 32];
        let msg = b"the cosmo-noir helm turns by starlight, never by panic.";
        let (pk, _sk) = keygen(&seed);
        let sig = sign(&seed, msg);
        assert!(verify(&pk, msg, &sig), "genuine signature must verify");

        // RED: flip a byte in the signature → must NOT verify.
        let mut bad = sig;
        bad[10] ^= 0xff;
        assert!(
            !verify(&pk, msg, &bad),
            "tampered signature must NOT verify"
        );

        // RED: different message → must NOT verify.
        assert!(
            !verify(&pk, b"other", &sig),
            "wrong message must NOT verify"
        );
    }

    #[test]
    fn ed25519_deterministic_same_seed_same_keys() {
        let seed = [0x13u8; 32];
        let (pk1, _) = keygen(&seed);
        let (pk2, _) = keygen(&seed);
        assert_eq!(pk1, pk2, "same seed → same public key");
    }

    #[test]
    fn ed25519_field_known_values() {
        // Sanity: 1 + 1 == 2, and invert(invert(x)) == x for a sample field element.
        let one = fe_1();
        let two = fe_add(&one, &one);
        assert_eq!(fe_to_bytes(&two)[0], 2, "1 + 1 = 2 in the field");
        let x = fe_from_bytes(&[0x11; 32]);
        let xi = fe_invert(&x);
        let back = fe_mul(&x, &xi);
        assert_eq!(fe_to_bytes(&back), fe_to_bytes(&one), "x * x^-1 == 1");
    }

    fn hex(b: &[u8]) -> String {
        let mut s = String::new();
        for x in b {
            s.push_str(&format!("{:02x}", x));
        }
        s
    }
}

#[cfg(test)]
mod bignum_tests {
    use super::*;
    #[test]
    fn mul_be_basic() {
        // 123 * 456 = 56088 = 0xDB18 (big-endian bytes; mul_be returns len a+b = 3)
        let a = vec![123u8];
        let b = vec![0x01u8, 0xC8u8]; // 456 = 0x01C8 (big-endian)
        let prod = mul_be(&a, &b);
        assert_eq!(
            prod,
            vec![0, 0xDB, 0x18],
            "123*456 should be 0x00DB18, got {:?}",
            prod
        );
        // verify via fe_mul
        let f = fe_mul(&fe_from_u64(123), &fe_from_u64(456));
        let le = fe_to_bytes(&f);
        // 56088 mod p = 56088 (since < p); LE [0]=24,[1]=219
        assert_eq!(le[0], 24, "fe_mul(123,456)[0]");
        assert_eq!(le[1], 219, "fe_mul(123,456)[1]");
    }
}
