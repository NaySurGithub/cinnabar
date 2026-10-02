//! Pure native HUD stat layout. Runtime adapters supply authoritative values and clocks.

use super::motion::{heart_lift, hunger_shake_offset, hunger_shakes};
use super::pinned::{
    HARMFUL_EFFECT_IDS, MAX_HEART_ROWS, MAX_MOUNT_HEARTS, damage_flash_phase, effect_blink_alpha,
    effect_icon_role, heart_role,
};
use super::{Cell, HeartVariant, HudEffect, HudPaint, SheetSprite};
use crate::BoundedStat;
use assets::HudTextureRole;

/// Authoritative HUD values plus retained timing supplied by its presentation owner.
#[derive(Clone, Debug)]
pub struct StatusPaintInput<'a> {
    pub health: Option<BoundedStat>,
    pub absorption: Option<BoundedStat>,
    pub armor: Option<BoundedStat>,
    pub hunger: Option<BoundedStat>,
    pub air: Option<BoundedStat>,
    pub heart_variant: HeartVariant,
    pub regenerating: bool,
    pub hardcore: bool,
    pub hunger_effect: bool,
    pub saturation_empty: bool,
    pub effects: &'a [HudEffect],
    pub now_tick: Option<u64>,
    pub now_millis: u64,
    pub last_health_drop_millis: Option<u64>,
    pub first_person: bool,
    pub hotbar_allowed: bool,
    pub survival_stats_visible: bool,
    pub mount_health: Option<(f32, f32)>,
    pub mount_jump: Option<f32>,
}

/// The `textures/ui` path a HUD role was compiled from.
fn path(role: HudTextureRole) -> &'static str {
    let source = role.source_path();
    source.strip_suffix(".png").unwrap_or(source)
}

/// The hardcore variant of a heart foreground, which lives in `textures/ui/hardcore/`.
fn hardcore_path(role: HudTextureRole) -> Option<&'static str> {
    Some(match role {
        HudTextureRole::HeartFull => "textures/ui/hardcore/heart",
        HudTextureRole::HeartHalf => "textures/ui/hardcore/heart_half",
        HudTextureRole::HeartFlashFull => "textures/ui/hardcore/heart_flash",
        HudTextureRole::HeartFlashHalf => "textures/ui/hardcore/heart_flash_half",
        HudTextureRole::AbsorptionHeartFull => "textures/ui/hardcore/absorption_heart",
        HudTextureRole::AbsorptionHeartHalf => "textures/ui/hardcore/absorption_heart_half",
        HudTextureRole::FreezeHeartFull => "textures/ui/hardcore/freeze_heart",
        HudTextureRole::FreezeHeartHalf => "textures/ui/hardcore/freeze_heart_half",
        HudTextureRole::FreezeHeartFlashFull => "textures/ui/hardcore/freeze_heart_flash",
        HudTextureRole::FreezeHeartFlashHalf => "textures/ui/hardcore/freeze_heart_flash_half",
        _ => return None,
    })
}

/// Health plus absorption in hearts, and the row pitch the stacked rows use.
struct HeartRows {
    current: u32,
    absorption: u32,
    health_hearts: u32,
    total: u32,
    rows: u32,
    pitch: f32,
}

fn heart_rows(input: &StatusPaintInput<'_>) -> Option<HeartRows> {
    let health = input.health?;
    let scale = u32::from(health.scale()).max(1);
    // Half-heart units on the reference 20-point scale.
    let current = u32::from(health.current()).div_ceil(scale);
    let maximum = u32::from(health.maximum()) / scale;
    let absorption = input
        .absorption
        .map(|stat| u32::from(stat.current()).div_ceil(u32::from(stat.scale()).max(1)))
        .unwrap_or(0);
    let health_hearts = maximum.div_ceil(2).min(u32::from(MAX_HEART_ROWS) * 10);
    let total = (health_hearts + absorption.div_ceil(2).min(20)).max(1);
    let rows = total.div_ceil(10).max(1);
    let pitch = (10 - rows.saturating_sub(2)).max(3) as f32;
    Some(HeartRows {
        current,
        absorption,
        health_hearts,
        total,
        rows,
        pitch,
    })
}

/// Capture the frame's native HUD art.
pub fn capture_status_hud(
    input: &StatusPaintInput<'_>,
    sheet: Option<&dyn Fn(HudTextureRole) -> SheetSprite>,
) -> HudPaint {
    let now_tick = input.now_tick;
    let mode_allows_hotbar = input.hotbar_allowed;
    let mut paint = HudPaint {
        effects: effects(input, now_tick),
        // First person only, and never in spectator (no interaction targeting).
        crosshair: sheet
            .filter(|_| input.first_person && mode_allows_hotbar)
            .map(|sheet| sheet_sprite(sheet, HudTextureRole::Crosshair)),
        ..HudPaint::default()
    };
    if !input.survival_stats_visible {
        return paint;
    }
    if let Some(rows) = heart_rows(input) {
        paint.hearts = hearts(input, &rows, now_tick);
        paint.armor = armor(input, &rows);
    }
    match input.mount_health {
        Some(health) => paint.mount_hearts = mount_hearts(health),
        None => paint.hunger = hunger(input, now_tick),
    }
    paint.bubbles = bubbles(input);
    if let (Some(charge), Some(sheet)) = (input.mount_jump, sheet) {
        let filled = (charge.clamp(0.0, 1.0) * 183.0).floor().clamp(0.0, 182.0);
        paint.mount_jump = Some((
            sheet_sprite(sheet, HudTextureRole::MountJumpBackground),
            sheet_sprite(sheet, HudTextureRole::MountJumpProgress),
            filled,
        ));
    }
    paint
}

fn sheet_sprite(
    sheet: &dyn Fn(HudTextureRole) -> SheetSprite,
    role: HudTextureRole,
) -> SheetSprite {
    sheet(role)
}

fn hearts(input: &StatusPaintInput<'_>, rows: &HeartRows, now_tick: Option<u64>) -> Vec<Cell> {
    let variant = input.heart_variant;
    let flash = damage_flash_phase(input.last_health_drop_millis, input.now_millis);
    let tick = now_tick.unwrap_or(input.now_millis / 50);
    let regenerating = input.regenerating;
    let hardcore = input.hardcore;
    let mut cells = Vec::new();
    for index in 0..rows.total {
        let lift = heart_lift(
            index,
            rows.health_hearts,
            rows.current + rows.absorption,
            regenerating,
            tick,
        );
        let at = [
            (index % 10) as f32 * 8.0,
            -((index / 10) as f32) * rows.pitch - lift,
        ];
        cells.push(Cell::icon(at, path(HudTextureRole::HeartBackground)));
        let foreground = if index < rows.health_hearts {
            heart_role(variant, flash, rows.current.saturating_sub(index * 2))
        } else {
            match rows
                .absorption
                .saturating_sub((index - rows.health_hearts) * 2)
            {
                0 => None,
                1 => Some(HudTextureRole::AbsorptionHeartHalf),
                _ => Some(HudTextureRole::AbsorptionHeartFull),
            }
        };
        if let Some(role) = foreground {
            let mut cell = Cell::icon(at, path(role));
            cell.preferred = hardcore.then(|| hardcore_path(role)).flatten();
            cells.push(cell);
        }
    }
    cells
}

/// Armor sits one row above the highest heart row, only while armor is worn.
fn armor(input: &StatusPaintInput<'_>, rows: &HeartRows) -> Vec<Cell> {
    let Some(armor) = input.armor else {
        return Vec::new();
    };
    let points = u32::from(armor.current()).div_ceil(u32::from(armor.scale()).max(1));
    if points == 0 {
        return Vec::new();
    }
    let y = -((rows.rows - 1) as f32) * rows.pitch - 10.0;
    (0..10u32)
        .map(|index| {
            let role = match points.saturating_sub(index * 2) {
                0 => HudTextureRole::ArmorEmpty,
                1 => HudTextureRole::ArmorHalf,
                _ => HudTextureRole::ArmorFull,
            };
            Cell::icon([index as f32 * 8.0, y], path(role))
        })
        .collect()
}

/// Right-to-left from the control's position, shaking on empty saturation.
fn hunger(input: &StatusPaintInput<'_>, now_tick: Option<u64>) -> Vec<Cell> {
    let Some(hunger) = input.hunger else {
        return Vec::new();
    };
    let current = u32::from(hunger.current()).div_ceil(u32::from(hunger.scale()).max(1));
    let (background, full, half) = if input.hunger_effect {
        (
            HudTextureRole::HungerEffectBackground,
            HudTextureRole::HungerEffectFull,
            HudTextureRole::HungerEffectHalf,
        )
    } else {
        (
            HudTextureRole::HungerBackground,
            HudTextureRole::HungerFull,
            HudTextureRole::HungerHalf,
        )
    };
    let tick = now_tick.unwrap_or(0);
    // Without a server clock there is no tick to pulse on, so no shake.
    let shaking = now_tick.is_some() && hunger_shakes(input.saturation_empty, current, tick);
    let mut cells = Vec::new();
    for index in 0..10u32 {
        let shake = if shaking {
            hunger_shake_offset(index, tick)
        } else {
            0.0
        };
        let at = [-8.0 - index as f32 * 8.0, shake];
        cells.push(Cell::icon(at, path(background)));
        let role = match current.saturating_sub(index * 2) {
            0 => None,
            1 => Some(half),
            _ => Some(full),
        };
        if let Some(role) = role {
            cells.push(Cell::icon(at, path(role)));
        }
    }
    cells
}

/// Mount hearts replace the hunger row while riding, capped at 30 over three rows.
fn mount_hearts((current, maximum): (f32, f32)) -> Vec<Cell> {
    let hearts = (((maximum + 0.5) / 2.0) as u16).clamp(1, MAX_MOUNT_HEARTS);
    let filled = current.clamp(0.0, maximum).ceil() as u32;
    let mut cells = Vec::new();
    for index in 0..u32::from(hearts) {
        let at = [
            -8.0 - (index % 10) as f32 * 8.0,
            -((index / 10) as f32) * 10.0,
        ];
        cells.push(Cell::icon(at, path(HudTextureRole::HeartBackground)));
        let role = match filled.saturating_sub(index * 2) {
            0 => None,
            1 => Some(HudTextureRole::MountHeartHalf),
            _ => Some(HudTextureRole::MountHeartFull),
        };
        if let Some(role) = role {
            cells.push(Cell::icon(at, path(role)));
        }
    }
    cells
}

/// Air bubbles while submerged (air below its maximum), with the popping tail.
fn bubbles(input: &StatusPaintInput<'_>) -> Vec<Cell> {
    let Some(air) = input.air else {
        return Vec::new();
    };
    let current = u32::from(air.current());
    let maximum = u32::from(air.maximum()).max(1);
    if current >= maximum {
        return Vec::new();
    }
    let full = (current.saturating_sub(2) * 10).div_ceil(maximum);
    let popping = (current * 10).div_ceil(maximum).saturating_sub(full);
    (0..(full + popping).min(10))
        .map(|index| {
            let role = if index < full {
                HudTextureRole::BubbleFull
            } else {
                HudTextureRole::BubblePop
            };
            Cell::icon([-8.0 - index as f32 * 8.0, 0.0], path(role))
        })
        .collect()
}

/// Beneficial row, then harmful, leftward from the control's top-right corner,
/// each a 24x24 background under an 18x18 icon, blinking before expiry.
fn effects(input: &StatusPaintInput<'_>, now_tick: Option<u64>) -> Vec<Cell> {
    let mut rows: [Vec<_>; 2] = [Vec::new(), Vec::new()];
    for effect in input.effects {
        if !effect.visible_at_tick(now_tick) || effect_icon_role(effect.effect_id).is_none() {
            continue;
        }
        rows[usize::from(HARMFUL_EFFECT_IDS.contains(&effect.effect_id))].push(effect);
    }
    let mut cells = Vec::new();
    for (row, effects) in rows.iter_mut().enumerate() {
        effects.sort_by_key(|effect| effect.effect_id);
        let y = 1.0 + row as f32 * 25.0;
        for (column, effect) in effects.iter().enumerate() {
            let x = -25.0 * (column as f32 + 1.0);
            let alpha = effect_blink_alpha(effect, now_tick);
            let background = if effect.ambient {
                HudTextureRole::EffectBackgroundAmbient
            } else {
                HudTextureRole::EffectBackground
            };
            cells.push(Cell {
                size: [24.0, 24.0],
                alpha,
                ..Cell::icon([x, y], path(background))
            });
            if let Some(icon) = effect_icon_role(effect.effect_id) {
                cells.push(Cell {
                    size: [18.0, 18.0],
                    alpha,
                    ..Cell::icon([x + 3.0, y + 3.0], path(icon))
                });
            }
        }
    }
    cells
}
