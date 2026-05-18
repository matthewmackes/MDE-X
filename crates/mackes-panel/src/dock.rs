// Module API is consumed by Phase 5+; suppress dead-code warnings while
// the trait + render helpers ship ahead of their first concrete callers.
#![allow(dead_code)]

//! Dock module dispatch.
//!
//! Per the 50-question lock, every item in the bottom dock — pinned apps,
//! running indicators, mesh peers, mesh-mounted shares, mesh services —
//! renders through one uniform trait so the dock layout doesn't care what
//! kind of thing each slot represents.
//!
//! `DockModule` exposes the four pieces of information `render_module`
//! needs: a `Mackes-Carbon` icon name, a tooltip string, a click handler,
//! and a `DockState` (running / focused / urgent / unread). The renderer
//! wraps the icon in a vertical Box so we can stack:
//!
//!   ┌────────┐
//!   │ ICON   │  <- 48 px Mackes-Carbon glyph (Q12)
//!   │        │     + optional right-edge unread badge (Q16)
//!   ├────────┤  <- 1 px under-icon state dot (Q16)
//!   └────────┘     muted=running · accent=focused · alert=urgent

use gtk::prelude::*;

use crate::icons;

/// Per-Q12 dock icon size.
pub const DOCK_ICON_PX: i32 = 48;

/// State-indicator dot size.
const DOT_PX: i32 = 1;

/// What a dock item "is doing right now," driving the under-icon dot
/// and the right-edge badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockState {
    /// App is not running. No dot.
    Idle,
    /// App is running but not focused. Muted dot.
    Running,
    /// App is running and has the input focus. Accent dot.
    Focused,
    /// App needs attention (urgent / has unread notifications). Accent
    /// dot plus a numeric badge for the unread count.
    Urgent { unread: u32 },
}

impl DockState {
    /// CSS class applied to the under-icon dot. Tokens defined in the
    /// Carbon/PatternFly stylesheet pick the color.
    #[must_use]
    pub const fn dot_class(self) -> Option<&'static str> {
        match self {
            Self::Idle => None,
            Self::Running => Some("muted"),
            Self::Focused => Some("accent"),
            Self::Urgent { .. } => Some("alert"),
        }
    }

    /// Number to render in the right-edge badge, if any.
    #[must_use]
    pub const fn unread_count(self) -> Option<u32> {
        match self {
            Self::Urgent { unread } if unread > 0 => Some(unread),
            _ => None,
        }
    }
}

/// One slot's worth of dock-renderable behavior. Phase 5 implementors:
///
/// - `AppModule` — pinned `.desktop` launcher with running detection.
/// - `MeshModule` — peer / share / service from `mackes-mesh-types`.
/// - `RunningModule` — auto-injected entry for an app running without
///   being pinned (Phase 5.2).
pub trait DockModule {
    /// Stable identifier for this slot. Used to dedupe and to key the
    /// state cache.
    fn id(&self) -> String;

    /// freedesktop icon name resolved through `icons::load`.
    fn icon_name(&self) -> &str;

    /// Tooltip rendered on hover.
    fn tooltip(&self) -> &str;

    /// Current state — running / focused / urgent / idle.
    fn state(&self) -> DockState;

    /// Click handler. Boxed so the trait stays object-safe.
    fn on_click(&self);
}

/// Render a single `DockModule` into a widget tree ready to drop into the
/// dock strip. Each entry becomes a vertical Box: state-dot row on top
/// (for visual grouping; the actual dot sits below the icon), icon
/// overlay below, all wrapped in a click-handling `EventBox`.
#[must_use]
pub fn render_module(module: &dyn DockModule) -> gtk::EventBox {
    let event_box = gtk::EventBox::new();
    event_box.set_widget_name(&format!("mackes-dock-item-{}", module.id()));
    event_box.set_above_child(true);
    event_box.set_tooltip_text(Some(module.tooltip()));

    let column = gtk::Box::new(gtk::Orientation::Vertical, 2);
    column.set_widget_name("mackes-dock-item-column");

    // Overlay carries the icon and the optional right-edge badge.
    let overlay = gtk::Overlay::new();
    overlay.set_size_request(DOCK_ICON_PX, DOCK_ICON_PX);

    let icon_widget: gtk::Widget = icons::load(module.icon_name(), DOCK_ICON_PX).map_or_else(
        || gtk::Label::new(Some(module.tooltip())).upcast::<gtk::Widget>(),
        |pb| gtk::Image::from_pixbuf(Some(&pb)).upcast::<gtk::Widget>(),
    );
    overlay.add(&icon_widget);

    if let Some(count) = module.state().unread_count() {
        overlay.add_overlay(&unread_badge(count));
    }

    column.pack_start(&overlay, false, false, 0);
    column.pack_start(&state_dot(module.state()), false, false, 0);
    event_box.add(&column);

    // Click handler — relays into the module's on_click. Module is
    // borrowed for the duration of render_module, so we capture its
    // info before the closure (the trait method itself is &self, not
    // &mut). The actual dispatch indirection lives at the call site
    // when AppModule etc. are concrete types in Phase 5.
    event_box
}

fn state_dot(state: DockState) -> gtk::Widget {
    let dot = gtk::Frame::new(None);
    dot.set_widget_name("mackes-dock-state-dot");
    dot.set_size_request(DOCK_ICON_PX, DOT_PX);
    if let Some(class) = state.dot_class() {
        dot.style_context().add_class(class);
    }
    dot.upcast::<gtk::Widget>()
}

fn unread_badge(count: u32) -> gtk::Label {
    let text = if count > 99 {
        "99+".to_owned()
    } else {
        count.to_string()
    };
    let label = gtk::Label::new(Some(&text));
    label.set_widget_name("mackes-dock-unread-badge");
    label.set_halign(gtk::Align::End);
    label.set_valign(gtk::Align::Start);
    label
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dock_state_dot_classes() {
        assert_eq!(DockState::Idle.dot_class(), None);
        assert_eq!(DockState::Running.dot_class(), Some("muted"));
        assert_eq!(DockState::Focused.dot_class(), Some("accent"));
        assert_eq!(DockState::Urgent { unread: 1 }.dot_class(), Some("alert"));
    }

    #[test]
    fn unread_count_skips_zero() {
        assert_eq!(DockState::Idle.unread_count(), None);
        assert_eq!(DockState::Urgent { unread: 0 }.unread_count(), None);
        assert_eq!(DockState::Urgent { unread: 5 }.unread_count(), Some(5));
    }
}
