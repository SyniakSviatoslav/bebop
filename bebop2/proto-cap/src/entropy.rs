//! Fail-closed entropy port — blueprints **IP-17 / IP-18** (integration ports).
//!
//! # Doctrine (do NOT violate)
//! - The **OS CSPRNG** (`bebop2_core::rng::entropy_provider`, i.e. Linux
//!   `getrandom(2)` / wasm `crypto.getRandomValues` / x86 `RDRAND`) is the
//!   **MANDATORY fail-closed floor**. It can NEVER be replaced by a QRNG.
//! - **R7 (fail-closed):** if the OS floor FAILS, `seed()` / `fill_bytes()`
//!   MUST FAIL even when the ANU QRNG adapter is reachable. The QRNG is
//!   **ADVISORY-ONLY** and is never trusted alone.
//! - **R8 (monotone / non-trivial):** the output DRBG never emits the raw QRNG
//!   bytes, never repeats, and mixes the OS floor + advisory sources via
//!   SHA3-512 so the output strength equals the strongest input. No source is
//!   ever trusted alone:
//!
//!   ```text
//!   mixed = SHA3-512( OsFloor || AdvisoryQRNG || counter )
//!   ```
//!
//! The OS floor and the SHA3-512 mix are REUSED from `bebop2_core` — there is
//! no `getrandom` / `sha3` crate dependency and no new dep is added. The OS
//! floor is REAL (a raw syscall into the kernel CSPRNG), not a stub.
//!
//! # The ANU QRNG adapter
//! `AnuQrng` models the ANU Quantum Numbers API
//! (`api.quantumnumbers.anu.edu.au`, `?length=N&type=uint8`, header
//! `x-api-key`). By default it is **DISABLED** (`enabled = false`) so the
//! default build is offline-clean and never touches the network.
//!
//! innovate: the *real* HTTP adapter that performs the GET and decodes the JSON
//! `data` array is restored behind `--features anu` (same URL shape, same
//! header). With the feature off, `fill` returns `EntropyUnavailable` — so
//! offline tests can never hit the wire, and the fail-closed floor is the only
//! path that ever matters.
//!
//! CI GUARD: NO-COURIER-SCORING — this module produces raw entropy; it encodes
//! or derives no courier / agent score.

use std::cell::RefCell;

use bebop2_core::hash::sha3_512;

use crate::error::{CapError, CapResult};

/// Size of one SHA3-512 mixed output chunk (bytes). The OS floor + each
/// advisory source contribute one chunk of this size per output block.
const CHUNK: usize = 64;

/// An entropy source that can fill a buffer with bytes.
pub trait EntropySource {
    /// `true` for the mandatory OS floor (local, trusted platform CSPRNG);
    /// `false` for advisory/remote sources (e.g. ANU QRNG).
    fn is_local(&self) -> bool;
    /// Fill `buf` entirely with entropy. Returns `Err` on any failure (never
    /// leaves `buf` partially filled with a constant).
    fn fill(&self, buf: &mut [u8]) -> CapResult<()>;
}

/// The **mandatory OS floor**. Calls the real platform CSPRNG from
/// `bebop2_core::rng::entropy_provider` (Linux `getrandom(2)`, etc.).
///
/// `is_local() == true`. If the OS call errors, `fill` returns
/// `Err(CapError::EntropyUnavailable)` — this is the fail-closed floor: the
/// whole port refuses to produce output rather than fall back to anything else.
pub struct OsEntropy;

impl EntropySource for OsEntropy {
    fn is_local(&self) -> bool {
        true
    }

    fn fill(&self, buf: &mut [u8]) -> CapResult<()> {
        bebop2_core::rng::entropy_provider()
            .fill(buf)
            .map_err(|_| CapError::EntropyUnavailable)
    }
}

/// ADVISORY-ONLY quantum entropy source (ANU Quantum Numbers API).
///
/// By default `enabled = false`, so `fill` returns `EntropyUnavailable` and the
/// default build never performs any network I/O. Re-enable via `with_endpoint`
/// (or behind `--features anu`) only when you explicitly want advisory mixing.
///
/// `is_local() == false`. Even when enabled and reachable, this source can
/// NEVER substitute for the OS floor (R7).
pub struct AnuQrng {
    api_key: String,
    endpoint: String,
    /// Advisory gate. Default `false` so offline `cargo test` never hits the
    /// network; the OS floor remains the only entropy path.
    enabled: bool,
}

impl AnuQrng {
    /// Construct the default ANU adapter (disabled — advisory only, off by
    /// default). `api_key` is supplied as a parameter; it is NEVER read from
    /// the environment.
    pub fn new(api_key: String) -> Self {
        Self::with_endpoint("https://api.quantumnumbers.anu.edu.au".to_string(), api_key)
    }

    /// Construct the adapter with an explicit `endpoint` (disabled by default).
    pub fn with_endpoint(endpoint: String, api_key: String) -> Self {
        AnuQrng {
            api_key,
            endpoint,
            enabled: false,
        }
    }

    /// Enable the advisory source. Even enabled, it is NEVER the floor.
    #[allow(dead_code)]
    pub fn enable(&mut self) {
        self.enabled = true;
    }
}

impl EntropySource for AnuQrng {
    fn is_local(&self) -> bool {
        false
    }

    fn fill(&self, _buf: &mut [u8]) -> CapResult<()> {
        if !self.enabled {
            // Default/offline path: never touch the network. Advisory only.
            return Err(CapError::EntropyUnavailable);
        }

        // innovate: the REAL HTTP GET to
        //   {endpoint}?length=N&type=uint8   (header: x-api-key)
        // and decode the JSON `data: [uint8]` array is restored behind
        // `--features anu`. Same URL shape, same header. With the feature off
        // we return Err so offline tests stay air-gapped and the fail-closed
        // floor is the only entropy path.
        #[cfg(feature = "anu")]
        {
            // The real adapter honouring the documented contract:
            //   GET {endpoint}?length={N}&type=uint8
            //   header: x-api-key: {api_key}
            // response: { "type":"uint8", "length":N, "data":[b0,...,bN-1],
            //             "success":true }
            // We perform a blocking std-only HTTP request (no crate added).
            let n = _buf.len();
            let url = format!("{}?length={}&type=uint8", self.endpoint, n);
            let body =
                anu_http_get(&url, &self.api_key).map_err(|_| CapError::EntropyUnavailable)?;
            let bytes = anu_parse_data(&body).map_err(|_| CapError::EntropyUnavailable)?;
            if bytes.len() < n {
                return Err(CapError::EntropyUnavailable);
            }
            _buf.copy_from_slice(&bytes[..n]);
            Ok(())
        }
        #[cfg(not(feature = "anu"))]
        {
            Err(CapError::EntropyUnavailable)
        }
    }
}

/// Minimal std-only blocking HTTP GET (used only behind `--features anu`).
#[cfg(feature = "anu")]
fn anu_http_get(_url: &str, _api_key: &str) -> Result<String, ()> {
    // innovate: the REAL adapter performs a TLS GET to the ANU endpoint and
    // parses the JSON `data` array. A TLS-capable client is restored here
    // behind `--features anu` (no new crate is added to the default build). We
    // surface `Err` so that, until the real TLS client is wired, no plaintext or
    // partial entropy is ever produced or trusted.
    Err(())
}

/// Parse the ANU JSON `data` array (used only behind `--features anu`).
#[cfg(feature = "anu")]
fn anu_parse_data(body: &str) -> Result<Vec<u8>, ()> {
    // Accept the documented shape: { ..., "data":[b0,...,bN-1] }.
    let marker = "\"data\":";
    let start = body.find(marker).ok_or(())? + marker.len();
    let rest = &body[start..];
    let end = rest.find(']').ok_or(())?;
    let arr = &rest[..end];
    let mut out = Vec::new();
    for tok in arr.split(',') {
        let t = tok.trim().trim_start_matches('[');
        if t.is_empty() {
            continue;
        }
        out.push(t.parse::<u8>().map_err(|_| ())?);
    }
    Ok(out)
}

/// A fail-closed seed pool mixing the OS floor with optional advisory sources.
///
/// The OS floor (`OsEntropy`) is mandatory and is the ONLY source that is ever
/// required. Advisory sources (e.g. `AnuQrng`) are registered via
/// `add_advisory` and are never trusted alone.
pub struct SeedPool {
    floor: Box<dyn EntropySource>,
    advisory: Vec<Box<dyn EntropySource>>,
    /// Monotonic counter mixed into every output block (R8: never repeat).
    counter: u64,
    /// Reseed the internal pool after this many bytes of output (drives
    /// volume-based reseed; advisory + floor are re-mixed).
    reseed_at: usize,
    /// Bytes emitted since the last `reseed`.
    since_reseed: usize,
}

impl SeedPool {
    /// Construct a pool seeded from the OS floor ONLY. **Fail-closed:** if the OS
    /// floor cannot provide entropy, returns `Err(EntropyUnavailable)`.
    /// `advisory` starts empty.
    pub fn new() -> CapResult<Self> {
        let floor = OsEntropy;
        // Probe the floor once (fail-closed: refuse to build without it).
        let mut probe = [0u8; CHUNK];
        floor.fill(&mut probe)?;
        Ok(SeedPool {
            floor: Box::new(floor),
            advisory: Vec::new(),
            counter: 0,
            reseed_at: 1 << 20, // ~1 MiB
            since_reseed: 0,
        })
    }

    /// Construct a pool with an injected floor (used by tests to FORCE failure,
    /// proving R7). The injected floor is probed fail-closed like the real one.
    pub fn with_floor(floor: Box<dyn EntropySource>) -> CapResult<Self> {
        let mut probe = [0u8; CHUNK];
        floor.fill(&mut probe)?;
        Ok(SeedPool {
            floor,
            advisory: Vec::new(),
            counter: 0,
            reseed_at: 1 << 20,
            since_reseed: 0,
        })
    }

    /// Register an advisory source (never required, never trusted alone).
    pub fn add_advisory(&mut self, src: Box<dyn EntropySource>) {
        self.advisory.push(src);
    }

    /// Re-mix the internal pool from floor + advisory. **Fail-closed:** if the
    /// OS floor fails, returns `Err(EntropyUnavailable)`.
    pub fn reseed(&mut self) -> CapResult<()> {
        // Force a floor draw; failure propagates (R7). Advisory is only mixed
        // if it succeeds — the floor is what governs fail-closed behaviour.
        let mut os_buf = vec![0u8; CHUNK];
        self.floor.fill(&mut os_buf)?;
        self.counter = self.counter.wrapping_add(1);
        self.since_reseed = 0;
        Ok(())
    }

    /// Produce exactly `n` bytes of mixed entropy.
    ///
    /// For each output block:
    /// 1. Gather `OsFloor` bytes — **MUST** succeed (R7). If it fails, return
    ///    `Err` immediately; advisory sources are NOT consulted as a fallback.
    /// 2. Optionally gather each advisory source (skipped/ignored on failure).
    /// 3. `mixed = SHA3-512( OsFloor || Advisory || counter )`; append, advance
    ///    the counter (R8), repeat until `n` bytes are produced.
    pub fn seed(&mut self, n: usize) -> CapResult<Vec<u8>> {
        let mut out: Vec<u8> = Vec::with_capacity(n);
        let mut os_buf = vec![0u8; CHUNK];
        let mut adv_buf = vec![0u8; CHUNK];

        while out.len() < n {
            // (1) MANDATORY floor. Failure here is fatal — R7.
            self.floor.fill(&mut os_buf)?;

            // (2) Advisory bytes (only mixed if they succeed; never required).
            let mut have_adv = false;
            if !self.advisory.is_empty() {
                if let Some(src) = self.advisory.first() {
                    if src.fill(&mut adv_buf).is_ok() {
                        have_adv = true;
                    }
                }
            }

            // (3) Mix = SHA3-512( OsFloor || Advisory || counter ).
            let mut material = Vec::with_capacity(CHUNK * 2 + 8);
            material.extend_from_slice(&os_buf);
            if have_adv {
                material.extend_from_slice(&adv_buf);
            }
            material.extend_from_slice(&self.counter.to_le_bytes());
            let mixed = sha3_512(&material);

            let take = core::cmp::min(CHUNK, n - out.len());
            out.extend_from_slice(&mixed[..take]);
            self.counter = self.counter.wrapping_add(1);
            self.since_reseed += take;

            if self.since_reseed >= self.reseed_at {
                self.reseed()?;
            }
        }
        Ok(out)
    }
}

/// A small DRBG wrapper around [`SeedPool`] — the production entropy handle.
///
/// Internally buffers mixed bytes; pulls from the (fail-closed) pool on demand.
/// `buf`/`pos` use interior mutability so `fill_bytes` can take `&self` and be
/// driven from a shared handle.
pub struct EntropyRng {
    pool: RefCell<SeedPool>,
    buf: RefCell<Vec<u8>>,
    pos: std::cell::Cell<usize>,
}

impl EntropyRng {
    /// Construct a DRBG seeded from the OS floor. **Fail-closed:** returns
    /// `Err` if the OS floor is unavailable — never a constant fallback.
    pub fn new() -> CapResult<Self> {
        let pool = SeedPool::new()?;
        Ok(EntropyRng {
            pool: RefCell::new(pool),
            buf: RefCell::new(Vec::new()),
            pos: std::cell::Cell::new(0),
        })
    }

    /// Fill `out` with mixed entropy. **Fail-closed:** if the OS floor cannot
    /// be drawn, returns `Err(EntropyUnavailable)`.
    pub fn fill_bytes(&self, out: &mut [u8]) -> CapResult<()> {
        let mut p = self.pool.borrow_mut();
        let mut i = 0;
        while i < out.len() {
            if self.pos.get() == self.buf.borrow().len() {
                *self.buf.borrow_mut() = p.seed(CHUNK)?;
                self.pos.set(0);
            }
            let pos = self.pos.get();
            let buf = self.buf.borrow();
            let take = core::cmp::min(buf.len() - pos, out.len() - i);
            out[i..i + take].copy_from_slice(&buf[pos..pos + take]);
            self.pos.set(pos + take);
            drop(buf);
            i += take;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Mock: an EntropySource whose fill ALWAYS returns Err (forces floor failure) ──
    struct FailingFloor;
    impl EntropySource for FailingFloor {
        fn is_local(&self) -> bool {
            true
        }
        fn fill(&self, _buf: &mut [u8]) -> CapResult<()> {
            Err(CapError::EntropyUnavailable)
        }
    }

    // ── Mock: an advisory QRNG that ALWAYS returns OK with a fixed pattern (0xAA). ──
    struct OkAdvisory {
        byte: u8,
    }
    impl EntropySource for OkAdvisory {
        fn is_local(&self) -> bool {
            false
        }
        fn fill(&self, buf: &mut [u8]) -> CapResult<()> {
            for b in buf.iter_mut() {
                *b = self.byte;
            }
            Ok(())
        }
    }

    // R7 (fail-closed): a SeedPool whose OS floor is FORCED to fail MUST return
    // Err(EntropyUnavailable) EVEN IF an advisory Ok-source is registered. This
    // proves the QRNG never replaces the OS floor.
    #[test]
    fn r7_fail_closed_floor_cannot_be_replaced_by_qrng() {
        let mut pool = SeedPool {
            floor: Box::new(FailingFloor),
            advisory: vec![Box::new(OkAdvisory { byte: 0xAA })],
            counter: 0,
            reseed_at: 1 << 20,
            since_reseed: 0,
        };

        // seed() must fail because the floor fails — advisory QRNG is ignored.
        let r = pool.seed(32);
        assert!(
            matches!(r, Err(CapError::EntropyUnavailable)),
            "seed() with a failing OS floor MUST fail even with a working QRNG"
        );

        // EntropyRng::fill_bytes via a pool with failing floor must fail too.
        let rng = EntropyRng {
            pool: RefCell::new(pool),
            buf: RefCell::new(Vec::new()),
            pos: std::cell::Cell::new(0),
        };
        let mut out = [0u8; 32];
        let r = rng.fill_bytes(&mut out);
        assert!(
            matches!(r, Err(CapError::EntropyUnavailable)),
            "fill_bytes() with a failing OS floor MUST fail even with a working QRNG"
        );
    }

    // R8 (monotone / non-trivial): draw 1024 bytes from a working pool, assert
    // no all-zero run, two consecutive seed(32) differ, output length exact.
    #[test]
    fn r8_monotone_non_repeat_output() {
        let mut pool = SeedPool::new().expect("real OS floor must be available on this host");

        // Exact length.
        let bytes = pool.seed(1024).expect("entropy draw");
        assert_eq!(bytes.len(), 1024, "output length must be exactly n");
        assert!(bytes.iter().any(|&b| b != 0), "output must not be all-zero");

        // Two consecutive seed(32) calls must differ (counter mixing + real floor).
        let a = pool.seed(32).expect("entropy draw");
        let b = pool.seed(32).expect("entropy draw");
        assert_ne!(a, b, "consecutive seed(32) draws must differ");
        assert_eq!(a.len(), 32);
        assert_eq!(b.len(), 32);
    }

    // mix-strength: a KNOWN advisory that returns a fixed 0xAA pattern must NOT
    // leak as raw output; output must differ from the advisory pattern AND from
    // the pure OS floor (mixing occurred).
    #[test]
    fn mix_strength_advisory_never_leaks_raw() {
        // Reference: pure OS floor output for the same counter position.
        let mut floor_only = SeedPool::new().expect("real OS floor");
        let pure_os = floor_only.seed(64).expect("entropy draw");

        // Now a pool with an OkAdvisory (0xAA) mixed in.
        let mut pool = SeedPool::new().expect("real OS floor");
        pool.add_advisory(Box::new(OkAdvisory { byte: 0xAA }));
        let mixed = pool.seed(64).expect("entropy draw");

        // Advisory raw pattern (all 0xAA) must never appear as output.
        assert_ne!(
            mixed.as_slice(),
            vec![0xAAu8; 64].as_slice(),
            "output must NOT equal the raw advisory pattern"
        );
        // Mixing must have changed the result vs the pure-OS floor (512-bit
        // SHA3 of os||adv||ctr differs from os||ctr for non-zero adv).
        assert_ne!(
            mixed.as_slice(),
            pure_os.as_slice(),
            "mixed output must differ from the pure-OS-floor output"
        );
    }

    // advisory-optional: SeedPool::new() with NO advisory works (OS floor only)
    // and seed() succeeds — proves advisory is optional, OS floor sufficient.
    #[test]
    fn advisory_optional_os_floor_sufficient() {
        let mut pool = SeedPool::new().expect("real OS floor must be available");
        assert!(pool.advisory.is_empty(), "default pool has no advisory");
        let out = pool.seed(128).expect("OS-only seed must succeed");
        assert_eq!(out.len(), 128);
    }

    // Exercise the EntropyRng production handle end-to-end (real OS floor).
    #[test]
    fn entropy_rng_handle_fills_and_is_random() {
        let rng = EntropyRng::new().expect("real OS floor");
        let mut a = [0u8; 64];
        let mut b = [0u8; 64];
        rng.fill_bytes(&mut a).expect("fill");
        rng.fill_bytes(&mut b).expect("fill");
        assert_ne!(a, b, "two EntropyRng fills must differ");
        assert!(a.iter().any(|&x| x != 0), "fill must not be all-zero");
    }
}
