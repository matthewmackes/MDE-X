//! EPIC-MESH-PROBE (MESH-PROBE-2) — the nmap probe engine.
//!
//! Per Q3/Q4, nmap is the sole probe engine. This module owns:
//!   * [`fast_argv`] / [`deep_argv`] — pure-fn `nmap` argv builders
//!     for the two-tier cadence (Q6): a fast liveness/known-port pass
//!     and a deep `-sV`/NSE identification pass. Both emit `-oX -`
//!     (XML to stdout) and are `-T`-rate-limited (never `-T5`).
//!   * [`parse_nmap_xml`] — roxmltree parse of `-oX` output into
//!     `mde_card` Host cards with Service children (Q7).
//!   * [`scan`] — shell `nmap`, parse the result; nmap-absent ⇒ empty
//!     + warn (no panic). The `Requires: nmap` RPM dep (MESH-PROBE-3)
//!     guarantees the binary in production; this graceful-degrade is
//!     for dev hosts / pre-install peers.
//!
//! The scheduled two-tier worker + GFS write + Bus `probe/changed`
//! event are MESH-PROBE-4; the operator-facing `mackesd probe scan`
//! CLI in `bin/mackesd.rs` is the runtime entry point that makes this
//! engine reachable end-to-end today (§0.12).

use std::process::Command;

use mde_card::probe::{host_card, service_card, HostFacts, HostSource, ServiceFacts};
use mde_card::Card;

/// Curated port set both profiles scan. Union of the media ports the
/// EPIC-SYNC-APP-CONFIG discovery needs (Airsonic 4040, Jellyfin
/// 8096, Navidrome 4533) + the MESH-A-7 well-known connect-action
/// ports (SSH/HTTP/HTTPS/SMB/RDP/VNC/FTP/CUPS/psql/mysql/redis/
/// HTTP-alt/mongo). Kept small so the fast pass stays ~sub-second
/// per host.
pub const CURATED_PORTS: &[u16] = &[
    21, 22, 80, 443, 445, 631, 3306, 3389, 4040, 4533, 5432, 5900, 6379, 8080, 8096, 27017,
];

/// nmap timing template. `-T3` ("normal", the nmap default) is the
/// polite choice — fast enough for an 8-peer mesh + a LAN segment
/// without the IDS-tripping aggression of `-T4`/`-T5`. The design
/// §7 risk note mandates "not `-T5`".
const TIMING: &str = "-T3";

/// nmap binary name (overridable in `scan` for tests).
pub const DEFAULT_NMAP_BINARY: &str = "nmap";

/// Which probe profile to run (Q6 two-tier cadence).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Fast liveness + curated-port open/closed pass (no `-sV`).
    Fast,
    /// Deep `-sV --version-all` + bundled-NSE identification pass.
    Deep,
}

/// Comma-joined curated port list for `-p`.
fn port_spec() -> String {
    CURATED_PORTS
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// Build the `nmap` argv (sans binary) for the fast profile over
/// `targets`. Pure — exposed for unit tests.
#[must_use]
pub fn fast_argv(targets: &[String]) -> Vec<String> {
    let mut argv = vec![
        TIMING.to_owned(),
        "-p".to_owned(),
        port_spec(),
        "--open".to_owned(),
        "-oX".to_owned(),
        "-".to_owned(),
    ];
    argv.extend(targets.iter().cloned());
    argv
}

/// Build the `nmap` argv (sans binary) for the deep profile over
/// `targets`. `nse_dir` is the bundled-NSE script path
/// (MESH-PROBE-3); when empty the `--script` flag is omitted so the
/// argv still runs against stock nmap. Pure — exposed for tests.
#[must_use]
pub fn deep_argv(targets: &[String], nse_dir: &str) -> Vec<String> {
    let mut argv = vec![
        TIMING.to_owned(),
        "-sV".to_owned(),
        "--version-all".to_owned(),
        "-p".to_owned(),
        port_spec(),
        "--open".to_owned(),
    ];
    if !nse_dir.is_empty() {
        argv.push("--script".to_owned());
        argv.push(nse_dir.to_owned());
    }
    argv.push("-oX".to_owned());
    argv.push("-".to_owned());
    argv.extend(targets.iter().cloned());
    argv
}

/// Parse nmap `-oX` XML into a Host card per up-host, each with a
/// Service child card per open port. `source` + `now_ts` are the
/// scan context the XML doesn't carry (the caller knows whether it
/// scanned a mesh peer / LAN / arbitrary target, and the wall clock).
/// Malformed XML ⇒ empty vec (logged by the caller). Hosts that are
/// not `up`, and ports that are not `open`, are skipped.
#[must_use]
pub fn parse_nmap_xml(xml: &str, source: HostSource, now_ts: u64) -> Vec<Card> {
    // Real nmap `-oX` output opens with `<!DOCTYPE nmaprun>`;
    // roxmltree rejects a DOCTYPE unless `allow_dtd` is set.
    let opts = roxmltree::ParsingOptions {
        allow_dtd: true,
        ..roxmltree::ParsingOptions::default()
    };
    let doc = match roxmltree::Document::parse_with_options(xml, opts) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for host in doc.descendants().filter(|n| n.has_tag_name("host")) {
        // Only `up` hosts.
        let up = host
            .children()
            .find(|n| n.has_tag_name("status"))
            .and_then(|s| s.attribute("state"))
            .is_some_and(|st| st == "up");
        if !up {
            continue;
        }
        // First IPv4 address.
        let Some(ip) = host
            .children()
            .filter(|n| n.has_tag_name("address"))
            .find(|n| n.attribute("addrtype") == Some("ipv4"))
            .and_then(|n| n.attribute("addr"))
        else {
            continue;
        };
        // Optional first hostname.
        let hostname = host
            .descendants()
            .find(|n| n.has_tag_name("hostname"))
            .and_then(|n| n.attribute("name"))
            .unwrap_or("")
            .to_owned();

        let mut services = Vec::new();
        for port in host.descendants().filter(|n| n.has_tag_name("port")) {
            let open = port
                .children()
                .find(|n| n.has_tag_name("state"))
                .and_then(|s| s.attribute("state"))
                .is_some_and(|st| st == "open");
            if !open {
                continue;
            }
            let Some(portid) = port
                .attribute("portid")
                .and_then(|p| p.parse::<u16>().ok())
            else {
                continue;
            };
            let svc = port.children().find(|n| n.has_tag_name("service"));
            let service_kind = svc
                .and_then(|s| s.attribute("name"))
                .unwrap_or("")
                .to_owned();
            let product = svc
                .and_then(|s| s.attribute("product"))
                .unwrap_or("")
                .to_owned();
            let version = svc
                .and_then(|s| s.attribute("version"))
                .unwrap_or("")
                .to_owned();
            services.push(service_card(
                &ServiceFacts {
                    port: portid,
                    service_kind,
                    product,
                    version,
                    fingerprint: String::new(),
                },
                now_ts,
            ));
        }

        out.push(host_card(
            &HostFacts {
                ip: ip.to_owned(),
                hostname,
                source,
                trust_state: String::new(),
                last_seen: now_ts,
            },
            services,
            now_ts,
        ));
    }
    out
}

/// Run an nmap `profile` against `targets` via `binary`, returning the
/// parsed inventory cards. Best-effort: a missing nmap binary, a
/// non-zero exit with no usable XML, or unparseable output all yield
/// an empty vec (logged at warn) rather than an error — the probe
/// must never crash the daemon. `nse_dir` is passed to the deep
/// profile only.
#[must_use]
pub fn scan(
    binary: &str,
    profile: Profile,
    targets: &[String],
    nse_dir: &str,
    source: HostSource,
    now_ts: u64,
) -> Vec<Card> {
    if targets.is_empty() {
        return Vec::new();
    }
    let argv = match profile {
        Profile::Fast => fast_argv(targets),
        Profile::Deep => deep_argv(targets, nse_dir),
    };
    let output = match Command::new(binary).args(&argv).output() {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!(
                target: "mackesd::probe_nmap",
                binary = %binary,
                error = %e,
                "could not spawn nmap (graceful-degrade; empty inventory). \
                 Install nmap (Requires: nmap in the RPM) to enable probing.",
            );
            return Vec::new();
        }
    };
    // nmap exits non-zero in some partial-scan cases but still emits
    // usable XML on stdout; parse whatever we got. Empty/garbage ⇒
    // parse returns empty.
    let xml = String::from_utf8_lossy(&output.stdout);
    let cards = parse_nmap_xml(&xml, source, now_ts);
    if cards.is_empty() && !output.status.success() {
        tracing::warn!(
            target: "mackesd::probe_nmap",
            code = ?output.status.code(),
            "nmap produced no parseable hosts (non-zero exit)",
        );
    }
    cards
}

#[cfg(test)]
mod tests {
    use super::*;
    use mde_card::probe::{host_facts, service_facts};
    use mde_card::CardKind;

    // A faithful `nmap -sV -oX -` sample: one up host (10.42.0.5,
    // peer-a.mesh.mde) with two open service ports (Airsonic 4040,
    // Jellyfin 8096) + one closed port that must be skipped, plus a
    // second host that is `down` and must be skipped entirely.
    const NMAP_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE nmaprun>
<nmaprun scanner="nmap" args="nmap -sV -oX - 10.42.0.5" start="1700000000" version="7.94" xmloutputversion="1.05">
<scaninfo type="syn" protocol="tcp" numservices="16"/>
<host starttime="1700000001" endtime="1700000010">
<status state="up" reason="echo-reply" reason_ttl="64"/>
<address addr="10.42.0.5" addrtype="ipv4"/>
<hostnames>
<hostname name="peer-a.mesh.mde" type="PTR"/>
</hostnames>
<ports>
<port protocol="tcp" portid="4040">
<state state="open" reason="syn-ack" reason_ttl="64"/>
<service name="http" product="Airsonic" version="11.1" method="probed" conf="10"/>
</port>
<port protocol="tcp" portid="8096">
<state state="open" reason="syn-ack" reason_ttl="64"/>
<service name="http" product="Jellyfin" version="10.9" method="probed" conf="10"/>
</port>
<port protocol="tcp" portid="22">
<state state="closed" reason="conn-refused" reason_ttl="64"/>
<service name="ssh" method="table" conf="3"/>
</port>
</ports>
</host>
<host starttime="1700000001" endtime="1700000010">
<status state="down" reason="no-response"/>
<address addr="10.42.0.99" addrtype="ipv4"/>
</host>
<runstats>
<finished time="1700000010" elapsed="9.0" exit="success"/>
<hosts up="1" down="1" total="2"/>
</runstats>
</nmaprun>"#;

    #[test]
    fn fast_argv_is_rate_limited_and_xml_stdout() {
        let argv = fast_argv(&["10.42.0.5".to_owned()]);
        assert!(argv.contains(&"-T3".to_owned()), "polite timing present");
        assert!(!argv.contains(&"-T5".to_owned()), "never -T5");
        assert!(!argv.contains(&"-T4".to_owned()), "not aggressive -T4");
        // -oX - => XML to stdout.
        let ox = argv.iter().position(|a| a == "-oX").expect("-oX present");
        assert_eq!(argv[ox + 1], "-");
        // No -sV in the fast pass.
        assert!(!argv.contains(&"-sV".to_owned()));
        // Target is last.
        assert_eq!(argv.last().unwrap(), "10.42.0.5");
    }

    #[test]
    fn deep_argv_has_version_detection_and_nse_when_dir_given() {
        let argv = deep_argv(&["10.42.0.5".to_owned()], "/usr/share/mde/nmap");
        assert!(argv.contains(&"-sV".to_owned()));
        assert!(argv.contains(&"--version-all".to_owned()));
        assert!(argv.contains(&"-T3".to_owned()));
        assert!(!argv.contains(&"-T5".to_owned()));
        let s = argv.iter().position(|a| a == "--script").expect("--script");
        assert_eq!(argv[s + 1], "/usr/share/mde/nmap");
    }

    #[test]
    fn deep_argv_omits_script_when_nse_dir_empty() {
        let argv = deep_argv(&["10.42.0.5".to_owned()], "");
        assert!(!argv.contains(&"--script".to_owned()));
        assert!(argv.contains(&"-sV".to_owned()));
    }

    #[test]
    fn port_spec_lists_curated_ports() {
        let spec = port_spec();
        assert!(spec.contains("4040")); // Airsonic
        assert!(spec.contains("8096")); // Jellyfin
        assert!(spec.contains("22")); // SSH
        assert!(spec.starts_with("21,"));
    }

    #[test]
    fn parse_extracts_up_host_with_open_services() {
        let cards = parse_nmap_xml(NMAP_XML, HostSource::Mesh, 1700);
        // Only the up host (down host skipped).
        assert_eq!(cards.len(), 1);
        let host = &cards[0];
        assert_eq!(host.kind, CardKind::Host);
        let hf = host_facts(host).expect("host facts");
        assert_eq!(hf.ip, "10.42.0.5");
        assert_eq!(hf.hostname, "peer-a.mesh.mde");
        assert_eq!(hf.source, HostSource::Mesh);
        assert_eq!(hf.last_seen, 1700);
    }

    #[test]
    fn parse_skips_closed_ports_keeps_open() {
        let cards = parse_nmap_xml(NMAP_XML, HostSource::Mesh, 1);
        let host = &cards[0];
        // 4040 + 8096 open; 22 closed → 2 services.
        assert_eq!(host.children.len(), 2);
        let ports: Vec<u16> = host
            .children
            .iter()
            .filter_map(|c| service_facts(c).map(|f| f.port))
            .collect();
        assert!(ports.contains(&4040));
        assert!(ports.contains(&8096));
        assert!(!ports.contains(&22), "closed port skipped");
    }

    #[test]
    fn parse_captures_service_product_and_version() {
        let cards = parse_nmap_xml(NMAP_XML, HostSource::Lan, 1);
        let svc = cards[0]
            .children
            .iter()
            .find_map(|c| service_facts(c).filter(|f| f.port == 8096))
            .expect("jellyfin service");
        assert_eq!(svc.service_kind, "http");
        assert_eq!(svc.product, "Jellyfin");
        assert_eq!(svc.version, "10.9");
    }

    #[test]
    fn parse_returns_empty_for_garbage() {
        assert!(parse_nmap_xml("not xml at all", HostSource::Mesh, 0).is_empty());
        assert!(parse_nmap_xml("", HostSource::Mesh, 0).is_empty());
    }

    #[test]
    fn scan_with_missing_binary_returns_empty() {
        // The graceful-degrade path: nmap not installed.
        let cards = scan(
            "/nonexistent/nmap-xyz",
            Profile::Fast,
            &["10.42.0.5".to_owned()],
            "",
            HostSource::Mesh,
            0,
        );
        assert!(cards.is_empty());
    }

    #[test]
    fn scan_with_empty_targets_is_noop() {
        let cards = scan(
            DEFAULT_NMAP_BINARY,
            Profile::Deep,
            &[],
            "",
            HostSource::Mesh,
            0,
        );
        assert!(cards.is_empty());
    }
}
