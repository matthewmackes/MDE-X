//! Clipboard session — Wayland dispatch state and event loop (BUS-5.1).
//!
//! Connects to the compositor's wlr-data-control interface, watches for
//! clipboard changes, and logs every selection event. Future sub-tasks
//! (BUS-5.2 publisher, BUS-5.4 subscriber) extend this module.

use anyhow::Context as _;
use tracing::{debug, info, warn};
use wayland_client::{
    globals::{registry_queue_init, GlobalListContents},
    protocol::{wl_registry, wl_seat},
    Connection, Dispatch, QueueHandle,
};

use crate::proto::{
    self, WlSeat, ZwlrDataControlDeviceV1, ZwlrDataControlManagerV1, ZwlrDataControlOfferV1,
};

/// Full dispatch state for a single clipboard session.
pub struct AppState {
    /// MIME types accumulating for the in-flight offer (reset at each DataOffer).
    pending_mimes: Vec<String>,
    /// Offer proxy introduced by DataOffer — kept alive until Selection/PrimarySelection.
    pending_offer: Option<ZwlrDataControlOfferV1>,
    /// Last regular-clipboard offer; must be destroyed when a new Selection arrives.
    prev_selection: Option<ZwlrDataControlOfferV1>,
    /// Last primary-selection offer; must be destroyed when a new PrimarySelection arrives.
    ///
    /// NOTE: if a compositor shares one offer between Selection + PrimarySelection (rare),
    /// `prev_selection` and `prev_primary` both hold a reference to the same server-side
    /// object. The second `destroy()` call would be a protocol error. BUS-5.2 will add
    /// per-offer ObjectId tracking to avoid this edge case.
    prev_primary: Option<ZwlrDataControlOfferV1>,
    /// Total regular clipboard changes observed this session.
    pub selection_count: u64,
}

impl AppState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            pending_mimes: Vec::new(),
            pending_offer: None,
            prev_selection: None,
            prev_primary: None,
            selection_count: 0,
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

// ── Registry (dynamic global changes — observed but not acted upon) ────────────

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for AppState {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

// ── Seat (capabilities events — not needed for clipboard watching) ─────────────

impl Dispatch<WlSeat, ()> for AppState {
    fn event(
        _: &mut Self,
        _: &WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

// ── Data control manager (no events) ──────────────────────────────────────────

impl Dispatch<ZwlrDataControlManagerV1, ()> for AppState {
    fn event(
        _: &mut Self,
        _: &ZwlrDataControlManagerV1,
        _event: proto::zwlr_data_control_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        // Manager has no events in the protocol spec.
    }
}

// ── Data control device ───────────────────────────────────────────────────────

impl Dispatch<ZwlrDataControlDeviceV1, ()> for AppState {
    fn event(
        state: &mut Self,
        _: &ZwlrDataControlDeviceV1,
        event: proto::zwlr_data_control_device_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use proto::zwlr_data_control_device_v1::Event;
        match event {
            Event::DataOffer { id } => {
                // New offer arriving. Reset MIME accumulation and keep the
                // proxy alive so Selection/PrimarySelection can reference it.
                state.pending_mimes.clear();
                state.pending_offer = Some(id);
                debug!("clipboard: data_offer — new offer introduced");
            }
            Event::Selection { id } => {
                match &id {
                    Some(_) => {
                        state.selection_count += 1;
                        info!(
                            count = state.selection_count,
                            mime_types = ?state.pending_mimes,
                            "clipboard: selection changed"
                        );
                    }
                    None => {
                        info!("clipboard: selection cleared");
                    }
                }
                // `id` is a duplicate reference to `pending_offer`. Drop it — we
                // use `pending_offer` as the canonical holder.
                drop(id);
                // Destroy the PREVIOUS cycle's offer (required by the protocol spec).
                if let Some(old) = state.prev_selection.take() {
                    old.destroy();
                }
                // Park the current offer as "previous" for the next Selection cycle.
                state.prev_selection = state.pending_offer.take();
            }
            Event::Finished => {
                // The seat was destroyed. The supervisor will restart us.
                warn!("clipboard: data-control device finished (seat gone) — exiting");
                std::process::exit(0);
            }
            Event::PrimarySelection { id } => {
                debug!("clipboard: primary selection changed");
                // Destroy the previous primary-selection offer.
                if let Some(old) = state.prev_primary.take() {
                    old.destroy();
                }
                // `id` is an independent reference even if the same server-side
                // object is referenced by prev_selection. Store it as prev_primary
                // and destroy it on the next PrimarySelection event.
                state.prev_primary = id;
            }
            _ => {}
        }
    }
}

// ── Data control offer (MIME type announcements) ──────────────────────────────

impl Dispatch<ZwlrDataControlOfferV1, ()> for AppState {
    fn event(
        state: &mut Self,
        _: &ZwlrDataControlOfferV1,
        event: proto::zwlr_data_control_offer_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let proto::zwlr_data_control_offer_v1::Event::Offer { mime_type } = event {
            debug!(?mime_type, "clipboard: offer mime type");
            state.pending_mimes.push(mime_type);
        }
    }
}

// ── Session entry point ───────────────────────────────────────────────────────

/// Connect to the Wayland display, discover the data-control globals, and
/// watch for clipboard events until the compositor disconnects or the daemon
/// is killed. The mded `clipd_supervisor` worker restarts this process on exit.
pub fn run(conn: &Connection) -> anyhow::Result<()> {
    let (globals, mut queue) = registry_queue_init::<AppState>(conn)
        .context("failed to initialise Wayland registry")?;
    let qh = queue.handle();
    let mut state = AppState::new();

    let seat: WlSeat = globals
        .bind(&qh, 1..=9, ())
        .context("compositor does not advertise wl_seat — is a seat attached?")?;

    let mgr: ZwlrDataControlManagerV1 = globals
        .bind(&qh, 1..=2, ())
        .context("compositor does not support zwlr_data_control_manager_v1 (sway ≥ 1.2 required)")?;

    // Subscribe to all clipboard events for this seat.
    let _device = mgr.get_data_device(&seat, &qh, ());

    // Flush our subscribe request and receive the initial selection event.
    queue
        .roundtrip(&mut state)
        .context("Wayland roundtrip failed")?;

    info!(
        initial_mimes = state.pending_mimes.len(),
        "mde-clipd: watching clipboard"
    );

    loop {
        queue
            .blocking_dispatch(&mut state)
            .context("Wayland dispatch error")?;
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_state_is_empty() {
        let s = AppState::new();
        assert_eq!(s.selection_count, 0);
        assert!(s.pending_mimes.is_empty());
        assert!(s.pending_offer.is_none());
        assert!(s.prev_selection.is_none());
        assert!(s.prev_primary.is_none());
    }

    #[test]
    fn default_equals_new() {
        let s = AppState::default();
        assert_eq!(s.selection_count, 0);
        assert!(s.pending_mimes.is_empty());
    }

    #[test]
    fn pending_mimes_accumulate() {
        let mut s = AppState::new();
        s.pending_mimes.push("text/plain".to_string());
        s.pending_mimes.push("text/html".to_string());
        assert_eq!(s.pending_mimes.len(), 2);
        assert_eq!(s.pending_mimes[0], "text/plain");
    }

    #[test]
    fn pending_mimes_clear_simulates_data_offer() {
        let mut s = AppState::new();
        s.pending_mimes.push("text/plain".to_string());
        // Simulates the DataOffer handler clearing the slate.
        s.pending_mimes.clear();
        assert!(s.pending_mimes.is_empty());
    }

    #[test]
    fn selection_count_increments() {
        let mut s = AppState::new();
        s.selection_count += 1;
        s.selection_count += 1;
        assert_eq!(s.selection_count, 2);
    }

    #[test]
    fn selection_count_starts_at_zero() {
        let s = AppState::new();
        assert_eq!(s.selection_count, 0);
    }
}
