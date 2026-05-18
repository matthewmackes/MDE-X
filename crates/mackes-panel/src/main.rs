//! mackes-panel — top status bar + bottom dock for Mackes XFCE Workstation.
//!
//! Phase 0.5: two strut-anchored windows on the primary monitor:
//!
//!   ┌──────────────────────────────────────────┐  top bar  (20 px, every monitor in later phases)
//!   │                                          │
//!   │            <maximized window>            │
//!   │                                          │
//!   ├──────────────────────────────────────────┤  bottom dock (80 px, primary monitor only)
//!   └──────────────────────────────────────────┘
//!
//! Neither stripe carries content yet — Phase 1.x fills them in (appmenu,
//! clock, status cluster, dock icons). The Dock type-hint tells xfwm4 to
//! reserve struts so maximized windows render between the two stripes.

#![forbid(unsafe_code)]

use gdk::prelude::*;
use gtk::prelude::*;

const TOP_BAR_HEIGHT_PX: i32 = 20;
const DOCK_HEIGHT_PX: i32 = 80;
const APP_ID: &str = "shell.mackes.Panel";

/// Each window we build gets the same PatternFly-dark surface (#151515)
/// per Q15. Inlined here so the very-first-boot stripe is visible without
/// loading external CSS files.
const PLACEHOLDER_CSS: &[u8] = b"window { background-color: #151515; }";

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id(APP_ID)
        .flags(gio::ApplicationFlags::FLAGS_NONE)
        .build();

    app.connect_activate(build_panels);

    // Quit cleanly on SIGTERM / SIGINT. unix_signal_add_local runs on the
    // GTK main thread (gtk::Application is !Send). Without this systemd
    // would SIGKILL us after TimeoutStopSec.
    let app_for_sigterm = app.clone();
    glib::unix_signal_add_local(libc::SIGTERM, move || {
        app_for_sigterm.quit();
        glib::ControlFlow::Break
    });
    let app_for_sigint = app.clone();
    glib::unix_signal_add_local(libc::SIGINT, move || {
        app_for_sigint.quit();
        glib::ControlFlow::Break
    });

    app.run()
}

fn build_panels(app: &gtk::Application) {
    let geom = primary_monitor_geometry().unwrap_or_default();
    build_top_bar(app, &geom);
    build_bottom_dock(app, &geom);
}

fn build_top_bar(app: &gtk::Application, geom: &FallbackGeometry) {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("mackes-panel-top")
        .decorated(false)
        .skip_taskbar_hint(true)
        .skip_pager_hint(true)
        .resizable(false)
        .type_hint(gdk::WindowTypeHint::Dock)
        .build();
    window.set_default_size(geom.width, TOP_BAR_HEIGHT_PX);
    window.move_(geom.x, geom.y);
    apply_placeholder_style(&window);
    window.show_all();
}

fn build_bottom_dock(app: &gtk::Application, geom: &FallbackGeometry) {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("mackes-panel-dock")
        .decorated(false)
        .skip_taskbar_hint(true)
        .skip_pager_hint(true)
        .resizable(false)
        .type_hint(gdk::WindowTypeHint::Dock)
        .build();
    window.set_default_size(geom.width, DOCK_HEIGHT_PX);
    window.move_(geom.x, geom.y + geom.height - DOCK_HEIGHT_PX);
    apply_placeholder_style(&window);
    window.show_all();
}

fn apply_placeholder_style(window: &gtk::ApplicationWindow) {
    let style = window.style_context();
    let provider = gtk::CssProvider::new();
    provider
        .load_from_data(PLACEHOLDER_CSS)
        .expect("inline css must parse");
    style.add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
}

/// Rectangle covering the primary monitor in CSS pixels.
#[derive(Debug, Clone, Copy)]
struct FallbackGeometry {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl Default for FallbackGeometry {
    /// Last-resort defaults for headless/CI environments where no display
    /// is connected. 1920×1080 is the most common pixel-perfect target.
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        }
    }
}

/// Primary monitor's geometry in CSS pixels. Returns `None` if there's no
/// connected display (CI / sandboxed builds) so callers fall back.
fn primary_monitor_geometry() -> Option<FallbackGeometry> {
    let display = gdk::Display::default()?;
    let monitor = display.primary_monitor()?;
    let rect = monitor.geometry();
    Some(FallbackGeometry {
        x: rect.x(),
        y: rect.y(),
        width: rect.width(),
        height: rect.height(),
    })
}
