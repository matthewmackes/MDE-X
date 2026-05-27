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
 * Panels: Appearance, Apps, Snapshots, Maintain, Help, Wizard
 */

// ============================================================
// Appearance (Look & Feel)
// ============================================================
const AppearancePanel = ({ state, setState, toast }) => {
  const [theme, setTheme] = useState("PadOS-Dark");
  const [icons, setIcons] = useState("Carbon");
  const [cursor, setCursor] = useState("Adwaita");
  const [cursorSize, setCursorSize] = useState(24);
  const [uiFont, setUiFont] = useState("IBM Plex Sans 10");
  const [monoFont, setMonoFont] = useState("IBM Plex Mono 10");
  const [aa, setAa] = useState(true);
  const [hinting, setHinting] = useState("slight");
  const [rgba, setRgba] = useState("rgb");
  const [dark, setDark] = useState(true);

  return (
    <div className="content-inner">
      <div className="breadcrumbs"><span>Mackes Shell</span><span className="sep">/</span><span>Look & Feel</span><span className="sep">/</span><span>Appearance</span></div>
      <h1 className="page-title">Appearance</h1>
      <p className="page-subtitle">Theme, icons, cursor, fonts, and wallpaper — all backed by xfconf. Changes apply immediately.</p>

      <div style={{ display: "grid", gridTemplateColumns: "1.2fr 1fr", gap: 32 }}>
        <div>
          <SectionH title="Theme" />
          <Field label="GTK theme" helper="Discovered in /usr/share/themes and ~/.themes.">
            <Select value={theme} onChange={v => { setTheme(v); toast(`Theme → ${v}`, "success"); }}
              options={["PadOS-Dark", "PadOS-Light", "Adwaita", "Adwaita-dark", "Arc-Dark", "Materia-dark", "Mackes-Warm"]} />
          </Field>
          <div className="row between" style={{ padding: "8px 0" }}>
            <div>
              <div className="form-label">Prefer dark variant</div>
              <div className="form-helper">/Settings/Gtk/ApplicationPreferDarkTheme</div>
            </div>
            <Toggle value={dark} onChange={setDark} />
          </div>

          <SectionH title="Icons & cursor" />
          <Field label="Icon theme">
            <Select value={icons} onChange={setIcons} options={["Carbon", "Adwaita", "Papirus-Dark", "Numix-Circle", "Tela-dark"]} />
          </Field>
          <div className="grid-2">
            <Field label="Cursor theme">
              <Select value={cursor} onChange={setCursor} options={["Adwaita", "Bibata-Modern-Classic", "DMZ-White", "Capitaine"]} />
            </Field>
            <Field label={`Cursor size · ${cursorSize}px`}>
              <input type="range" className="slider" min={16} max={64} step={4} value={cursorSize} onChange={e => setCursorSize(+e.target.value)} />
            </Field>
          </div>

          <SectionH title="Fonts" />
          <Field label="Interface"><Select value={uiFont} onChange={setUiFont} options={["IBM Plex Sans 10", "Droid Sans 10", "Cantarell 11", "Inter 10", "Noto Sans 10"]} /></Field>
          <Field label="Monospace"><Select value={monoFont} onChange={setMonoFont} options={["IBM Plex Mono 10", "JetBrains Mono 10", "Fira Code 10", "Hack 10", "Monospace 10"]} /></Field>

          <SectionH title="Font rendering" />
          <div className="row between" style={{ padding: "8px 0" }}>
            <span className="form-label">Antialiasing</span>
            <Toggle value={aa} onChange={setAa} />
          </div>
          <div className="grid-2">
            <Field label="Hinting"><Select value={hinting} onChange={setHinting} options={["none", "slight", "medium", "full"]} /></Field>
            <Field label="Sub‑pixel order"><Select value={rgba} onChange={setRgba} options={["none", "rgb", "bgr", "vrgb", "vbgr"]} /></Field>
          </div>

          <SectionH title="Wallpaper" />
          <Field label="Monitor"><Select value="HDMI-A-1" onChange={() => {}} options={["HDMI-A-1 (3840×2160)", "DP-1 (2560×1440)"]} /></Field>
          <div className="grid-3" style={{ marginTop: 8 }}>
            {[1,2,3,4,5,6].map(i => (
              <div key={i} style={{ aspectRatio: "16/10", background: `linear-gradient(${30 + i * 60}deg, var(--gray-90), var(--accent-soft), var(--gray-80))`, cursor: "pointer", border: i === 1 ? "2px solid var(--accent)" : "1px solid var(--border-subtle-00)" }} />
            ))}
          </div>
        </div>

        {/* Live preview */}
        <div>
          <SectionH title="Live preview" meta="updates as you change values" />
          <div style={{ background: "var(--gray-90)", border: "1px solid var(--border-subtle-01)", padding: 16 }}>
            <div className="row between" style={{ background: "var(--gray-100)", borderBottom: "1px solid var(--border-subtle-00)", padding: "8px 12px", margin: -16, marginBottom: 12 }}>
              <span className="mono muted">~/Documents</span>
              <div className="row" style={{ gap: 6 }}>
                <div style={{ width: 10, height: 10, background: "var(--gray-70)", borderRadius: "50%" }} />
                <div style={{ width: 10, height: 10, background: "var(--gray-70)", borderRadius: "50%" }} />
                <div style={{ width: 10, height: 10, background: "var(--accent)", borderRadius: "50%" }} />
              </div>
            </div>
            <div style={{ marginTop: 24 }}>
              <div style={{ font: uiFont.includes("Mono") ? "var(--type-mono)" : "var(--type-heading-03)" }}>The quick brown fox</div>
              <div style={{ font: "var(--type-body-02)", color: "var(--text-secondary)", marginTop: 4 }}>jumps over the lazy dog · 0123456789</div>
              <div style={{ font: "var(--type-mono)", color: "var(--text-helper)", marginTop: 16 }}>$ mackes preset apply mackes</div>
            </div>
            <div className="row" style={{ marginTop: 24 }}>
              <Btn kind="primary" size="sm">Primary</Btn>
              <Btn kind="tertiary" size="sm">Tertiary</Btn>
              <Btn kind="ghost" size="sm">Ghost</Btn>
            </div>
          </div>

          <SectionH title="Active accent" />
          <Tile>
            <div className="row">
              <div style={{ width: 56, height: 56, background: "var(--accent)" }} />
              <div>
                <div style={{ font: "var(--type-heading-02)" }}>{state.presetLabel}</div>
                <div className="muted mono">{(window.PRESETS.find(p => p.name === state.preset) || {}).accent}</div>
                <div className="muted" style={{ marginTop: 4 }}>From preset: {state.presetLabel}</div>
              </div>
            </div>
          </Tile>

          <SectionH title="Locked by design system" />
          <Notif kind="info" title="Carbon Design System locks">
            Q‑CB1 Gray 100 palette · Q‑CB3 IBM Plex typography · Q‑CB5 Carbon icons. Per‑preset accent replaces Carbon blue but everything else is fixed.
          </Notif>
        </div>
      </div>
    </div>
  );
};

// ============================================================
// Apps
// ============================================================
const AppsPanel = ({ state, toast }) => {
  const [tab, setTab] = useState("install");
  const [cat, setCat] = useState("all");
  const [q, setQ] = useState("");
  const cats = ["all", ...new Set(window.APPS_CATALOG.map(a => a.category))];
  let filtered = window.APPS_CATALOG;
  if (tab === "install") filtered = filtered.filter(a => !a.installed);
  if (tab === "installed") filtered = filtered.filter(a => a.installed);
  if (tab === "remove") filtered = filtered.filter(a => a.installed);
  if (cat !== "all") filtered = filtered.filter(a => a.category === cat);
  if (q) filtered = filtered.filter(a => (a.name + a.desc).toLowerCase().includes(q.toLowerCase()));

  return (
    <div className="content-inner">
      <div className="breadcrumbs"><span>Mackes Shell</span><span className="sep">/</span><span>Apps</span></div>
      <h1 className="page-title">Apps</h1>
      <p className="page-subtitle">Curated app catalog per preset. Install, remove, and inspect the {window.APPS_CATALOG.filter(a => a.installed).length} packages currently on this machine.</p>

      <div className="tabs">
        <div className="tab" data-active={tab === "install"} onClick={() => setTab("install")}>Install</div>
        <div className="tab" data-active={tab === "remove"} onClick={() => setTab("remove")}>Remove bloat</div>
        <div className="tab" data-active={tab === "installed"} onClick={() => setTab("installed")}>Installed ({window.APPS_CATALOG.filter(a => a.installed).length})</div>
      </div>

      <div className="row between" style={{ marginBottom: 24 }}>
        <div className="row" style={{ flexWrap: "wrap" }}>
          {cats.map(c => (
            <button key={c} className="tag" style={{ cursor: "pointer", background: cat === c ? "var(--accent)" : "var(--gray-80)", color: cat === c ? "var(--text-on-color)" : "var(--text-secondary)", height: 28, padding: "0 12px" }} onClick={() => setCat(c)}>
              {c === "all" ? "All categories" : c}
            </button>
          ))}
        </div>
        <div style={{ position: "relative", width: 280 }}>
          <input className="input" placeholder="Search apps…" value={q} onChange={e => setQ(e.target.value)} style={{ paddingLeft: 36 }} />
          <div style={{ position: "absolute", left: 12, top: 12, color: "var(--text-helper)" }}><Icon name="search" /></div>
        </div>
      </div>

      <div className="grid-3">
        {filtered.map(a => (
          <div key={a.id} className="app-card">
            <div className="row between">
              <div className="app-icon">{a.icon}</div>
              <div className="row">
                {a.preset.includes(state.preset) && <Tag kind="accent">Preset</Tag>}
                {a.installed && <Tag kind="success">Installed</Tag>}
              </div>
            </div>
            <div>
              <div className="app-name">{a.name}</div>
              <div className="app-desc">{a.desc}</div>
            </div>
            <div className="row between">
              <span className="app-meta">{a.category} · {a.size}</span>
              {tab === "install" ? (
                <Btn kind="tertiary" size="sm" icon="download" onClick={() => toast(`Installing ${a.name}…`, "success")}>Install</Btn>
              ) : tab === "remove" ? (
                <Btn kind="danger" size="sm" icon="trash" onClick={() => toast(`Removed ${a.name}`)}>Remove</Btn>
              ) : (
                <Btn kind="ghost" size="sm">Open</Btn>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
};

// ============================================================
// Snapshots
// ============================================================
const SnapshotsPanel = ({ toast }) => {
  const [label, setLabel] = useState("");
  const [snaps, setSnaps] = useState(window.SNAPSHOTS);
  const create = () => {
    const name = (label || "snapshot").trim();
    const now = new Date().toISOString().replace("T", " ").substring(0, 16);
    setSnaps([{ id: `s${Date.now()}`, name, created: now, preset: "Mackes", size: "184 KB" }, ...snaps]);
    setLabel("");
    toast(`Created snapshot: ${name}`, "success");
  };
  const remove = (id) => { setSnaps(snaps.filter(s => s.id !== id)); toast("Snapshot deleted"); };
  return (
    <div className="content-inner">
      <div className="breadcrumbs"><span>Mackes Shell</span><span className="sep">/</span><span>Maintain</span><span className="sep">/</span><span>Snapshots</span></div>
      <h1 className="page-title">Snapshots</h1>
      <p className="page-subtitle">Restore points capture your live config (xfconf + xfce4-panel + theme stack) into a timestamped directory. Take a snapshot before risky changes.</p>

      <SectionH title="Create" />
      <Tile>
        <div className="row">
          <input className="input" placeholder="Optional label — e.g. before-theme-swap" value={label} onChange={e => setLabel(e.target.value)} style={{ flex: 1 }} />
          <Btn kind="primary" icon="snapshot" onClick={create}>Create restore point</Btn>
        </div>
        <div className="muted">Captures xfconf channels · panel layout · theme stack · mesh state · ~/.config/mackes/ · ~/.local/share/mackes/</div>
      </Tile>

      <SectionH title="Existing" meta={`${snaps.length} snapshots · ${snaps.length * 184} KB total`} />
      <DataTable
        columns={[
          { key: "name", title: "Label", render: r => <span><Icon name="snapshot" /> &nbsp;{r.name}</span> },
          { key: "created", title: "Created", render: r => <span className="dt-mono">{r.created}</span> },
          { key: "preset", title: "From preset", render: r => <Tag kind="accent">{r.preset}</Tag> },
          { key: "size", title: "Size", render: r => <span className="dt-mono">{r.size}</span> },
          { key: "actions", title: "", width: 200, render: r => (
            <div className="row end">
              <Btn kind="ghost" size="sm" icon="reset" onClick={() => toast(`Restored ${r.name}`)}>Restore</Btn>
              <Btn kind="ghost" size="sm" icon="trash" onClick={() => remove(r.id)}>Delete</Btn>
            </div>
          ) },
        ]}
        rows={snaps}
        rowKey="id"
      />
    </div>
  );
};

// ============================================================
// Maintain hub (lists sub-screens)
// ============================================================
const MaintainPanel = ({ navigate }) => (
  <div className="content-inner">
    <div className="breadcrumbs"><span>Mackes Shell</span><span className="sep">/</span><span>Maintain</span></div>
    <h1 className="page-title">Maintain</h1>
    <p className="page-subtitle">Snapshots, drift, repair, logs, and reset paths. Mackes never auto‑modifies; every change runs from here.</p>

    <div className="grid-3">
      {[
        { id: "snapshots", title: "Snapshots", icon: "snapshot", desc: "Capture and restore xfconf/panel/theme state.", meta: "4 snapshots" },
        { id: "drift", title: "Drift", icon: "drift", desc: "Items diverging from the active preset.", meta: "3 differ", warn: true },
        { id: "system-update", title: "System update", icon: "download", desc: "dnf upgrade and Mackes self‑update.", meta: "0 pending" },
        { id: "fonts", title: "Fonts", icon: "image", desc: "Install IBM Plex, JetBrains Mono, Nerd Fonts.", meta: "12 installed" },
        { id: "power", title: "Power profiles", icon: "bolt", desc: "Balanced / Performance / Power‑saver.", meta: "Balanced" },
        { id: "resources", title: "Resources", icon: "server", desc: "CPU, RAM, GPU, IO live snapshot.", meta: "38% RAM" },
        { id: "health", title: "Health check", icon: "health", desc: "11 checks: services, mounts, mesh, RPM signature.", meta: "All passing" },
        { id: "dependencies", title: "Dependencies", icon: "cube", desc: "Verify Mackes RPM provides + recommends.", meta: "OK" },
        { id: "logs", title: "Logs", icon: "log", desc: "mackes.log, journalctl filtered.", meta: "1.4 MB" },
        { id: "repair", title: "Repair", icon: "wrench", desc: "Re‑bootstrap xfce4-panel from preset.", meta: "" },
        { id: "reset", title: "Reset to preset", icon: "reset", desc: "Wipe back to chosen preset's declared state.", meta: "" },
        { id: "uninstall", title: "Uninstall", icon: "trash", desc: "Complete removal + final snapshot tarball.", meta: "", danger: true },
      ].map(c => (
        <Tile key={c.id} clickable onClick={() => navigate(c.id)}>
          <div className="row between">
            <Icon name={c.icon} size={20} color={c.danger ? "var(--support-error)" : c.warn ? "var(--support-warning)" : "var(--accent)"} />
            {c.warn && <Tag kind="warning">{c.meta}</Tag>}
            {c.danger && <Tag kind="error">destructive</Tag>}
            {!c.warn && !c.danger && c.meta && <Tag>{c.meta}</Tag>}
          </div>
          <div style={{ font: "var(--type-heading-02)", marginTop: 4 }}>{c.title}</div>
          <div className="muted">{c.desc}</div>
        </Tile>
      ))}
    </div>
  </div>
);

// ============================================================
// Help — topic browser
// ============================================================
const HelpPanel = () => {
  const [active, setActive] = useState("getting-started");
  const groups = window.HELP_TOPICS.reduce((acc, t) => {
    acc[t.section] = acc[t.section] || [];
    acc[t.section].push(t);
    return acc;
  }, {});
  return (
    <div style={{ display: "grid", gridTemplateColumns: "260px 1fr", height: "100%" }}>
      <div style={{ borderRight: "1px solid var(--border-subtle-00)", background: "var(--layer-01)", overflowY: "auto", padding: "24px 0" }}>
        {Object.entries(groups).map(([sec, items]) => (
          <div key={sec} style={{ marginBottom: 16 }}>
            <div className="side-nav-group-title">{sec}</div>
            {items.map(t => (
              <button key={t.id} className="side-nav-item" data-active={active === t.id} onClick={() => setActive(t.id)} style={{ height: 32 }}>
                <span className="sn-label">{t.title}</span>
              </button>
            ))}
          </div>
        ))}
      </div>
      <div className="content-inner" style={{ padding: 32, maxWidth: 760 }}>
        <div className="breadcrumbs"><span>Help</span><span className="sep">/</span><span>{(window.HELP_TOPICS.find(t => t.id === active) || {}).title}</span></div>
        <HelpContent topic={active} />
      </div>
    </div>
  );
};

const HelpContent = ({ topic }) => {
  if (topic === "getting-started") return (
    <div className="md">
      <h1>Getting started</h1>
      <p>Mackes Shell replaces <code>xfce4-settings-manager</code> as your daily control panel and adds a mesh fabric across every machine you own. After the wizard finishes you can pick any of the eight tabs in the left rail.</p>
      <h2>Five-minute tour</h2>
      <ul>
        <li><strong>Dashboard</strong> — service health, drift, hardware, quick actions.</li>
        <li><strong>Look & Feel</strong> — theme, icons, fonts, wallpaper (all backed by xfconf).</li>
        <li><strong>Network → Mesh VPN</strong> — your peers, control node, join links.</li>
        <li><strong>Apps</strong> — install/remove from the per-preset catalog.</li>
        <li><strong>Maintain → Snapshots</strong> — restore points before risky changes.</li>
      </ul>
      <h2>Common first actions</h2>
      <div className="pre">{`mackes status              # show this node's state
mackes peers               # mesh roster
mackes snapshot create     # restore point before tinkering
mackes preset show mackes  # see what a preset declares`}</div>
    </div>
  );
  if (topic === "mesh-vpn") return (
    <div className="md">
      <h1>Mesh VPN</h1>
      <p>Mackes runs a self-hosted <code>Headscale</code> control plane with vanilla Tailscale clients. Up to 16 peers per mesh; cross-network discovery via the Tailscale-bootstrap rendezvous (only the seed peer is registered there).</p>
      <h2>Routes</h2>
      <ul>
        <li><strong>direct</strong> — WireGuard tunnel, both peers reachable.</li>
        <li><strong>DERP</strong> — fallback relay when NAT traversal fails. Slower; shown as a dashed edge in the topology view.</li>
      </ul>
      <h2>Control node</h2>
      <p>One peer holds the canonical roster. If it disappears for more than 120 seconds, the next eligible peer takes the role via Headscale election.</p>
    </div>
  );
  if (topic === "presets") return (
    <div className="md">
      <h1>Presets</h1>
      <p>A preset is a complete declared state — wallpaper, theme, panel layout, app set, mesh defaults. Switching presets is reversible (a pre‑switch snapshot is always created).</p>
      <ul>
        <li><code>#!</code> — CrunchBang reincarnation</li>
        <li><code>Mackes</code> — warm-dark house style (default)</li>
        <li><code>Daylight</code> — cool yellow accent</li>
        <li><code>Vanilla</code> — Fedora XFCE defaults</li>
        <li><code>Node</code> — headless mesh-only (auto-selected with no display)</li>
      </ul>
    </div>
  );
  return (
    <div className="md">
      <h1>{(window.HELP_TOPICS.find(t => t.id === topic) || {}).title}</h1>
      <p>Documentation for this topic is rendered from <code>docs/help/{topic}.md</code> in the Mackes Shell source repository. The same content is reachable headlessly via <code>mackes help {topic}</code>.</p>
      <p>This is a prototype preview — switch topics in the left rail to see Getting started, Mesh VPN, and Presets, which have full rendered content.</p>
    </div>
  );
};

// ============================================================
// Wizard (first-run)
// ============================================================
const Wizard = ({ onClose, state, setState, toast }) => {
  const [step, setStep] = useState(0);
  const [picked, setPicked] = useState(state.preset);
  const steps = ["Welcome", "Environment scan", "Hardware", "Pick preset", "Network", "Snapshot", "Review", "Apply"];
  const next = () => setStep(s => Math.min(s + 1, steps.length - 1));
  const back = () => setStep(s => Math.max(s - 1, 0));

  return (
    <div className="wizard">
      <div className="wizard-header">
        <div className="brand-logo" style={{ width: 32, height: 32 }} />
        <div>
          <div style={{ font: "var(--type-heading-02)" }}>Mackes Shell · First-run setup</div>
          <div className="muted mono">version 1.0.0 — "XFCE Provisioner"</div>
        </div>
        <div className="wizard-steps" style={{ marginLeft: "auto" }}>
          {steps.map((s, i) => (
            <div key={i} className="step" data-active={i === step}>
              <span className="num">{i + 1}</span>
              <span>{s}</span>
            </div>
          ))}
        </div>
      </div>

      <div className="wizard-body">
        {step === 0 && <WizardWelcome />}
        {step === 1 && <WizardEnvScan />}
        {step === 2 && <WizardHardware />}
        {step === 3 && <WizardPresetPick picked={picked} setPicked={setPicked} />}
        {step === 4 && <WizardNetwork />}
        {step === 5 && <WizardSnapshot />}
        {step === 6 && <WizardReview picked={picked} />}
        {step === 7 && <WizardApply onDone={() => { setState({ ...state, preset: picked, presetLabel: window.PRESETS.find(p => p.name === picked).display }); toast(`Preset "${picked}" applied`, "success"); onClose(); }} />}
      </div>

      <div className="wizard-foot">
        <Btn kind="ghost" onClick={onClose}>Skip wizard</Btn>
        <div className="row">
          {step > 0 && <Btn kind="tertiary" onClick={back}>Back</Btn>}
          {step < 7 && <Btn kind="primary" onClick={next}>{step === 6 ? "Apply" : "Continue"}</Btn>}
        </div>
      </div>
    </div>
  );
};

const WizardWelcome = () => (
  <div style={{ maxWidth: 720 }}>
    <div style={{ font: "300 48px/56px 'IBM Plex Sans'", letterSpacing: "-0.02em" }}>Welcome to <span style={{ color: "var(--accent)" }}>Mackes Shell</span>.</div>
    <p style={{ font: "var(--type-body-02)", color: "var(--text-secondary)", marginTop: 24, maxWidth: 60 + "ch" }}>
      A single control panel for XFCE on Fedora — plus a mesh fabric that connects every one of your machines.
      This wizard takes about two minutes and ends with a working setup. Everything is reversible via Snapshots.
    </p>
    <div className="grid-3" style={{ marginTop: 40 }}>
      {[
        { icon: "paint", title: "One panel", desc: "Replaces xfce4-settings-manager. Carbon UI." },
        { icon: "mesh", title: "Mesh fabric", desc: "Files, clipboard, SSH, notifications across 16 peers." },
        { icon: "snapshot", title: "Reversible", desc: "Every change can be rolled back from a snapshot." },
      ].map(c => (
        <Tile key={c.title} outlined>
          <Icon name={c.icon} size={24} color="var(--accent)" />
          <div style={{ font: "var(--type-heading-02)", marginTop: 8 }}>{c.title}</div>
          <div className="muted">{c.desc}</div>
        </Tile>
      ))}
    </div>
  </div>
);

const WizardEnvScan = () => (
  <div style={{ maxWidth: 720 }}>
    <h1 className="page-title" style={{ fontSize: 32 }}>Environment scan</h1>
    <p className="page-subtitle">Checking your system before any changes are made.</p>
    {[
      { label: "Fedora release", value: "41 (Workstation)", status: "ok" },
      { label: "XFCE", value: "4.20.0", status: "ok" },
      { label: "xfconf", value: "channels reachable", status: "ok" },
      { label: "Display server", value: "X11 (preferred)", status: "ok" },
      { label: "sudo / PolicyKit", value: "available", status: "ok" },
      { label: "OpenSSH server", value: "not yet enabled", status: "warn" },
      { label: "Network", value: "online (Ethernet, 1 Gbps)", status: "ok" },
      { label: "Free disk", value: "612 GiB on /home", status: "ok" },
    ].map(c => (
      <div key={c.label} style={{ display: "grid", gridTemplateColumns: "200px 1fr 32px", padding: "12px 0", borderBottom: "1px solid var(--border-subtle-00)", alignItems: "center" }}>
        <span className="muted">{c.label}</span>
        <span>{c.value}</span>
        <Dot status={c.status} />
      </div>
    ))}
  </div>
);

const WizardHardware = () => (
  <div style={{ maxWidth: 720 }}>
    <h1 className="page-title" style={{ fontSize: 32 }}>This machine</h1>
    <p className="page-subtitle">Detected hardware. Mackes adapts panel scaling and power profile defaults to your setup.</p>
    <Tile>
      {Object.entries({
        Hostname: "anvil",
        OS: "Fedora Linux 41 (Workstation Edition)",
        CPU: window.HARDWARE.cpu,
        RAM: window.HARDWARE.ram,
        GPU: window.HARDWARE.gpu,
        Disk: window.HARDWARE.disk,
        "Form factor": "desktop (no battery)",
      }).map(([k, v]) => (
        <div key={k} style={{ display: "grid", gridTemplateColumns: "140px 1fr", padding: "6px 0", borderBottom: "1px solid var(--border-subtle-00)" }}>
          <span className="muted">{k}</span>
          <span>{v}</span>
        </div>
      ))}
    </Tile>
  </div>
);

const WizardPresetPick = ({ picked, setPicked }) => (
  <div>
    <div style={{ textAlign: "center", marginBottom: 32 }}>
      <h1 className="page-title" style={{ fontSize: 36 }}>Pick a preset</h1>
      <p className="page-subtitle" style={{ margin: "0 auto" }}>Four moods. Click one. You can change later.</p>
    </div>
    <div className="preset-grid" style={{ maxWidth: 920, margin: "0 auto" }}>
      {window.PRESETS.map(p => (
        <div key={p.name} className="preset-card" data-selected={picked === p.name} onClick={() => setPicked(p.name)}>
          <div className="thumb" style={{ background: p.bgGradient }}>
            <div className="accent-stripe" style={{ background: p.accent }} />
            <div style={{ position: "absolute", inset: 0, display: "grid", placeItems: "center" }}>
              <div style={{ font: "300 56px/56px 'IBM Plex Sans'", color: p.accent, letterSpacing: "-0.02em" }}>{p.display}</div>
            </div>
          </div>
          <div className="body">
            <div className="row between">
              <div className="name">{p.display}</div>
              <div className="radio-circ" />
            </div>
            <div className="muted">{p.subtitle}</div>
            <div className="voice">{p.voice}</div>
          </div>
        </div>
      ))}
    </div>
  </div>
);

const WizardNetwork = () => (
  <div style={{ maxWidth: 720 }}>
    <h1 className="page-title" style={{ fontSize: 32 }}>Mesh network</h1>
    <p className="page-subtitle">Join an existing Mackes mesh, or create a new one with this machine as the control node.</p>
    <div className="grid-2" style={{ marginTop: 16 }}>
      <div className="radio-row" data-selected="true">
        <div className="radio-circ" />
        <div>
          <div style={{ font: "var(--type-heading-02)" }}>Create new mesh</div>
          <div className="muted">This machine becomes the control node. Mesh ID auto‑generated.</div>
        </div>
      </div>
      <div className="radio-row">
        <div className="radio-circ" />
        <div>
          <div style={{ font: "var(--type-heading-02)" }}>Join with link</div>
          <div className="muted">Paste a <span className="mono">mesh-join://</span> link from another peer.</div>
        </div>
      </div>
    </div>
    <Field label="OpenSSH server" helper="Enabled by default on first install. Required for Mesh SSH.">
      <div className="row"><Toggle value={true} onChange={() => {}} /> <span className="muted">Enable on boot</span></div>
    </Field>
  </div>
);

const WizardSnapshot = () => (
  <div style={{ maxWidth: 720 }}>
    <h1 className="page-title" style={{ fontSize: 32 }}>Take a baseline snapshot</h1>
    <p className="page-subtitle">Captures your current xfconf channels, panel layout, and theme stack so the wizard's changes are reversible.</p>
    <Tile>
      <Field label="Label"><input className="input" defaultValue="fresh-install" /></Field>
      <div className="row" style={{ marginTop: 8 }}>
        <Tag kind="success">~/.config/xfce4/</Tag>
        <Tag kind="success">xfconf channels</Tag>
        <Tag kind="success">~/.local/share/mackes/</Tag>
        <Tag>est. 184 KB</Tag>
      </div>
    </Tile>
  </div>
);

const WizardReview = ({ picked }) => {
  const p = window.PRESETS.find(x => x.name === picked);
  return (
    <div style={{ maxWidth: 720 }}>
      <h1 className="page-title" style={{ fontSize: 32 }}>Review</h1>
      <p className="page-subtitle">Everything Mackes is about to do. Nothing has changed yet.</p>
      <Tile>
        <div className="row between">
          <div>
            <div className="muted">Selected preset</div>
            <div style={{ font: "var(--type-heading-03)", marginTop: 4 }}>{p.display}</div>
          </div>
          <div style={{ width: 32, height: 32, background: p.accent }} />
        </div>
      </Tile>
      <div className="spacer-md" />
      {[
        "Create mesh \"anvil-home-mesh\" and bind this machine as control",
        `Apply preset "${p.display}" theme stack, panel layout, and app set`,
        "Enable OpenSSH server, sshd@boot",
        "Install LightDM greeter wallpaper",
        "Take snapshot \"fresh-install\"",
        "Refresh mDNS bridge defaults",
      ].map((s, i) => (
        <div key={i} className="row" style={{ padding: "8px 0", borderBottom: "1px solid var(--border-subtle-00)" }}>
          <Icon name="check" color="var(--support-success)" />
          <span>{s}</span>
        </div>
      ))}
    </div>
  );
};

const WizardApply = ({ onDone }) => {
  const [pct, setPct] = useState(0);
  const [log, setLog] = useState([]);
  const steps = [
    "Mounting xfconf channels…",
    "Writing /Net/ThemeName = PadOS-Dark",
    "Linking Carbon icon theme",
    "Bootstrapping xfce4-panel layout",
    "Setting wallpaper on monitor0",
    "Provisioning headscale control plane",
    "Bringing up tailscaled",
    "Registering peer anvil.mesh = 100.64.0.1",
    "Installing curated app set (12 packages)",
    "Snapshotting fresh-install → 184 KB",
    "All systems green.",
  ];
  useEffect(() => {
    let i = 0;
    const tick = () => {
      if (i >= steps.length) { setPct(100); setTimeout(onDone, 600); return; }
      setLog(l => [...l, steps[i]]);
      setPct(Math.round(((i + 1) / steps.length) * 100));
      i++;
      setTimeout(tick, 280);
    };
    tick();
  }, []);
  return (
    <div style={{ maxWidth: 720 }}>
      <h1 className="page-title" style={{ fontSize: 32 }}>Applying…</h1>
      <p className="page-subtitle">{pct}% complete</p>
      <div className="progress" style={{ marginBottom: 24 }}><div style={{ width: pct + "%" }} /></div>
      <div className="code" style={{ height: 280, overflowY: "auto" }}>
        {log.map((l, i) => <div key={i}>[{(new Date()).toLocaleTimeString()}] {l}</div>)}
      </div>
    </div>
  );
};

Object.assign(window, { AppearancePanel, AppsPanel, SnapshotsPanel, MaintainPanel, HelpPanel, Wizard });
