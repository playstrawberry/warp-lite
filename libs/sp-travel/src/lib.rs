//! `sp-travel`: one way to travel for the small teleport scripts (warp-lite, home-lite, spawn-lite, back-lite).
//!
//! A script keeps its own destinations, commands, permissions and texts; everything between "this player asked to
//! go there" and "he arrived" is here, once:
//!
//! - [`Location`]: a saved place, checked whenever it is read from a file or taken from a player ([`Location::valid`]).
//! - [`queue`]: the wait before a travel and the cooldown after an arrival, with hard caps on what is kept.
//! - [`safety`]: the look at a loaded destination and the one teleport call, with the arrival checked.
//! - [`files`]: bounded reads, writes that replace a file whole, and the tab-separated list of named locations.
//! - [`settings`]: a strict `key=number` settings file.
//! - `runtime` (the plugin target only): [`runtime::Travels`], the wait's timer and the events that cancel it.
//!
//! Every refusal is a key of the calling script's lang files ([`KEYS`]); a script's test checks it has them all.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod files;
mod location;
pub mod queue;
pub mod safety;
pub mod settings;

#[cfg(target_arch = "wasm32")]
pub mod runtime;

pub use location::{Id, Location, moved, name};

/// The lang keys this library answers with. A script that travels has every one of them in its four lang files.
/// `waiting` takes the destination's label and the seconds, `teleported` the label.
pub const KEYS: [&str; 12] = [
    "already_waiting",
    "busy",
    "cancelled",
    "cooldown",
    "damaged",
    "invalid_location",
    "moved",
    "teleport_failed",
    "teleported",
    "unsafe_target",
    "waiting",
    "world_unavailable",
];
