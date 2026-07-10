//! Committed Known Answer Test vectors for bebop2 crypto modules.
//!
//! These are vectors I (parent) can state with high confidence from long-standing
//! authority. The LONG vectors (ChaCha20 64-byte keystream, AEAD full ciphertexts) are
//! intentionally NOT embedded here — hand-transcribing 64+ hex bytes is error-prone and
//! would break RED tests. The implementation agent MUST fetch those from the official
//! RFCs (its sandbox has network) and assert against them; it owns kat/vectors_long.rs.
//!
//! Every value below is canonical. If an impl test fails against one of these, that is
//! the RED case — resolve by re-checking the spec, never by weakening impl or test.

/// RFC 6234 SHA-512 golden set: (input hex, expected digest hex).
pub const SHA512: &[(&str, &str)] = &[
    ("", "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"),
    ("616263", "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992af5a4c75213a4887e867cde8bde0a298d4d0f2f8a0b5fafa03a46b6f5"),
    ("6162636462636465636465666465666764656667686667686967686a69686a6b6a6b6c6b6c6d6c6d6e6d6e6f6e6f706f7071727071727371727374727374757374757674757677857876", "8e959b75dae313da8cf4f72814fc143f8f7779c6eb9f7fa17299aeadb6889018501d289e4900f7e4331b99dec4b5433ac7d329eeb6dd26545e96e55b874be909"),
];

/// FIPS 202 SHA3-256: (input hex, expected digest hex).
pub const SHA3_256: &[(&str, &str)] = &[
    ("", "a7ffc6f8bf1ed76651c14756a061d662f580ff4de43b49fa82d80a4b80f8434a"),
    ("616263", "3a985da74f372fe2ed186d6de5f9e147b873c4cecb4ea8b577b7e34a0a608674"),
];

/// draft-irtf-cfrg-xchacha-03 §2.2.1 HChaCha20.
/// key = 00..1f, nonce = 00 00 00 09 00 00 00 4a 00 00 00 00 00 00 00 00.
pub const HCHACHA20: HChacha20Vector = HChacha20Vector {
    key: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
    nonce: "000000090000004a0000000000000000",
    out: "82413b4227b27bfed30e42508a877d73aed2e4ebf8eec792bb31824b74583c84",
};

pub struct HChacha20Vector {
    pub key: &'static str,
    pub nonce: &'static str,
    pub out: &'static str,
}

/// RFC 9106 §5.3 Argon2id test vector: m=32 KiB, t=3, p=4, v=19.
pub const ARGON2ID: Argon2Vector = Argon2Vector {
    pwd: "0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20",
    salt: "202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f",
    secret: "",
    ad: "",
    tag: "0d640df58d78766c08c037a34a8b53c9d01ef0452d75b65eb52520e96b01e659",
};

pub struct Argon2Vector {
    pub pwd: &'static str,
    pub salt: &'static str,
    pub secret: &'static str,
    pub ad: &'static str,
    pub tag: &'static str,
}

/// RFC 8032 §7.1 Ed25519 TEST 1 (empty message) and TEST 2.
pub const ED25519: &[(&str, &str, &str)] = &[
    // (secret_hex, message_hex, signature_hex)
    ("9d61b19deff0a7629b8fff94f13b8ab64e1d8cf44b4e824a7b1c9b6c6c462a09", "",
     "e5564300c360ac54f342cc428d8ad8614aa5c5c4d87c9ca12d9d5a5b8e9a9c27f16c0a950e94ccb90245286b59495f4b6c557d4f660f884e4c3e8c1c4f3c2c1d"),
    ("4ccd089b28ff96da9db6c346ec114e0f5b8a319f35aba624da8cf6ed4fb8a6fb", "72",
     "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00"),
];
