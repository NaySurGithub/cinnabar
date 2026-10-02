/// One retained authoritative status effect.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HudEffect {
    pub effect_id: i32,
    pub amplifier: i32,
    pub ambient: bool,
    pub particles: bool,
    /// Server tick after which the effect is no longer presented. `None` is an
    /// effectively infinite (negative wire duration) effect.
    pub expires_at_tick: Option<u64>,
}

impl HudEffect {
    #[must_use]
    pub fn visible_at_tick(&self, now_tick: Option<u64>) -> bool {
        match (self.expires_at_tick, now_tick) {
            (None, _) => true,
            // Without a server clock the effect stays visible until removed.
            (Some(_), None) => true,
            (Some(expires), Some(now)) => now < expires,
        }
    }

    /// Remaining whole seconds, used for the Java expiry blink.
    #[must_use]
    pub fn remaining_ticks(&self, now_tick: Option<u64>) -> Option<u64> {
        match (self.expires_at_tick, now_tick) {
            (Some(expires), Some(now)) => Some(expires.saturating_sub(now)),
            _ => None,
        }
    }
}

/// Heart row recolor derived from authoritative effects and freezing state.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum HeartVariant {
    #[default]
    Normal,
    Poisoned,
    Withered,
    Frozen,
}

/// Authoritative effects drive the same presentation in native and streamed HUDs.
/// Freezing wins over wither, then poison (including fatal poison).
#[must_use]
pub fn heart_variant(effects: &[HudEffect], now_tick: Option<u64>, freezing: f32) -> HeartVariant {
    if freezing >= 1.0 {
        return HeartVariant::Frozen;
    }
    let mut variant = HeartVariant::Normal;
    for effect in effects
        .iter()
        .filter(|effect| effect.visible_at_tick(now_tick))
    {
        match effect.effect_id {
            20 => return HeartVariant::Withered,
            19 | 25 => variant = HeartVariant::Poisoned,
            _ => {}
        }
    }
    variant
}

/// Bedrock's regeneration effect bobs the heart row.
#[must_use]
pub fn regeneration_active(effects: &[HudEffect], now_tick: Option<u64>) -> bool {
    effects
        .iter()
        .any(|effect| effect.effect_id == 10 && effect.visible_at_tick(now_tick))
}

/// Bedrock's hunger effect recolors the hunger row.
#[must_use]
pub fn hunger_effect_active(effects: &[HudEffect], now_tick: Option<u64>) -> bool {
    effects
        .iter()
        .any(|effect| effect.effect_id == 17 && effect.visible_at_tick(now_tick))
}
