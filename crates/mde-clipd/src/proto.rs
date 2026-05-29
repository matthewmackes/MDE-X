//! Protocol type re-exports for `wlr-data-control-unstable-v1`.
//!
//! Centralises the import path so `session` doesn't need to spell out
//! the full `wayland_protocols_wlr::data_control::v1::client::*` tree.

pub use wayland_client::protocol::wl_seat::WlSeat;
pub use wayland_protocols_wlr::data_control::v1::client::{
    zwlr_data_control_device_v1::{self, ZwlrDataControlDeviceV1},
    zwlr_data_control_manager_v1::{self, ZwlrDataControlManagerV1},
    zwlr_data_control_offer_v1::{self, ZwlrDataControlOfferV1},
};
