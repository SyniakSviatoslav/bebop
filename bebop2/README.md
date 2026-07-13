# bebop2 — from-scratch, zero-dependency, post-quantum

> Greenfield rebuild of bebop. NOT a refactor of `crates/bebop` — a parallel implementation
> that is functionally equivalent and at the end simply REPLACES the old one (old = oracle for
> equivalence tests, then swapped).
>
> Operator mandate (2026-07-10): full from-scratch, **no vendors, zero-dep**, post-quantum era
> software. Target envelope = AGC-class (2.048 MHz quartz, 2K core RAM, 36K core-rope ROM,
> radiation+weight+power envelope). "Almost everything must be machine code" — i.e. deterministically
> verifiable wasm with an EMPTY import section (no clock/RNG/socket reachable), executed bit-exact.
>
> Side-channel gate (accepted): KAT correctness + determinism (empty import, no secret-dependent
> branch — clippy disallowed + constant-time asserts). Full physical side-channel resistance is
> NOT provable without hardware; documented as a known gap, not claimed.

## Architecture principle — vectors/waves are NOT arrays (physicality + first-principles)

Everywhere the old code used `Vec<f64>` / tensor ops, ask first: **is this a fundamental
physical object or an abstraction?** Replace with the irreducible primitive:

- **Vector** = projection of state onto a basis. Prefer spectral/basis representations over
  dense buffers where the basis is known (eigenmodes of the Laplacian, VSA hyperplanes).
- **Wave / tensor** = eigenmode of a linear operator. Use `field_*` (graph Laplacian spectral
  propagator, already in rust-core) instead of dense tensor contraction.
- **Motion physics** = integral of trajectory (AGC computed inertial integrals, not "tensors").
  Prefer `kalman`/`lyapunov` over ad-hoc vector math.
- **Vector memory** = VSA (hyperplane bundling / permutations), not dense matrices.

This is the "physics as truth" abstraction layer: the math is the physics, not a data structure.

## Layout
```
bebop2/
  core/            # pure core+alloc, NO deps -> wasm (empty import). The "machine code".
    lib.rs         # exported primitives (field_*, vsa, cosine, cross, sinc, fexp[fixed C8],
                   #   kalman, lyapunov, chebyshev, fft, active-inference)
    pq_kem.rs      # ML-KEM-768 from scratch (FIPS 203)
    pq_dsa.rs      # ML-DSA-65 from scratch (FIPS 204)
    aead.rs        # XChaCha20-Poly1305 from scratch (RFC 8439)
    kdf.rs         # Argon2id from scratch
    hash.rs        # SHA-512 + SHA3 from scratch
    sign.rs        # Ed25519 from scratch (hybrid classical fallback)
    rng.rs         # CSPRNG from hardware entropy (in-tree, no getrandom dep)
    kat/           # FIPS 203/204 + RFC KAT vectors (committed ground truth)
  kernel/          # deterministic decide/fold/replay — no clock/rng/network
  cli/            # native binary, minimal in-tree TTY (no ratatui/crossterm deps)
  reloop/         # core-RE-loop v2: execute wasm bit-exact + envelope gate
```

## Known bugs / error patterns to carry forward (from fable + audit)
- **C8**: `fexp` range-reduction `|r|<=ln2/2` is WRONG for negative args (rust-core:482).
  FIX in core/lib.rs fexp: use correct `r = x - round(x/ln2)*ln2` symmetric reduction.
- **B4**: route used LIFO Vec (fixed in old cost_estimate; replicate the BinaryHeap+heuristic fix).
- **B8**: vault keystream reuse (fixed in old vault; new rng.rs must be per-nonce, never reuse).
- **B11**: field_physics dt hardcoded (fixed; new core uses stable dt=0.02 corridor).
- **C2**: stabilizer gate checked raw value, not saturated (fixed; new gate saturates first).
- **Fable meta-fallacy**: never verify LABELS — verify PROPERTIES (empty import, named-test
  greps, bit-exact execution). reloop/ enforces this.

## Build / verify
- `cargo build -p bebop2-core --target wasm32-unknown-unknown --release` → artifact must have
  EMPTY import section (reloop checks this).
- Every crypto primitive: KAT vectors in core/kat/ must pass bit-exact.
- `cargo test -p bebop2` → all equivalence tests vs the prior `crates/bebop` implementation (used as an oracle for behavioral parity, not a refactor source) pass.

---

## Status — 2026-07-12 (truthful, post-Phase-0)

**Protocol-in-code, not just in-name.** The red-team review (2026-07-12) correctly flagged
that `bebop2` was previously a "protocol-in-name" (signatures over non-canonical `serde_json`,
OpenSSL in `proto-wire`, self-captured KATs, unanchored capabilities). Those are now CLOSED:

| Red-team finding | Fix | Evidence |
|---|---|---|
| §2 `proto-wire` pulls OpenSSL/native-tls (66 crates, C compiler at build) | replaced with **rustls** | `cargo tree -i openssl-sys` → no match |
| §3A self-issued capabilities (auth bypass) | **AnchorRoster** + UCAN-subset delegation, fail-closed | `proto-cap` roster tests |
| §4A signatures over non-canonical JSON | **canonical TLV** codec (`tlv_signing_input`), `DOMAIN_DELEGATION` for delegation wire | `proto-cap` TLV tests |
| "neither PQ primitive has external KAT" | **ML-DSA-65 60/60 NIST ACVP** byte-exact (vendored `core/kat/acvp/`) | `cargo test -p bebop2-core` ACVP suite |
| numeric instability (C8/B- family) | 7 numeric fixes in `core` | `core` 157 tests green |

**Verified:** `cargo test --workspace` → **698 Rust tests pass, 0 fail**.

**Still TODO (honest gaps — not claimed done):**
- **Wire-spec document** — a standalone byte-level spec for the canonical TLV framing +
  WSS transport, with shared interop test vectors (so a second implementation can be built).
- **Versioning/negotiation** — no on-wire version handshake yet.
- **Second implementation / interop** — only one implementation exists; cross-impl vectors pending.
- **Entropy source (WS-1)** — fail-closed CSPRNG hardening still in flight (Wave 1).
- **Side-channel** — KAT correctness + empty-import determinism hold; full physical
  side-channel resistance is NOT provable without hardware (documented gap, not claimed).

See [`../docs/design/BEBOP-CLAIM-AUDIT-2026-07-12.md`](../docs/design/BEBOP-CLAIM-AUDIT-2026-07-12.md)
for the full claim-by-claim audit.
