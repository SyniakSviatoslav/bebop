//! Long Known Answer Test vectors for bebop2 crypto modules, fetched by the
//! implementation agent from official RFCs (hand-transcribing 64+ hex bytes is
//! error-prone, so these are sourced verbatim from RFC 8439 Appendix A).
//!
//! ChaCha20 64-byte keystream blocks (RFC 8439 Appendix A.1 + §2.3.2).
//! Each entry: (key_hex, nonce_hex, counter:u32, expected_keystream_hex).

pub struct Chacha20Vector {
    pub key: &'static str,
    pub nonce: &'static str,
    pub counter: u32,
    pub keystream: &'static str,
}

pub const CHACHA20: &[Chacha20Vector] = &[
    // RFC 8439 Appendix A.1 Test Vector #1: key=0, nonce=0, counter=0
    Chacha20Vector {
        key: "0000000000000000000000000000000000000000000000000000000000000000",
        nonce: "000000000000000000000000",
        counter: 0,
        keystream: "76b8e0ada0f13d90405d6ae55386bd28bdd219b8a08ded1aa836efcc8b770dc7\
                    da41597c5157488d7724e03fb8d84a376a43b8f41518a11cc387b669b2ee6586",
    },
    // RFC 8439 Appendix A.1 Test Vector #2: key=0, nonce=0, counter=1
    Chacha20Vector {
        key: "0000000000000000000000000000000000000000000000000000000000000000",
        nonce: "000000000000000000000000",
        counter: 1,
        keystream: "9f07e7be5551387a98ba977c732d080d\
                    cb0f29a048e3656912c6533e32ee7aed\
                    29b721769ce64e43d57133b074d839d5\
                    31ed1f28510afb45ace10a1f4b794d6f",
    },
    // RFC 8439 Appendix A.1 Test Vector #3: key=00..00 01 (last byte=1), nonce=0, counter=1
    Chacha20Vector {
        key: "0000000000000000000000000000000000000000000000000000000000000001",
        nonce: "000000000000000000000000",
        counter: 1,
        keystream: "3aeb5224ecf849929b9d828db1ced4dd\
                    832025e8018b8160b82284f3c949aa5a\
                    8eca00bbb4a73bdad192b5c42f73f2fd\
                    4e273644c8b36125a64addeb006c13a0",
    },
    // RFC 8439 §2.3.2 / §2.4.2 canonical: key=00..1f, nonce=09 00 00 00 4a 00 00 00, counter=1
    Chacha20Vector {
        key: "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f",
        nonce: "000000090000004a00000000",
        counter: 1,
        keystream: "10f1e7e4d13b5915500fdd1fa32071c4\
                    c7d1f4c733c068030422aa9ac3d46c4e\
                    d2826446079faa0914c2d705d98b02a2\
                    b5129cd1de164eb9cbd083e8a2503c4e",
    },
];
