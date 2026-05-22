//! KDC2-3.3 — D-Bus host scaffold.
//!
//! Exposes the `dev.mackes.MDE.Connect` interface on the user
//! session bus at `/dev/mackes/MDE/Connect`. Concrete methods
//! land in KDC2-3.4 (`ListDevices` + `GetDevice`), 3.5
//! (`PairDevice` + `UnpairDevice`), 3.6 (`RingDevice` +
//! `SendSms` + `SendFile`), 3.9 (`DeviceAdded` / `Removed` /
//! `Updated` signals).
//!
//! Single-instance guard via the standard zbus name-request
//! flow: the bus refuses the name if another mde-kdc instance
//! already owns it, surfacing as
//! `DbusError::NameAlreadyAcquired`. The mackesd supervisor
//! treats this as a fatal startup error (no point running two
//! Connect hosts on the same session bus).
//!
//! Bus + interface naming follows the freedesktop conventions
//! the v2.1 KDC2 lock pinned:
//!   * Bus name:   `dev.mackes.MDE.Connect`
//!   * Object path: `/dev/mackes/MDE/Connect`
//!   * Interface:  `dev.mackes.MDE.Connect1` (version-suffixed
//!     per freedesktop best practice so a v2 rev can coexist).

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use zbus::zvariant::Type;

use crate::pairing::{PairedDevice, PairingStore};

/// Bus name MDE acquires on the user session bus.
pub const BUS_NAME: &str = "dev.mackes.MDE.Connect";

/// Object path the Connect interface is hosted at.
pub const OBJECT_PATH: &str = "/dev/mackes/MDE/Connect";

/// Interface name (version-suffixed so a future v2 can
/// register `dev.mackes.MDE.Connect2` alongside).
pub const INTERFACE_NAME: &str = "dev.mackes.MDE.Connect1";

/// D-Bus host errors. Stable Display tokens for audit-log
/// entries.
#[derive(Debug)]
pub enum DbusError {
    /// zbus connection-time error (couldn't reach the session
    /// bus, no DBUS_SESSION_BUS_ADDRESS, etc.).
    Connect(String),
    /// Object registration failed.
    ObjectRegister(String),
    /// Bus-name request failed — either rejected by the bus or
    /// already acquired by another process.
    NameAlreadyAcquired,
}

impl std::fmt::Display for DbusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DbusError::Connect(s) => write!(f, "connect: {s}"),
            DbusError::ObjectRegister(s) => write!(f, "object_register: {s}"),
            DbusError::NameAlreadyAcquired => write!(f, "name_already_acquired"),
        }
    }
}

impl std::error::Error for DbusError {}

/// D-Bus-exposed view of a paired device. Subset of
/// [`crate::pairing::PairedDevice`] — drops the internal
/// `public_key_b64` field (not exposed over the bus) +
/// derives `zbus::zvariant::Type` so it can be returned from
/// the `ListDevices` / `GetDevice` methods.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DeviceInfo {
    /// Stable device id (KDC UUID).
    pub id: String,
    /// Display name.
    pub name: String,
    /// `phone` / `tablet` / `desktop` / `unknown`.
    pub kind: String,
    /// SHA-256 fingerprint, `AB:CD:EF:...` format.
    pub fingerprint: String,
    /// Plugin tokens the device advertised under
    /// `incomingCapabilities`.
    pub capabilities: Vec<String>,
    /// Unix epoch seconds of the pair operation.
    pub paired_at: i64,
    /// Unix epoch seconds of the most-recent reachability
    /// observation.
    pub last_seen_at: i64,
}

impl From<&PairedDevice> for DeviceInfo {
    fn from(d: &PairedDevice) -> Self {
        Self {
            id: d.id.clone(),
            name: d.name.clone(),
            kind: d.kind.clone(),
            fingerprint: d.fingerprint.clone(),
            capabilities: d.capabilities.clone(),
            paired_at: d.paired_at,
            last_seen_at: d.last_seen_at,
        }
    }
}

/// Pure helper: collect every paired device into `DeviceInfo`
/// records. Used by both the `ListDevices` D-Bus method + the
/// unit tests (which bypass D-Bus to test the conversion
/// logic directly).
#[must_use]
pub fn list_devices_from(store: &PairingStore) -> Vec<DeviceInfo> {
    store.iter().map(DeviceInfo::from).collect()
}

/// Pure helper: fetch one device by id. Returns `None` when
/// the id isn't in the store; the D-Bus method translates this
/// to a `zbus::fdo::Error::Failed` with a known error string.
#[must_use]
pub fn get_device_from(
    store: &PairingStore,
    device_id: &str,
) -> Option<DeviceInfo> {
    store.get(device_id).map(DeviceInfo::from)
}

/// The Connect interface implementation. Backed by the
/// shared `PairingStore`; method implementations grow per
/// KDC2-3.4..3.6.
pub struct ConnectInterface {
    pairing_store: Arc<PairingStore>,
}

#[zbus::interface(name = "dev.mackes.MDE.Connect1")]
impl ConnectInterface {
    /// Host's own version string. Used by `gdbus introspect`
    /// smoke tests + ad-hoc operator probes.
    async fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    /// KDC2-3.4 — return every paired device as a
    /// `Vec<DeviceInfo>`. Operator probe:
    ///
    /// ```text
    /// gdbus call --session --dest dev.mackes.MDE.Connect \
    ///   --object-path /dev/mackes/MDE/Connect \
    ///   --method dev.mackes.MDE.Connect1.ListDevices
    /// ```
    async fn list_devices(&self) -> Vec<DeviceInfo> {
        list_devices_from(&self.pairing_store)
    }

    /// KDC2-3.4 — fetch a paired device by id. Errors with
    /// `dev.mackes.MDE.Connect1.NoSuchDevice` when the id
    /// isn't paired.
    async fn get_device(&self, device_id: String) -> zbus::fdo::Result<DeviceInfo> {
        get_device_from(&self.pairing_store, &device_id).ok_or_else(|| {
            zbus::fdo::Error::Failed(format!("NoSuchDevice: {device_id}"))
        })
    }

    /// KDC2-3.9 — emitted when a fresh device pairs. Subscribers
    /// (`mde-workbench` peer list, `mde-peer-card`, drawer
    /// notifications) refresh their views off this signal.
    #[zbus(signal)]
    async fn device_added(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        device: DeviceInfo,
    ) -> zbus::Result<()>;

    /// KDC2-3.9 — emitted when a previously-paired device is
    /// unpaired (operator-initiated unpair OR cert-key
    /// mismatch on re-handshake).
    #[zbus(signal)]
    async fn device_removed(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        device_id: String,
    ) -> zbus::Result<()>;

    /// KDC2-3.9 — emitted on device state change (online ↔
    /// offline transition, capability set update, battery
    /// snapshot, etc.). Subscribers re-fetch via `GetDevice`
    /// to read the new state.
    #[zbus(signal)]
    async fn device_updated(
        emitter: &zbus::object_server::SignalEmitter<'_>,
        device_id: String,
    ) -> zbus::Result<()>;
}

/// Live D-Bus host handle. Holds the zbus Connection so it
/// stays alive for the daemon's lifetime; dropping the handle
/// surrenders the bus name + un-registers the object.
pub struct DbusServer {
    _connection: zbus::Connection,
}

impl DbusServer {
    /// Acquire the Connect bus name + register the
    /// ConnectInterface at `/dev/mackes/MDE/Connect` on the
    /// user session bus.
    ///
    /// Errors:
    ///   * `Connect` — couldn't reach the session bus.
    ///   * `ObjectRegister` — registering the interface failed.
    ///   * `NameAlreadyAcquired` — another mde-kdc is already
    ///     running (or another process owns the name).
    pub async fn start(pairing: Arc<PairingStore>) -> Result<Self, DbusError> {
        let interface = ConnectInterface {
            pairing_store: pairing,
        };
        let connection = zbus::connection::Builder::session()
            .map_err(|e| DbusError::Connect(format!("{e}")))?
            .serve_at(OBJECT_PATH, interface)
            .map_err(|e| DbusError::ObjectRegister(format!("{e}")))?
            .name(BUS_NAME)
            .map_err(|_e| {
                // zbus surfaces name-acquisition failures from
                // both validation + bus-side rejection through
                // the same Result. We classify the rejection
                // case as `NameAlreadyAcquired`; validation
                // errors (invalid bus name) wedge here too but
                // shouldn't fire because BUS_NAME is hard-coded
                // + matches the freedesktop format. (No
                // tracing dep in this crate; the caller can
                // log around this if needed.)
                DbusError::NameAlreadyAcquired
            })?
            .build()
            .await
            .map_err(|e| {
                let msg = format!("{e}");
                if msg.contains("NameInUse") || msg.contains("already") {
                    DbusError::NameAlreadyAcquired
                } else {
                    DbusError::Connect(msg)
                }
            })?;
        Ok(Self {
            _connection: connection,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bus_name_matches_freedesktop_convention() {
        // Reverse-DNS form, no slashes, alphanumeric + dots.
        assert!(BUS_NAME.starts_with("dev.mackes.MDE."));
        assert!(BUS_NAME
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_'));
    }

    #[test]
    fn object_path_matches_bus_name_shape() {
        // Object path mirrors the bus name with `.` → `/` +
        // leading `/`. Allows operators to derive one from the
        // other without consulting docs.
        assert_eq!(OBJECT_PATH, "/dev/mackes/MDE/Connect");
        let derived = format!("/{}", BUS_NAME.replace('.', "/"));
        assert_eq!(derived, OBJECT_PATH);
    }

    #[test]
    fn interface_name_includes_version_suffix() {
        // Interface name carries a numeric version suffix
        // (`1`) so a future v2 rev can coexist via
        // `dev.mackes.MDE.Connect2` without breaking v1
        // clients. Lock the current value.
        assert_eq!(INTERFACE_NAME, "dev.mackes.MDE.Connect1");
        assert!(INTERFACE_NAME.ends_with('1'));
    }

    #[test]
    fn dbus_error_display_uses_stable_tokens() {
        assert_eq!(
            format!("{}", DbusError::NameAlreadyAcquired),
            "name_already_acquired",
        );
        assert!(format!("{}", DbusError::Connect("x".into())).starts_with("connect: "));
        assert!(format!("{}", DbusError::ObjectRegister("y".into()))
            .starts_with("object_register: "));
    }

    // ─────────────────────────────────────────────────────────
    // KDC2-3.4 — ListDevices + GetDevice pure helpers
    //
    // The actual D-Bus dispatch (via zbus's runtime) requires
    // a live session bus + an async runtime context the unit
    // test fixture doesn't set up. We test the conversion +
    // lookup logic the methods delegate to.
    // ─────────────────────────────────────────────────────────

    use crate::pairing::PairedDevice;
    use tempfile::tempdir;

    fn make_store_with_devices(devices: Vec<PairedDevice>) -> PairingStore {
        let tmp = tempdir().unwrap();
        let mut store = PairingStore::open_or_init(tmp.path()).unwrap();
        for d in devices {
            store.upsert(d).unwrap();
        }
        // Leak the tempdir guard — the store's identity.pem
        // lives on disk for the store's lifetime.
        std::mem::forget(tmp);
        store
    }

    fn sample_device(id: &str) -> PairedDevice {
        PairedDevice {
            id: id.into(),
            name: format!("Device {id}"),
            kind: "phone".into(),
            fingerprint: "AB:CD:EF".into(),
            public_key_b64: "AA==".into(),
            capabilities: vec!["kdeconnect.clipboard".into()],
            paired_at: 1_700_000_000,
            last_seen_at: 1_700_000_500,
        }
    }

    #[test]
    fn device_info_drops_public_key_field() {
        let d = sample_device("abc");
        let info = DeviceInfo::from(&d);
        // DeviceInfo deliberately does NOT expose the public
        // key bytes over D-Bus (private to the pairing store).
        // Serialize check: no `publicKey` substring.
        let raw = serde_json::to_string(&info).unwrap();
        assert!(
            !raw.contains("public_key") && !raw.contains("publicKey"),
            "DeviceInfo must not leak public_key_b64: {raw}",
        );
        // The other fields are present.
        assert!(raw.contains("abc"));
        assert!(raw.contains("phone"));
        assert!(raw.contains("AB:CD:EF"));
    }

    #[test]
    fn list_devices_returns_empty_when_no_pairings() {
        let store = make_store_with_devices(vec![]);
        let infos = list_devices_from(&store);
        assert!(infos.is_empty());
    }

    #[test]
    fn list_devices_returns_each_paired_device() {
        let store = make_store_with_devices(vec![
            sample_device("a"),
            sample_device("b"),
            sample_device("c"),
        ]);
        let mut infos = list_devices_from(&store);
        // BTreeMap-backed iteration returns id-sorted.
        infos.sort_by(|x, y| x.id.cmp(&y.id));
        assert_eq!(infos.len(), 3);
        assert_eq!(infos[0].id, "a");
        assert_eq!(infos[1].id, "b");
        assert_eq!(infos[2].id, "c");
    }

    #[test]
    fn get_device_returns_some_for_known_id() {
        let store = make_store_with_devices(vec![sample_device("abc-123")]);
        let info = get_device_from(&store, "abc-123");
        assert!(info.is_some());
        assert_eq!(info.unwrap().id, "abc-123");
    }

    #[test]
    fn get_device_returns_none_for_unknown_id() {
        let store = make_store_with_devices(vec![sample_device("only-one")]);
        let info = get_device_from(&store, "never-paired");
        assert!(info.is_none());
    }
}
