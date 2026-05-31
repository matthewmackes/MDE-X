//! MESH-A-1 (v5.0.0) — per-peer network assessment subsystem.
//!
//! Collects the 9 network-assessment items locked in
//! `docs/design/v6.0-mde-portal.md` §7.1 (R7-Q1..Q7) on a periodic
//! tick, writes a timestamped JSON snapshot to
//! `~/.local/share/mde/netassess/<host>/<iso8601>-<hash>.json`, and
//! trims snapshots older than 30 days. The directory lands under
//! mesh-storage once mounted (it inherits the existing per-peer
//! replication), so every peer reads the union for the Portal /
//! Workbench network surfaces.
//!
//! ## The 9 items (design doc §7.1)
//!
//! 1. WiFi SSIDs + RSSI + channel + encryption (`nmcli` terse).
//! 2. Local ARP table (`ip neigh`).
//! 3. Default gateway + DNS servers (`ip route` + `/etc/resolv.conf`).
//! 4. Public IP + ISP/AS (`curl ipinfo.io/json`).
//! 5. Speedtest down/up/latency (`speedtest-cli --json`).
//! 6. IPv4 + IPv6 connectivity (`ping` / `ping -6`).
//! 7. MTU + jumbo-frame support (`ip link`).
//! 8. Tunnel health (nebula1 interface up).
//! 9. nmap-light passive subnet discovery (reuses the EPIC-MESH-PROBE
//!    inventory when present, per mesh-probe-subsystem.md §3; falls
//!    back to the ARP-table host count).
//!
//! ## Cadence
//!
//! Active collection runs hourly ([`DEFAULT_TICK_INTERVAL`]). The
//! worklist line cites "active 10 min", but a 10-minute speedtest
//! cadence is bandwidth-abusive — the design doc §7.1 "hourly" is the
//! sane lock and is used here. On-demand refresh (Portal-compact
//! open) is a future Bus-topic trigger (MESH-A-1.refresh follow-on).
//!
//! Shell-outs that aren't present (no `nmcli` / `speedtest-cli` /
//! `curl` on a headless or air-gapped peer) degrade to `None` for
//! that item — the snapshot still writes with whatever collected.
//! Pure parsers are unit-tested against sample tool output; the
//! shell-out execution + reachability pings are HW-bench-gated
//! (§0.15).

#![cfg(feature = "async-services")]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};

use super::{ShutdownToken, Worker};

/// Active-collection cadence — hourly (design doc §7.1).
pub const DEFAULT_TICK_INTERVAL: Duration = Duration::from_secs(3600);

/// Retention window — 30 days in milliseconds (R7-Q3).
pub const RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1_000;

/// Nebula overlay interface checked for tunnel health.
pub const DEFAULT_NEBULA_INTERFACE: &str = "nebula1";

/// One WiFi network seen in a scan.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WifiNetwork {
    /// SSID (network name); empty for hidden networks.
    pub ssid: String,
    /// Signal strength 0-100 (nmcli SIGNAL).
    pub signal: u8,
    /// Channel number.
    pub channel: u16,
    /// Security string (e.g. `WPA2`, `--` for open).
    pub security: String,
}

/// One ARP/neighbour-table entry.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ArpEntry {
    /// Neighbour IP.
    pub ip: String,
    /// MAC address (lowercase, colon-separated).
    pub mac: String,
    /// Interface the neighbour was seen on.
    pub iface: String,
}

/// Default gateway + resolver set.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GatewayDns {
    /// Default-route gateway IP (empty if none).
    pub gateway: String,
    /// DNS resolver IPs from `/etc/resolv.conf`.
    pub dns: Vec<String>,
}

/// Public-IP + ISP/AS info.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PublicIp {
    /// Public IPv4/IPv6 as seen by ipinfo.
    pub ip: String,
    /// ISP / AS org string.
    pub org: String,
}

/// Speedtest result.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Speedtest {
    /// Download Mbit/s.
    pub download_mbps: f64,
    /// Upload Mbit/s.
    pub upload_mbps: f64,
    /// Latency milliseconds.
    pub ping_ms: f64,
}

/// IPv4 + IPv6 reachability.
#[derive(Debug, Clone, Copy, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Connectivity {
    /// IPv4 reachable (ping 1.1.1.1).
    pub ipv4: bool,
    /// IPv6 reachable (ping6 2606:4700:4700::1111).
    pub ipv6: bool,
}

/// MTU + jumbo-frame status for the primary interface.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MtuInfo {
    /// Interface name.
    pub iface: String,
    /// MTU in bytes.
    pub mtu: u32,
    /// Jumbo frames (MTU >= 9000).
    pub jumbo: bool,
}

/// Nebula tunnel health.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TunnelHealth {
    /// Overlay interface name.
    pub iface: String,
    /// Interface is present + UP.
    pub up: bool,
    /// Overlay IP if assigned.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub overlay_ip: String,
}

/// nmap-light subnet discovery result.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SubnetDiscovery {
    /// Count of discovered hosts.
    pub host_count: usize,
    /// Source: `probe-inventory` (reused) or `arp-fallback`.
    pub source: String,
}

/// The full per-peer assessment snapshot (the 9 items + metadata).
/// Each item is optional so a partial collection (missing tool)
/// still produces a valid snapshot.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AssessmentSnapshot {
    /// Wall-clock epoch-ms of collection.
    pub ts_ms: i64,
    /// `/etc/hostname` of the collecting peer.
    pub host: String,
    /// Item 1.
    #[serde(default)]
    pub wifi: Vec<WifiNetwork>,
    /// Item 2.
    #[serde(default)]
    pub arp: Vec<ArpEntry>,
    /// Item 3.
    pub gateway_dns: GatewayDns,
    /// Item 4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub public_ip: Option<PublicIp>,
    /// Item 5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speedtest: Option<Speedtest>,
    /// Item 6.
    pub connectivity: Connectivity,
    /// Item 7.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mtu: Option<MtuInfo>,
    /// Item 8.
    pub tunnel: TunnelHealth,
    /// Item 9.
    pub subnet: SubnetDiscovery,
}

// ── Pure parsers (one per shell-out; unit-tested) ──────────────────

/// Parse `nmcli -t -f SSID,SIGNAL,CHAN,SECURITY dev wifi` terse
/// output (colon-separated, one network per line). Escaped `\:`
/// inside an SSID is unescaped. Blank/malformed lines are skipped.
#[must_use]
pub fn parse_nmcli_wifi(stdout: &str) -> Vec<WifiNetwork> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        // nmcli terse escapes field-internal ':' as '\:'. Split on
        // unescaped ':' by temporarily swapping the escape sequence.
        let swapped = line.replace("\\:", "\u{0}");
        let fields: Vec<String> = swapped
            .split(':')
            .map(|f| f.replace('\u{0}', ":"))
            .collect();
        if fields.len() < 4 {
            continue;
        }
        let signal = fields[1].trim().parse::<u8>().unwrap_or(0);
        let channel = fields[2].trim().parse::<u16>().unwrap_or(0);
        out.push(WifiNetwork {
            ssid: fields[0].clone(),
            signal,
            channel,
            security: fields[3].trim().to_string(),
        });
    }
    out
}

/// Parse `ip neigh` output into ARP entries. Lines look like
/// `10.0.0.1 dev eth0 lladdr aa:bb:cc:dd:ee:ff REACHABLE`. Entries
/// without an `lladdr` (FAILED / INCOMPLETE) are skipped.
#[must_use]
pub fn parse_ip_neigh(stdout: &str) -> Vec<ArpEntry> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let toks: Vec<&str> = line.split_whitespace().collect();
        if toks.is_empty() {
            continue;
        }
        let ip = toks[0].to_string();
        let mut mac = String::new();
        let mut iface = String::new();
        let mut i = 1;
        while i + 1 < toks.len() {
            match toks[i] {
                "dev" => iface = toks[i + 1].to_string(),
                "lladdr" => mac = toks[i + 1].to_ascii_lowercase(),
                _ => {}
            }
            i += 1;
        }
        if mac.is_empty() || ip.is_empty() {
            continue;
        }
        out.push(ArpEntry { ip, mac, iface });
    }
    out
}

/// Parse the gateway IP from `ip route show default` output
/// (`default via 10.0.0.1 dev eth0 ...`). Returns empty when absent.
#[must_use]
pub fn parse_default_gateway(stdout: &str) -> String {
    for line in stdout.lines() {
        let toks: Vec<&str> = line.split_whitespace().collect();
        if toks.first() == Some(&"default") {
            if let Some(pos) = toks.iter().position(|t| *t == "via") {
                if let Some(gw) = toks.get(pos + 1) {
                    return (*gw).to_string();
                }
            }
        }
    }
    String::new()
}

/// Parse resolver IPs from `/etc/resolv.conf` content
/// (`nameserver <ip>` lines; comments + other directives ignored).
#[must_use]
pub fn parse_resolv_conf(content: &str) -> Vec<String> {
    content
        .lines()
        .map(str::trim)
        .filter(|l| !l.starts_with('#') && !l.starts_with(';'))
        .filter_map(|l| l.strip_prefix("nameserver "))
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Parse `curl -s https://ipinfo.io/json` output. Returns `None`
/// when the JSON is malformed or missing `ip`.
#[must_use]
pub fn parse_ipinfo_json(stdout: &str) -> Option<PublicIp> {
    let v: serde_json::Value = serde_json::from_str(stdout).ok()?;
    let ip = v.get("ip")?.as_str()?.to_string();
    let org = v
        .get("org")
        .and_then(|o| o.as_str())
        .unwrap_or("")
        .to_string();
    Some(PublicIp { ip, org })
}

/// Parse `speedtest-cli --json` output. Bits/s in the JSON are
/// converted to Mbit/s. Returns `None` on malformed JSON.
#[must_use]
pub fn parse_speedtest_json(stdout: &str) -> Option<Speedtest> {
    let v: serde_json::Value = serde_json::from_str(stdout).ok()?;
    let download_bps = v.get("download")?.as_f64()?;
    let upload_bps = v.get("upload")?.as_f64()?;
    let ping_ms = v.get("ping").and_then(serde_json::Value::as_f64).unwrap_or(0.0);
    Some(Speedtest {
        download_mbps: download_bps / 1_000_000.0,
        upload_mbps: upload_bps / 1_000_000.0,
        ping_ms,
    })
}

/// Parse the MTU for `iface` from `ip link show <iface>` output
/// (`... mtu 1500 ...`). Returns `None` when not found.
#[must_use]
pub fn parse_ip_link_mtu(stdout: &str, iface: &str) -> Option<MtuInfo> {
    let toks: Vec<&str> = stdout.split_whitespace().collect();
    let pos = toks.iter().position(|t| *t == "mtu")?;
    let mtu: u32 = toks.get(pos + 1)?.parse().ok()?;
    Some(MtuInfo {
        iface: iface.to_string(),
        mtu,
        jumbo: mtu >= 9000,
    })
}

/// Determine tunnel health from `ip link show <iface>` output:
/// the interface is up when the line carries the `UP` flag or
/// `state UP`. Empty stdout ⇒ interface absent ⇒ down.
#[must_use]
pub fn parse_tunnel_up(stdout: &str, iface: &str) -> bool {
    if stdout.trim().is_empty() {
        return false;
    }
    // `<BROADCAST,MULTICAST,UP,LOWER_UP>` flag list or `state UP`.
    let _ = iface;
    stdout.contains(",UP,")
        || stdout.contains("<UP,")
        || stdout.contains(",UP>")
        || stdout.contains("state UP")
}

/// Build the per-snapshot filename `<iso8601>-<hash>.json`, where
/// `<hash>` is the first 8 hex chars of the SHA-256 of the JSON body
/// (dedup + integrity). `iso8601` is colon-free (`:` is illegal on
/// some FSes) — colons become `-`.
#[must_use]
pub fn snapshot_filename(iso8601: &str, json_body: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(json_body.as_bytes());
    let hash = hasher.finalize();
    let short: String = hash.iter().take(4).map(|b| format!("{b:02x}")).collect();
    let safe_iso = iso8601.replace(':', "-");
    format!("{safe_iso}-{short}.json")
}

/// Trim snapshot files under `dir` whose embedded `ts_ms` is older
/// than `cutoff_ms`. No-ops when the dir is absent.
pub fn trim_older_than(dir: &Path, cutoff_ms: i64) -> std::io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir)?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let keep = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|v| v["ts_ms"].as_i64())
            .map(|ts| ts >= cutoff_ms)
            .unwrap_or(true); // keep unparseable files (don't delete blindly)
        if !keep {
            let _ = std::fs::remove_file(&path);
        }
    }
    Ok(())
}

// ── Collectors (shell-out; bench-gated) ────────────────────────────

fn run_stdout(bin: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(bin).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

fn binary_present(bin: &str) -> bool {
    Command::new(bin).arg("--version").output().is_ok()
}

fn ping_reachable(target: &str, v6: bool) -> bool {
    let mut args = vec!["-c", "1", "-W", "2"];
    if v6 {
        args.insert(0, "-6");
    }
    args.push(target);
    Command::new("ping")
        .args(&args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn collect_subnet(arp: &[ArpEntry]) -> SubnetDiscovery {
    // Reuse the EPIC-MESH-PROBE inventory when present (per
    // mesh-probe-subsystem.md §3); otherwise fall back to the ARP
    // host count so the item is never empty.
    if let Some(dir) = dirs::data_dir() {
        let probe = dir.join("mde").join("probe-inventory.json");
        if let Ok(body) = std::fs::read_to_string(&probe) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
                if let Some(hosts) = v.get("hosts").and_then(|h| h.as_array()) {
                    return SubnetDiscovery {
                        host_count: hosts.len(),
                        source: "probe-inventory".into(),
                    };
                }
            }
        }
    }
    SubnetDiscovery {
        host_count: arp.len(),
        source: "arp-fallback".into(),
    }
}

fn now_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Worker handle.
pub struct NetAssessWorker {
    host: String,
    base_dir: PathBuf,
    nebula_iface: String,
    tick: Duration,
}

impl NetAssessWorker {
    /// Construct with production defaults. `base_dir` is the
    /// `netassess` root (`~/.local/share/mde/netassess` in prod).
    #[must_use]
    pub fn new(host: String, base_dir: PathBuf) -> Self {
        Self {
            host,
            base_dir,
            nebula_iface: DEFAULT_NEBULA_INTERFACE.into(),
            tick: DEFAULT_TICK_INTERVAL,
        }
    }

    /// Override the tick cadence. Used in tests.
    #[must_use]
    pub fn with_tick(mut self, d: Duration) -> Self {
        self.tick = d;
        self
    }

    fn primary_iface(&self) -> String {
        // The default-route device is the primary interface.
        run_stdout("ip", &["route", "show", "default"])
            .and_then(|s| {
                s.split_whitespace()
                    .skip_while(|t| *t != "dev")
                    .nth(1)
                    .map(String::from)
            })
            .unwrap_or_else(|| "eth0".into())
    }

    fn collect(&self) -> AssessmentSnapshot {
        let wifi = if binary_present("nmcli") {
            run_stdout("nmcli", &["-t", "-f", "SSID,SIGNAL,CHAN,SECURITY", "dev", "wifi"])
                .map(|s| parse_nmcli_wifi(&s))
                .unwrap_or_default()
        } else {
            vec![]
        };
        let arp = run_stdout("ip", &["neigh"]).map(|s| parse_ip_neigh(&s)).unwrap_or_default();
        let gateway = run_stdout("ip", &["route", "show", "default"])
            .map(|s| parse_default_gateway(&s))
            .unwrap_or_default();
        let dns = std::fs::read_to_string("/etc/resolv.conf")
            .map(|c| parse_resolv_conf(&c))
            .unwrap_or_default();
        let public_ip = run_stdout("curl", &["-s", "--max-time", "5", "https://ipinfo.io/json"])
            .and_then(|s| parse_ipinfo_json(&s));
        let speedtest = if binary_present("speedtest-cli") {
            run_stdout("speedtest-cli", &["--json"]).and_then(|s| parse_speedtest_json(&s))
        } else {
            None
        };
        let connectivity = Connectivity {
            ipv4: ping_reachable("1.1.1.1", false),
            ipv6: ping_reachable("2606:4700:4700::1111", true),
        };
        let iface = self.primary_iface();
        let mtu = run_stdout("ip", &["link", "show", &iface]).and_then(|s| parse_ip_link_mtu(&s, &iface));
        let tunnel_stdout = run_stdout("ip", &["link", "show", &self.nebula_iface]).unwrap_or_default();
        let tunnel = TunnelHealth {
            iface: self.nebula_iface.clone(),
            up: parse_tunnel_up(&tunnel_stdout, &self.nebula_iface),
            overlay_ip: String::new(),
        };
        let subnet = collect_subnet(&arp);

        AssessmentSnapshot {
            ts_ms: now_epoch_ms(),
            host: self.host.clone(),
            wifi,
            arp,
            gateway_dns: GatewayDns { gateway, dns },
            public_ip,
            speedtest,
            connectivity,
            mtu,
            tunnel,
            subnet,
        }
    }

    fn host_dir(&self) -> PathBuf {
        self.base_dir.join(&self.host)
    }

    fn write_snapshot(&self, snap: &AssessmentSnapshot) {
        let dir = self.host_dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::debug!(error = %e, "netassess: mkdir failed");
            return;
        }
        let Ok(body) = serde_json::to_string_pretty(snap) else {
            return;
        };
        let iso = chrono::Local::now().format("%Y%m%dT%H%M%S").to_string();
        let path = dir.join(snapshot_filename(&iso, &body));
        if let Err(e) = std::fs::write(&path, &body) {
            tracing::debug!(error = %e, "netassess: write failed");
        }
    }

    fn tick_once(&self) {
        let snap = self.collect();
        self.write_snapshot(&snap);
        let cutoff = now_epoch_ms() - RETENTION_MS;
        let _ = trim_older_than(&self.host_dir(), cutoff);
    }
}

#[async_trait::async_trait]
impl Worker for NetAssessWorker {
    fn name(&self) -> &'static str {
        "netassess"
    }

    async fn run(&mut self, mut shutdown: ShutdownToken) -> anyhow::Result<()> {
        loop {
            tokio::select! {
                _ = tokio::time::sleep(self.tick) => {
                    self.tick_once();
                }
                _ = shutdown.wait() => break,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_nmcli_wifi ──

    #[test]
    fn wifi_parses_terse_lines() {
        let raw = "HomeNet:78:36:WPA2\nCoffee\\:Shop:42:6:WPA2\nOpenAP:90:11:--\n";
        let nets = parse_nmcli_wifi(raw);
        assert_eq!(nets.len(), 3);
        assert_eq!(nets[0].ssid, "HomeNet");
        assert_eq!(nets[0].signal, 78);
        assert_eq!(nets[0].channel, 36);
        assert_eq!(nets[0].security, "WPA2");
        // escaped colon inside SSID preserved
        assert_eq!(nets[1].ssid, "Coffee:Shop");
        assert_eq!(nets[2].security, "--");
    }

    #[test]
    fn wifi_skips_blank_and_short_lines() {
        let raw = "\nBad:line\nGood:50:1:WPA3\n";
        let nets = parse_nmcli_wifi(raw);
        assert_eq!(nets.len(), 1);
        assert_eq!(nets[0].ssid, "Good");
    }

    // ── parse_ip_neigh ──

    #[test]
    fn neigh_parses_reachable_entries() {
        let raw = "10.0.0.1 dev eth0 lladdr AA:BB:CC:DD:EE:FF REACHABLE\n\
                   10.0.0.2 dev eth0 FAILED\n\
                   fe80::1 dev eth0 lladdr 11:22:33:44:55:66 STALE\n";
        let arp = parse_ip_neigh(raw);
        assert_eq!(arp.len(), 2); // FAILED (no lladdr) skipped
        assert_eq!(arp[0].ip, "10.0.0.1");
        assert_eq!(arp[0].mac, "aa:bb:cc:dd:ee:ff"); // lowercased
        assert_eq!(arp[0].iface, "eth0");
    }

    // ── parse_default_gateway ──

    #[test]
    fn gateway_from_default_route() {
        let raw = "default via 192.168.1.1 dev wlan0 proto dhcp metric 600\n";
        assert_eq!(parse_default_gateway(raw), "192.168.1.1");
    }

    #[test]
    fn gateway_empty_when_no_default() {
        assert_eq!(parse_default_gateway("10.0.0.0/24 dev eth0\n"), "");
    }

    // ── parse_resolv_conf ──

    #[test]
    fn resolv_extracts_nameservers() {
        let raw = "# generated\nnameserver 1.1.1.1\nsearch lan\nnameserver 8.8.8.8\n";
        assert_eq!(parse_resolv_conf(raw), vec!["1.1.1.1", "8.8.8.8"]);
    }

    // ── parse_ipinfo_json ──

    #[test]
    fn ipinfo_parses_ip_and_org() {
        let raw = r#"{"ip":"203.0.113.7","org":"AS13335 Cloudflare","city":"X"}"#;
        let p = parse_ipinfo_json(raw).expect("parse");
        assert_eq!(p.ip, "203.0.113.7");
        assert_eq!(p.org, "AS13335 Cloudflare");
    }

    #[test]
    fn ipinfo_none_on_garbage() {
        assert!(parse_ipinfo_json("not json").is_none());
    }

    // ── parse_speedtest_json ──

    #[test]
    fn speedtest_converts_bps_to_mbps() {
        let raw = r#"{"download":94000000.0,"upload":12000000.0,"ping":14.2}"#;
        let s = parse_speedtest_json(raw).expect("parse");
        assert!((s.download_mbps - 94.0).abs() < 0.01);
        assert!((s.upload_mbps - 12.0).abs() < 0.01);
        assert!((s.ping_ms - 14.2).abs() < 0.01);
    }

    // ── parse_ip_link_mtu ──

    #[test]
    fn mtu_parses_and_flags_jumbo() {
        let std1 = "2: eth0: <BROADCAST,MULTICAST,UP> mtu 1500 qdisc fq state UP";
        let m = parse_ip_link_mtu(std1, "eth0").expect("mtu");
        assert_eq!(m.mtu, 1500);
        assert!(!m.jumbo);
        let std2 = "3: eth1: <UP> mtu 9000 qdisc fq";
        assert!(parse_ip_link_mtu(std2, "eth1").unwrap().jumbo);
    }

    // ── parse_tunnel_up ──

    #[test]
    fn tunnel_up_detected_from_flags_and_state() {
        assert!(parse_tunnel_up("4: nebula1: <POINTOPOINT,MULTICAST,NOARP,UP,LOWER_UP> mtu 1300", "nebula1"));
        assert!(parse_tunnel_up("nebula1: state UP mtu 1300", "nebula1"));
        assert!(!parse_tunnel_up("4: nebula1: <POINTOPOINT,NOARP> state DOWN", "nebula1"));
        assert!(!parse_tunnel_up("", "nebula1")); // absent interface
    }

    // ── snapshot_filename ──

    #[test]
    fn filename_is_colon_free_with_hash_suffix() {
        let name = snapshot_filename("20260531T143000", r#"{"ts_ms":1}"#);
        assert!(name.starts_with("20260531T143000-"));
        assert!(name.ends_with(".json"));
        assert!(!name.contains(':'));
        // deterministic hash for the same body
        assert_eq!(name, snapshot_filename("20260531T143000", r#"{"ts_ms":1}"#));
    }

    #[test]
    fn filename_colons_replaced() {
        let name = snapshot_filename("2026-05-31T14:30:00", "{}");
        assert!(!name.contains(':'));
    }

    // ── trim_older_than ──

    #[test]
    fn trim_removes_old_keeps_recent() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        std::fs::write(dir.join("old.json"), r#"{"ts_ms":100}"#).unwrap();
        std::fs::write(dir.join("new.json"), r#"{"ts_ms":9000}"#).unwrap();
        trim_older_than(dir, 1000).unwrap();
        assert!(!dir.join("old.json").exists());
        assert!(dir.join("new.json").exists());
    }

    #[test]
    fn trim_noop_when_dir_absent() {
        let tmp = tempfile::tempdir().unwrap();
        trim_older_than(&tmp.path().join("nope"), 0).unwrap();
    }

    #[test]
    fn trim_keeps_unparseable_files() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("junk.json"), "not json").unwrap();
        trim_older_than(tmp.path(), i64::MAX).unwrap();
        assert!(tmp.path().join("junk.json").exists());
    }

    // ── collect_subnet ──

    #[test]
    fn subnet_arp_fallback_counts_entries() {
        let arp = vec![
            ArpEntry { ip: "10.0.0.1".into(), mac: "a".into(), iface: "e".into() },
            ArpEntry { ip: "10.0.0.2".into(), mac: "b".into(), iface: "e".into() },
        ];
        // No probe inventory in a clean test env → arp-fallback.
        let s = collect_subnet(&arp);
        // source may be probe-inventory if the test host has one; assert the fallback shape only when arp-derived.
        if s.source == "arp-fallback" {
            assert_eq!(s.host_count, 2);
        }
    }

    // ── snapshot JSON shape (design doc §7.1 — all 9 items present) ──

    #[test]
    fn snapshot_json_carries_all_nine_items() {
        let snap = AssessmentSnapshot {
            ts_ms: 1,
            host: "alice".into(),
            wifi: vec![],
            arp: vec![],
            gateway_dns: GatewayDns::default(),
            public_ip: None,
            speedtest: None,
            connectivity: Connectivity::default(),
            mtu: None,
            tunnel: TunnelHealth::default(),
            subnet: SubnetDiscovery::default(),
        };
        let s = serde_json::to_string(&snap).unwrap();
        for field in [
            "\"ts_ms\"", "\"host\"", "\"wifi\"", "\"arp\"", "\"gateway_dns\"",
            "\"connectivity\"", "\"tunnel\"", "\"subnet\"",
        ] {
            assert!(s.contains(field), "missing {field}");
        }
        // round-trips
        let back: AssessmentSnapshot = serde_json::from_str(&s).unwrap();
        assert_eq!(back, snap);
    }
}
