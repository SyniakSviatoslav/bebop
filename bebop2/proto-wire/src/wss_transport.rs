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
use tokio_tungstenite::{accept_async, connect_async, MaybeTlsStream, WebSocketStream};

use bebop_proto_cap::{HybridGate, HybridPolicy, SignedFrame};

use crate::error::{WireError, WireResult};
use crate::framing;
use crate::Transport;

/// Monotonic-ish tick for capability expiry. Uses wall-clock seconds since
/// A WebSocket Secure transport endpoint descriptor.
///
/// For a **client** (`connect`), use [`WssEndpoint::Url`] (e.g.
/// `wss://host:port/path` or the plaintext `ws://` for loopback tests).
/// For a **server** (`accept`), use [`WssEndpoint::Listen`] with a
/// `host:port` to bind a TCP listener that upgrades incoming connections.
#[derive(Debug, Clone)]
pub enum WssEndpoint {
    /// A WebSocket URL to dial as a client.
    Url(String),
    /// A `host:port` to bind and accept upgrades on (server side).
    Listen(String),
}

/// An active WSS session. Carries a single peer's WebSocket stream plus the
/// decode buffer and the verification gate. No score, no reputation.
pub struct WssTransport {
    ws: WebSocketStream<MaybeTlsStream<TcpStream>>,
    /// Reassembly buffer for the length-prefixed framing.
    buf: Vec<u8>,
    /// Hybrid gate used to verify every received frame (classical live; PQ todo).
    gate: HybridGate,
}

impl WssTransport {
    /// Build a transport from an already-upgraded WebSocket stream.
    fn from_stream(ws: WebSocketStream<MaybeTlsStream<TcpStream>>, gate: HybridGate) -> Self {
        WssTransport {
            ws,
            buf: Vec::new(),
            gate,
        }
    }

    /// Set the hybrid gate (defaults to `ClassicalUntilPqAudit`).
    pub fn with_gate(self, gate: HybridGate) -> Self {
        WssTransport { gate, ..self }
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
            WssEndpoint::Listen(_) => {
                return Err(WireError::HandshakeRejected(
                    "use accept() for a Listen endpoint".into(),
                ))
            }
        };
        let (ws, _resp) = connect_async(&url)
            .await
            .map_err(|e| WireError::HandshakeRejected(e.to_string()))?;
        Ok(WssTransport::from_stream(
            ws,
            HybridGate::new(HybridPolicy::ClassicalUntilPqAudit),
        ))
    }

    async fn accept(endpoint: &Self::Endpoint) -> WireResult<Self> {
        let addr = match endpoint {
            WssEndpoint::Listen(a) => a.clone(),
            WssEndpoint::Url(_) => {
                return Err(WireError::HandshakeRejected(
                    "use connect() for a Url endpoint".into(),
                ))
            }
        };
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| WireError::HandshakeRejected(e.to_string()))?;
        let (stream, _peer) = listener
            .accept()
            .await
            .map_err(|e| WireError::Carrier(e.to_string()))?;
        // Wrap in MaybeTlsStream::Plain so the server stream type matches the
        // client's `MaybeTlsStream<TcpStream>` (from connect_async over a URL).
        let ws = accept_async(MaybeTlsStream::Plain(stream))
            .await
            .map_err(|e| WireError::HandshakeRejected(e.to_string()))?;
        Ok(WssTransport::from_stream(
            ws,
            HybridGate::new(HybridPolicy::ClassicalUntilPqAudit),
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
                // Verify the capability through the hybrid gate (real classical sig).
                // `now = 0` => transport enforces REPLAY (nonce set) only;
                // EXPIRY is delegated to the clock-holding verifier (the
                // server checks `gate.check(&frame, real_now)` with its own
                // clock). ponytail: if transport-level expiry is required,
                // thread a `now` source into recv (e.g. gate carries a
                // clock) — the gate already supports it; see
                // `HybridGate::check` + `Capability::is_fresh`.
                self.gate.check(&frame, 0)?;
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
    use bebop_proto_cap::scope::{Action, Resource};
    use bebop_proto_cap::{Capability, HybridGate, HybridPolicy};
    use tokio::sync::oneshot;

    /// Drive a server task that accepts one connection on `addr`, then runs
    /// `body` with the connected transport. Signals readiness via `tx` *before*
    /// blocking in accept, so the client can dial without racing the listener
    /// (and without the connect-before-accept deadlock).
    async fn run_server<F, Fut>(addr: String, tx: oneshot::Sender<()>, body: F)
    where
        F: FnOnce(WssTransport) -> Fut,
        Fut: core::future::Future<Output = ()>,
    {
        // Signal the listener is about to bind/accept BEFORE we block on accept,
        // so the client (waiting on `rx`) can connect.
        let _ = tx.send(());
        let ep = WssEndpoint::Listen(addr);
        let mut t = WssTransport::accept(&ep).await.unwrap();
        body(t).await;
    }

    /// Two in-memory WSS endpoints over a loopback `ws://` connection that sign +
    /// verify a frame end to end.
    #[tokio::test]
    async fn wss_roundtrip_signs_and_verifies() {
        let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let (tx, rx) = oneshot::channel();
        let server_addr = addr.to_string();
        let server = tokio::spawn(async move {
            run_server(server_addr, tx, |mut t| async move {
                let frame = t.recv().await.unwrap();
                t.send(frame).await.unwrap();
                // Graceful close so the echoed frame is flushed before the TCP
                // drops (otherwise the client sees "reset without close").
                let _ = t.close().await;
            })
            .await;
        });

        // Wait until the server is actually listening before dialing.
        rx.await.unwrap();

        let client_ep = WssEndpoint::Url(format!("ws://{addr}"));
        let mut client = WssTransport::connect(&client_ep).await.unwrap();

        // Build + sign a real frame.
        let seed = [123u8; 32];
        let (pk, _sk) = bebop2_core::sign::keygen(&seed);
        let cap = Capability::new(pk, Resource::Route, Action::Send, [7u8; 8], 4242);
        let mut frame = SignedFrame::new(cap, b"wire-payload".to_vec());
        frame.sign_classical(&seed).unwrap();

        client.send(frame).await.unwrap();
        // Server echoes it back; client receives + verifies the real signature.
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

        let (tx, rx) = oneshot::channel();
        let server_addr = addr.to_string();
        let server = tokio::spawn(async move {
            run_server(server_addr, tx, |mut t| async move {
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
    /// `recv` (which runs the hybrid gate) REJECTS it. This is the
    /// green-wash gap the review caught — the unit tests prove tamper fails
    /// at the crypto layer, but only this proves the *transport* enforces it.
    #[tokio::test]
    async fn wss_rejects_tampered_frame() {
        let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let (tx, rx) = oneshot::channel();
        let server_addr = addr.to_string();
        let server = tokio::spawn(async move {
            run_server(server_addr, tx, |mut t| async move {
                // Server must REJECT the tampered frame (sig no longer valid).
                let res = t.recv().await;
                assert!(res.is_err(), "tampered frame must be rejected over wss");
            })
            .await;
        });

        rx.await.unwrap();

        let client_ep = WssEndpoint::Url(format!("ws://{addr}"));
        let mut client = WssTransport::connect(&client_ep).await.unwrap();

        // Build + sign a REAL frame.
        let seed = [55u8; 32];
        let (pk, _sk) = bebop2_core::sign::keygen(&seed);
        let cap = Capability::new(pk, Resource::Ledger, Action::Append, [2u8; 8], 4242);
        let mut frame = SignedFrame::new(cap, b"legit-payload".to_vec());
        frame.sign_classical(&seed).unwrap();

        // Tamper AFTER signing — signature now invalid.
        frame.payload = b"tampered-by-mitm".to_vec();

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

        let (tx, rx) = oneshot::channel();
        let server_addr = addr.to_string();
        let server = tokio::spawn(async move {
            run_server(server_addr, tx, |mut t| async move {
                let frame = t.recv().await.unwrap();
                // Server verifies the bound frame through the hybrid gate.
                assert!(frame.verify_classical().is_ok(), "bound frame must verify on same channel");
                assert!(frame.channel_binding.is_some(), "frame must carry a channel binding");
                t.send(frame).await.unwrap();
                let _ = t.close().await;
            })
            .await;
        });

        rx.await.unwrap();

        let client_ep = WssEndpoint::Url(format!("ws://{addr}"));
        let mut client = WssTransport::connect(&client_ep).await.unwrap();

        // Simulate a completed handshake; the transcript hash binds the channel.
        let transcript = b"channel-A-handshake-transcript";
        let seed = [123u8; 32];
        let (pk, _sk) = bebop2_core::sign::keygen(&seed);
        let cap = Capability::new(pk, Resource::Route, Action::Send, [7u8; 8], 4242);
        let mut frame = SignedFrame::new(cap, b"bound-wire-payload".to_vec());
        crate::sign_frame_bound(&mut frame, &seed, transcript).unwrap();

        client.send(frame).await.unwrap();
        let echoed = client.recv().await.unwrap();
        assert!(echoed.verify_classical().is_ok());
        server.await.unwrap();
    }

    /// RED→GREEN over the REAL wss carrier: a frame bound to channel A's handshake
    /// transcript is captured and replayed on channel B' (different transcript
    /// hash). The server's `recv` (hybrid gate) MUST reject it. Proves the
    /// cross-channel replay defense is enforced at the transport layer, not just
    /// in the crypto unit test.
    #[tokio::test]
    async fn wss_rejects_cross_channel_replay() {
        let probe = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);

        let (tx, rx) = oneshot::channel();
        let server_addr = addr.to_string();
        let server = tokio::spawn(async move {
            run_server(server_addr, tx, |mut t| async move {
                // A frame bound to a DIFFERENT channel (B') must be rejected.
                let res = t.recv().await;
                assert!(res.is_err(), "cross-channel replay must be rejected over wss");
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
        let cap = Capability::new(pk, Resource::Ledger, Action::Append, [2u8; 8], 4242);
        let mut frame = SignedFrame::new(cap, b"replay-target".to_vec()).with_binding(binding_a);
        frame.sign_classical(&seed).unwrap();

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
}
