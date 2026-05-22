//! KDC2-2.10.a — host-side UDP/1716 broadcast runner.
//!
//! The pure-data half (encoder + decoder + registry) lives in
//! `mde_kdc_proto::discovery`. This module wires a
//! `tokio::net::UdpSocket` to UDP/1716 so the daemon can:
//!
//!   * Broadcast its own [`mde_kdc_proto::discovery::Announce`]
//!     to `255.255.255.255:1716` every 30 s.
//!
//!   * Receive datagrams from the same port + feed every
//!     decoded peer announce into a
//!     [`DiscoveryRegistry`] via `inject_real`.
//!
//! Both halves are concrete by design — there's no async-trait
//! seam — so the production code reads top-to-bottom. Tests use
//! loopback + the synchronous helpers so they're CI-safe
//! without privileged ports.
//!
//! The mDNS host runner (`_kdeconnect._udp.local.` via
//! mdns-sd) is queued as KDC2-2.9.a — slightly different
//! lifecycle, separate file when it lands.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::Instant;

use mde_kdc_proto::discovery::{
    decode_announce_datagram, encode_announce_datagram, Announce,
    DiscoveryRegistry, BroadcastError, KDC_UDP_PORT, MAX_BROADCAST_BYTES,
};

/// Broadcast cadence — matches upstream KDE Connect's 30 s
/// re-announce window. Operator-tunable in a future
/// policy.toml knob; baked in for now.
pub const BROADCAST_INTERVAL: Duration = Duration::from_secs(30);

/// Errors the runner may surface during setup. Once `run` is
/// going, transient I/O is logged + skipped — a partially
/// failed broadcast tick must not kill the daemon.
#[derive(Debug)]
pub enum RunnerError {
    /// `UdpSocket::bind` failed (port busy, permission denied,
    /// no socket capability).
    Bind(std::io::Error),
    /// `set_broadcast(true)` failed — required so we can send
    /// to the broadcast address.
    BroadcastFlag(std::io::Error),
}

impl std::fmt::Display for RunnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunnerError::Bind(e) => write!(f, "bind: {e}"),
            RunnerError::BroadcastFlag(e) => write!(f, "broadcast_flag: {e}"),
        }
    }
}

impl std::error::Error for RunnerError {}

/// Async runner for the UDP/1716 broadcast loop.
///
/// `bind_port` defaults to [`KDC_UDP_PORT`] (1716) in
/// production. Tests pass `0` to get an ephemeral port.
pub struct UdpBroadcastRunner {
    /// Live socket.
    socket: Arc<UdpSocket>,
    /// Shared registry the runner feeds with decoded peer
    /// announces.
    registry: Arc<AsyncMutex<DiscoveryRegistry>>,
    /// Our identity to broadcast each tick. Captured at
    /// construction; if the user renames the host, restart the
    /// daemon.
    self_announce: Announce,
}

impl UdpBroadcastRunner {
    /// Bind a UDP socket on `0.0.0.0:bind_port`, flip the
    /// broadcast flag, and return a ready-to-run runner.
    /// Doesn't actually start ticking until `run` is awaited.
    pub async fn bind(
        bind_port: u16,
        self_announce: Announce,
        registry: Arc<AsyncMutex<DiscoveryRegistry>>,
    ) -> Result<Self, RunnerError> {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), bind_port);
        let socket = UdpSocket::bind(addr).await.map_err(RunnerError::Bind)?;
        socket
            .set_broadcast(true)
            .map_err(RunnerError::BroadcastFlag)?;
        Ok(Self {
            socket: Arc::new(socket),
            registry,
            self_announce,
        })
    }

    /// Local-port the socket is actually bound to. Used by
    /// tests that bind to port 0 + need to know where to send.
    pub fn local_port(&self) -> std::io::Result<u16> {
        Ok(self.socket.local_addr()?.port())
    }

    /// One iteration of the broadcast loop. Pure helper so
    /// tests can drive a single tick.
    pub async fn broadcast_once(&self, ts_ms: i64) -> std::io::Result<usize> {
        let datagram = encode_announce_datagram(&self.self_announce, ts_ms)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{e}")))?;
        let target = SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), KDC_UDP_PORT);
        self.socket.send_to(&datagram, target).await
    }

    /// Receive one datagram + decode it into an [`Announce`].
    /// Returns the parsed announce + the sender's address so the
    /// caller can record peer reachability. Returns
    /// `Ok(None)` for a datagram that decoded as the wrong kind
    /// (handled silently — could be a stray clipboard packet
    /// from a misconfigured peer).
    ///
    /// Buffered against `MAX_BROADCAST_BYTES` — bigger datagrams
    /// surface as `WouldBlock`-style discards so a hostile peer
    /// can't OOM the runner.
    pub async fn recv_one(&self) -> std::io::Result<Option<(Announce, SocketAddr)>> {
        let mut buf = vec![0u8; MAX_BROADCAST_BYTES];
        let (n, src) = self.socket.recv_from(&mut buf).await?;
        match decode_announce_datagram(&buf[..n]) {
            Ok(announce) => Ok(Some((announce, src))),
            Err(BroadcastError::WrongPacketKind(_)) => Ok(None),
            Err(e) => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{e}"),
            )),
        }
    }

    /// Drain one received announce into the registry. Glue
    /// between [`recv_one`] and the shared [`DiscoveryRegistry`]
    /// — caller-visible because tests want to assert the
    /// registry got fed.
    pub async fn ingest_one(&self, announce: Announce, now_ms: i64) {
        let mut guard = self.registry.lock().await;
        guard.inject_real(announce, now_ms);
    }

    /// Main loop. Concurrent broadcast tick + recv loop. Runs
    /// until the supplied shutdown future resolves. Returns
    /// `Ok(())` on clean shutdown.
    pub async fn run(
        self: Arc<Self>,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) -> Result<(), std::io::Error> {
        let mut interval = tokio::time::interval(BROADCAST_INTERVAL);
        let started = Instant::now();
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return Ok(());
                    }
                }
                _ = interval.tick() => {
                    let ts_ms = started.elapsed().as_millis() as i64;
                    let _ = self.broadcast_once(ts_ms).await;
                }
                got = self.recv_one() => {
                    if let Ok(Some((announce, _src))) = got {
                        let now_ms = started.elapsed().as_millis() as i64;
                        self.ingest_one(announce, now_ms).await;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mde_kdc_proto::discovery::DeviceType;
    use mde_kdc_proto::PROTOCOL_VERSION;

    fn sample_announce(id: &str) -> Announce {
        Announce {
            device_id: id.into(),
            device_name: format!("test-host {}", mde_kdc_proto::MDE_DEVICE_NAME_SUFFIX),
            device_type: DeviceType::Desktop,
            protocol_version: PROTOCOL_VERSION,
            incoming_capabilities: vec!["kdeconnect.ping".into()],
            outgoing_capabilities: vec!["kdeconnect.ping".into()],
        }
    }

    fn new_registry() -> Arc<AsyncMutex<DiscoveryRegistry>> {
        Arc::new(AsyncMutex::new(DiscoveryRegistry::new()))
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bind_succeeds_on_ephemeral_port() {
        let r = UdpBroadcastRunner::bind(0, sample_announce("me"), new_registry())
            .await
            .unwrap();
        let port = r.local_port().unwrap();
        assert!(port > 0, "ephemeral bind returned port 0");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn ingest_one_records_into_registry() {
        let registry = new_registry();
        let r = UdpBroadcastRunner::bind(0, sample_announce("me"), Arc::clone(&registry))
            .await
            .unwrap();
        r.ingest_one(sample_announce("peer-A"), 1000).await;
        let guard = registry.lock().await;
        assert_eq!(guard.relayer_for("peer-A"), Some("self"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn round_trip_broadcast_and_decode() {
        // Sender + receiver bound to two ephemeral ports on
        // loopback. The sender shoots a datagram at the
        // receiver's port; the receiver decodes + ingests.
        let sender_registry = new_registry();
        let receiver_registry = new_registry();
        let sender = UdpBroadcastRunner::bind(0, sample_announce("sender"), sender_registry)
            .await
            .unwrap();
        let receiver = UdpBroadcastRunner::bind(0, sample_announce("recv"), Arc::clone(&receiver_registry))
            .await
            .unwrap();
        let recv_port = receiver.local_port().unwrap();

        // Encode + send directly to the receiver's loopback
        // port (skips the broadcast-address path which CI
        // sandboxes block).
        let bytes = encode_announce_datagram(&sample_announce("sender"), 100).unwrap();
        let target = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), recv_port);
        sender.socket.send_to(&bytes, target).await.unwrap();

        // Receiver pulls + ingests.
        let (got, _src) = tokio::time::timeout(
            Duration::from_secs(2),
            receiver.recv_one(),
        )
        .await
        .expect("recv timed out")
        .unwrap()
        .expect("received None");
        assert_eq!(got.device_id, "sender");
        receiver.ingest_one(got, 200).await;
        let guard = receiver_registry.lock().await;
        assert_eq!(guard.relayer_for("sender"), Some("self"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn recv_one_silently_drops_wrong_kind_datagrams() {
        // A peer broadcasts a clipboard packet on UDP/1716 by
        // mistake. The runner must not treat that as an error
        // (which would log noise) — it returns Ok(None).
        let registry = new_registry();
        let receiver = UdpBroadcastRunner::bind(0, sample_announce("recv"), registry)
            .await
            .unwrap();
        let recv_port = receiver.local_port().unwrap();

        let bad_packet = mde_kdc_proto::wire::Packet {
            id: 1,
            kind: "kdeconnect.clipboard".into(),
            body: serde_json::json!({}),
            ..Default::default()
        };
        let mut bytes = serde_json::to_vec(&bad_packet).unwrap();
        bytes.push(b'\n');
        let sender_socket = UdpSocket::bind(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            0,
        ))
        .await
        .unwrap();
        let target = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), recv_port);
        sender_socket.send_to(&bytes, target).await.unwrap();

        let result = tokio::time::timeout(Duration::from_secs(2), receiver.recv_one())
            .await
            .expect("recv timed out")
            .unwrap();
        assert!(result.is_none(), "wrong-kind packet should yield None");
    }
}
