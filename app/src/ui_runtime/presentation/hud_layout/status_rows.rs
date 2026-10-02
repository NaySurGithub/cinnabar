//! Captures authoritative native runtime values for the shared pure HUD owner.

use super::{HudFrame, HudTexturePages, UiRuntime};
use crate::ui_runtime::presentation::forms::hud_renderers::{HudPaint, SheetSprite};

pub(in super::super) fn capture(
    runtime: &UiRuntime,
    frame: &HudFrame,
    sheet: Option<&HudTexturePages>,
) -> HudPaint {
    let now_tick = runtime.estimated_server_tick(frame.now_millis);
    let input = ui::native_hud::StatusPaintInput {
        health: runtime.hud().health(),
        absorption: runtime.hud().absorption(),
        armor: runtime.hud().armor(),
        hunger: runtime.hud().hunger(),
        air: runtime.hud().air(),
        heart_variant: runtime.gameplay_hud().heart_variant(now_tick),
        regenerating: runtime.gameplay_hud().regeneration_active(now_tick),
        hardcore: runtime.gameplay_hud().hardcore(),
        hunger_effect: runtime.gameplay_hud().hunger_effect_active(now_tick),
        saturation_empty: runtime.gameplay_hud().saturation_empty(),
        effects: runtime.gameplay_hud().effects(),
        now_tick,
        now_millis: frame.now_millis,
        last_health_drop_millis: runtime.last_health_drop_millis(),
        first_person: frame.first_person,
        hotbar_allowed: runtime
            .player_game_mode()
            .is_none_or(|mode| mode.shows_hotbar()),
        survival_stats_visible: runtime.survival_stats_visible(),
        mount_health: frame.mount_health,
        mount_jump: frame.mount_jump,
    };
    let sprite = |role| {
        let sheet = sheet.expect("sprite resolver only supplied for an installed HUD sheet");
        SheetSprite {
            page: sheet.page,
            uv: sheet.sprite(role).uv,
        }
    };
    ui::native_hud::capture_status_hud(&input, sheet.map(|_| &sprite as &dyn Fn(_) -> _))
}
