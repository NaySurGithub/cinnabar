//! Guest SDK for player mods, generated from the same `extension` WIT contract used by the host.
//! A server Experience's client part uses `experience-sdk`'s `client` feature instead.

/// Ticks in one Bedrock day, shared by capability validation and sky math.
pub const BEDROCK_DAY_TICKS: u32 = 24_000;
/// Maximum remote players exposed by one local gameplay snapshot.
pub const MAX_GAMEPLAY_PLAYERS: usize = 128;
/// Maximum accumulated camera change per axis in one callback, in radians.
pub const MAX_CAMERA_DELTA_RADIANS: f32 = 0.25;

/// `extension` 0.1, frozen: HUD label, demo key, visual time and gameplay reads.
pub mod bindings {
    wit_bindgen::generate!({
        path: "wit/0.1",
        world: "extension",
        pub_export_macro: true,
    });
}

/// `extension` 0.2: 0.1 plus screens beside the container screens, the session's items and
/// recipes, declared keys and event callbacks.
#[allow(
    clippy::too_many_arguments,
    reason = "wit-bindgen flattens screen-layout records into canonical ABI parameters"
)]
pub mod v0_2 {
    wit_bindgen::generate!({
        path: [
            "wit/0.1",
            "../experience-sdk/wit/client/deps/server-experience",
            "../experience-sdk/wit/session",
            "wit",
        ],
        world: "cinnabar:extension/player-mod@0.2.0",
        generate_all,
        pub_export_macro: true,
        export_macro_name: "export_player_mod",
    });
}
