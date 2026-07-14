//! WSS transport — WebSocket Secure edge/browser fallback carrier.
//!
//! Used where a raw QUIC (iroh) endpoint is unreachable. Same envelope + frame
//! contract as `iroh_transport`; only the carrier differs. This implementation is
//! REAL and tested: it connects/accepts over WebSocket, carries signed
//! [`bebop_proto_cap::SignedFrame`]s via the [`framing`] length-prefixed envelope
//! as WebSocket binary messages, signs on send and verifies on recv through the
//! hybrid gate.
//!
//! CI GUARD: NO-COURIER-SCORING — same neutrality rule as iroh. The transport
//! moves signed frames; it never grades the mover.

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::Message;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_rustls::{TlsAcceptor, TlsConnector};
use tokio_tungstenite::{accept_async, client_async, WebSocketStream};

use bebop_proto_cap::roster::AnchorRoster;
use bebop_proto_cap::{HybridGate, HybridPolicy, RevocationSet, SignedFrame};

use crate::error::{WireError, WireResult};
use crate::framing;
use crate::Transport;

/// Unified WSS byte stream so `connect` and `accept` produce ONE `WssTransport` type while BOTH ends
/// support real rustls TLS (C5, full migration). Plaintext (`ws://`, loopback tests) and TLS (`wss://`)
/// coexist. All variants are `Unpin` (TcpStream + tokio-rustls TlsStream over an Unpin IO), so the
/// poll methods project via `Pin::new` with no `unsafe`/pin-project.
pub(crate) enum WssStream {
    Plain(TcpStream),
    ClientTls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
    ServerTls(Box<tokio_rustls::server::TlsStream<TcpStream>>),
}

impl AsyncRead for WssStream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            WssStream::Plain(s) => Pin::new(s).poll_read(cx, buf),
            WssStream::ClientTls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
            WssStream::ServerTls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}
impl AsyncWrite for WssStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, b: &[u8]) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            WssStream::Plain(s) => Pin::new(s).poll_write(cx, b),
            WssStream::ClientTls(s) => Pin::new(s.as_mut()).poll_write(cx, b),
            WssStream::ServerTls(s) => Pin::new(s.as_mut()).poll_write(cx, b),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            WssStream::Plain(s) => Pin::new(s).poll_flush(cx),
            WssStream::ClientTls(s) => Pin::new(s.as_mut()).poll_flush(cx),
            WssStream::ServerTls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            WssStream::Plain(s) => Pin::new(s).poll_shutdown(cx),
            WssStream::ClientTls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
            WssStream::ServerTls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

/// Server TLS config with a fresh self-signed cert (DEV/TEST). Production deployments MUST use
/// [`WssEndpoint::ListenTlsWithCert`] to supply a real operator cert/key (see
/// [`server_tls_config_from_der`]); this self-signed path only proves the TLS accept path and lets
/// the `wss://` handshake test run end-to-end.
fn server_tls_config() -> WireResult<rustls::ServerConfig> {
    let ck = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
        .map_err(|e| WireError::Carrier(format!("self-signed cert: {e}")))?;
    let cert = ck.cert.der().clone();
    let key = rustls::pki_types::PrivatePkcs8KeyDer::from(ck.signing_key.serialize_der());
    // ring as the explicit PRIMARY provider (see client_rustls_config) — never the aws-lc default.
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| WireError::Carrier(format!("tls versions: {e}")))?
        .with_no_client_auth()
        .with_single_cert(vec![cert], key.into())
        .map_err(|e| WireError::Carrier(format!("server tls config: {e}")))
}

/// PRODUCTION server TLS config from an OPERATOR-supplied certificate chain + PKCS#8
/// private key (both DER). Fail-closed: an empty chain, or a key that does not match the
/// leaf certificate, returns `Err` — never a silent fallback to a self-signed cert.
/// (`rustls::with_single_cert` cryptographically checks that the key matches the leaf.)
fn server_tls_config_from_der(
    cert_chain_der: &[Vec<u8>],
    key_pkcs8_der: &[u8],
) -> WireResult<rustls::ServerConfig> {
    if cert_chain_der.is_empty() {
        return Err(WireError::Carrier(
            "operator TLS: empty certificate chain".into(),
        ));
    }
    let certs: Vec<rustls::pki_types::CertificateDer<'static>> = cert_chain_der
        .iter()
        .map(|d| rustls::pki_types::CertificateDer::from(d.clone()))
        .collect();
    let key = rustls::pki_types::PrivateKeyDer::from(rustls::pki_types::PrivatePkcs8KeyDer::from(
        key_pkcs8_der.to_vec(),
    ));
    // ring as the explicit PRIMARY provider (never the aws-lc default), same as the dev path.
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| WireError::Carrier(format!("tls versions: {e}")))?
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| WireError::Carrier(format!("operator tls config (key/cert mismatch?): {e}")))
}

/// Monotonic-ish tick for capability expiry. Uses wall-clock seconds since
/// A WebSocket Secure transport endpoint descriptor.
///
/// For a **client** (`connect`), use [`WssEndpoint::Url`] (e.g.
/// `wss://host:port/path` or the plaintext `ws://` for loopback tests).
/// For a **server** (`accept`), use [`WssEndpoint::Listen`] with a
/// `host:port` to bind a TCP listener that upgrades incoming connections.
#[derive(Clone)]
pub enum WssEndpoint {
    /// A WebSocket URL to dial as a client.
    Url(String),
    /// A `host:port` to bind and accept plaintext `ws://` upgrades on (server side).
    Listen(String),
    /// A `host:port` to bind and accept over rustls TLS (`wss://`) with a self-signed dev cert.
    /// DEV/TEST ONLY — the ephemeral self-signed cert is not verifiable by a real client
    /// trust store. Production uses [`WssEndpoint::ListenTlsWithCert`].
    ListenTls(String),
    /// Operator-cert TLS: bind + accept presenting an OPERATOR-supplied certificate chain +
    /// PKCS#8 private key (both DER-encoded). Unlike [`WssEndpoint::ListenTls`] (an ephemeral
    /// self-signed dev cert), this serves the operator's real CA-issued cert.
    ///
    /// CAVEAT (no over-claim): a peer only *verifies* this cert when it runs a HARDENED client
    /// (`--no-default-features`, i.e. `insecure-tls` OFF). The DEFAULT client is accept-any, so
    /// TLS server-authentication holds end-to-end only against a hardened peer; frame integrity
    /// is protected regardless by the app-layer hybrid gate (RequireBoth + channel binding).
    /// NOT yet covered (follow-ups): cert hot-reload/rotation, PEM ingest (DECART), mTLS.
    /// DER (not PEM) so no cert-parsing dependency is pulled into the sovereign core.
    ListenTlsWithCert {
        /// `host:port` to bind.
        addr: String,
        /// Certificate chain, leaf first (DER-encoded X.509).
        cert_chain_der: Vec<Vec<u8>>,
        /// PKCS#8-encoded (DER) private key for the leaf certificate.
        key_pkcs8_der: Vec<u8>,
    },
}

// Manual Debug: NEVER print the operator private key (or raw cert bytes) — a derived
// Debug would leak secret key material into any log/trace of a `WssEndpoint`.
impl std::fmt::Debug for WssEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WssEndpoint::Url(u) => f.debug_tuple("Url").field(u).finish(),
            WssEndpoint::Listen(a) => f.debug_tuple("Listen").field(a).finish(),
            WssEndpoint::ListenTls(a) => f.debug_tuple("ListenTls").field(a).finish(),
            WssEndpoint::ListenTlsWithCert {
                addr,
                cert_chain_der,
                ..
            } => f
                .debug_struct("ListenTlsWithCert")
                .field("addr", addr)
                .field("cert_chain_len", &cert_chain_der.len())
                .field("key_pkcs8_der", &"<redacted>")
                .finish(),
        }
    }
}

/// An active WSS session. Carries a single peer's WebSocket stream plus the
/// decode buffer and the verification gate. No score, no reputation.
pub struct WssTransport {
    ws: WebSocketStream<WssStream>,
    /// Reassembly buffer for the length-prefixed framing.
    buf: Vec<u8>,
    /// Hybrid gate used to verify every received frame (classical live; PQ todo).
    /// Now also enforces the anchor-rooted delegation chain (root-of-trust).
    gate: HybridGate,
    /// Enrolled trust-anchor roster consulted by the gate on every `recv`.
    roster: AnchorRoster,
    /// UCAN-style revocation set (MESH-11) consulted by the gate on every
    /// `recv`. A capability/key in this set is rejected even with valid
    /// signatures. Empty by default; callers wire in gossiped revocations.
    revocations: RevocationSet,
}

impl WssTransport {
    /// Build a transport from an already-upgraded WebSocket stream.
    fn from_stream(
        ws: WebSocketStream<WssStream>,
        gate: HybridGate,
        roster: AnchorRoster,
        revocations: RevocationSet,
    ) -> Self {
        WssTransport {
            ws,
            buf: Vec::new(),
            gate,
            roster,
            revocations,
        }
    }

    /// Set the hybrid gate (defaults to `ClassicalUntilPqAudit`).
    pub fn with_gate(self, gate: HybridGate) -> Self {
        WssTransport { gate, ..self }
    }

    /// The DER of the peer's leaf certificate negotiated during the TLS handshake, if
    /// this is a client-side TLS session. Test-only accessor: lets a test assert which
    /// certificate the SERVER actually presented — so a routing bug that served the
    /// self-signed dev cert instead of the operator cert goes RED end-to-end.
    #[cfg(test)]
    pub(crate) fn peer_leaf_cert_der(&self) -> Option<Vec<u8>> {
        match self.ws.get_ref() {
            WssStream::ClientTls(s) => s
                .get_ref()
                .1
                .peer_certificates()
                .and_then(|c| c.first())
                .map(|c| c.as_ref().to_vec()),
            _ => None,
        }
    }

    /// Set the enrolled trust-anchor roster used to verify delegation chains.
    pub fn with_roster(self, roster: AnchorRoster) -> Self {
        WssTransport { roster, ..self }
    }

    /// Set the UCAN-style revocation set (MESH-11) used to reject revoked
    /// capabilities/keys on every `recv`. A real mesh would gossip revocations
    /// and fold them in here; an empty set accepts everything that is otherwise
    /// valid (the pre-MESH-11 behaviour).
    pub fn with_revocations(self, revocations: RevocationSet) -> Self {
        WssTransport {
            revocations,
            ..self
        }
    }

    /// Graceful close: send a WebSocket Close frame so the peer sees a clean
    /// shutdown. `Drop` is NOT async, so without an explicit `close()` the
    /// underlying TCP is aborted on drop (no fd leak, but abrupt). Call this
    /// before dropping a long-lived session.
    /// ponytail: graceful WS close needs an async hook; Drop can't await, so we
    /// expose `close()` and leave Drop = abrupt-but-safe (tungstenite closes
    /// the TCP, no leak). Upgrade: impl an async `Drop` wrapper if needed.
    pub async fn close(&mut self) -> WireResult<()> {
        self.ws
            .close(None)
            .await
            .map_err(|e| WireError::Carrier(e.to_string()))
    }
}

impl Transport for WssTransport {
    type Endpoint = WssEndpoint;

    async fn connect(endpoint: &Self::Endpoint) -> WireResult<Self> {
        let url = match endpoint {
            WssEndpoint::Url(u) => u.clone(),
            _ => {
                return Err(WireError::HandshakeRejected(
                    "use accept() for a Listen/ListenTls endpoint".into(),
                ))
            }
        };
        // Parse scheme/host/port for the transport connection.
        let uri: http::Uri = url
            .parse()
            .map_err(|e| WireError::HandshakeRejected(format!("bad url: {e}")))?;
        let host = uri
            .host()
            .ok_or_else(|| WireError::HandshakeRejected("url has no host".into()))?
            .to_string();
        let secure = uri.scheme_str() == Some("wss");
        let port = uri.port_u16().unwrap_or(if secure { 443 } else { 80 });
        let tcp = TcpStream::connect((host.as_str(), port))
            .await
            .map_err(|e| WireError::HandshakeRejected(e.to_string()))?;
        // C5 (full rustls migration): `wss://` does a real CLIENT TLS handshake via
        // `client_rustls_config` (webpki-roots verification when hardened, accept-any under
        // `insecure-tls`). `ws://` stays plaintext (loopback tests). The server side is symmetric
        // (see `accept` + `ListenTls`), so a real `wss://` connection now completes end-to-end.
        let stream = if secure {
            let connector = TlsConnector::from(Arc::new(crate::iroh_transport::client_rustls_config()));
            let dns = rustls::pki_types::ServerName::try_from(host.clone())
                .map_err(|e| WireError::HandshakeRejected(format!("bad server name: {e}")))?;
            WssStream::ClientTls(Box::new(
                connector
                    .connect(dns, tcp)
                    .await
                    .map_err(|e| WireError::HandshakeRejected(format!("client tls: {e}")))?,
            ))
        } else {
            WssStream::Plain(tcp)
        };
        let (ws, _resp) = client_async(&url, stream)
            .await
            .map_err(|e| WireError::HandshakeRejected(e.to_string()))?;
        Ok(WssTransport::from_stream(
            ws,
            // PQ-IN-FORCE: the hybrid gate requires BOTH the classical (Ed25519)
            // and post-quantum (ML-DSA-65) signatures on the live wire. A
            // classical-only frame is rejected (PqVerifyFailed) — this is what
            // closes red-team H5 ("post-quantum not in force"). The transitional
            // `ClassicalUntilPqAudit` policy is NOT used on the live carrier.
            HybridGate::new(HybridPolicy::RequireBoth),
            AnchorRoster::new(),
            // MESH-11: empty revocation set on the live carrier by default; a
            // real mesh folds gossiped revocations in via `with_revocations`.
            RevocationSet::new(),
        ))
    }

    // innovate: H6 (red-team) — the WSS carrier runs over `MaybeTlsStream::Plain`
    // (no TLS); confidentiality/integrity in transit rely on the PQ+classical
    // signature envelope, NOT transport encryption. This is a DELIBERATE local-
    // first dev default, NOT a production posture. Upgrade trigger: when a real
    // deployment needs wire confidentiality, wrap the TcpStream in `TlsStream`
    // (native rustls) before `accept_async`/`connect_async`, OR route the
    // transport over a QUIC/Noise channel. Until then the wire is authenticated
    // but readable by a passive on-path observer.

    async fn accept(endpoint: &Self::Endpoint) -> WireResult<Self> {
        // Resolve the bind address + the server TLS config (built BEFORE binding so a bad
        // operator cert fails fast). `None` = plaintext `ws://`.
        let (addr, tls_cfg): (String, Option<rustls::ServerConfig>) = match endpoint {
            WssEndpoint::Listen(a) => (a.clone(), None),
            WssEndpoint::ListenTls(a) => (a.clone(), Some(server_tls_config()?)),
            WssEndpoint::ListenTlsWithCert {
                addr,
                cert_chain_der,
                key_pkcs8_der,
            } => (
                addr.clone(),
                Some(server_tls_config_from_der(cert_chain_der, key_pkcs8_der)?),
            ),
            WssEndpoint::Url(_) => {
                return Err(WireError::HandshakeRejected(
                    "use connect() for a Url endpoint".into(),
                ))
            }
        };
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| WireError::HandshakeRejected(e.to_string()))?;
        let (tcp, _peer) = listener
            .accept()
            .await
            .map_err(|e| WireError::Carrier(e.to_string()))?;
        // C5 (full rustls migration): `ListenTls`/`ListenTlsWithCert` complete a real SERVER TLS
        // handshake (self-signed dev cert / operator cert respectively) so `wss://` works
        // end-to-end; `Listen` stays plaintext (loopback tests).
        let stream = match tls_cfg {
            Some(cfg) => {
                let acceptor = TlsAcceptor::from(Arc::new(cfg));
                WssStream::ServerTls(Box::new(
                    acceptor
                        .accept(tcp)
                        .await
                        .map_err(|e| WireError::HandshakeRejected(format!("server tls: {e}")))?,
                ))
            }
            None => WssStream::Plain(tcp),
        };
        let ws = accept_async(stream)
            .await
            .map_err(|e| WireError::HandshakeRejected(e.to_string()))?;
        Ok(WssTransport::from_stream(
            ws,
            // PQ-IN-FORCE: see `connect()` above. RequireBoth on accept too.
            HybridGate::new(HybridPolicy::RequireBoth),
            AnchorRoster::new(),
            // MESH-11: empty revocation set on the live carrier by default; a
            // real mesh folds gossiped revocations in via `with_revocations`.
            RevocationSet::new(),
        ))
    }

    async fn send(&mut self, frame: SignedFrame) -> WireResult<()> {
        // Frame the signed frame: serialize the SignedFrame, wrap in an Envelope,
        // then length-prefix it for the carrier.
        let inner = serde_json::to_vec(&frame)?;
        let envelope = crate::envelope::Envelope::new([0u8; 16], inner);
        let bytes = framing::encode(&envelope)?;
        self.ws
            .send(Message::Binary(bytes))
            .await
            .map_err(|e| WireError::Carrier(e.to_string()))?;
        Ok(())
    }

    async fn recv(&mut self) -> WireResult<SignedFrame> {
        loop {
            // Try to decode a complete envelope from the buffer first.
            if let Some(env) = framing::decode(&mut self.buf)? {
                let frame: SignedFrame = serde_json::from_slice(&env.payload)?;
                // Verify the capability through the hybrid gate: anchor-rooted
                // delegation chain (root-of-trust) + real classical sig + replay
                // + expiry. `now` is the REAL wall-clock tick (not hardcoded 0),
                // so capability expiry is actually enforced. The chain is taken
                // from the frame's own `delegation_chain` field. A self-signed
                // frame (no anchor-rooted chain) is rejected by the gate before
                // the frame is returned — closing red-team §3A on the live path.
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                self.gate.check(
                    &frame,
                    &self.roster,
                    &frame.delegation_chain,
                    &self.revocations,
                    now,
                )?;
                return Ok(frame);
            }
            // Need more bytes: read a WS message.
            let msg = self
                .ws
                .next()
                .await
                .ok_or(WireError::Carrier("peer closed connection".into()))?
                .map_err(|e| WireError::Carrier(e.to_string()))?;
            match msg {
                Message::Binary(data) => self.buf.extend_from_slice(&data),
                // A clean close handshake is EOF, not a carrier fault.
                Message::Close(_) => return Err(WireError::Closed),
                // Ignore ping/pong/text; we only carry binary envelopes.
                _ => continue,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bebop_proto_cap::roster::{AnchorRoster, Delegation, Effect};
    use bebop_proto_cap::scope::{Action, Resource, Scope};
    use bebop_proto_cap::{Capability, HybridGate, HybridPolicy, SignedFrame};
    use tokio::sync::oneshot;

    /// (seed, pk) for a deterministic Ed25519 key.
    fn key(seed_byte: u8) -> ([u8; 32], [u8; 32]) {
        let seed = [seed_byte; 32];
        let (pk, _) = bebop2_core::sign::keygen(&seed);
        (seed, pk)
    }

    /// Build a frame signed by `leaf`, plus an anchor-rooted delegation chain
    /// (anchor -> leaf) carrying the same scope, and a roster enrolling anchor.
    /// The capability is HYBRID: it carries a real ML-DSA-65 `subject_key_pq` and
    /// the frame is signed under BOTH the classical (Ed25519) and PQ (ML-DSA-65)
    /// legs, so it satisfies the live `RequireBoth` gate (closes red-team H5).
    fn anchored_frame(
        anchor_seed: &[u8; 32],
        anchor_pk: &[u8; 32],
        leaf_seed: &[u8; 32],
        leaf_pk: &[u8; 32],
        resource: Resource,
        action: Action,
        nonce: [u8; 8],
        expiry: u64,
    ) -> (SignedFrame, AnchorRoster, Vec<Delegation>) {
        // PQ half of the hybrid identity, derived from the SAME leaf seed as the
        // classical key. IMPORTANT: pq_pk (public, 1952B) goes into the cap's
        // `subject_key_pq`; pq_sk (secret, 4032B) signs — never swap them.
        let (pq_pk, pq_sk) = bebop2_core::pq_dsa::keygen(leaf_seed);
        let cap = Capability::new_hybrid(
            *leaf_pk,
            pq_pk.bytes.clone(),
            resource,
            action,
            nonce,
            expiry,
        );
        let mut f = SignedFrame::new(cap, b"wire-payload".to_vec());
        f.sign_classical(leaf_seed).unwrap();
        // Real PQ signature over the frame's binding-signing domain.
        f.sign_pq(&pq_sk.bytes.clone().try_into().unwrap(), &[0u8; 32])
            .unwrap();
        let link = Delegation::sign(
            *anchor_pk,
            *leaf_pk,
            Scope::new(resource, action),
            Effect::new(resource, action),
            expiry,
            nonce,
            anchor_seed,
        )
        .unwrap();
        let mut roster = AnchorRoster::new();
        roster.enroll(anchor_pk);
        (f, roster, vec![link])
    }

    /// Drive a server task that accepts one connection on `addr`, then runs
    /// `body` with the connected transport (carrying `roster`). Signals readiness
    /// via `tx` *before* blocking in accept, so the client can dial without racing.
    async fn run_server<F, Fut>(
        addr: String,
        roster: AnchorRoster,
        tx: oneshot::Sender<()>,
        body: F,
    ) where
        F: FnOnce(WssTransport) -> Fut,
        Fut: core::future::Future<Output = ()>,
    {
        let _ = tx.send(());
        let ep = WssEndpoint::Listen(addr);
        let t = WssTransport::accept(&ep).await.unwrap().with_roster(roster);
        body(t).await;
    }

    /// Same as `run_server` but accepts over real rustls TLS (self-signed cert) — the `wss://` path.
    async fn run_server_tls<F, Fut>(
        addr: String,
        roster: AnchorRoster,
        tx: oneshot::Sender<()>,
        body: F,
    ) where
        F: FnOnce(WssTransport) -> Fut,
        Fut: core::future::Future<Output = ()>,
    {
        let _ = tx.send(());
        let ep = WssEndpoint::ListenTls(addr);
        let t = WssTransport::accept(&ep).await.unwrap().with_roster(roster);
        body(t).await;
    }

    #[tokio::test]
    async fn hardened_verifier_rejects_self_signed_cert() {
        // C5 SECURITY PROOF: the REAL webpki-roots verifier (NOT accept-any) REJECTS an untrusted
        // (self-signed) server cert. Builds a hardened client config EXPLICITLY here (independent of the
        // `insecure-tls` feature) so this negative check runs under the default test build — closing the
        // "verifier compile-checked only" gap the 3-model review flagged.
        use tokio_rustls::TlsConnector;
        let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let (tx, rx) = oneshot::channel();
        let saddr = addr.to_string();
        let server = tokio::spawn(async move {
            let _ = tx.send(());
            // Server presents a self-signed cert; the hardened client below must reject it.
            let _ = WssTransport::accept(&WssEndpoint::ListenTls(saddr)).await;
        });
        rx.await.unwrap();
        let mut roots = rustls::RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let cfg = rustls::ClientConfig::builder_with_provider(std::sync::Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
        let tcp = TcpStream::connect(addr).await.unwrap();
        let dns = rustls::pki_types::ServerName::try_from("localhost").unwrap();
        let res = TlsConnector::from(std::sync::Arc::new(cfg)).connect(dns, tcp).await;
        assert!(
            res.is_err(),
            "hardened webpki-roots verifier MUST reject the self-signed server cert"
        );
        let _ = server.await;
    }

    // ── Prod operator-cert ListenTls variant ───────────────────────────────────────
    // The operator-cert config builder must be fail-closed: a valid cert+matching key
    // builds a config, but a mismatched key or an empty chain is REJECTED (never a
    // silent fallback to a self-signed cert). RED against a builder that skips validation.
    #[test]
    fn operator_tls_config_is_fail_closed() {
        let op = rcgen::generate_simple_self_signed(vec!["operator.example".to_string()]).unwrap();
        let op_cert = op.cert.der().to_vec();
        let op_key = op.signing_key.serialize_der();

        assert!(
            super::server_tls_config_from_der(&[op_cert.clone()], &op_key).is_ok(),
            "valid operator cert + matching key must build a server config"
        );
        // A key from a DIFFERENT keypair does not match the cert → must be rejected.
        let other = rcgen::generate_simple_self_signed(vec!["other.example".to_string()]).unwrap();
        let wrong_key = other.signing_key.serialize_der();
        assert!(
            super::server_tls_config_from_der(&[op_cert.clone()], &wrong_key).is_err(),
            "a key that does not match the certificate MUST be rejected (fail-closed)"
        );
        assert!(
            super::server_tls_config_from_der(&[], &op_key).is_err(),
            "an empty certificate chain MUST be rejected"
        );
    }

    // Proof that the `server_tls_config_from_der` BUILDER presents the operator cert over
    // real TLS. This builds the acceptor directly (it does NOT exercise accept()'s routing —
    // `wss_listen_tls_with_operator_cert_roundtrip` covers that end-to-end). An accept-any
    // client completes the handshake and reads back the exact certificate presented; RED if
    // the builder emitted a different cert.
    #[cfg(feature = "insecure-tls")]
    #[tokio::test]
    async fn operator_tls_presents_the_operator_cert() {
        let op = rcgen::generate_simple_self_signed(vec!["operator.example".to_string()]).unwrap();
        let op_cert = op.cert.der().to_vec();
        let op_key = op.signing_key.serialize_der();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let cfg = super::server_tls_config_from_der(&[op_cert.clone()], &op_key)
            .expect("operator config builds");
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let acceptor = TlsAcceptor::from(std::sync::Arc::new(cfg));
            let _ = acceptor.accept(tcp).await; // drive the server handshake to completion
        });

        let tcp = TcpStream::connect(addr).await.unwrap();
        let client_cfg = crate::iroh_transport::client_rustls_config(); // accept-any (insecure-tls)
        let dns = rustls::pki_types::ServerName::try_from("operator.example").unwrap();
        let tls = TlsConnector::from(std::sync::Arc::new(client_cfg))
            .connect(dns, tcp)
            .await
            .expect("accept-any client completes the TLS handshake");
        let (_io, conn) = tls.get_ref();
        let presented = conn
            .peer_certificates()
            .expect("server presented a certificate");
        assert_eq!(
            presented[0].as_ref(),
            op_cert.as_slice(),
            "server MUST present the operator-supplied cert, not a self-signed dev cert"
        );
        let _ = server.await;
    }

    // Full `accept()` path: `ListenTlsWithCert(operator cert)` must serve a working wss://
    // endpoint that a client connects to and round-trips a signed frame through — proving
    // accept() ROUTES the operator variant to a real TLS server, not just that the config
    // builder works in isolation. (Accept-any client under the default `insecure-tls`.)
    #[cfg(feature = "insecure-tls")]
    #[tokio::test]
    async fn wss_listen_tls_with_operator_cert_roundtrip() {
        let op = rcgen::generate_simple_self_signed(vec!["operator.example".to_string()]).unwrap();
        let op_cert = op.cert.der().to_vec();
        let op_key = op.signing_key.serialize_der();

        let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let (a_seed, a_pk) = key(2);
        let (l_seed, l_pk) = key(3);
        let (frame, roster, chain) = anchored_frame(
            &a_seed, &a_pk, &l_seed, &l_pk, Resource::Route, Action::Send, [7u8; 8], 9_999_999_999,
        );

        let (tx, rx) = oneshot::channel();
        let saddr = addr.to_string();
        let server_roster = roster.clone();
        let op_cert_srv = op_cert.clone(); // server moves its copy; keep `op_cert` for the assert
        let server = tokio::spawn(async move {
            let _ = tx.send(());
            let ep = WssEndpoint::ListenTlsWithCert {
                addr: saddr,
                cert_chain_der: vec![op_cert_srv],
                key_pkcs8_der: op_key,
            };
            let mut t = WssTransport::accept(&ep)
                .await
                .unwrap()
                .with_roster(server_roster);
            let f = t.recv().await.unwrap();
            t.send(f).await.unwrap();
            let _ = t.close().await;
        });
        rx.await.unwrap();

        let client_ep = WssEndpoint::Url(format!("wss://{addr}"));
        let mut client = WssTransport::connect(&client_ep)
            .await
            .expect("wss:// handshake against the operator cert must complete")
            .with_roster(roster.clone());
        // GUARDRAIL: assert the cert the server presented THROUGH accept()'s routing is the
        // OPERATOR cert. RED if accept()'s ListenTlsWithCert arm served server_tls_config()
        // (the self-signed fallback bug) — the accept-any client alone would NOT catch that.
        assert_eq!(
            client.peer_leaf_cert_der().as_deref(),
            Some(op_cert.as_slice()),
            "accept(ListenTlsWithCert) MUST present the operator cert, not the self-signed dev cert"
        );
        let mut signed = frame;
        signed.delegation_chain = chain;
        client.send(signed).await.unwrap();
        let echoed = client.recv().await.unwrap();
        assert_eq!(echoed.payload, b"wire-payload");
        assert!(echoed.verify_classical().is_ok());
        server.await.unwrap();
    }

    #[tokio::test]
    async fn wss_tls_handshake_roundtrip() {
        // C5 PROOF (not compile-only): a REAL wss:// handshake end-to-end. The server does a rustls
        // TLS accept (self-signed cert via server_tls_config); the client does a rustls TLS connect.
        // Under the default `insecure-tls` the client accepts the self-signed cert, so this exercises
        // the actual TLS handshake + the signed-frame round-trip — proving the full migration works.
        let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let (a_seed, a_pk) = key(2);
        let (l_seed, l_pk) = key(3);
        let (frame, roster, chain) = anchored_frame(
            &a_seed, &a_pk, &l_seed, &l_pk, Resource::Route, Action::Send, [7u8; 8], 9_999_999_999,
        );

        let (tx, rx) = oneshot::channel();
        let server_addr = addr.to_string();
        let server_roster = roster.clone();
        let server = tokio::spawn(async move {
            run_server_tls(server_addr, server_roster, tx, |mut t| async move {
                let frame = t.recv().await.unwrap();
                t.send(frame).await.unwrap();
                let _ = t.close().await;
            })
            .await;
        });

        rx.await.unwrap();

        let client_ep = WssEndpoint::Url(format!("wss://{addr}"));
        let mut client = WssTransport::connect(&client_ep)
            .await
            .expect("wss:// TLS handshake must complete")
            .with_roster(roster.clone());
        let mut signed = frame;
        signed.delegation_chain = chain;
        client.send(signed).await.unwrap();

        let echoed = client.recv().await.unwrap();
        assert_eq!(echoed.payload, b"wire-payload");
        assert!(echoed.verify_classical().is_ok());

        server.await.unwrap();
    }

    /// Two in-memory WSS endpoints over a loopback `ws://` connection that sign +
    /// verify a frame end to end. The client carries a real anchor chain.
    #[tokio::test]
    async fn wss_roundtrip_signs_and_verifies() {
        let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let (a_seed, a_pk) = key(2);
        let (l_seed, l_pk) = key(3);
        let (frame, roster, chain) = anchored_frame(
            &a_seed,
            &a_pk,
            &l_seed,
            &l_pk,
            Resource::Route,
            Action::Send,
            [7u8; 8],
            9_999_999_999,
        );

        let (tx, rx) = oneshot::channel();
        let server_addr = addr.to_string();
        let server_roster = roster.clone();
        let server = tokio::spawn(async move {
            run_server(server_addr, server_roster, tx, |mut t| async move {
                let frame = t.recv().await.unwrap();
                t.send(frame).await.unwrap();
                let _ = t.close().await;
            })
            .await;
        });

        rx.await.unwrap();

        let client_ep = WssEndpoint::Url(format!("ws://{addr}"));
        let mut client = WssTransport::connect(&client_ep)
            .await
            .unwrap()
            .with_roster(roster.clone());
        let mut signed = frame;
        signed.delegation_chain = chain;
        client.send(signed).await.unwrap();

        let echoed = client.recv().await.unwrap();
        assert_eq!(echoed.payload, b"wire-payload");
        assert!(echoed.verify_classical().is_ok());

        server.await.unwrap();
    }

    #[tokio::test]
    async fn wss_rejects_unsigned_frame() {
        let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let (a_seed, a_pk) = key(2);
        let (l_seed, l_pk) = key(3);
        let (_f, roster, _c) = anchored_frame(
            &a_seed,
            &a_pk,
            &l_seed,
            &l_pk,
            Resource::Route,
            Action::Send,
            [7u8; 8],
            9_999_999_999,
        );

        let (tx, rx) = oneshot::channel();
        let server_addr = addr.to_string();
        let server = tokio::spawn(async move {
            run_server(server_addr, roster, tx, |mut t| async move {
                let res = t.recv().await;
                assert!(res.is_err(), "unsigned frame must be rejected");
            })
            .await;
        });

        rx.await.unwrap();

        let client_ep = WssEndpoint::Url(format!("ws://{addr}"));
        let mut client = WssTransport::connect(&client_ep).await.unwrap();

        // Send a frame with NO classical signature -> server recv must reject.
        let seed = [9u8; 32];
        let (pk, _sk) = bebop2_core::sign::keygen(&seed);
        let cap = Capability::new(pk, Resource::Ledger, Action::Append, [3u8; 8], 100);
        let unsigned = SignedFrame::new(cap, b"unsigned".to_vec());
        client.send(unsigned).await.unwrap();

        server.await.unwrap();
    }

    /// RED over the REAL wss carrier: sign a frame, then TAMPER with the
    /// payload AFTER signing, send it over the socket, and assert the server's
    /// `recv` (which runs the hybrid gate) REJECTS it.
    #[tokio::test]
    async fn wss_rejects_tampered_frame() {
        let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let (a_seed, a_pk) = key(2);
        let (l_seed, l_pk) = key(3);
        let (frame, roster, chain) = anchored_frame(
            &a_seed,
            &a_pk,
            &l_seed,
            &l_pk,
            Resource::Ledger,
            Action::Append,
            [2u8; 8],
            9_999_999_999,
        );

        let (tx, rx) = oneshot::channel();
        let server_addr = addr.to_string();
        let server = tokio::spawn(async move {
            run_server(server_addr, roster, tx, |mut t| async move {
                let res = t.recv().await;
                assert!(res.is_err(), "tampered frame must be rejected over wss");
            })
            .await;
        });

        rx.await.unwrap();

        let client_ep = WssEndpoint::Url(format!("ws://{addr}"));
        let mut client = WssTransport::connect(&client_ep).await.unwrap();

        let mut frame = frame;
        frame.delegation_chain = chain;
        frame.sign_classical(&l_seed).unwrap();
        // Tamper AFTER signing — signature now invalid.
        frame.payload = b"tampered-by-mitm".to_vec();

        client.send(frame).await.unwrap();

        server.await.unwrap();
    }

    /// RED→GREEN over the REAL wss carrier: the weaponized self-issue takeover.
    /// An attacker mints its OWN key, signs a capability naming itself as
    /// subject, sends it with NO anchor-rooted delegation chain (or a chain it
    /// forged). The server's `recv` (hybrid gate + roster) MUST reject it as
    /// `UnknownIssuer`. This is the live-path proof that red-team §3A is closed.
    #[tokio::test]
    async fn wss_rejects_self_signed_frame_over_real_carrier() {
        let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        // Server enrolls a REAL anchor the attacker does not control.
        let (_a_seed, a_pk) = key(2);
        let mut roster = AnchorRoster::new();
        roster.enroll(&a_pk);

        let (tx, rx) = oneshot::channel();
        let server_addr = addr.to_string();
        let server = tokio::spawn(async move {
            run_server(server_addr, roster, tx, |mut t| async move {
                let res = t.recv().await;
                assert!(res.is_err(), "self-signed frame MUST be rejected over wss");
            })
            .await;
        });

        rx.await.unwrap();

        let client_ep = WssEndpoint::Url(format!("ws://{addr}"));
        let mut client = WssTransport::connect(&client_ep).await.unwrap();

        // Attacker: own key, self-attested capability, NO anchor chain.
        let (atk_seed, atk_pk) = key(99);
        let cap = Capability::new(
            atk_pk,
            Resource::Ledger,
            Action::Append,
            [1u8; 8],
            9_999_999_999,
        );
        let mut frame = SignedFrame::new(cap, b"takeover".to_vec());
        frame.sign_classical(&atk_seed).unwrap(); // real sig over self-attested auth
                                                  // delegation_chain is empty -> verify_chain -> UnknownIssuer.

        client.send(frame).await.unwrap();

        server.await.unwrap();
    }

    #[test]
    fn gate_policy_is_neutral_no_scoring() {
        // The gate only ever checks signatures/nonce/scope — there is no score path.
        let gate = HybridGate::new(HybridPolicy::ClassicalUntilPqAudit);
        assert!(!format!("{gate:?}").contains("score"));
    }

    /// Channel binding (F7) happy path: handshake -> hash -> bind -> sign -> verify.
    /// The frame is signed via `sign_frame_bound` (the carrier send path) using a
    /// handshake-transcript hash, then verified on the SAME channel.
    #[tokio::test]
    async fn wss_channel_bound_frame_verifies_on_same_channel() {
        let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let (a_seed, a_pk) = key(2);
        let (l_seed, l_pk) = key(3);
        let (frame, roster, chain) = anchored_frame(
            &a_seed,
            &a_pk,
            &l_seed,
            &l_pk,
            Resource::Route,
            Action::Send,
            [7u8; 8],
            9_999_999_999,
        );

        let (tx, rx) = oneshot::channel();
        let server_addr = addr.to_string();
        let server_roster = roster.clone();
        let server = tokio::spawn(async move {
            run_server(server_addr, server_roster, tx, |mut t| async move {
                let frame = t.recv().await.unwrap();
                assert!(
                    frame.verify_classical().is_ok(),
                    "bound frame must verify on same channel"
                );
                assert!(
                    frame.channel_binding.is_some(),
                    "frame must carry a channel binding"
                );
                t.send(frame).await.unwrap();
                let _ = t.close().await;
            })
            .await;
        });

        rx.await.unwrap();

        let client_ep = WssEndpoint::Url(format!("ws://{addr}"));
        let mut client = WssTransport::connect(&client_ep)
            .await
            .unwrap()
            .with_roster(roster.clone());

        // Simulate a completed handshake; the transcript hash binds the channel.
        // The frame subject MUST be the chain's leaf (leaf_pk) so the tail binds.
        // Hybrid capability (RequireBoth in force on the wire): derive the PQ public
        // key from the SAME domain-separated PQ seed sign_frame_bound uses (C6 — NOT the
        // raw leaf seed) so its PQ signature verifies against this cap's subject_key_pq.
        let transcript = b"channel-A-handshake-transcript";
        let pq_seed = bebop2_core::pq_dsa::derive_pq_seed(&l_seed);
        let (bound_pq_pk, _bound_pq_sk) = bebop2_core::pq_dsa::keygen(&pq_seed);
        let cap = Capability::new_hybrid(
            l_pk,
            bound_pq_pk.bytes.clone(),
            Resource::Route,
            Action::Send,
            [7u8; 8],
            9_999_999_999,
        );
        let mut frame = SignedFrame::new(cap, b"bound-wire-payload".to_vec());
        crate::sign_frame_bound(&mut frame, &l_seed, transcript).unwrap();
        frame.delegation_chain = chain;

        client.send(frame).await.unwrap();
        let echoed = client.recv().await.unwrap();
        assert!(echoed.verify_classical().is_ok());
        server.await.unwrap();
    }

    /// RED→GREEN over the REAL wss carrier: a frame bound to channel A's handshake
    /// transcript is captured and replayed on channel B' (different transcript
    /// hash). The server's `recv` (hybrid gate) MUST reject it.
    #[tokio::test]
    async fn wss_rejects_cross_channel_replay() {
        let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let (a_seed, a_pk) = key(2);
        let (l_seed, l_pk) = key(3);
        let (_f, roster, chain) = anchored_frame(
            &a_seed,
            &a_pk,
            &l_seed,
            &l_pk,
            Resource::Ledger,
            Action::Append,
            [2u8; 8],
            9_999_999_999,
        );

        let (tx, rx) = oneshot::channel();
        let server_addr = addr.to_string();
        let server = tokio::spawn(async move {
            run_server(server_addr, roster, tx, |mut t| async move {
                let res = t.recv().await;
                assert!(
                    res.is_err(),
                    "cross-channel replay must be rejected over wss"
                );
            })
            .await;
        });

        rx.await.unwrap();

        let client_ep = WssEndpoint::Url(format!("ws://{addr}"));
        let mut client = WssTransport::connect(&client_ep).await.unwrap();

        // Channel A transcript + its binding.
        let transcript_a = b"channel-A-handshake-transcript";
        let binding_a = crate::handshake::channel_binding_hash(transcript_a);
        let seed = [55u8; 32];
        let (pk, _sk) = bebop2_core::sign::keygen(&seed);
        let cap = Capability::new(
            pk,
            Resource::Ledger,
            Action::Append,
            [2u8; 8],
            9_999_999_999,
        );
        let mut frame = SignedFrame::new(cap, b"replay-target".to_vec()).with_binding(binding_a);
        frame.sign_classical(&seed).unwrap();
        frame.delegation_chain = chain;

        // Attacker swaps the binding field to channel B''s binding but keeps the
        // old signature (which covers binding_a), then sends over channel B'.
        let transcript_b = b"channel-B-prime-handshake-transcript";
        let binding_b = crate::handshake::channel_binding_hash(transcript_b);
        let mut replayed = frame;
        replayed.channel_binding = Some(binding_b);
        assert_ne!(binding_a, binding_b);

        client.send(replayed).await.unwrap();
        server.await.unwrap();
    }

    /// RED→GREEN over the REAL wss carrier: a classical-only frame (no PQ leg)
    /// MUST be rejected now that the live transport enforces `RequireBoth`
    /// (closes red-team H5: "post-quantum not in force"). If this passes, a
    /// revert to `ClassicalUntilPqAudit` would make it fail — catching the
    /// regression at the wire boundary, not just in the unit gate.
    #[tokio::test]
    async fn wss_rejects_classical_only_frame_on_require_both_wire() {
        let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let (a_seed, a_pk) = key(2);
        let (l_seed, l_pk) = key(3);
        let (_f, roster, chain) = anchored_frame(
            &a_seed,
            &a_pk,
            &l_seed,
            &l_pk,
            Resource::Route,
            Action::Send,
            [9u8; 8],
            9_999_999_999,
        );

        let (tx, rx) = oneshot::channel();
        let server_addr = addr.to_string();
        let server_roster = roster.clone();
        let server = tokio::spawn(async move {
            run_server(server_addr, server_roster, tx, |mut t| async move {
                let res = t.recv().await;
                assert!(
                    res.is_err(),
                    "classical-only frame MUST be rejected when RequireBoth is in force"
                );
            })
            .await;
        });

        rx.await.unwrap();

        let client_ep = WssEndpoint::Url(format!("ws://{addr}"));
        let mut client = WssTransport::connect(&client_ep)
            .await
            .unwrap()
            .with_roster(roster.clone());

        // Build a CLASSICAL-ONLY capability (no subject_key_pq) and sign it
        // classically only — exactly the pre-fix acceptance path.
        let cap = Capability::new(l_pk, Resource::Route, Action::Send, [9u8; 8], 9_999_999_999);
        let mut frame = SignedFrame::new(cap, b"classical-only-payload".to_vec());
        frame.sign_classical(&l_seed).unwrap();
        frame.delegation_chain = chain;

        client.send(frame).await.unwrap();
        server.await.unwrap();
    }
}
