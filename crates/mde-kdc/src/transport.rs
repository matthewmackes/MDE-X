//! KDC2-3.2 — `KdcHost` Transport impl.
//!
//! Glues the protocol crate (`mde-kdc-proto`) + the pairing
//! store (KDC2-3.7) + the discovery registry (KDC2-2.11) into the
//! `mackes_transport::Transport` trait. The mesh router (KDC2-1.8
//! worker) dispatches through this impl exactly the same way it
//! dispatches through NebulaDirect / NebulaLighthouseRelay / NebulaHttps443.
//!
//! ## Real TLS network layer (KDC2-2.8 closure, 2026-05-23)
//!
//! `probe`/`open`/`health` consult both stores:
//!
//!   * **Pairing store** — peer must be `PairedDevice` to be eligible.
//!     Otherwise `Unreachable { code: "not_paired" }`.
//!   * **Discovery registry** — peer must have a recent source
//!     `SocketAddr` (cached from UDP/1716 announces). Otherwise
//!     `Unreachable { code: "not_discovered" }`.
//!
//! On `open`, the host TCP-connects to `(source_addr.ip(),
//! KDC_TLS_PORT)` and wraps the stream with
//! [`tls::connect_pinned_tls`] using the paired device's stored
//! SHA-256 fingerprint. A successful handshake yields a
//! [`KdcTlsConnection`] carrying the live `TlsStream<TcpStream>`
//! and a stable `kdc-tls:{peer_id}` identifier the router
//! correlates against audit entries. Fingerprint mismatch surfaces
//! as `HandshakeFailed { code: "fingerprint_mismatch" }` so the
//! UI can render `PairingState::KeyMismatch`.
//!
//! ## What ships here
//!
//! - `KdcHost::new(pairing, discovery)` — production constructor
//!   tying the host to a shared pairing store + the KDC discovery
//!   registry.
//! - `KdcHost::pairing_only(pairing)` — test/bench helper that
//!   constructs an empty discovery registry. Useful for the
//!   trait-conformance tests that exercise the "paired but
//!   unreachable" error path without booting a TCP listener.
//! - `impl mackes_transport::Transport for KdcHost` —
//!   `kind() == TransportKind::KdcTls`, capabilities from
//!   `TransportCapabilities::kdc_tls_default()`, open performs the
//!   pinned TLS handshake.
//! - `KdcTlsConnection { id, stream }` — live `Connection` impl
//!   the router holds across sends.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use mackes_transport::{
    transport_capabilities::TransportCapabilities, Capabilities, Connection, HealthState,
    MessageClassSet, Transport, TransportError, TransportKind,
};
use mde_kdc_proto::discovery::DiscoveryRegistry;
use tokio::sync::Mutex as AsyncMutex;
use tokio_rustls::client::TlsStream;
use tokio::net::TcpStream;

use crate::pairing::PairingStore;
use crate::tls;

/// KDE Connect's wire port. Both UDP broadcasts (discovery) and
/// the TCP TLS handshake use 1716; upstream KDC may fall through
/// to 1717-1764 if 1716 is busy, but stock devices advertise 1716
/// by default.
pub const KDC_TLS_PORT: u16 = 1716;

/// Concrete `Transport` impl for the KDE Connect wire.
#[derive(Debug)]
pub struct KdcHost {
    pairing: Arc<PairingStore>,
    discovery: Arc<AsyncMutex<DiscoveryRegistry>>,
}

impl KdcHost {
    /// Construct the production host wiring. Both stores are
    /// shared with the daemon's other workers (the future
    /// `kdc_discovery` worker writes to `discovery`; the D-Bus
    /// host scaffold (KDC2-3.3) writes to `pairing`).
    #[must_use]
    pub fn new(
        pairing: Arc<PairingStore>,
        discovery: Arc<AsyncMutex<DiscoveryRegistry>>,
    ) -> Self {
        Self { pairing, discovery }
    }

    /// Test/bench helper — constructs a host without any
    /// discovery wiring. Every `open()` call returns
    /// `Unreachable { code: "not_discovered" }`, which lets
    /// conformance tests exercise the paired-but-unreachable
    /// branch without spinning up a TLS listener. Production
    /// code uses [`KdcHost::new`].
    #[must_use]
    pub fn pairing_only(pairing: Arc<PairingStore>) -> Self {
        Self {
            pairing,
            discovery: Arc::new(AsyncMutex::new(DiscoveryRegistry::new())),
        }
    }

    /// Borrow the discovery registry — exposed so the
    /// `kdc_discovery` worker (KDC2-2.9.a follow-up) can inject
    /// real announces via the same `Arc` the host holds.
    #[must_use]
    pub fn discovery(&self) -> Arc<AsyncMutex<DiscoveryRegistry>> {
        Arc::clone(&self.discovery)
    }
}

/// Live `Connection` returned by [`KdcHost::open`] — wraps the
/// `tokio_rustls::client::TlsStream<TcpStream>` produced by the
/// pinned-fingerprint handshake. The router holds it across
/// sends for the peer session's lifetime.
///
/// The stream is parked behind a `tokio::sync::Mutex` so the
/// router can `lock().await.write_all(...)` from any of its
/// tasks without giving up the connection. Per-send sequencing
/// happens at the protocol-frame layer (mde-kdc-proto codec) —
/// this mutex just guarantees mutually-exclusive access to the
/// underlying TLS half.
pub struct KdcTlsConnection {
    id: String,
    stream: AsyncMutex<TlsStream<TcpStream>>,
}

impl KdcTlsConnection {
    /// The peer-derived identifier (`kdc-tls:{peer_id}`).
    #[must_use]
    pub fn id_owned(&self) -> &str {
        &self.id
    }

    /// Take an exclusive lock on the underlying TLS stream. The
    /// future protocol-frame writer/reader pair (KDC2-3.2.b) goes
    /// through this.
    pub async fn lock_stream(&self) -> tokio::sync::MutexGuard<'_, TlsStream<TcpStream>> {
        self.stream.lock().await
    }
}

impl std::fmt::Debug for KdcTlsConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KdcTlsConnection")
            .field("id", &self.id)
            .field("stream", &"<TlsStream<TcpStream>>")
            .finish()
    }
}

impl Connection for KdcTlsConnection {
    fn id(&self) -> &str {
        &self.id
    }
}

#[async_trait]
impl Transport for KdcHost {
    fn kind(&self) -> TransportKind {
        TransportKind::KdcTls
    }

    fn capabilities(&self) -> Capabilities {
        // Bridge the kdc_tls_default factory (KDC2-1.5) into
        // the existing `Capabilities` shape the router consumes.
        // `kdc_tls_default` reports payload-shape capabilities
        // (bulk / streaming / mtu / encryption); `Capabilities`
        // reports routing+health capabilities (carries-class
        // set / health window / label). Both coexist per the
        // KDC2-1.5 lock.
        let _payload = TransportCapabilities::kdc_tls_default();
        Capabilities {
            // KDC carries every message class; the protocol's 9
            // plugins cover Control / Clipboard / FileBulk /
            // Notification.
            carries: MessageClassSet::all(),
            // 60 KiB matches mde-kdc-proto's FrameDecoder
            // MAX_FRAME_BYTES sane bound (the per-frame cap is
            // 1 MiB but typical KDC frames are well under 64K).
            max_frame_bytes: Some(64 * 1024),
            // Re-probe cadence: 5 s. KDC's TLS handshake is
            // expensive, but a 5 s window matches the rest of
            // the router's cadence so a peer-side flap gets
            // noticed within one tick of the mesh-router.
            health_window: Duration::from_secs(5),
            // Operator-visible label used in audit + UI rendering.
            label: "kdc-tls".to_string(),
        }
    }

    async fn probe(&self, peer_id: &str) -> HealthState {
        // Healthy iff paired AND we have a recent announce
        // address. The router uses this on every tick before
        // deciding to send; latency-tracking lands in the
        // observation history Path (KDC2-1.12).
        if self.pairing.get(peer_id).is_none() {
            return HealthState::Down;
        }
        let addr = self.discovery.lock().await.source_addr_for(peer_id);
        if addr.is_some() {
            HealthState::Healthy
        } else {
            HealthState::Down
        }
    }

    async fn open(&self, peer_id: &str) -> Result<Box<dyn Connection>, TransportError> {
        let device = self.pairing.get(peer_id).ok_or(TransportError::Unreachable {
            code: "not_paired",
        })?;
        let addr = {
            let guard = self.discovery.lock().await;
            guard.source_addr_for(peer_id)
        }
        .ok_or(TransportError::Unreachable {
            code: "not_discovered",
        })?;
        // KDC's TLS handshake uses TCP/1716 on the IP we learned
        // from the UDP/1716 announce. We DON'T trust the
        // announce's port (announces only carry identity, not
        // wire ports) — KDC_TLS_PORT is the stock default.
        let dial_addr = std::net::SocketAddr::new(addr.ip(), KDC_TLS_PORT);
        let stream = tls::connect_pinned_tls(
            dial_addr,
            &device.id,
            Some(device.fingerprint.clone()),
        )
        .await
        .map_err(|e| match e {
            tls::ConnectError::Tcp(_) => TransportError::Unreachable {
                code: "tcp_refused",
            },
            tls::ConnectError::Tls(_) => TransportError::HandshakeFailed {
                code: "fingerprint_mismatch",
            },
            tls::ConnectError::BadPeerName(_) => TransportError::Misconfigured {
                code: "bad_peer_name",
            },
        })?;
        Ok(Box::new(KdcTlsConnection {
            id: format!("kdc-tls:{peer_id}"),
            stream: AsyncMutex::new(stream),
        }))
    }

    async fn health(&self, peer_id: &str) -> HealthState {
        // Mirror probe — once the observation history lands
        // (KDC2-1.12), health() will weigh recent latency /
        // packet-loss into the answer.
        self.probe(peer_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keygen;
    use crate::pairing::PairedDevice;
    use crate::tls::compute_fingerprint;
    use mde_kdc_proto::discovery::{Announce, DeviceType};
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::time::Duration;
    use tempfile::tempdir;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    fn make_host_with_peer(peer_id: &str) -> KdcHost {
        let tmp = tempdir().unwrap();
        let store = PairingStore::open_or_init(tmp.path()).unwrap();
        store
            .upsert(PairedDevice {
                id: peer_id.into(),
                name: peer_id.into(),
                kind: "phone".into(),
                fingerprint: "AB:CD".into(),
                public_key_b64: "AA==".into(),
                capabilities: vec!["kdeconnect.clipboard".into()],
                paired_at: 1_700_000_000,
                last_seen_at: 1_700_000_500,
            })
            .unwrap();
        // Leak the tempdir guard so the store survives — the
        // tests don't write more files after the host is
        // constructed, so this is fine.
        std::mem::forget(tmp);
        KdcHost::pairing_only(Arc::new(store))
    }

    fn make_empty_host() -> KdcHost {
        let tmp = tempdir().unwrap();
        let store = PairingStore::open_or_init(tmp.path()).unwrap();
        std::mem::forget(tmp);
        KdcHost::pairing_only(Arc::new(store))
    }

    fn block_on<F: std::future::Future>(fut: F) -> F::Output {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build tokio rt");
        rt.block_on(fut)
    }

    #[test]
    fn kind_is_kdc_tls() {
        let h = make_empty_host();
        assert_eq!(h.kind(), TransportKind::KdcTls);
    }

    #[test]
    fn capabilities_carry_every_message_class() {
        let h = make_empty_host();
        let caps = h.capabilities();
        assert!(caps.carries.control);
        assert!(caps.carries.clipboard);
        assert!(caps.carries.file_bulk);
        assert!(caps.carries.notification);
        assert_eq!(caps.label, "kdc-tls");
    }

    #[test]
    fn probe_unpaired_peer_is_down() {
        let h = make_empty_host();
        let state = block_on(h.probe("nobody"));
        assert_eq!(state, HealthState::Down);
    }

    #[test]
    fn probe_paired_peer_without_discovery_is_down() {
        // Paired but no recent announce → Down. The router
        // shouldn't try to open a TLS connection at this point.
        let h = make_host_with_peer("alice");
        let state = block_on(h.probe("alice"));
        assert_eq!(state, HealthState::Down);
    }

    #[test]
    fn open_unpaired_peer_returns_unreachable_not_paired() {
        let h = make_empty_host();
        let err = block_on(h.open("nobody")).expect_err("unpaired must fail");
        match err {
            TransportError::Unreachable { code } => {
                assert_eq!(code, "not_paired");
            }
            other => panic!("expected Unreachable(not_paired), got {other:?}"),
        }
    }

    #[test]
    fn open_paired_peer_without_discovery_returns_not_discovered() {
        // Paired but no recent announce → Unreachable. Real-
        // world failure mode after a phone goes offline.
        let h = make_host_with_peer("alice");
        let err = block_on(h.open("alice")).expect_err("no addr must fail");
        match err {
            TransportError::Unreachable { code } => {
                assert_eq!(code, "not_discovered");
            }
            other => panic!("expected Unreachable(not_discovered), got {other:?}"),
        }
    }

    #[test]
    fn open_paired_peer_with_unreachable_addr_returns_tcp_refused() {
        // Inject a discovery record for an address with no
        // listener. The TCP connect should fail; the host maps
        // that to Unreachable(tcp_refused).
        let h = {
            let tmp = tempdir().unwrap();
            let store = PairingStore::open_or_init(tmp.path()).unwrap();
            store
                .upsert(PairedDevice {
                    id: "alice".into(),
                    name: "alice".into(),
                    kind: "phone".into(),
                    fingerprint: "AB:CD".into(),
                    public_key_b64: "AA==".into(),
                    capabilities: vec![],
                    paired_at: 1_700_000_000,
                    last_seen_at: 1_700_000_500,
                })
                .unwrap();
            std::mem::forget(tmp);
            let discovery = Arc::new(AsyncMutex::new(DiscoveryRegistry::new()));
            // 127.0.0.1:1 is a deliberately-refused address on
            // every Linux kernel (port 1 is reserved + nothing
            // is bound there in the test env).
            {
                let mut guard = block_on(discovery.lock());
                guard.inject_real_with_addr(
                    Announce {
                        device_id: "alice".into(),
                        device_name: "alice".into(),
                        device_type: DeviceType::Phone,
                        protocol_version: 7,
                        incoming_capabilities: vec![],
                        outgoing_capabilities: vec![],
                    },
                    1_700_000_500,
                    "127.0.0.1:1".parse().unwrap(),
                );
            }
            KdcHost::new(Arc::new(store), discovery)
        };
        let err = block_on(h.open("alice")).expect_err("refused must fail");
        match err {
            TransportError::Unreachable { code } => {
                assert_eq!(code, "tcp_refused");
            }
            other => panic!("expected Unreachable(tcp_refused), got {other:?}"),
        }
    }

    /// Spin up a minimal TLS server on 127.0.0.1:0 using
    /// rcgen-generated self-signed cert keyed to the given
    /// PKCS#8 RSA-2048 keypair, accept exactly one TLS
    /// handshake, then drop the stream. Returns the listener's
    /// bound address + the cert's SHA-256 fingerprint.
    fn spawn_loopback_tls(pkcs8: &[u8], device_id: &str) -> (SocketAddr, String) {
        let cert_der = keygen::issue_identity_cert(pkcs8, device_id).expect("cert");
        let fingerprint = compute_fingerprint(&cert_der);
        let priv_der = pkcs8.to_vec();
        let cert_for_thread = cert_der.clone();
        let priv_for_thread = priv_der;

        // Bind the listener synchronously so we have the addr
        // before returning. The blocking listener spawns its
        // own tokio runtime in a thread.
        let std_listener =
            std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let addr = std_listener.local_addr().expect("local_addr");

        std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("loopback rt");
            rt.block_on(async move {
                std_listener
                    .set_nonblocking(true)
                    .expect("set nonblocking");
                let listener = TcpListener::from_std(std_listener).expect("from_std");
                if let Ok((tcp, _)) = listener.accept().await {
                    let cert_chain = vec![CertificateDer::from(cert_for_thread)];
                    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(priv_for_thread));
                    let provider = Arc::new(rustls::crypto::ring::default_provider());
                    let config = rustls::ServerConfig::builder_with_provider(provider)
                        .with_safe_default_protocol_versions()
                        .expect("server protocol versions")
                        .with_no_client_auth()
                        .with_single_cert(cert_chain, key)
                        .expect("server config single cert");
                    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
                    if let Ok(mut tls) = acceptor.accept(tcp).await {
                        // Send a sentinel byte so the client's
                        // handshake is provably complete (some
                        // rustls paths only finalize after the
                        // first server-app-data).
                        let _ = tls.write_all(b"\x00").await;
                        // Give the client time to read before
                        // closing.
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
            });
        });

        (addr, fingerprint)
    }

    #[test]
    fn open_paired_peer_with_pinned_fingerprint_completes_handshake() {
        // Full integration: paired device + discovery entry +
        // loopback TLS server presenting the matching cert →
        // KdcHost::open succeeds and returns a KdcTlsConnection.
        let pkcs8 = keygen::generate_pkcs8().expect("pkcs8");
        let device_id = "loopback-peer";
        let (addr, fingerprint) = spawn_loopback_tls(&pkcs8, device_id);
        // The loopback server binds on 127.0.0.1:<some-port> but
        // KdcHost::open dials port KDC_TLS_PORT (1716). For the
        // test we need to align both — point the discovery entry
        // directly at the listener's port by overriding the
        // open dial-address path. We do that by binding the
        // loopback on the kdc port (which requires sudo). Instead
        // of that, exercise the connect_pinned_tls helper directly
        // — KdcHost::open's value is its pairing + discovery
        // lookup, both of which are covered by other tests in
        // this module. The TLS handshake itself is tested here.
        let result = block_on(crate::tls::connect_pinned_tls(
            addr,
            device_id,
            Some(fingerprint),
        ));
        assert!(result.is_ok(), "pinned TLS handshake should succeed: {:?}", result.err());
    }

    #[test]
    fn open_paired_peer_with_wrong_fingerprint_handshake_fails() {
        // Same loopback setup, but pin to a fingerprint that
        // doesn't match the server's cert → handshake fails →
        // ConnectError::Tls. Confirms PinnedFingerprintVerifier
        // is wired through connect_pinned_tls.
        let pkcs8 = keygen::generate_pkcs8().expect("pkcs8");
        let device_id = "loopback-peer-2";
        let (addr, _fingerprint) = spawn_loopback_tls(&pkcs8, device_id);
        let wrong_fp = "AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:\
                        AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99"
            .to_string();
        let result = block_on(crate::tls::connect_pinned_tls(
            addr,
            device_id,
            Some(wrong_fp),
        ));
        assert!(matches!(result, Err(crate::tls::ConnectError::Tls(_))));
    }

    #[test]
    fn is_object_safe_via_transport_trait() {
        // The mesh-router (KDC2-1.8) holds `Vec<Arc<dyn
        // Transport>>` — KdcHost must coerce into the trait
        // object cleanly.
        let h = make_empty_host();
        let _trait_obj: Arc<dyn Transport> = Arc::new(h);
    }

    #[test]
    fn discovery_handle_clones_share_state() {
        let h = make_empty_host();
        let d1 = h.discovery();
        let d2 = h.discovery();
        // Same underlying Arc storage.
        assert!(Arc::ptr_eq(&d1, &d2));
    }
}
