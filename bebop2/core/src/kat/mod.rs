//! Known Answer Test vectors for bebop2 crypto.
pub mod vectors;       // parent-embedded short canonical vectors (SHA-512, SHA3-256, HChaCha20, Argon2id, Ed25519)
// vectors_long (ChaCha20 keystream, full AEAD ciphertexts) is created by the crypto
// implementation agent, which fetches the long vectors from the official RFCs.
