//! Discord Rich Presence: the activity card for the client's state, published
//! over Discord's local IPC by a background worker.
//!
//! The worker connects lazily and backs off while Discord isn't running, so a
//! missing Discord never affects the game.

mod card;
mod link;
mod worker;

pub use card::{Card, Destination, Status};
pub use worker::PresenceWorker;

/// Cinnabar's Discord application; presence stays off until one is registered.
pub const APPLICATION_ID: Option<&str> = None;

/// Rich Presence art asset key uploaded to the Discord application.
pub const LOGO_ASSET: &str = "logo";
