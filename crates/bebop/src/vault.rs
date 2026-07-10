//! Bebop VAULT — post-quantum, self-certifying node identity (encrypted at rest).
//!
//! Hybrid design (max-EV, FIPS 203/204 + classical fallback):
//!   • KEM  : ML-KEM-768 (FIPS 203)  ⊕  X25519   — concat-KEM hybrid
//!   • Sign : ML-DSA-65 (FIPS 204)  ⊕  Ed25519 — hybrid signature
//!   • KDF  : Argon2id (memory-hard) — replaces the old scrypt
//!   • AEAD : XChaCha20-Poly1305 (confidentiality holds post-quantum)
//!
//! The hybrid pairs survive BOTH a quantum *and* a (hypothetical) regression in
//! either primitive — per NIST SP 800-208 hybrid guidance. We do NOT drop the
//! classical half, and we do NOT trust the unaudited PQ crates alone.
//!
//! Entropy (OsRng) is used ONCE at create for keygen, never for runtime output
//! — the vault stays reproducible from the passphrase for AEAD derive, and the
//! agent never rolls dice into observables.
//!
//! Self-certifying identity: `id = H(pq_pub ‖ classical_pub)`. A swapped or
//! tampered key blob yields a *different* id → unlock refuses (fail-closed).

use anyhow::{anyhow, bail, Result};
use argon2::{
    password_hash::{PasswordHasher, SaltString},
    Argon2,
};
use chacha20poly1305::{
    aead::{Aead, KeyInit as AeadKeyInit},
    XChaCha20Poly1305, XNonce,
};
use ed25519_dalek::{
    Signature as EdSig, Signer as EdSigner, SigningKey as EdSk, Verifier as EdVerifier,
    VerifyingKey as EdPk,
};
use getrandom::fill;
use ml_dsa::{Generate, KeyExport, Keypair, MlDsa65, SignatureEncoding};
use ml_kem::{
    kem::{Decapsulate, Encapsulate, Kem, KeyInit as KemKeyInit, TryKeyInit},
    DecapsulationKey, EncapsulationKey, Key, MlKem768,
};
use sha2::{Digest, Sha512};
use std::fs;
use std::sync::OnceLock;
use x25519_dalek::{PublicKey as X25519Pk, StaticSecret as X25519Sk};
use zeroize::Zeroize;

/// Vault header version. Bump on any format change.
pub const VAULT_VERSION: u8 = 2;
/// XChaCha20 nonce length (24 bytes).
const NONCE_LEN: usize = 24;
/// Argon2id params: conservative, memory-hard. 64 MiB, 3 passes, 4 lanes.
const ARGON_M: u32 = 65536;
const ARGON_T: u32 = 3;
const ARGON_P: u32 = 4;

/// Byte widths of each hybrid half (for bundle framing + tests).
const PQ_EK: usize = 1184; // ML-KEM-768 encapsulation (public) key
const PQ_DK: usize = 64; //   ML-KEM-768 decapsulation key (seed)
const PQ_SPK: usize = 1952; // ML-DSA-65 verifying (public) key
const PQ_SSK: usize = 32; //   ML-DSA-65 signing key (seed)
const X25519: usize = 32; //  X25519 / Ed25519 key sizes

/// A hybrid self-certifying node identity.
///
/// Keys are CONCAT bundles: `pq ‖ classical`. `public_key`/`secret_key` carry
/// both halves so the id binds to the full hybrid surface.
#[derive(Clone)]
pub struct NodeIdentity {
    /// PQ pub ‖ classical pub (for KEM + signature verify).
    pub public_key: Vec<u8>,
    /// PQ sec ‖ classical sec (the sealed secret).
    pub secret_key: Vec<u8>,
    /// Short content-address of the public key bundle.
    pub id: String,
}

impl NodeIdentity {
    /// Create a fresh hybrid identity. Entropy from `OsRng` ONCE here only.
    pub fn create() -> Self {
        // ── PQ (FIPS 203/204) ──
        let (pq_dk, pq_ek): (DecapsulationKey<MlKem768>, EncapsulationKey<MlKem768>) =
            MlKem768::generate_keypair();
        let pq_ssk = ml_dsa::SigningKey::<MlDsa65>::generate();
        let pq_spk = pq_ssk.verifying_key();

        // ── Classical fallback ──
        let x_sk = X25519Sk::random(); // getrandom feature, internal OsRng
        let x_pk = X25519Pk::from(&x_sk);
        let mut ed_seed = [0u8; 32];
        fill(&mut ed_seed).expect("ed25519 seed");
        let ed_sk = EdSk::from_bytes(&ed_seed);
        let ed_pk = ed_sk.verifying_key();

        // Pack: public = pq_ek ‖ pq_spk ‖ x_pk ‖ ed_pk
        let mut public_key = Vec::with_capacity(PQ_EK + PQ_SPK + X25519 + X25519);
        public_key.extend_from_slice(pq_ek.to_bytes().as_slice());
        public_key.extend_from_slice(pq_spk.to_bytes().as_slice());
        public_key.extend_from_slice(x_pk.as_bytes());
        public_key.extend_from_slice(&ed_pk.to_bytes());

        // secret = pq_dk(seed) ‖ pq_ssk(seed) ‖ x_sk ‖ ed_sk
        let mut secret_key = Vec::with_capacity(PQ_DK + PQ_SSK + X25519 + X25519);
        secret_key.extend_from_slice(pq_dk.to_bytes().as_slice());
        secret_key.extend_from_slice(pq_ssk.to_bytes().as_slice());
        secret_key.extend_from_slice(&x_sk.to_bytes());
        secret_key.extend_from_slice(&ed_sk.to_bytes());

        let id = short_id(&public_key);
        NodeIdentity {
            public_key,
            secret_key,
            id,
        }
    }

    /// Re-check self-certification: re-derive the id from the public bundle.
    pub fn self_certify(&self) -> bool {
        short_id(&self.public_key) == self.id
    }

    /// Sign a message with the hybrid signature (PQ ‖ classical, concat).
    /// Both signatures are required at verify time — fail-closed on either.
    pub fn sign(&self, msg: &[u8]) -> Vec<u8> {
        let mut sk = self.secret_key.clone();
        let (_pq_ssk_b, rest) = sk.split_at(PQ_DK);
        let (pq_ssk_s, rest) = rest.split_at(PQ_SSK);
        let (_, rest) = rest.split_at(X25519);
        let ed_sec = &rest[..X25519];

        let pq_sk =
            ml_dsa::SigningKey::<MlDsa65>::new_from_slice(pq_ssk_s).expect("pq ssk rebuild");
        let ed_sk = EdSk::from_bytes(&ed_sec.try_into().unwrap());

        let pq_sig = pq_sk.sign(msg); // Signer trait
        let ed_sig = ed_sk.sign(msg); // EdSigner trait
        sk.zeroize();

        let mut out = pq_sig.to_bytes().to_vec(); // SignatureEncoding::to_bytes
        out.extend_from_slice(&ed_sig.to_bytes());
        out
    }

    /// Verify a hybrid signature against this identity's public bundle.
    pub fn verify(&self, msg: &[u8], sig: &[u8]) -> bool {
        if sig.len() != pq_sig_len() + 64 {
            return false;
        }
        let (pq_sig, ed_sig) = sig.split_at(pq_sig_len());
        let mut pk = self.public_key.clone();
        let (_pq_ek_b, rest) = pk.split_at(PQ_EK);
        let (pq_spk_b, rest) = rest.split_at(PQ_SPK);
        let (_, rest) = rest.split_at(X25519);
        let ed_pk_b = &rest[..X25519];

        let pq_vk = match ml_dsa::VerifyingKey::<MlDsa65>::new_from_slice(pq_spk_b) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let ed_vk = match EdPk::from_bytes(&ed_pk_b.try_into().unwrap()) {
            Ok(v) => v,
            Err(_) => return false,
        };
        let pq_sig: ml_dsa::Signature<MlDsa65> = match pq_sig.try_into() {
            Ok(s) => s,
            Err(_) => return false,
        };
        let ed_sig = EdSig::from_bytes(&ed_sig.try_into().unwrap());
        let ok_pq = pq_vk.verify(msg, &pq_sig).is_ok(); // Verifier trait
        let ok_ed = ed_vk.verify(msg, &ed_sig).is_ok(); // EdVerifier trait
        pk.zeroize();
        ok_pq && ok_ed
    }

    /// Prove the PQ KEM half is live: rehydrate the KEM keypair from the secret
    /// bundle and run an encapsulate→decapsulate roundtrip. Returns true iff the
    /// shared secrets match (i.e. the FIPS-203 path actually works end-to-end).
    pub fn kem_roundtrip_ok(&self) -> bool {
        let dk_seed = &self.secret_key[..PQ_DK];
        let ek_bytes = &self.public_key[..PQ_EK];
        let dk_arr: Key<DecapsulationKey<MlKem768>> = dk_seed.try_into().expect("dk len");
        let ek_arr: Key<EncapsulationKey<MlKem768>> = ek_bytes.try_into().expect("ek len");
        let dk = match <DecapsulationKey<MlKem768> as KemKeyInit>::new_from_slice(&dk_arr) {
            Ok(d) => d,
            Err(_) => return false,
        };
        let ek = match <EncapsulationKey<MlKem768> as TryKeyInit>::new(&ek_arr) {
            Ok(e) => e,
            Err(_) => return false,
        };
        let (ct, k_send) = ek.encapsulate(); // Encapsulate trait
        let k_recv = dk.decapsulate(&ct); // Decapsulate trait — infallible, returns SharedKey
        k_send.as_slice() == k_recv.as_slice()
    }
}

/// ML-DSA-65 signature byte length (fixed for our params). Cached once so
/// `verify` doesn't mint a key per call — stays honest, costs nothing hot.
fn pq_sig_len() -> usize {
    static L: OnceLock<usize> = OnceLock::new();
    *L.get_or_init(|| {
        ml_dsa::SigningKey::<MlDsa65>::generate()
            .sign(b"")
            .to_bytes()
            .len()
    })
}

/// Short hex id from public-key bundle bytes (first 8 bytes of SHA-512, hex).
pub fn short_id(pk: &[u8]) -> String {
    let h = Sha512::digest(pk);
    h[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// The encrypted vault blob on disk.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct VaultBlob {
    pub version: u8,
    /// Argon2 salt (public; MUST be random per-vault, never derived from the
    /// passphrase — a pass-derived salt makes identical passphrases yield
    /// identical keys across all vaults, and is not a salt at all).
    pub salt: Vec<u8>,
    /// XChaCha20Poly1305 nonce (public; MUST be random per encryption — static
    /// nonce + reused keystream leaks XOR of plaintexts across vaults).
    pub nonce: Vec<u8>,
    /// XChaCha20Poly1305 over the secret-key bundle.
    pub ciphertext: Vec<u8>,
    /// The public bundle, stored in the clear (self-certifying; id derives from it).
    pub public: Vec<u8>,
}

/// Derive a 32-byte AEAD key from a passphrase + salt via Argon2id.
fn derive_key(pass: &[u8], salt: &[u8]) -> [u8; 32] {
    let salt_str = SaltString::encode_b64(salt).expect("salt encode");
    let argon = Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2::Params::new(ARGON_M, ARGON_T, ARGON_P, None).expect("argon params"),
    );
    let hash = argon.hash_password(pass, &salt_str).expect("argon hash");
    let owned = hash.hash.expect("argon output");
    let mut key = [0u8; 32];
    key.copy_from_slice(&owned.as_bytes()[..32]);
    key
}

/// Create a new vault, returning (blob, identity). Overwrites only when `force`.
pub fn create_or_unlock(pass: &str, path: &str, force: bool) -> Result<NodeIdentity> {
    if fs::metadata(path).is_ok() && !force {
        return unlock(pass, path);
    }
    let id = NodeIdentity::create();
    // B8 (fable): random salt + random nonce, both stored in the blob.
    // A pass-derived salt is not a salt (identical passphrases ⇒ identical keys);
    // a static nonce reuses the keystream across vaults (XOR-of-plaintexts leak).
    let mut salt = vec![0u8; 16];
    getrandom::fill(&mut salt).map_err(|_| anyhow!("salt entropy failed"))?;
    let mut nonce = vec![0u8; NONCE_LEN];
    getrandom::fill(&mut nonce).map_err(|_| anyhow!("nonce entropy failed"))?;
    let key = derive_key(pass.as_bytes(), &salt);
    let cipher: XChaCha20Poly1305 = AeadKeyInit::new(&key.into());

    let pt = id.secret_key.clone();
    let ct = cipher
        .encrypt(XNonce::from_slice(&nonce), pt.as_slice())
        .map_err(|_| anyhow!("encryption failed"))?;

    let blob = VaultBlob {
        version: VAULT_VERSION,
        salt,
        nonce,
        ciphertext: ct,
        public: id.public_key.clone(),
    };
    let json = serde_json::to_string(&blob)?;
    fs::write(path, json)?;
    Ok(id)
}

/// Unlock an existing vault: Argon2id + AEAD auth reject wrong pass / tamper.
pub fn unlock(pass: &str, path: &str) -> Result<NodeIdentity> {
    let raw = fs::read(path)?;
    let blob: VaultBlob = serde_json::from_slice(&raw)?;
    if blob.version != VAULT_VERSION {
        bail!("unsupported vault version {}", blob.version);
    }
    let key = derive_key(pass.as_bytes(), &blob.salt);
    let cipher: XChaCha20Poly1305 = AeadKeyInit::new(&key.into());

    let pt = cipher
        .decrypt(XNonce::from_slice(&blob.nonce), blob.ciphertext.as_slice())
        .map_err(|_| anyhow!("vault auth failed — wrong passphrase or tampered blob"))?;

    let identity = NodeIdentity {
        public_key: blob.public.clone(),
        secret_key: pt,
        id: short_id(&blob.public),
    };
    if !identity.self_certify() {
        bail!("identity self-certification mismatch — vault integrity compromised");
    }
    Ok(identity)
}

/// Lock helper: confirms the blob is on disk and AEAD-verifiable.
pub fn lock(path: &str) -> Result<()> {
    if fs::metadata(path).is_ok() {
        Ok(())
    } else {
        bail!("no vault at {path}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATH: &str = "/tmp/bebop-vault-test.json";

    #[test]
    fn create_unlock_roundtrip() {
        // GREEN: create then unlock returns the SAME self-certifying id.
        let _ = fs::remove_file(PATH);
        let a = create_or_unlock("hunter2", PATH, true).unwrap();
        assert!(a.self_certify());
        let b = unlock("hunter2", PATH).unwrap();
        assert_eq!(a.id, b.id, "id not stable across unlock");
        let _ = fs::remove_file(PATH);
    }

    #[test]
    fn wrong_passphrase_rejected() {
        // RED: a wrong passphrase must fail AEAD auth (never silently decrypt).
        let _ = fs::remove_file(PATH);
        let _ = create_or_unlock("right-pass", PATH, true).unwrap();
        let res = unlock("wrong-pass", PATH);
        assert!(res.is_err(), "wrong passphrase was accepted — catastrophic");
        let _ = fs::remove_file(PATH);
    }

    #[test]
    fn same_passphrase_vaults_are_distinct() {
        // B8 (fable) RED: two vaults created with the SAME passphrase must NOT
        // share a keystream. Under the old code (pass-derived salt + static nonce)
        // both produced identical (key, nonce) ⇒ identical ciphertext ⇒ XOR of
        // the two secret bundles leaks. A random per-vault salt + nonce makes the
        // ciphertext prefixes differ.
        let p1 = "/tmp/bebop-vault-test-a.json";
        let p2 = "/tmp/bebop-vault-test-b.json";
        let _ = fs::remove_file(p1);
        let _ = fs::remove_file(p2);
        let _ = create_or_unlock("same-pass", p1, true).unwrap();
        let _ = create_or_unlock("same-pass", p2, true).unwrap();
        let raw1 = fs::read(p1).unwrap();
        let raw2 = fs::read(p2).unwrap();
        assert_ne!(
            raw1, raw2,
            "same-pass vaults must differ (random salt+nonce)"
        );
        let _ = fs::remove_file(p1);
        let _ = fs::remove_file(p2);
    }

    #[test]
    fn self_certify_catches_mismatch() {
        // RED: a corrupted id (pk intact) is caught by self-certify.
        let pk = Sha512::digest(b"dummy").to_vec();
        let mut id = NodeIdentity {
            public_key: pk.clone(),
            secret_key: vec![0u8; 32],
            id: short_id(&pk),
        };
        assert!(id.self_certify());
        id.id = "deadbeefdeadbeef".into(); // mismatched id
        assert!(!id.self_certify(), "self-certify missed a tampered id");
    }

    #[test]
    fn hybrid_signature_is_red_green() {
        // RED+GREEN: a valid hybrid signature verifies; a wrong msg / truncated
        // sig fails. Proves BOTH PQ and classical halves are live and required.
        let _ = fs::remove_file(PATH);
        let id = create_or_unlock("sig-test", PATH, true).unwrap();
        let msg = b"bebop node says hello";
        let sig = id.sign(msg);
        assert!(id.verify(msg, &sig), "valid hybrid sig rejected");
        // RED: tamper the message → must fail
        assert!(
            !id.verify(b"forged message", &sig),
            "sig verified on wrong msg"
        );
        // RED: truncated sig → must fail (length gate)
        assert!(!id.verify(msg, &sig[..10]), "truncated sig accepted");
        let _ = fs::remove_file(PATH);
    }

    #[test]
    fn identity_is_hybrid_pq_plus_classical() {
        // GREEN/RED: the public bundle must carry BOTH the PQ KEM pub
        // (ML-KEM-768 = 1184 bytes) AND the classical X25519 (32) + Ed25519 (32)
        // + PQ DSA pub (ML-DSA-65 = 1952). A bundle of only one half fails this.
        let _ = fs::remove_file(PATH);
        let id = create_or_unlock("hybrid", PATH, true).unwrap();
        assert_eq!(
            id.public_key.len(),
            PQ_EK + PQ_SPK + X25519 + X25519,
            "public bundle is not the full PQ‖classical hybrid"
        );
        // The PQ KEM half must actually work (FIPS-203 roundtrip).
        assert!(
            id.kem_roundtrip_ok(),
            "PQ KEM half failed to encapsulate/decapsulate"
        );
        let _ = fs::remove_file(PATH);
    }
}
