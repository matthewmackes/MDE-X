//! Rust Start menu popover — mirrors the Mackes Notification Drawer
//! shape natively in mackes-panel.
//!
//! Q5 lock (2026-05-19): left-clicking the Start (`M`) button opens
//! this popover instead of the legacy `gtk::Menu` apple menu. The
//! popover mirrors the drawer's section grammar:
//!
//! 1. **Quick Actions** — icon-only row of system actions with
//!    `FisherPrice` per-action colors (suggestion #1 lock — apple-menu
//!    actions live here now that the top bar is gone).
//! 2. **Toggles** — Mesh, Bluetooth, Do Not Disturb, Caffeine
//!    (binary on/off chips). Phase 4.4 lock (2026-05-19) wired the
//!    chips' click handlers to the actual system mutators (mirrors
//!    `mackes/drawer.py`'s `_mesh_toggle` / `_bluetooth_toggle` /
//!    `_dnd_toggle` / `_caffeine_toggle`) and added the Bluetooth
//!    chip per the v3.0.0 design Q33 lock.
//! 3. **Volume** — `pactl get-sink-volume` driven slider (live).
//! 4. **Brightness** — 7 discrete level buttons.
//! 5. **Footer** — link out to the full drawer (Super+M) for the
//!    Mesh / Fleet / Services / Notifications sections that haven't
//!    been ported to native Rust yet.
//!
//! Anchored bottom-left under the Start button via
//! `Popover::set_relative_to` + `Position::Top`.

use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;

use gtk::prelude::*;

/// One Quick-Action button.
struct ActionSpec {
    /// Carbon-symbolic icon name (falls back to the single-glyph
    /// label when the theme isn't installed).
    icon: &'static str,
    fallback: &'static str,
    label: &'static str,
    /// Color group — drives the `.mackes-action-<color>` CSS class so
    /// the button paints with a `FisherPrice` primary background (one
    /// per role) when hovered.
    color: &'static str,
    /// Shell command to execute on click.
    command: ActionCommand,
}

enum ActionCommand {
    /// Spawn `mackes <args>`.
    Mackes(&'static [&'static str]),
    /// Spawn `loginctl <verb>`.
    Loginctl(&'static str),
    /// Spawn arbitrary program + args.
    Spawn(&'static str, &'static [&'static str]),
    /// 1.1.0 (#25): open the Carbon-themed logout dialog (Sign Out /
    /// Restart / Shut Down). Replaces the prior path of spawning
    /// `xfce4-session-logout` directly from the Quick Actions row.
    LogoutDialog,
}

const QUICK_ACTIONS: &[ActionSpec] = &[
    ActionSpec {
        icon: "help-about-symbolic",
        fallback: "?",
        label: "About",
        color: "blue",
        command: ActionCommand::Mackes(&["--about"]),
    },
    ActionSpec {
        icon: "preferences-system-symbolic",
        fallback: "⚙",
        label: "Settings",
        color: "teal",
        command: ActionCommand::Mackes(&[]),
    },
    ActionSpec {
        icon: "system-software-update-symbolic",
        fallback: "↻",
        label: "Update",
        color: "green",
        command: ActionCommand::Spawn(
            "terminator",
            &["-x", "bash", "-c", "sudo dnf upgrade --refresh; bash"],
        ),
    },
    ActionSpec {
        icon: "applications-other-symbolic",
        fallback: "▦",
        label: "Apps",
        color: "orange",
        command: ActionCommand::Mackes(&["--focus", "apps"]),
    },
    ActionSpec {
        icon: "system-shutdown-symbolic",
        fallback: "⏻",
        label: "Sleep",
        color: "indigo",
        command: ActionCommand::Loginctl("suspend"),
    },
    ActionSpec {
        icon: "view-refresh-symbolic",
        fallback: "↺",
        label: "Restart",
        color: "cyan",
        command: ActionCommand::Loginctl("reboot"),
    },
    ActionSpec {
        icon: "system-shutdown-symbolic",
        fallback: "✕",
        label: "Shut Down",
        color: "maroon",
        command: ActionCommand::Loginctl("poweroff"),
    },
    ActionSpec {
        icon: "system-lock-screen-symbolic",
        fallback: "🔒",
        label: "Lock",
        color: "purple",
        command: ActionCommand::Loginctl("lock-session"),
    },
    ActionSpec {
        icon: "system-log-out-symbolic",
        fallback: "⏴",
        label: "Sign Out",
        color: "amber",
        // 1.1.0 (#25): route through the Carbon logout dialog rather
        // than xfce4-session-logout directly. The dialog itself
        // delegates to `xfce4-session-logout --logout --fast` so
        // user state still tears down via the XFCE session manager.
        command: ActionCommand::LogoutDialog,
    },
];

/// Build the popover anchored to `relative_to`. The popover dismisses
/// automatically on outside-click (GTK default) and re-shows the next
/// time the Start button is left-clicked.
#[must_use]
pub fn build(relative_to: &gtk::Widget) -> gtk::Popover {
    let popover = gtk::Popover::new(Some(relative_to));
    popover.set_widget_name("mackes-start-menu");
    popover.set_position(gtk::PositionType::Top);
    popover.set_modal(true);

    let column = gtk::Box::new(gtk::Orientation::Vertical, 12);
    column.set_widget_name("mackes-start-column");
    column.set_margin_top(12);
    column.set_margin_bottom(12);
    column.set_margin_start(14);
    column.set_margin_end(14);

    column.pack_start(&build_quick_actions_row(&popover), false, false, 0);
    column.pack_start(
        &gtk::Separator::new(gtk::Orientation::Horizontal),
        false,
        false,
        0,
    );
    column.pack_start(&build_toggles_row(), false, false, 0);
    column.pack_start(&build_volume_row(), false, false, 0);
    column.pack_start(&build_brightness_row(), false, false, 0);
    column.pack_start(
        &gtk::Separator::new(gtk::Orientation::Horizontal),
        false,
        false,
        0,
    );
    column.pack_start(&build_footer(&popover), false, false, 0);

    popover.add(&column);
    column.show_all();
    popover
}

fn build_quick_actions_row(popover: &gtk::Popover) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    row.set_widget_name("mackes-start-quick-actions");
    row.set_homogeneous(true);
    for action in QUICK_ACTIONS {
        row.pack_start(&build_quick_action(action, popover), false, false, 0);
    }
    row
}

fn build_quick_action(action: &'static ActionSpec, popover: &gtk::Popover) -> gtk::Button {
    let button = gtk::Button::new();
    button.set_widget_name(&format!(
        "mackes-action-{}",
        action.label.to_ascii_lowercase().replace(' ', "-")
    ));
    button
        .style_context()
        .add_class(&format!("mackes-action-{}", action.color));
    button.set_relief(gtk::ReliefStyle::None);
    button.set_focus_on_click(false);
    button.set_tooltip_text(Some(action.label));
    if let Some(atk) = button.accessible() {
        atk.set_name(&format!("Start-menu quick action: {}", action.label));
    }

    let cell = gtk::Box::new(gtk::Orientation::Vertical, 2);
    if let Some(pb) = crate::icons::load(action.icon, 22) {
        cell.pack_start(&gtk::Image::from_pixbuf(Some(&pb)), false, false, 0);
    } else {
        let glyph = gtk::Label::new(Some(action.fallback));
        glyph.style_context().add_class("mackes-action-glyph");
        cell.pack_start(&glyph, false, false, 0);
    }
    let label = gtk::Label::new(Some(action.label));
    label.style_context().add_class("mackes-action-label");
    cell.pack_start(&label, false, false, 0);
    button.add(&cell);

    let popover_for_handler = popover.clone();
    button.connect_clicked(move |_| {
        match &action.command {
            ActionCommand::Mackes(args) => {
                spawn("mackes", args);
            }
            ActionCommand::Loginctl(verb) => {
                spawn("loginctl", &[verb]);
            }
            ActionCommand::Spawn(bin, args) => {
                spawn(bin, args);
            }
            ActionCommand::LogoutDialog => {
                // E.25 — retire the inline GTK module + delegate to
                // the stand-alone Iced binary that v2.0.0 ships.
                spawn("mde-logout-dialog", &[]);
            }
        }
        popover_for_handler.popdown();
    });
    button
}

// ---------------------------------------------------------------------
// Quick Toggles (Phase 4.4)
//
// Each chip is bound to a `ToggleSpec` that pairs a *probe* (reads the
// underlying system state) with a *mutator* (flips that state). Both
// halves run through a `CommandRunner` trait so tests can inject a
// scripted runner instead of spawning real subprocesses.
//
// The production runner (`SystemRunner`) shells out to:
//
//   - Mesh:      `tailscale status` (probe), `tailscale up` /
//                `tailscale down` (mutate).
//   - Bluetooth: `bluetoothctl show` (probe), `bluetoothctl power
//                on|off` (mutate). Mirrors `_bluetooth_state` /
//                `_bluetooth_toggle` in `mackes/drawer.py`.
//   - DND:       `xfconf-query -c xfce4-notifyd -p /do-not-disturb`
//                (probe), `xfconf-query ... -n -t bool -s true|false`
//                (mutate). Mirrors `_dnd_state` / `_dnd_toggle`.
//   - Caffeine:  `xfconf-query -c xfce4-power-manager -p
//                /xfce4-power-manager/presentation-mode` (probe);
//                same path with `-n -t bool -s true|false` (mutate).
//                Mirrors `_caffeine_state` / `_caffeine_toggle`.
//
// On failure the chip reverts to its prior visual state and surfaces
// an error toast through `crate::toasts::show_error()`.
// ---------------------------------------------------------------------

/// One subprocess invocation: program + argv (sans program). Owned
/// `String`s so callers can build dynamic argv at runtime (e.g.
/// `xfconf-query ... -s true` vs `... -s false`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Cmd {
    program: String,
    args: Vec<String>,
}

impl Cmd {
    fn new(program: &str, args: &[&str]) -> Self {
        Self {
            program: program.to_string(),
            args: args.iter().map(|s| (*s).to_string()).collect(),
        }
    }
}

/// Outcome of running a command — exit success + captured stdout.
/// Stderr is intentionally dropped: the toggle code only inspects
/// stdout (for xfconf-query's `true`/`false` payload) and the exit
/// status (for `tailscale status`'s success-as-up signal).
#[derive(Debug, Clone, PartialEq, Eq)]
struct CmdResult {
    success: bool,
    stdout: String,
}

impl CmdResult {
    const fn failure() -> Self {
        Self {
            success: false,
            stdout: String::new(),
        }
    }
}

/// Test-injectable subprocess runner. Production code uses
/// `SystemRunner`; tests use `ScriptedRunner` from the `tests` module.
trait CommandRunner {
    /// Run a command synchronously and return success + stdout.
    /// Implementations must NOT panic on missing binaries — the toggle
    /// code already treats a `success=false` result as the fail path.
    fn run(&self, cmd: &Cmd) -> CmdResult;
}

/// Production runner — spawns real `std::process::Command` invocations.
struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, cmd: &Cmd) -> CmdResult {
        let result = Command::new(&cmd.program).args(&cmd.args).output();
        match result {
            Ok(o) => CmdResult {
                success: o.status.success(),
                stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            },
            Err(_) => CmdResult::failure(),
        }
    }
}

/// One quick-toggle spec: probe + mutator pair. The probe returns
/// `(state_known, currently_on)`; mutator takes the *current* state
/// and emits the command list to flip it. Mutator returns multiple
/// commands so e.g. xfconf-query can emit `-n -t bool -s ...` in one
/// shot.
struct ToggleSpec {
    label: &'static str,
    key: &'static str,
    /// Human description of what the toggle controls — first sentence
    /// of the tooltip.
    human_tooltip: &'static str,
    /// Command summary appended to the tooltip in parentheses, e.g.
    /// "(tailscale up/down)". Keeps the underlying mutator discoverable
    /// for power users + matches the Phase 4.4 acceptance criteria.
    command_tooltip: &'static str,
    probe: fn(&dyn CommandRunner) -> bool,
    /// Build the mutator command for the desired new state. Returns a
    /// Vec because xfconf-query uses a single multi-flag invocation
    /// but a real toggle could in theory chain commands.
    mutate: fn(new_on: bool) -> Vec<Cmd>,
}

/// `tailscale status` exit-0 means the mesh is up. Matches
/// `mackes/drawer.py::_read_mesh` semantics — we treat a successful
/// status command as "the daemon is happy and we're online."
fn probe_mesh(runner: &dyn CommandRunner) -> bool {
    runner.run(&Cmd::new("tailscale", &["status"])).success
}

fn mutate_mesh(new_on: bool) -> Vec<Cmd> {
    if new_on {
        // Drawer goes through `tailscale_up_via_headscale()` for the
        // mesh_perf flags. From the panel we don't have a Python
        // import; `tailscale up` (no args) preserves the user's last
        // login URL + tags, which is the closest we can get without
        // duplicating the Python helper into Rust. The full flag set
        // lives in Phase 4.3 (drawer port) — until then this matches
        // what the user gets by typing `tailscale up` in a terminal.
        vec![Cmd::new("tailscale", &["up"])]
    } else {
        vec![Cmd::new("tailscale", &["down"])]
    }
}

/// `bluetoothctl show` → look for `Powered: yes`. Mirrors
/// `_bluetooth_state` from the Python drawer.
fn probe_bluetooth(runner: &dyn CommandRunner) -> bool {
    let result = runner.run(&Cmd::new("bluetoothctl", &["show"]));
    if !result.success {
        return false;
    }
    for line in result.stdout.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("Powered:") {
            return rest.trim().eq_ignore_ascii_case("yes");
        }
    }
    false
}

fn mutate_bluetooth(new_on: bool) -> Vec<Cmd> {
    vec![Cmd::new(
        "bluetoothctl",
        &["power", if new_on { "on" } else { "off" }],
    )]
}

/// xfconf-query prints `true` / `false` on stdout. Matches the
/// `_dnd_state` semantics in the Python drawer.
fn probe_xfconf_bool(runner: &dyn CommandRunner, channel: &str, prop: &str) -> bool {
    let result = runner.run(&Cmd::new("xfconf-query", &["-c", channel, "-p", prop]));
    result.success && result.stdout.trim().eq_ignore_ascii_case("true")
}

fn mutate_xfconf_bool(channel: &str, prop: &str, new_on: bool) -> Vec<Cmd> {
    // `-n` creates the property if it doesn't exist (mirrors what the
    // Python drawer does — first-run DND toggle on a fresh user shouldn't
    // fail because the key isn't in xfconf yet).
    let value = if new_on { "true" } else { "false" };
    vec![Cmd::new(
        "xfconf-query",
        &["-c", channel, "-p", prop, "-n", "-t", "bool", "-s", value],
    )]
}

fn probe_dnd(runner: &dyn CommandRunner) -> bool {
    probe_xfconf_bool(runner, "xfce4-notifyd", "/do-not-disturb")
}

fn mutate_dnd(new_on: bool) -> Vec<Cmd> {
    mutate_xfconf_bool("xfce4-notifyd", "/do-not-disturb", new_on)
}

fn probe_caffeine(runner: &dyn CommandRunner) -> bool {
    probe_xfconf_bool(
        runner,
        "xfce4-power-manager",
        "/xfce4-power-manager/presentation-mode",
    )
}

fn mutate_caffeine(new_on: bool) -> Vec<Cmd> {
    mutate_xfconf_bool(
        "xfce4-power-manager",
        "/xfce4-power-manager/presentation-mode",
        new_on,
    )
}

const TOGGLE_SPECS: &[ToggleSpec] = &[
    ToggleSpec {
        label: "Mesh",
        key: "mesh",
        human_tooltip: "Mesh: connect this peer to the Mackes mesh fabric",
        command_tooltip: "tailscale up / tailscale down",
        probe: probe_mesh,
        mutate: mutate_mesh,
    },
    ToggleSpec {
        label: "Bluetooth",
        key: "bluetooth",
        human_tooltip: "Bluetooth: power the local adapter on or off",
        command_tooltip: "bluetoothctl power on|off",
        probe: probe_bluetooth,
        mutate: mutate_bluetooth,
    },
    ToggleSpec {
        label: "Do Not Disturb",
        key: "dnd",
        human_tooltip: "Do Not Disturb: silence xfce4-notifyd notifications",
        command_tooltip: "xfconf-query -c xfce4-notifyd -p /do-not-disturb",
        probe: probe_dnd,
        mutate: mutate_dnd,
    },
    ToggleSpec {
        label: "Caffeine",
        key: "caffeine",
        human_tooltip: "Caffeine: keep the screen awake (presentation mode)",
        command_tooltip:
            "xfconf-query -c xfce4-power-manager -p /xfce4-power-manager/presentation-mode",
        probe: probe_caffeine,
        mutate: mutate_caffeine,
    },
];

/// Compose the full tooltip string for a chip: "<human> — currently
/// <state>. <command summary>". Pulled out so tests can assert on
/// the exact wording without instantiating GTK widgets.
fn tooltip_for(spec: &ToggleSpec, on: bool) -> String {
    format!(
        "{human} — currently {state}. ({cmd})",
        human = spec.human_tooltip,
        state = if on { "on" } else { "off" },
        cmd = spec.command_tooltip,
    )
}

/// Compose the accessibility name: short, screen-reader-friendly.
fn ax_name_for(spec: &ToggleSpec, on: bool) -> String {
    format!(
        "{label} toggle, currently {state}",
        label = spec.label,
        state = if on { "on" } else { "off" },
    )
}

fn build_toggles_row() -> gtk::Box {
    let runner: Rc<dyn CommandRunner> = Rc::new(SystemRunner);
    build_toggles_row_with_runner(&runner)
}

fn build_toggles_row_with_runner(runner: &Rc<dyn CommandRunner>) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.set_widget_name("mackes-start-toggles");
    for spec in TOGGLE_SPECS {
        row.pack_start(&toggle_chip(spec, runner.clone()), false, false, 0);
    }
    row
}

/// Run a toggle's mutators in order and report success. Production
/// uses `SystemRunner`; tests inject a scripted runner.
///
/// Visible-state semantics: on success we return the new `on` state.
/// On failure we return the prior state so the caller can leave the
/// chip's visual state untouched.
fn run_toggle(
    spec: &ToggleSpec,
    prior_on: bool,
    runner: &dyn CommandRunner,
) -> Result<bool, String> {
    let new_on = !prior_on;
    for cmd in (spec.mutate)(new_on) {
        let result = runner.run(&cmd);
        if !result.success {
            return Err(format!(
                "{label}: command `{program}` failed",
                label = spec.label,
                program = cmd.program,
            ));
        }
    }
    Ok(new_on)
}

// `runner` is taken by-value here even though clippy's pedantic lint
// argues for `&Rc<…>`: we clone() it into the click closure, the
// original Rc is dropped at function exit, and accepting it by-value
// keeps the call-site (`toggle_chip(spec, runner.clone())`) tidy.
#[allow(clippy::needless_pass_by_value)]
fn toggle_chip(spec: &'static ToggleSpec, runner: Rc<dyn CommandRunner>) -> gtk::Button {
    let on = (spec.probe)(runner.as_ref());

    let chip = gtk::Button::with_label(spec.label);
    chip.set_widget_name(&format!("mackes-toggle-{key}", key = spec.key));
    chip.style_context().add_class("mackes-start-toggle");
    if on {
        chip.style_context().add_class("on");
    }
    chip.set_relief(gtk::ReliefStyle::None);
    chip.set_tooltip_text(Some(&tooltip_for(spec, on)));
    if let Some(atk) = chip.accessible() {
        atk.set_name(&ax_name_for(spec, on));
    }

    // RefCell wraps the chip's perceived state so the click handler
    // can flip it, persist on success, or revert on failure. We can't
    // re-probe synchronously on every click — `tailscale up` returns
    // after several seconds — so we model "perceived state" locally
    // and rely on the next popover-open to re-probe.
    let perceived = Rc::new(RefCell::new(on));
    let chip_for_handler = chip.clone();
    let runner_for_handler = runner.clone();
    chip.connect_clicked(move |_| {
        let prior = *perceived.borrow();
        match run_toggle(spec, prior, runner_for_handler.as_ref()) {
            Ok(new_on) => {
                *perceived.borrow_mut() = new_on;
                let style = chip_for_handler.style_context();
                if new_on {
                    style.add_class("on");
                } else {
                    style.remove_class("on");
                }
                chip_for_handler.set_tooltip_text(Some(&tooltip_for(spec, new_on)));
                if let Some(atk) = chip_for_handler.accessible() {
                    atk.set_name(&ax_name_for(spec, new_on));
                }
                crate::toasts::show(&format!(
                    "{label} {state}",
                    label = spec.label,
                    state = if new_on { "enabled" } else { "disabled" },
                ));
            }
            Err(msg) => {
                // Hold the prior visual state — no add/remove on the
                // CSS class, no tooltip rewrite. The toast surfaces
                // the failure so the user sees why the click didn't
                // appear to do anything.
                crate::toasts::show_error(&msg);
            }
        }
    });

    chip
}

fn build_volume_row() -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.set_widget_name("mackes-start-volume");
    let label = gtk::Label::new(Some("Volume"));
    label.set_halign(gtk::Align::Start);
    label.set_width_chars(10);
    row.pack_start(&label, false, false, 0);

    let scale = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    scale.set_widget_name("mackes-volume-slider");
    scale.set_draw_value(false);
    scale.set_hexpand(true);
    scale.set_value(f64::from(read_volume_percent()));

    scale.connect_value_changed(|s| {
        // Scale value is 0..=100 in 1.0 steps. Float → u32 via a
        // safe clamp + `as` chain to avoid clippy's float-truncation
        // pedantic warning.
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let pct = s.value().round().clamp(0.0, 100.0) as u32;
        let arg = format!("{pct}%");
        let _ = Command::new("pactl")
            .args(["set-sink-volume", "@DEFAULT_SINK@", &arg])
            .spawn();
    });
    row.pack_start(&scale, true, true, 0);
    row
}

fn read_volume_percent() -> u32 {
    let out = Command::new("pactl")
        .args(["get-sink-volume", "@DEFAULT_SINK@"])
        .output();
    let Ok(o) = out else {
        return 0;
    };
    if !o.status.success() {
        return 0;
    }
    let s = String::from_utf8_lossy(&o.stdout);
    let mut digits = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else if ch == '%' && !digits.is_empty() {
            return digits.parse().unwrap_or(0);
        } else {
            digits.clear();
        }
    }
    0
}

/// 7 discrete brightness levels per Q-lock — 15% / 30% / 45% / 60% /
/// 75% / 90% / 100%. Defined at module scope so clippy doesn't ding
/// the items-after-statements pedantic lint.
const BRIGHTNESS_LEVELS: &[u32] = &[15, 30, 45, 60, 75, 90, 100];

fn build_brightness_row() -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.set_widget_name("mackes-start-brightness");
    let label = gtk::Label::new(Some("Brightness"));
    label.set_halign(gtk::Align::Start);
    label.set_width_chars(10);
    row.pack_start(&label, false, false, 0);

    let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    buttons
        .style_context()
        .add_class("mackes-brightness-buttons");
    for &pct in BRIGHTNESS_LEVELS {
        let b = gtk::Button::with_label(&format!("{pct}"));
        b.set_widget_name(&format!("mackes-brightness-{pct}"));
        b.style_context().add_class("mackes-brightness-step");
        b.set_relief(gtk::ReliefStyle::None);
        b.set_tooltip_text(Some(&format!("Set brightness to {pct}%")));
        if let Some(atk) = b.accessible() {
            atk.set_name(&format!("Set screen brightness to {pct} percent"));
        }
        b.connect_clicked(move |_| {
            let arg = format!("{pct}%");
            // brightnessctl is the most-common Fedora helper; fall back
            // to xrandr on systems without backlight ACPI.
            if Command::new("brightnessctl")
                .args(["set", &arg])
                .spawn()
                .is_err()
            {
                let _ = Command::new("xrandr")
                    .args([
                        "--output",
                        "eDP-1",
                        "--brightness",
                        &format!("{:.2}", f64::from(pct) / 100.0),
                    ])
                    .spawn();
            }
        });
        buttons.pack_start(&b, true, true, 0);
    }
    row.pack_start(&buttons, true, true, 0);
    row
}

fn build_footer(popover: &gtk::Popover) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    row.set_widget_name("mackes-start-footer");
    let link = gtk::Button::with_label("Open notification drawer (Super+M)");
    link.set_widget_name("mackes-start-drawer-link");
    link.style_context().add_class("mackes-start-footer-link");
    link.set_relief(gtk::ReliefStyle::None);
    if let Some(atk) = link.accessible() {
        atk.set_name("Open the notification drawer (Super+M)");
    }
    let popover_for_handler = popover.clone();
    link.connect_clicked(move |_| {
        spawn("mackes", &["--drawer"]);
        popover_for_handler.popdown();
    });
    row.pack_start(&link, true, true, 0);
    row
}

fn spawn(bin: &str, args: &[&str]) {
    if let Err(e) = Command::new(bin).args(args).spawn() {
        eprintln!("mackes-panel: start-menu spawn {bin} failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_actions_carry_nine_entries() {
        // Q5 + suggestion #1 lock — Quick Actions row is the apple
        // menu's old home. About / Settings / Update / Apps / Sleep /
        // Restart / Shut Down / Lock / Sign Out = 9 entries.
        assert_eq!(QUICK_ACTIONS.len(), 9);
    }

    #[test]
    fn every_action_has_a_color_and_label() {
        for a in QUICK_ACTIONS {
            assert!(!a.color.is_empty(), "{} has no color", a.label);
            assert!(!a.label.is_empty());
            assert!(!a.fallback.is_empty());
        }
    }

    #[test]
    fn no_two_actions_share_a_label() {
        let mut labels: Vec<&str> = QUICK_ACTIONS.iter().map(|a| a.label).collect();
        labels.sort_unstable();
        let pre_dedup = labels.len();
        labels.dedup();
        assert_eq!(pre_dedup, labels.len(), "duplicate labels in QUICK_ACTIONS");
    }

    #[test]
    fn brightness_has_seven_levels() {
        // Q-lock requires 7 discrete brightness levels. Source of
        // truth is the module-scope BRIGHTNESS_LEVELS constant —
        // assert directly on it so a future commit that adds or
        // removes a level breaks this test.
        assert_eq!(BRIGHTNESS_LEVELS.len(), 7);
    }

    #[test]
    fn brightness_levels_span_15_to_100_inclusive() {
        assert_eq!(BRIGHTNESS_LEVELS.first(), Some(&15));
        assert_eq!(BRIGHTNESS_LEVELS.last(), Some(&100));
    }

    #[test]
    fn brightness_levels_are_strictly_ascending() {
        for win in BRIGHTNESS_LEVELS.windows(2) {
            assert!(win[0] < win[1], "{} not < {}", win[0], win[1]);
        }
    }

    #[test]
    fn brightness_levels_are_within_percent_range() {
        for &pct in BRIGHTNESS_LEVELS {
            assert!(pct <= 100);
        }
    }

    #[test]
    fn every_action_carries_a_carbon_symbolic_icon() {
        // Per Q14 / Q5: every Quick Action ships a Carbon-symbolic.
        for a in QUICK_ACTIONS {
            assert!(
                a.icon.ends_with("-symbolic"),
                "icon for {} is not symbolic: {}",
                a.label,
                a.icon
            );
        }
    }

    #[test]
    fn sleep_restart_shutdown_use_loginctl_variants() {
        for label in ["Sleep", "Restart", "Shut Down", "Lock"] {
            let a = QUICK_ACTIONS
                .iter()
                .find(|a| a.label == label)
                .unwrap_or_else(|| panic!("Quick Actions missing {label}"));
            assert!(
                matches!(a.command, ActionCommand::Loginctl(_)),
                "{label} should route through loginctl"
            );
        }
    }

    #[test]
    fn sign_out_routes_through_logout_dialog() {
        let a = QUICK_ACTIONS
            .iter()
            .find(|a| a.label == "Sign Out")
            .expect("Sign Out present");
        assert!(matches!(a.command, ActionCommand::LogoutDialog));
    }

    #[test]
    fn about_action_invokes_mackes_with_about_flag() {
        let a = QUICK_ACTIONS
            .iter()
            .find(|a| a.label == "About")
            .expect("About present");
        if let ActionCommand::Mackes(args) = &a.command {
            assert_eq!(*args, &["--about"]);
        } else {
            panic!("About should be Mackes(--about)");
        }
    }

    #[test]
    fn update_action_spawns_terminator() {
        let a = QUICK_ACTIONS
            .iter()
            .find(|a| a.label == "Update")
            .expect("Update present");
        if let ActionCommand::Spawn(bin, _args) = &a.command {
            assert_eq!(*bin, "terminator");
        } else {
            panic!("Update should be Spawn(terminator, ...)");
        }
    }

    #[test]
    fn no_two_actions_share_a_color() {
        // FisherPrice palette: each Quick Action carries a unique
        // color tag so the hover/CSS classes don't collide.
        let mut colors: Vec<&str> = QUICK_ACTIONS.iter().map(|a| a.color).collect();
        colors.sort_unstable();
        let pre = colors.len();
        colors.dedup();
        assert_eq!(pre, colors.len(), "duplicate color in QUICK_ACTIONS");
    }

    // -----------------------------------------------------------------
    // Phase 4.4 — Quick Toggle behaviors
    // -----------------------------------------------------------------

    /// Test-only command runner: replays scripted responses keyed by
    /// the exact `(program, args)` tuple and records every invocation
    /// so tests can assert on the sequence of commands the toggle
    /// emitted. Unknown commands return `CmdResult::failure()` so
    /// tests that forget to script a path get a failed toggle (and a
    /// failing assertion downstream) rather than a confusing silent
    /// success.
    struct ScriptedRunner {
        responses: std::cell::RefCell<Vec<(Cmd, CmdResult)>>,
        calls: std::cell::RefCell<Vec<Cmd>>,
    }

    impl ScriptedRunner {
        fn new(responses: Vec<(Cmd, CmdResult)>) -> Self {
            Self {
                responses: std::cell::RefCell::new(responses),
                calls: std::cell::RefCell::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<Cmd> {
            self.calls.borrow().clone()
        }
    }

    impl CommandRunner for ScriptedRunner {
        fn run(&self, cmd: &Cmd) -> CmdResult {
            self.calls.borrow_mut().push(cmd.clone());
            // First exact match wins. Multiple scripted responses for
            // the same command let tests model "call N succeeds, call
            // N+1 fails" but we don't need that today — drain pattern
            // matched cleanly.
            let mut responses = self.responses.borrow_mut();
            responses
                .iter()
                .position(|(c, _)| c == cmd)
                .map_or_else(CmdResult::failure, |pos| responses.remove(pos).1)
        }
    }

    #[allow(clippy::panic, clippy::expect_used)]
    fn spec(label: &str) -> &'static ToggleSpec {
        TOGGLE_SPECS
            .iter()
            .find(|s| s.label == label)
            .expect("test asks for an unknown TOGGLE_SPECS label")
    }

    #[test]
    fn toggle_specs_carry_four_entries() {
        // Phase 4.4 + v3.0.0 Q33 lock — Mesh / Bluetooth / DND /
        // Caffeine.
        assert_eq!(TOGGLE_SPECS.len(), 4);
    }

    #[test]
    fn toggle_specs_cover_all_four_labels() {
        let labels: Vec<&str> = TOGGLE_SPECS.iter().map(|s| s.label).collect();
        assert!(labels.contains(&"Mesh"));
        assert!(labels.contains(&"Bluetooth"));
        assert!(labels.contains(&"Do Not Disturb"));
        assert!(labels.contains(&"Caffeine"));
    }

    #[test]
    fn no_two_toggles_share_a_label_or_key() {
        let mut labels: Vec<&str> = TOGGLE_SPECS.iter().map(|s| s.label).collect();
        labels.sort_unstable();
        let pre = labels.len();
        labels.dedup();
        assert_eq!(pre, labels.len(), "duplicate toggle label");

        let mut keys: Vec<&str> = TOGGLE_SPECS.iter().map(|s| s.key).collect();
        keys.sort_unstable();
        let pre = keys.len();
        keys.dedup();
        assert_eq!(pre, keys.len(), "duplicate toggle key");
    }

    #[test]
    fn tooltip_includes_human_description_and_command_summary() {
        // Acceptance gate from the Phase 4.4 lock: every chip's
        // tooltip must describe (a) what the toggle controls in human
        // language and (b) the underlying command.
        for s in TOGGLE_SPECS {
            let on_tip = tooltip_for(s, true);
            let off_tip = tooltip_for(s, false);
            for tip in [&on_tip, &off_tip] {
                assert!(
                    tip.contains(s.human_tooltip),
                    "tooltip missing human description for {}: {tip}",
                    s.label
                );
                assert!(
                    tip.contains(s.command_tooltip),
                    "tooltip missing command summary for {}: {tip}",
                    s.label
                );
            }
            assert!(on_tip.contains("currently on"));
            assert!(off_tip.contains("currently off"));
        }
    }

    #[test]
    fn ax_name_includes_label_and_state() {
        for s in TOGGLE_SPECS {
            let on = ax_name_for(s, true);
            let off = ax_name_for(s, false);
            assert!(on.contains(s.label));
            assert!(off.contains(s.label));
            assert!(on.contains("currently on"));
            assert!(off.contains("currently off"));
        }
    }

    #[test]
    fn mesh_mutator_emits_tailscale_up_when_turning_on() {
        let cmds = mutate_mesh(true);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0], Cmd::new("tailscale", &["up"]));
    }

    #[test]
    fn mesh_mutator_emits_tailscale_down_when_turning_off() {
        let cmds = mutate_mesh(false);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0], Cmd::new("tailscale", &["down"]));
    }

    #[test]
    fn bluetooth_mutator_emits_power_on_off() {
        assert_eq!(
            mutate_bluetooth(true),
            vec![Cmd::new("bluetoothctl", &["power", "on"])]
        );
        assert_eq!(
            mutate_bluetooth(false),
            vec![Cmd::new("bluetoothctl", &["power", "off"])]
        );
    }

    #[test]
    fn dnd_mutator_routes_through_xfconf_query() {
        let cmds = mutate_dnd(true);
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].program, "xfconf-query");
        assert!(cmds[0].args.contains(&"xfce4-notifyd".to_string()));
        assert!(cmds[0].args.contains(&"/do-not-disturb".to_string()));
        assert!(cmds[0].args.contains(&"true".to_string()));

        let cmds = mutate_dnd(false);
        assert!(cmds[0].args.contains(&"false".to_string()));
    }

    #[test]
    fn caffeine_mutator_targets_presentation_mode() {
        let cmds = mutate_caffeine(true);
        assert_eq!(cmds.len(), 1);
        assert!(cmds[0].args.contains(&"xfce4-power-manager".to_string()));
        assert!(cmds[0]
            .args
            .contains(&"/xfce4-power-manager/presentation-mode".to_string()));
        assert!(cmds[0].args.contains(&"true".to_string()));
    }

    #[test]
    fn dnd_mutator_passes_n_flag_to_create_missing_property() {
        // `xfconf-query -n` creates the property if absent. Without
        // this the first toggle on a fresh user fails because the
        // /do-not-disturb key doesn't exist yet in the channel.
        let cmds = mutate_dnd(true);
        assert!(cmds[0].args.contains(&"-n".to_string()));
    }

    #[test]
    fn probe_mesh_reads_tailscale_status() {
        let on = ScriptedRunner::new(vec![(
            Cmd::new("tailscale", &["status"]),
            CmdResult {
                success: true,
                stdout: String::new(),
            },
        )]);
        assert!(probe_mesh(&on));

        let off = ScriptedRunner::new(vec![(
            Cmd::new("tailscale", &["status"]),
            CmdResult {
                success: false,
                stdout: String::new(),
            },
        )]);
        assert!(!probe_mesh(&off));
    }

    #[test]
    fn probe_bluetooth_parses_powered_yes() {
        let on = ScriptedRunner::new(vec![(
            Cmd::new("bluetoothctl", &["show"]),
            CmdResult {
                success: true,
                stdout: "Controller XX\n\tPowered: yes\n\tAlias: foo\n".to_string(),
            },
        )]);
        assert!(probe_bluetooth(&on));

        let off = ScriptedRunner::new(vec![(
            Cmd::new("bluetoothctl", &["show"]),
            CmdResult {
                success: true,
                stdout: "Controller XX\n\tPowered: no\n".to_string(),
            },
        )]);
        assert!(!probe_bluetooth(&off));
    }

    #[test]
    fn probe_bluetooth_returns_false_when_command_fails() {
        let runner = ScriptedRunner::new(vec![]);
        assert!(!probe_bluetooth(&runner));
    }

    #[test]
    fn probe_xfconf_bool_parses_true_false_payload() {
        let truthy = ScriptedRunner::new(vec![(
            Cmd::new("xfconf-query", &["-c", "ch", "-p", "/p"]),
            CmdResult {
                success: true,
                stdout: "true\n".to_string(),
            },
        )]);
        assert!(probe_xfconf_bool(&truthy, "ch", "/p"));

        let falsy = ScriptedRunner::new(vec![(
            Cmd::new("xfconf-query", &["-c", "ch", "-p", "/p"]),
            CmdResult {
                success: true,
                stdout: "false".to_string(),
            },
        )]);
        assert!(!probe_xfconf_bool(&falsy, "ch", "/p"));

        let absent = ScriptedRunner::new(vec![]);
        assert!(!probe_xfconf_bool(&absent, "ch", "/p"));
    }

    #[test]
    fn run_toggle_executes_the_right_command_for_mesh() {
        // Prior state: off → click flips to on → mutator emits
        // `tailscale up`.
        let runner = ScriptedRunner::new(vec![(
            Cmd::new("tailscale", &["up"]),
            CmdResult {
                success: true,
                stdout: String::new(),
            },
        )]);
        let result = run_toggle(spec("Mesh"), false, &runner);
        assert_eq!(result, Ok(true));
        assert_eq!(runner.calls(), vec![Cmd::new("tailscale", &["up"])]);
    }

    #[test]
    fn run_toggle_executes_tailscale_down_when_prior_was_on() {
        let runner = ScriptedRunner::new(vec![(
            Cmd::new("tailscale", &["down"]),
            CmdResult {
                success: true,
                stdout: String::new(),
            },
        )]);
        let result = run_toggle(spec("Mesh"), true, &runner);
        assert_eq!(result, Ok(false));
        assert_eq!(runner.calls(), vec![Cmd::new("tailscale", &["down"])]);
    }

    #[test]
    fn run_toggle_executes_bluetoothctl_power_for_bluetooth() {
        let runner = ScriptedRunner::new(vec![(
            Cmd::new("bluetoothctl", &["power", "on"]),
            CmdResult {
                success: true,
                stdout: String::new(),
            },
        )]);
        let result = run_toggle(spec("Bluetooth"), false, &runner);
        assert_eq!(result, Ok(true));
        assert_eq!(
            runner.calls(),
            vec![Cmd::new("bluetoothctl", &["power", "on"])]
        );
    }

    #[test]
    fn run_toggle_executes_xfconf_set_for_dnd() {
        let cmd = Cmd::new(
            "xfconf-query",
            &[
                "-c",
                "xfce4-notifyd",
                "-p",
                "/do-not-disturb",
                "-n",
                "-t",
                "bool",
                "-s",
                "true",
            ],
        );
        let runner = ScriptedRunner::new(vec![(
            cmd.clone(),
            CmdResult {
                success: true,
                stdout: String::new(),
            },
        )]);
        let result = run_toggle(spec("Do Not Disturb"), false, &runner);
        assert_eq!(result, Ok(true));
        assert_eq!(runner.calls(), vec![cmd]);
    }

    #[test]
    fn run_toggle_executes_xfconf_set_for_caffeine() {
        let cmd = Cmd::new(
            "xfconf-query",
            &[
                "-c",
                "xfce4-power-manager",
                "-p",
                "/xfce4-power-manager/presentation-mode",
                "-n",
                "-t",
                "bool",
                "-s",
                "false",
            ],
        );
        let runner = ScriptedRunner::new(vec![(
            cmd.clone(),
            CmdResult {
                success: true,
                stdout: String::new(),
            },
        )]);
        let result = run_toggle(spec("Caffeine"), true, &runner);
        assert_eq!(result, Ok(false));
        assert_eq!(runner.calls(), vec![cmd]);
    }

    #[test]
    fn run_toggle_returns_err_when_subprocess_fails() {
        // No scripted response → ScriptedRunner returns failure for
        // every call → run_toggle must surface an Err.
        let runner = ScriptedRunner::new(vec![]);
        let result = run_toggle(spec("Mesh"), false, &runner);
        let msg = result.expect_err("expected Err on subprocess failure");
        assert!(msg.contains("Mesh"));
        assert!(msg.contains("tailscale"));
    }

    #[test]
    fn run_toggle_failure_leaves_state_perceptible_to_caller() {
        // The Err branch carries no Ok(new_on), so the chip's prior
        // visual state is unchanged. This test pins the contract at
        // the run_toggle return-type level.
        let runner = ScriptedRunner::new(vec![]);
        let prior_on = true;
        let result = run_toggle(spec("Bluetooth"), prior_on, &runner);
        // No Ok(new_on) means the caller keeps `prior_on` displayed —
        // that's the "revert to prior visual state" contract.
        assert!(result.is_err());
    }

    #[test]
    fn cmd_equality_is_program_plus_args() {
        // ScriptedRunner relies on PartialEq for command matching, so
        // make sure equality is exactly program + argv.
        let a = Cmd::new("foo", &["a", "b"]);
        let b = Cmd::new("foo", &["a", "b"]);
        let c = Cmd::new("foo", &["a"]);
        let d = Cmd::new("bar", &["a", "b"]);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
    }

    #[test]
    fn cmd_result_failure_is_empty_and_not_successful() {
        let r = CmdResult::failure();
        assert!(!r.success);
        assert!(r.stdout.is_empty());
    }
}
