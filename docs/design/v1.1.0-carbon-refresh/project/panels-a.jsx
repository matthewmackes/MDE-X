/*
 * RETIRED v1.1.0 Carbon-refresh design mockup — historical only.
 *
 * Superseded by:
 *   v2.0.0 PatternFly reskin (later retired itself, 2026-05-18)
 *   v2.0.0 Mackes Desktop Environment (Iced + sway, locked 2026-05-19)
 *   ChromeOS Classic visual lock (locked 2026-05-24)
 *   Material Symbols icon set (locked 2026-05-25, Q43)
 *
 * Stale references in this file (DO NOT update inline — historical
 * mockup, preserved as-is for design archaeology):
 *
 *   * "16 peers" — superseded by Q3 of the 100-Q tightening survey
 *     2026-05-25; current cap is 8 peers. EPIC-MASTER-3 sweep
 *     (2026-05-26) closes by adding this banner per §0.13's
 *     quarterly-retirement-audit clause.
 *   * "Tailscale" / "Headscale" — superseded by Nebula mesh fabric
 *     (locked v2.5, see docs/design/v2.5-nebula-fabric.md).
 *   * "Carbon" icon refs — superseded by Material Symbols per
 *     Q43 + EPIC-UI-MATERIAL.
 *
 * Panels: Dashboard, Mesh VPN, Mesh Services
 */

// ============================================================
// Dashboard
// ============================================================
const DashboardPanel = ({ state, navigate, toast }) => {
  const services = window.SERVICES;
  const peers = window.PEERS;
  const onlinePeers = peers.filter(p => p.online).length;
  const okServices = services.filter(s => s.status === "ok").length;

  return (
    <div className="content-inner">
      <div className="breadcrumbs"><span>Mackes Shell</span><span className="sep">/</span><span>Dashboard</span></div>
      <h1 className="page-title">Dashboard</h1>
      <p className="page-subtitle">Live state of this machine and the mesh fabric it belongs to.</p>

      {/* Hero status strip */}
      <div className="grid-4">
        <div className="stat-tile accent-border">
          <div className="stat-label">Active preset</div>
          <div className="stat-value">{state.presetLabel}</div>
          <div className="stat-foot"><Dot /> {state.preset === "vanilla" ? "Fedora defaults preserved" : "Custom Mackes preset"}</div>
        </div>
        <div className="stat-tile">
          <div className="stat-label">Mesh peers</div>
          <div className="stat-value">{onlinePeers}<span className="muted" style={{ font: "300 16px/24px 'IBM Plex Sans'" }}> / {peers.length}</span></div>
          <div className="stat-foot"><Dot /> {peers.length - onlinePeers} offline · 16 max</div>
        </div>
        <div className="stat-tile">
          <div className="stat-label">Services discovered</div>
          <div className="stat-value">{okServices}</div>
          <div className="stat-foot"><Dot status="warn" /> 1 warning · Last scan 2m ago</div>
        </div>
        <div className="stat-tile">
          <div className="stat-label">Last snapshot</div>
          <div className="stat-value" style={{ font: "300 22px/40px 'IBM Plex Sans'" }}>before‑theme‑swap</div>
          <div className="stat-foot mono">2026‑05‑17 09:14</div>
        </div>
      </div>

      {/* Service health row */}
      <SectionH title="Service health" meta={<span className="mono">refreshed 4s ago</span>} />
      <div className="grid-3">
        {[
          { name: "xfce4-panel", status: "ok" },
          { name: "xfdesktop", status: "ok" },
          { name: "xfsettingsd", status: "ok" },
          { name: "xfconfd", status: "ok" },
          { name: "NetworkManager", status: "ok" },
          { name: "sshd", status: "ok" },
          { name: "tailscaled", status: "ok" },
          { name: "headscale", status: "ok" },
          { name: "qnmd (mesh-fs)", status: "warn" },
        ].map(s => (
          <Tile key={s.name}>
            <div className="row between">
              <div className="row" style={{ gap: 8 }}>
                <Dot status={s.status} />
                <span style={{ font: "var(--type-heading-01)" }}>{s.name}</span>
              </div>
              <span className="mono">{s.status === "ok" ? "active" : "degraded"}</span>
            </div>
            <div className="muted mono" style={{ font: "var(--type-mono-sm)" }}>pid {Math.floor(1000 + Math.random() * 9000)} · {Math.floor(2 + Math.random() * 60)}m</div>
          </Tile>
        ))}
      </div>

      {/* Drift warning */}
      <SectionH title="Configuration drift" meta={<Tag kind="warning">{window.DRIFT.length} differ</Tag>} />
      <Notif kind="warning" title={`${window.DRIFT.length} items differ from preset "${state.presetLabel}"`} icon="!">
        <div style={{ display: "flex", flexDirection: "column", gap: 4 }}>
          {window.DRIFT.map((d, i) => (
            <div key={i} className="mono">  •  <span style={{ color: "var(--text-primary)" }}>{d.section}.{d.field}</span>: preset={d.expected} <span style={{ color: "var(--support-warning)" }}>live={d.actual}</span></div>
          ))}
        </div>
        <div style={{ marginTop: 12, display: "flex", gap: 8 }}>
          <Btn kind="primary" size="sm" onClick={() => navigate("snapshots")}>Snapshot first</Btn>
          <Btn kind="tertiary" size="sm" onClick={() => navigate("reset")}>Open Maintain → Reset</Btn>
        </div>
      </Notif>

      {/* Hardware + Recent activity */}
      <div style={{ display: "grid", gridTemplateColumns: "1.2fr 1fr", gap: 16, marginTop: 32 }}>
        <div>
          <SectionH title="This machine" meta={<span className="mono">{window.HARDWARE.uptime} uptime</span>} />
          <Tile>
            {[
              ["Hostname", window.HARDWARE.hostname],
              ["OS", window.HARDWARE.os],
              ["CPU", window.HARDWARE.cpu],
              ["RAM", window.HARDWARE.ram],
              ["GPU", window.HARDWARE.gpu],
              ["Disk", window.HARDWARE.disk],
            ].map(([k, v]) => (
              <div key={k} style={{ display: "grid", gridTemplateColumns: "120px 1fr", padding: "6px 0", borderBottom: "1px solid var(--border-subtle-00)" }}>
                <span className="muted">{k}</span>
                <span>{v}</span>
              </div>
            ))}
          </Tile>
        </div>
        <div>
          <SectionH title="Recent activity" meta={<a>View log →</a>} />
          <Tile>
            {window.RECENT_ACTIVITY.map((a, i) => (
              <div key={i} style={{ display: "grid", gridTemplateColumns: "80px 1fr 100px", padding: "6px 0", borderBottom: i < 5 ? "1px solid var(--border-subtle-00)" : "0", alignItems: "center" }}>
                <span className="mono muted">{a.t}</span>
                <span>{a.what}</span>
                <span style={{ textAlign: "right" }}><Tag>{a.who}</Tag></span>
              </div>
            ))}
          </Tile>
        </div>
      </div>

      {/* Quick actions */}
      <SectionH title="Quick actions" />
      <div className="grid-3">
        {[
          { label: "Take snapshot", icon: "snapshot", action: () => { toast("Snapshot created: quick-snapshot", "success"); } },
          { label: "Open Appearance", icon: "paint", action: () => navigate("appearance") },
          { label: "Open Mesh VPN", icon: "mesh", action: () => navigate("mesh-vpn") },
          { label: "Health check", icon: "health", action: () => toast("Health check passed: 11/11 checks", "success") },
          { label: "View logs", icon: "log", action: () => navigate("logs") },
          { label: "Repair", icon: "wrench", action: () => navigate("repair") },
        ].map(a => (
          <Tile key={a.label} clickable onClick={a.action}>
            <div className="row" style={{ gap: 12 }}>
              <Icon name={a.icon} size={20} color="var(--accent)" />
              <span style={{ font: "var(--type-heading-02)" }}>{a.label}</span>
              <span style={{ marginLeft: "auto" }}><Icon name="right" color="var(--text-helper)" /></span>
            </div>
          </Tile>
        ))}
      </div>
    </div>
  );
};

// ============================================================
// Mesh VPN — with topology + table modes
// ============================================================
const MeshVpnPanel = ({ state, toast }) => {
  const [view, setView] = useState("topology"); // topology | table
  const [selected, setSelected] = useState("kiln");
  const [showAddPeer, setShowAddPeer] = useState(false);
  const peers = window.PEERS;
  const sel = peers.find(p => p.id === selected);

  return (
    <div className="content-inner">
      <div className="breadcrumbs"><span>Mackes Shell</span><span className="sep">/</span><span>Network</span><span className="sep">/</span><span>Mesh VPN</span></div>
      <h1 className="page-title">Mesh VPN</h1>
      <p className="page-subtitle">Self‑hosted Headscale + Tailscale clients route packets between peers regardless of physical network. Up to 16 peers per mesh.</p>

      {/* Status notif + actions */}
      <div className="row between" style={{ marginBottom: 16 }}>
        <div style={{ flex: 1 }}>
          <Notif kind="success" title={`Connected · ${peers.filter(p => p.online).length}/16 peers`} icon="✓">
            Control node: <span className="mono">kiln.mesh</span> · this peer (<span className="mono">anvil.mesh</span>) eligible for failover.
          </Notif>
        </div>
      </div>
      <div className="row" style={{ marginBottom: 24 }}>
        <Btn kind="primary" icon="plus" onClick={() => setShowAddPeer(true)}>Add peer</Btn>
        <Btn kind="tertiary" icon="settings" onClick={() => toast("Diagnostics ready")}>Diagnostics</Btn>
        <Btn kind="ghost" icon="refresh" onClick={() => toast("Refreshed mesh state")}>Refresh</Btn>
        <div style={{ marginLeft: "auto", display: "flex", border: "1px solid var(--border-subtle-01)" }}>
          <button className="btn ghost sm" data-active={view === "topology"} onClick={() => setView("topology")} style={{ borderRadius: 0, background: view === "topology" ? "var(--accent-soft)" : "transparent", color: view === "topology" ? "var(--accent)" : "var(--text-secondary)" }}>Topology</button>
          <button className="btn ghost sm" onClick={() => setView("table")} style={{ borderRadius: 0, background: view === "table" ? "var(--accent-soft)" : "transparent", color: view === "table" ? "var(--accent)" : "var(--text-secondary)" }}>Table</button>
        </div>
      </div>

      {view === "topology" ? (
        <div style={{ display: "grid", gridTemplateColumns: "1fr 340px", gap: 16 }}>
          <MeshTopology peers={peers} selected={selected} onSelect={setSelected} />
          <MeshPeerDetail peer={sel} toast={toast} />
        </div>
      ) : (
        <DataTable
          searchable
          columns={[
            { key: "name", title: "Hostname", render: r => <span><Dot status={r.online ? "ok" : "fail"} /> &nbsp;{r.name}{r.control && <Tag kind="accent" style={{ marginLeft: 8 }}>control</Tag>}</span> },
            { key: "ip", title: "Mesh IP", render: r => <span className="dt-mono">{r.ip}</span> },
            { key: "route", title: "Route", render: r => <span className="dt-mono">{r.route}</span> },
            { key: "rtt", title: "RTT", render: r => <span className="dt-mono">{r.online ? r.rtt + "ms" : "—"}</span> },
            { key: "lastSeen", title: "Last seen", render: r => <span className="dt-mono">{r.lastSeen}</span> },
            { key: "os", title: "OS / role" },
            { key: "status", title: "Status", render: r => <Tag kind={r.online ? "success" : "error"}>{r.online ? "online" : "offline"}</Tag> },
          ]}
          rows={peers}
          rowKey="id"
          selectedKey={selected}
          onRowClick={r => setSelected(r.id)}
        />
      )}

      <SectionH title="Control node" />
      <Tile>
        <div className="row between">
          <div>
            <div style={{ font: "var(--type-heading-02)" }}>kiln.mesh holds the control role</div>
            <div className="muted" style={{ marginTop: 4 }}>Snapshot age: 14s ago · Election quorum 4/6 reachable peers · This peer eligible for failover after 120s gap.</div>
          </div>
          <div className="row">
            <Tag kind="accent">Headscale</Tag>
            <Tag kind="info">WireGuard</Tag>
            <Tag kind="neutral">DERP fallback</Tag>
          </div>
        </div>
      </Tile>

      {showAddPeer && <AddPeerModal onClose={() => setShowAddPeer(false)} toast={toast} />}
    </div>
  );
};

const MeshTopology = ({ peers, selected, onSelect }) => {
  const w = 720, h = 460;
  const pad = 30;
  const pos = peers.map(p => ({ ...p, px: pad + p.x * (w - pad * 2), py: pad + p.y * (h - pad * 2) }));
  const control = pos.find(p => p.control);

  return (
    <div className="topo" style={{ height: h }}>
      <svg viewBox={`0 0 ${w} ${h}`} preserveAspectRatio="xMidYMid meet">
        <defs>
          <radialGradient id="ring" cx="50%" cy="50%" r="50%">
            <stop offset="0%" stopColor="var(--accent)" stopOpacity="0.15"/>
            <stop offset="60%" stopColor="var(--accent)" stopOpacity="0.04"/>
            <stop offset="100%" stopColor="var(--accent)" stopOpacity="0"/>
          </radialGradient>
        </defs>
        {/* faint grid */}
        {Array.from({ length: 11 }).map((_, i) => (
          <line key={"v"+i} x1={i*w/10} y1={0} x2={i*w/10} y2={h} stroke="var(--border-subtle-00)" strokeWidth="1" opacity="0.5" />
        ))}
        {Array.from({ length: 8 }).map((_, i) => (
          <line key={"h"+i} x1={0} y1={i*h/7} x2={w} y2={i*h/7} stroke="var(--border-subtle-00)" strokeWidth="1" opacity="0.5" />
        ))}
        {/* accent ring around control */}
        <circle cx={control.px} cy={control.py} r="170" fill="url(#ring)" />
        {/* edges */}
        {pos.filter(p => !p.control).map(p => (
          <line key={"e"+p.id} x1={control.px} y1={control.py} x2={p.px} y2={p.py}
            stroke={p.online ? "var(--accent)" : "var(--gray-70)"}
            strokeWidth={selected === p.id ? "2" : "1"}
            strokeOpacity={p.online ? (selected === p.id ? 0.9 : 0.45) : 0.25}
            strokeDasharray={p.route === "DERP" ? "4 4" : "0"} />
        ))}
        {/* pulses on online edges */}
        {pos.filter(p => p.online && !p.control).map((p, i) => (
          <circle key={"pulse"+p.id} r="3" fill="var(--accent)">
            <animateMotion dur={`${2 + i * 0.4}s`} repeatCount="indefinite" path={`M${control.px},${control.py} L${p.px},${p.py}`} />
            <animate attributeName="opacity" values="0;1;0" dur={`${2 + i * 0.4}s`} repeatCount="indefinite" />
          </circle>
        ))}
      </svg>
      {pos.map(p => (
        <div key={p.id}
          className="topo-peer-card"
          data-control={p.control}
          style={{ position: "absolute", left: `calc(${(p.px / w) * 100}% - 78px)`, top: `calc(${(p.py / h) * 100}% - 20px)`, borderColor: selected === p.id ? "var(--accent)" : undefined, background: selected === p.id ? "var(--accent-soft)" : undefined }}
          onClick={() => onSelect(p.id)}>
          <Dot status={p.online ? "ok" : "fail"} />
          <span className="tpc-name">{p.name.replace(".mesh", "")}</span>
          {p.control && <Tag kind="accent">CTL</Tag>}
        </div>
      ))}
    </div>
  );
};

const MeshPeerDetail = ({ peer, toast }) => (
  <Tile>
    <div className="row between">
      <div>
        <div className="muted" style={{ font: "var(--type-label-01)" }}>PEER</div>
        <div style={{ font: "var(--type-heading-03)" }}>{peer.name}</div>
      </div>
      <Tag kind={peer.online ? "success" : "error"}>{peer.online ? "online" : "offline"}</Tag>
    </div>
    <div className="divider" />
    {[
      ["Mesh IP", peer.ip, true],
      ["Route", peer.route, true],
      ["RTT", peer.online ? peer.rtt + " ms" : "—", true],
      ["Last seen", peer.lastSeen, false],
      ["OS", peer.os, false],
      ["Role", peer.role, false],
    ].map(([k, v, mono]) => (
      <div key={k} style={{ display: "grid", gridTemplateColumns: "92px 1fr", padding: "4px 0" }}>
        <span className="muted" style={{ font: "var(--type-label-01)" }}>{k}</span>
        <span className={mono ? "mono" : ""}>{v}</span>
      </div>
    ))}
    <div className="divider" />
    <div className="row" style={{ flexWrap: "wrap" }}>
      <Btn kind="primary" size="sm" icon="server" onClick={() => toast(`ssh ${peer.name} → opening`)}>Mesh SSH</Btn>
      <Btn kind="tertiary" size="sm" icon="folder" onClick={() => toast(`Mounted ~/QNM-Mesh/${peer.id}`)}>Browse files</Btn>
      <Btn kind="ghost" size="sm" icon="bell">Notify</Btn>
    </div>
  </Tile>
);

const AddPeerModal = ({ onClose, toast }) => {
  const link = "mesh-join://eyJtIjoia2lsbi5tZXNoIiwiayI6IjV1cGVyc2VjcmV0IiwiZSI6IjIwMjYtMDUtMTdUMTA6MjQ6MDBaIn0";
  const [copied, setCopied] = useState(false);
  return (
    <Modal title="Add peer" sub="Share this join link. Valid for 10 minutes." onClose={onClose}
      actions={
        <>
          <button className="btn ghost" onClick={onClose} style={{ flex: 1, height: 64, justifyContent: "flex-start", padding: "16px 16px" }}>Cancel</button>
          <button className="btn primary" onClick={() => { toast("Copied join link", "success"); setCopied(true); }} style={{ flex: 1, height: 64, justifyContent: "flex-start", padding: "16px 16px" }}>
            <Icon name="copy" />{copied ? "Copied!" : "Copy link"}
          </button>
        </>
      }>
      <Field label="Mesh ID" helper="Your control node holds the canonical roster.">
        <input className="input" value="anvil-home-mesh" readOnly />
      </Field>
      <Field label="Join link" helper="Paste into the joining peer's Mackes wizard, or scan as QR.">
        <input className="input mono" value={link} readOnly />
      </Field>
      <div className="row" style={{ marginTop: 16 }}>
        <Tag kind="info">expires 10m</Tag>
        <Tag>1 of 10 remaining slots</Tag>
        <Tag>WireGuard preshared</Tag>
      </div>
      <div className="code" style={{ marginTop: 16 }}>{`# Or run on the joining peer:\nmackes join '${link.substring(0, 48)}...'`}</div>
    </Modal>
  );
};

// ============================================================
// Mesh Services
// ============================================================
const MeshServicesPanel = ({ toast }) => {
  const [filter, setFilter] = useState("all");
  const [gatewayOn, setGatewayOn] = useState(true);
  const services = window.SERVICES;
  const filtered = filter === "all" ? services : services.filter(s => s.peer === filter);

  const kinds = [...new Set(services.map(s => s.kind))];
  const peers = window.PEERS;

  return (
    <div className="content-inner">
      <div className="breadcrumbs"><span>Mackes Shell</span><span className="sep">/</span><span>Network</span><span className="sep">/</span><span>Mesh Services</span></div>
      <h1 className="page-title">Mesh Services</h1>
      <p className="page-subtitle">Discover HTTP services across every mesh peer. Open in browser, launch native clients, or expose everything under a single https://media.mesh URL.</p>

      <div className="row" style={{ marginBottom: 16 }}>
        <Btn kind="primary" icon="refresh" onClick={() => toast("Probed 6 peers · found 12 services")}>Scan now</Btn>
        <Btn kind="ghost" icon="settings" onClick={() => toast("mDNS bridge config opened")}>mDNS bridge</Btn>
        <div style={{ marginLeft: "auto", color: "var(--text-helper)" }}>
          <span className="mono">{services.length} services on {new Set(services.map(s => s.peer)).size} peers</span>
        </div>
      </div>

      {/* Filter pills */}
      <div className="row" style={{ flexWrap: "wrap", marginBottom: 24 }}>
        <button className="tag" style={{ cursor: "pointer", background: filter === "all" ? "var(--accent)" : "var(--gray-80)", color: filter === "all" ? "var(--text-on-color)" : "var(--text-secondary)", height: 28, padding: "0 12px" }} onClick={() => setFilter("all")}>All peers</button>
        {peers.map(p => (
          <button key={p.id} className="tag" style={{ cursor: "pointer", background: filter === p.id ? "var(--accent)" : "var(--gray-80)", color: filter === p.id ? "var(--text-on-color)" : "var(--text-secondary)", height: 28, padding: "0 12px" }} onClick={() => setFilter(p.id)}>
            <Dot status={p.online ? "ok" : "fail"} /> &nbsp;{p.name.replace(".mesh", "")}
          </button>
        ))}
      </div>

      <SectionH title="Discovered services" meta={`${filtered.length} shown`} />
      <div className="grid-3">
        {filtered.map(s => (
          <Tile key={s.id} clickable onClick={() => toast(`Opening ${s.url}`)}>
            <div className="row between">
              <Tag>{s.kind}</Tag>
              <Dot status={s.status} />
            </div>
            <div style={{ font: "var(--type-heading-02)" }}>{s.name}</div>
            <div className="mono muted">on {s.peer}.mesh</div>
            <div className="mono" style={{ color: "var(--accent)", overflow: "hidden", textOverflow: "ellipsis" }}>{s.url}</div>
          </Tile>
        ))}
      </div>

      <SectionH title="Unified gateway" meta={<span className="mono">https://media.mesh</span>} />
      <Tile>
        <div className="row between">
          <div>
            <div style={{ font: "var(--type-heading-02)", marginBottom: 4 }}>Caddy reverse proxy</div>
            <div className="muted">Exposes every mesh service at <span className="mono">https://media.mesh/&lt;service&gt;/&lt;peer&gt;/</span> with auto‑renewed certs from a private CA installed into each peer's trust store.</div>
          </div>
          <div className="row" style={{ gap: 16 }}>
            <span className="muted">Gateway</span>
            <Toggle value={gatewayOn} onChange={v => { setGatewayOn(v); toast(v ? "Gateway enabled" : "Gateway disabled"); }} />
          </div>
        </div>
        {gatewayOn && (
          <div className="code">{`https://media.mesh/jellyfin/vault/    →  http://vault.mesh:8096
https://media.mesh/grafana/kiln/      →  http://kiln.mesh:3000
https://media.mesh/homeasst/kiln/     →  http://kiln.mesh:8123
... 9 more routes`}</div>
        )}
      </Tile>

      <SectionH title="mDNS bridge" meta="relay service announcements across the mesh" />
      <Tile>
        <div className="muted" style={{ marginBottom: 12 }}>Service types currently relayed:</div>
        <div className="row" style={{ flexWrap: "wrap" }}>
          {["_http._tcp", "_https._tcp", "_smb._tcp", "_ipp._tcp", "_airplay._tcp", "_chromecast._tcp", "_homekit._tcp"].map(t => (
            <Tag key={t} kind="info">{t}</Tag>
          ))}
        </div>
        <div className="muted mono" style={{ marginTop: 12, font: "var(--type-mono-sm)" }}>+ 5 private types kept local (_workstation._tcp, _ssh._tcp, _sftp-ssh._tcp, ...)</div>
      </Tile>
    </div>
  );
};

Object.assign(window, { DashboardPanel, MeshVpnPanel, MeshServicesPanel });
