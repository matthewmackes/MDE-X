//! Per-panel views — one module per group leaf. CB-1.x ports
//! land these incrementally; each module ships a state struct,
//! a `Message` variant set, an `update` reducer that returns
//! the parent app's `Message`, and a `view` builder over
//! [`Element<'_, crate::Message>`].

pub mod fonts;
pub mod themes;
