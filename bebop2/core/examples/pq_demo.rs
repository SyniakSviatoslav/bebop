//! Runnable PQ demo (P3 — honest "examples/" the roadmap asked for).
//!
//! Proves the from-scratch post-quantum core is LIVE and self-consistent:
//!   - ML-KEM-768 (FIPS 203) encapsulation → shared secret
//!   - ML-DSA-65 (FIPS 204) keygen → sign → verify a frame authorization
//!
//! Run: `cargo run --example pq_demo -p bebop2-core`
//! No vendors, no network, no clock. Determinism via `keygen_derivable` (DSA).

use bebop2_core::pq_dsa::{keygen_derivable, sign, verify, MlDsa65Sig};
use bebop2_core::pq_kem::{decaps, encaps, keygen_from_entropy};

fn main() {
    // --- KEM: ML-KEM-768 (FIPS 203) ---
    let (ek, dk) = keygen_from_entropy().expect("KEM keygen draws fresh OS entropy");
    // `encaps` consumes a caller-supplied RNG closure (each call a unique 32 bytes).
    let mut demo_entropy = [0u8; 32];
    let mut fill = |buf: &mut [u8]| {
        buf.copy_from_slice(&demo_entropy);
        // advance so repeated calls differ (real use: EntropyRng, never repeated)
        for b in demo_entropy.iter_mut() {
            *b = b.wrapping_add(1);
        }
    };
    let (ss_sender, ct) = encaps(&ek, &mut fill);
    let ss_receiver = decaps(&dk, &ct);
    assert_eq!(
        ss_sender, ss_receiver,
        "KEM shared secrets must match (FIPS 203)"
    );
    println!(
        "[KEM] ML-KEM-768 shared secret established: {} bytes",
        ss_sender.len()
    );

    // --- DSA: ML-DSA-65 (FIPS 204) — deterministic, derivable from a seed ---
    let seed = [7u8; 32]; // caller-supplied domain seed (C6: never OS-entropic)
    let (pk, sk) = keygen_derivable(&seed);
    let msg = b"authorize: node=alpha action=route frame=42";
    let rnd = [3u8; 32]; // deterministic signing randomness (FIPS 204 sampling)
    let sig: MlDsa65Sig = sign(&sk, msg, &rnd);
    assert!(verify(&pk, msg, &sig), "ML-DSA-65 signature must verify");
    println!(
        "[DSA] ML-DSA-65 signed + verified authorization ({} byte sig)",
        sig.bytes.len()
    );

    // Tamper must fail (RED): a flipped byte in the message breaks verification.
    let mut tampered = msg.to_vec();
    tampered[0] ^= 0xff;
    assert!(
        !verify(&pk, &tampered, &sig),
        "tampered message must NOT verify"
    );
    println!("[DSA] RED check: tampered authorization correctly rejected");

    println!("\nPQ core demo OK — ML-KEM-768 + ML-DSA-65 live, from scratch, zero-dep.");
}
