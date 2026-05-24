//! VV-1 / VV-1.5 / VV-2 (v4.1.0) — pure-function generator for
//! the `kamailio-mde` + `rtpengine-mde` configuration set.
//!
//! Per `docs/design/v4.1-voice-video.md` §11, every config the
//! operator's policy can drive is produced here as a pure
//! function: input → output, no I/O, fully snapshot-testable
//! with `insta`. Callers (the `mackesd voice render-config`
//! subcommand introduced in VV-1; the mackesd
//! `voice_supervisor` worker in VV-2) handle the actual writes.
//!
//! Status by task:
//!
//! * **VV-1 (this commit)** — minimal `VoiceDesired` (just this
//!   peer's identity + nebula bind facts) producing a config
//!   set Kamailio + `RTPengine` can boot from: SIP transports on
//!   loopback + the Nebula tun device, the basic module load
//!   list, a single `route()` block that answers OPTIONS and
//!   rejects everything else with `503` (so the daemon
//!   advertises a sensible "I am here but not yet routing")
//!   plus the `RTPengine` NG socket config. No mesh dial
//!   routing, no Vitelity yet.
//!
//! * **VV-2 (next commit)** — `VoiceDesired` expands to carry
//!   the peer roster (drives `dispatcher.list`) + Vitelity
//!   sub-account (drives `uacreg.list`) + the `route[MESH]` /
//!   `route[VITELITY_*]` blocks in `kamailio.cfg`. The
//!   `voice_supervisor` worker calls `generate()` on every
//!   policy reload and writes the result atomically.
//!
//! * **VV-3 / VV-4** — the `voice_mesh` + `voice_public`
//!   policy kinds drive the contents of `VoiceDesired` from
//!   the approved-policy snapshot in mackesd's store.
//!
//! Outputs are returned as owned `String`s rather than
//! borrowed slices so the caller can write each to disk under
//! any name without re-allocating.

#![cfg_attr(not(test), forbid(unsafe_code))]

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

/// Operator-visible identity + mesh-binding facts that drive
/// the minimal VV-1 configuration.
///
/// VV-2 expands this with peer rosters, Vitelity sub-account
/// credentials, and per-DID inbound rules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceDesired {
    /// This peer's stable identifier — used as the comment
    /// header on every generated file so the operator can
    /// tell at a glance which peer's config they're looking
    /// at.
    pub node_id: String,

    /// The Nebula tun-device name Kamailio + `RTPengine` should
    /// bind to (defaults to `nebula1` — Nebula's default tun
    /// device on Linux). Carved out so a future deployment
    /// that renames the tun (e.g. via `tun.dev: nebula0` in
    /// `/etc/nebula/config.yaml`) can override without a
    /// generator change.
    pub mesh_bind_device: String,

    /// The Nebula overlay IP this peer holds (e.g.
    /// `192.168.42.7`). Kamailio binds its TLS listener +
    /// `RTPengine` binds its RTP port range to this address on
    /// the mesh interface.
    pub mesh_bind_address: String,

    /// RTP port range for `RTPengine`, written into
    /// `rtpengine.conf` as `port-min` / `port-max`. Default:
    /// `30000..=40000` per design doc §5.2.
    pub rtp_port_min: u16,

    /// RTP port range upper bound (inclusive).
    pub rtp_port_max: u16,
}

impl VoiceDesired {
    /// A sensible default for boot — used by the systemd
    /// `ExecStartPre=` hook when mackesd's policy store is
    /// empty (first-boot, recovery, or single-peer dev rig).
    /// Produces a config Kamailio + `RTPengine` will accept and
    /// run; the daemons answer OPTIONS health checks and
    /// reject everything else with `503` until VV-2..VV-4 fill
    /// in the routing.
    #[must_use]
    pub fn boot_default(node_id: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            mesh_bind_device: "nebula1".to_string(),
            // `0.0.0.0` is a deliberate "bind everywhere"
            // fallback for the case where mackesd hasn't
            // observed the Nebula tun address yet. The systemd
            // unit additionally pins the daemon to the
            // `nebula1` device via `BindToDevice=`-equivalent
            // semantics, so this can never accidentally expose
            // a public listener.
            mesh_bind_address: "0.0.0.0".to_string(),
            rtp_port_min: 30_000,
            rtp_port_max: 40_000,
        }
    }
}

/// The four generated `kamailio-mde` + `rtpengine-mde` config
/// files. Field names match the on-disk filenames the systemd
/// units expect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigSet {
    /// `/etc/kamailio-mde/kamailio.cfg` — the procedural
    /// Kamailio config.
    pub kamailio_cfg: String,
    /// `/etc/kamailio-mde/dispatcher.list` — per-peer routing
    /// destinations consumed by Kamailio's `dispatcher`
    /// module. VV-1 ships an empty file; VV-2 fills it from
    /// the `voice_mesh` policy.
    pub dispatcher_list: String,
    /// `/etc/kamailio-mde/uacreg.list` — per-peer Vitelity
    /// sub-account binding consumed by Kamailio's `uac`
    /// module. VV-1 ships an empty file; VV-13/14 fill it
    /// from the `voice_public` policy.
    pub uacreg_list: String,
    /// `/etc/rtpengine-mde/rtpengine.conf` — `RTPengine` relay
    /// configuration.
    pub rtpengine_conf: String,
}

/// Render the four-file config set from `desired`.
///
/// Pure function: same input always yields exactly the same
/// byte output. Callers (mackesd's `render-config` CLI; the
/// `voice_supervisor` worker) handle the write-and-reload
/// step.
#[must_use]
pub fn generate(desired: &VoiceDesired) -> ConfigSet {
    ConfigSet {
        kamailio_cfg: render_kamailio_cfg(desired),
        dispatcher_list: render_dispatcher_list(desired),
        uacreg_list: render_uacreg_list(desired),
        rtpengine_conf: render_rtpengine_conf(desired),
    }
}

fn header_hash(file_name: &str, desired: &VoiceDesired) -> String {
    format!(
        "# Generated by mde-voice-config\n\
         # File: {file_name}\n\
         # Node: {node}\n\
         # Mesh bind: {addr} on {dev}\n\
         # Edit the voice_mesh / voice_public policies instead of this file.\n\n",
        file_name = file_name,
        node = desired.node_id,
        addr = desired.mesh_bind_address,
        dev = desired.mesh_bind_device,
    )
}

fn header_hashbang(file_name: &str, desired: &VoiceDesired) -> String {
    // Kamailio's cfg uses `#!KAMAILIO` as its magic preprocessor
    // marker on line 1 — the comment header has to come after
    // that, not before.
    format!(
        "#!KAMAILIO\n\
         #\n\
         # Generated by mde-voice-config\n\
         # File: {file_name}\n\
         # Node: {node}\n\
         # Mesh bind: {addr} on {dev}\n\
         # Edit the voice_mesh / voice_public policies instead of this file.\n\n",
        file_name = file_name,
        node = desired.node_id,
        addr = desired.mesh_bind_address,
        dev = desired.mesh_bind_device,
    )
}

#[allow(clippy::too_many_lines)] // Single procedural cfg template; splitting would obscure structure.
fn render_kamailio_cfg(desired: &VoiceDesired) -> String {
    let mut out = header_hashbang("kamailio.cfg", desired);

    out.push_str(
        "##### --- global parameters -----------------------------------\n\
         debug=2\n\
         log_stderror=no\n\
         log_facility=LOG_LOCAL0\n\
         server_header=\"Server: kamailio-mde\"\n\
         user_agent_header=\"User-Agent: kamailio-mde\"\n\
         memdbg=5\n\
         memlog=5\n\
         /* socket binding — loopback (UDP) for the embedded PJSIP\n\
          * client + Nebula (TLS) for inter-peer mesh signaling */\n\
         listen=udp:127.0.0.1:5060 advertise 127.0.0.1:5060\n",
    );
    let _ = writeln!(
        out,
        "listen=tls:{addr}:5061 advertise {addr}:5061",
        addr = desired.mesh_bind_address,
    );
    out.push_str(
        "tcp_connection_lifetime=3604\n\
         tcp_async=yes\n\
         tcp_max_connections=2048\n\
         /* mode-of-operation: stateful proxy + record-route */\n\
         children=4\n\n\
         ##### --- module loading --------------------------------------\n\
         loadmodule \"jsonrpcs.so\"\n\
         loadmodule \"kex.so\"\n\
         loadmodule \"corex.so\"\n\
         loadmodule \"tm.so\"\n\
         loadmodule \"tmx.so\"\n\
         loadmodule \"sl.so\"\n\
         loadmodule \"rr.so\"\n\
         loadmodule \"pv.so\"\n\
         loadmodule \"maxfwd.so\"\n\
         loadmodule \"usrloc.so\"\n\
         loadmodule \"registrar.so\"\n\
         loadmodule \"textops.so\"\n\
         loadmodule \"siputils.so\"\n\
         loadmodule \"xlog.so\"\n\
         loadmodule \"sanity.so\"\n\
         loadmodule \"ctl.so\"\n\
         loadmodule \"cfg_rpc.so\"\n\
         loadmodule \"counters.so\"\n\
         loadmodule \"dispatcher.so\"\n\
         loadmodule \"uac.so\"\n\
         loadmodule \"rtpengine.so\"\n\
         loadmodule \"htable.so\"\n\
         loadmodule \"presence.so\"\n\
         loadmodule \"presence_xml.so\"\n\
         loadmodule \"msilo.so\"\n\
         loadmodule \"acc.so\"\n\
         loadmodule \"auth.so\"\n\
         loadmodule \"path.so\"\n\
         loadmodule \"tls.so\"\n\n",
    );

    out.push_str(
        "##### --- module configuration --------------------------------\n\
         modparam(\"ctl\", \"binrpc\", \"unix:/var/run/kamailio-mde/kamcmd.sock\")\n\
         modparam(\"jsonrpcs\", \"transport\", 0)\n\
         modparam(\"jsonrpcs\", \"fifo_name\", \"/var/run/kamailio-mde/kamailio_rpc.fifo\")\n\
         modparam(\"usrloc\", \"db_mode\", 0)\n\
         modparam(\"registrar\", \"method_filtering\", 1)\n\
         modparam(\"registrar\", \"max_contacts\", 4)\n\
         modparam(\"dispatcher\", \"list_file\", \"/etc/kamailio-mde/dispatcher.list\")\n\
         modparam(\"dispatcher\", \"flags\", 2)\n\
         modparam(\"dispatcher\", \"force_dst\", 1)\n\
         modparam(\"uac\", \"reg_db_url\", \"text:///etc/kamailio-mde/uacreg.list\")\n\
         modparam(\"uac\", \"reg_contact_addr\", \"127.0.0.1:5060\")\n\
         modparam(\"uac\", \"reg_timer_interval\", 60)\n\
         modparam(\"rtpengine\", \"rtpengine_sock\", \"unix:/var/run/rtpengine-mde/ng.sock\")\n\
         modparam(\"rtpengine\", \"rtpengine_tout_ms\", 1000)\n\
         modparam(\"acc\", \"log_flag\", 1)\n\
         modparam(\"acc\", \"log_missed_flag\", 2)\n\
         modparam(\"tls\", \"config\", \"/etc/kamailio-mde/tls.cfg\")\n\n",
    );

    out.push_str(
        "##### --- request routing -------------------------------------\n\
         request_route {\n\
             /* sanity + max-forwards + record-route housekeeping */\n\
             if (!mf_process_maxfwd_header(\"10\")) {\n\
                 sl_send_reply(\"483\", \"Too Many Hops\");\n\
                 exit;\n\
             }\n\
             if (!sanity_check(\"17895\", \"7\")) {\n\
                 xlog(\"L_INFO\", \"sanity failed for $rm $ru\\n\");\n\
                 exit;\n\
             }\n\
             /* OPTIONS keepalive — VV-7a's Backend panel polls\n\
              * this to confirm the daemon is alive. */\n\
             if (is_method(\"OPTIONS\") && uri==myself) {\n\
                 sl_send_reply(\"200\", \"OK\");\n\
                 exit;\n\
             }\n\
             /* REGISTER from the embedded PJSIP client — VV-1\n\
              * stores the binding so the dialog plane can find\n\
              * the local endpoint. */\n\
             if (is_method(\"REGISTER\")) {\n\
                 if (!save(\"location\")) {\n\
                     sl_reply_error();\n\
                 }\n\
                 exit;\n\
             }\n\
             /* VV-2 inserts the mesh + Vitelity routes here.\n\
              * VV-1 ships a clean 503 so misdirected INVITEs\n\
              * fail fast rather than getting silently dropped. */\n\
             record_route();\n\
             sl_send_reply(\"503\", \"VV-1 baseline — mesh + Vitelity routes land in VV-2/VV-4/VV-14\");\n\
             exit;\n\
         }\n",
    );

    out
}

fn render_dispatcher_list(desired: &VoiceDesired) -> String {
    let mut out = header_hash("dispatcher.list", desired);
    out.push_str(
        "# format: setid destination [flags [priority [attrs [body]]]]\n\
         # VV-2 populates one row per remote peer's mde-local AOR\n\
         # from the voice_mesh policy. VV-1 ships the file empty\n\
         # so the dispatcher module loads cleanly.\n",
    );
    out
}

fn render_uacreg_list(desired: &VoiceDesired) -> String {
    let mut out = header_hash("uacreg.list", desired);
    out.push_str(
        "# format: l_uuid l_username l_domain r_username r_domain realm\n\
         #         auth_username auth_password auth_proxy expires flags reg_delay\n\
         # VV-13/14 populate this from the voice_public policy.\n\
         # VV-1 ships the file empty so the uac module loads\n\
         # cleanly with no outbound registrations.\n",
    );
    out
}

fn render_rtpengine_conf(desired: &VoiceDesired) -> String {
    let mut out = format!(
        "; Generated by mde-voice-config\n\
         ; File: rtpengine.conf\n\
         ; Node: {node}\n\
         ; Mesh bind: {addr} on {dev}\n\
         ; Edit the voice_mesh / voice_public policies instead of this file.\n\n\
         [rtpengine]\n",
        node = desired.node_id,
        addr = desired.mesh_bind_address,
        dev = desired.mesh_bind_device,
    );
    let _ = write!(
        out,
        "interface = lo/127.0.0.1;mesh/{addr}\n\
         listen-ng = /var/run/rtpengine-mde/ng.sock\n\
         port-min = {pmin}\n\
         port-max = {pmax}\n\
         log-level = 5\n\
         log-facility = local1\n\
         log-stderr = false\n\
         pidfile = /var/run/rtpengine-mde/rtpengine.pid\n\
         foreground = true\n\
         no-fallback = false\n\
         delete-delay = 30\n\
         timeout = 60\n\
         silent-timeout = 3600\n\
         ; No transcoding — operator-locked 2026-05-24. The\n\
         ; embedded PJSIP client negotiates PCMU when dialing\n\
         ; PSTN; mesh-to-mesh stays Opus end-to-end.\n\
         codec-strip-default = false\n",
        addr = desired.mesh_bind_address,
        pmin = desired.rtp_port_min,
        pmax = desired.rtp_port_max,
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_desired() -> VoiceDesired {
        VoiceDesired {
            node_id: "alice-laptop".to_string(),
            mesh_bind_device: "nebula1".to_string(),
            mesh_bind_address: "192.168.42.7".to_string(),
            rtp_port_min: 30_000,
            rtp_port_max: 40_000,
        }
    }

    #[test]
    fn generate_is_deterministic() {
        let a = generate(&fixture_desired());
        let b = generate(&fixture_desired());
        assert_eq!(a, b);
    }

    #[test]
    fn generate_emits_all_four_files() {
        let set = generate(&fixture_desired());
        assert!(!set.kamailio_cfg.is_empty());
        assert!(!set.dispatcher_list.is_empty());
        assert!(!set.uacreg_list.is_empty());
        assert!(!set.rtpengine_conf.is_empty());
    }

    #[test]
    fn kamailio_binds_to_the_mesh_address() {
        let set = generate(&fixture_desired());
        assert!(
            set.kamailio_cfg.contains("listen=tls:192.168.42.7:5061"),
            "kamailio.cfg should bind the mesh TLS transport to the supplied address\n\
             actual:\n{}",
            set.kamailio_cfg,
        );
    }

    #[test]
    fn kamailio_includes_loopback_for_embedded_pjsip() {
        let set = generate(&fixture_desired());
        assert!(set.kamailio_cfg.contains("listen=udp:127.0.0.1:5060"));
    }

    #[test]
    fn kamailio_options_route_responds_200_ok() {
        let set = generate(&fixture_desired());
        // VV-7a's Backend panel polls OPTIONS — must be answered
        // 200 OK, not handed to the catch-all 503.
        assert!(set.kamailio_cfg.contains("is_method(\"OPTIONS\")"));
        assert!(set.kamailio_cfg.contains("sl_send_reply(\"200\", \"OK\")"));
    }

    #[test]
    fn kamailio_register_saves_location() {
        let set = generate(&fixture_desired());
        assert!(set.kamailio_cfg.contains("is_method(\"REGISTER\")"));
        assert!(set.kamailio_cfg.contains("save(\"location\")"));
    }

    #[test]
    fn kamailio_includes_dispatcher_module_pointing_at_list_file() {
        let set = generate(&fixture_desired());
        assert!(set.kamailio_cfg.contains("loadmodule \"dispatcher.so\""));
        assert!(set
            .kamailio_cfg
            .contains("\"/etc/kamailio-mde/dispatcher.list\""));
    }

    #[test]
    fn kamailio_includes_uac_module_pointing_at_uacreg_file() {
        let set = generate(&fixture_desired());
        assert!(set.kamailio_cfg.contains("loadmodule \"uac.so\""));
        assert!(set.kamailio_cfg.contains("text:///etc/kamailio-mde/uacreg.list"));
    }

    #[test]
    fn kamailio_includes_rtpengine_ng_socket() {
        let set = generate(&fixture_desired());
        assert!(set.kamailio_cfg.contains("loadmodule \"rtpengine.so\""));
        assert!(set
            .kamailio_cfg
            .contains("unix:/var/run/rtpengine-mde/ng.sock"));
    }

    #[test]
    fn rtpengine_uses_supplied_port_range() {
        let set = generate(&fixture_desired());
        assert!(set.rtpengine_conf.contains("port-min = 30000"));
        assert!(set.rtpengine_conf.contains("port-max = 40000"));
    }

    #[test]
    fn rtpengine_binds_mesh_interface_to_supplied_address() {
        let set = generate(&fixture_desired());
        assert!(set.rtpengine_conf.contains("mesh/192.168.42.7"));
        assert!(set.rtpengine_conf.contains("lo/127.0.0.1"));
    }

    #[test]
    fn dispatcher_list_ships_as_empty_format_doc() {
        let set = generate(&fixture_desired());
        // No data rows yet; just the header + format note.
        assert!(set.dispatcher_list.contains("VV-2 populates"));
        assert!(!set
            .dispatcher_list
            .lines()
            .any(|l| !l.is_empty() && !l.starts_with('#')));
    }

    #[test]
    fn uacreg_list_ships_as_empty_format_doc() {
        let set = generate(&fixture_desired());
        assert!(set.uacreg_list.contains("VV-13/14 populate"));
        assert!(!set
            .uacreg_list
            .lines()
            .any(|l| !l.is_empty() && !l.starts_with('#')));
    }

    #[test]
    fn boot_default_emits_nebula1_and_zero_address() {
        let desired = VoiceDesired::boot_default("first-boot-node");
        let set = generate(&desired);
        assert!(set.kamailio_cfg.contains("listen=tls:0.0.0.0:5061"));
        assert!(set.rtpengine_conf.contains("mesh/0.0.0.0"));
    }

    #[test]
    fn header_carries_node_id() {
        let set = generate(&fixture_desired());
        for body in [
            &set.kamailio_cfg,
            &set.dispatcher_list,
            &set.uacreg_list,
            &set.rtpengine_conf,
        ] {
            assert!(
                body.contains("Node: alice-laptop"),
                "header should name the source node\n\nactual:\n{body}",
            );
        }
    }

    #[test]
    fn snapshot_default_config_set() {
        let set = generate(&fixture_desired());
        insta::assert_snapshot!("kamailio.cfg", set.kamailio_cfg);
        insta::assert_snapshot!("dispatcher.list", set.dispatcher_list);
        insta::assert_snapshot!("uacreg.list", set.uacreg_list);
        insta::assert_snapshot!("rtpengine.conf", set.rtpengine_conf);
    }
}
