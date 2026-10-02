//! Native HUD sprite presentation shared by application and browser renderers.
//! Authored JSON-UI controls own placement; this module paints their custom renderers.

mod model;
mod motion;
mod paint;
mod pinned;
mod status;

pub use model::{
    HeartVariant, HudEffect, heart_variant, hunger_effect_active, regeneration_active,
};
pub use paint::{Cell, HudPaint, HudPaintTarget, SheetSprite, paint, paint_progress};
pub use pinned::effect_icon_role;
pub use status::{StatusPaintInput, capture_status_hud};

/// The built-in Java-styled HUD pack: `(pack path, namespace, bytes)`, layered
/// under every server pack.
pub const JAVA_HUD_PACK: [(&str, &str, &[u8]); 2] = [
    (
        "ui/hud_screen.json",
        "hud",
        include_bytes!("../../../assets/java-hud/ui/hud_screen.json"),
    ),
    (
        "ui/scoreboards.json",
        "scoreboard",
        include_bytes!("../../../assets/java-hud/ui/scoreboards.json"),
    ),
];
