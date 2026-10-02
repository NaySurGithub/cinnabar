//! Shared damage and death visual clocks; transports own status event admission.

/// Ticks the hurt tint and hurt-driven animations stay active; needs independent measurement.
pub const HURT_DURATION_TICKS: u8 = 10;
/// Alpha of the red damage overlay while hurt or dying; needs independent measurement.
pub const HURT_OVERLAY_ALPHA: f32 = 0.4;
/// Native red damage tint, shared by actor publications.
pub const HURT_OVERLAY_RGBA: [f32; 4] = [1.0, 0.0, 0.0, HURT_OVERLAY_ALPHA];
/// Ticks a dying actor takes to tip fully over; needs independent measurement.
pub const DEATH_DURATION_TICKS: u8 = 20;

/// Ticks a picked-up item takes to reach its collector; needs independent measurement.
pub const PICKUP_DURATION_TICKS: u8 = 3;

/// A dropped item flying to the actor that collected it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorPickup {
    pub collector_runtime_id: u64,
    /// Ticks elapsed, saturating at [`PICKUP_DURATION_TICKS`].
    pub ticks: u8,
}

/// Client-derived damage and death presentation state, advanced per tick.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ActorStatus {
    /// Ticks of hurt state remaining.
    pub hurt_time: u8,
    /// The current hurt came without damage, so it shows no red flash (`SkipRedFlashComponent`).
    pub skip_red_flash: bool,
    /// Server-streamed hurt direction, when the server provides one.
    pub hurt_direction: Option<f32>,
    /// Ticks elapsed since death, saturating at [`DEATH_DURATION_TICKS`].
    pub death_time: u8,
    pub dead: bool,
    /// `age_ticks` when the fuse metadata was last received.
    pub fuse_age_ticks: u32,
    /// Ticks since the actor spawned; drives dropped-item spin and bob phase.
    pub age_ticks: u32,
    pub pickup: Option<ActorPickup>,
    /// `(in_water, in_lava)` sampled from the block at the actor; `None` before the first sample.
    pub fluid: Option<(bool, bool)>,
    /// Bed orientation in degrees under a sleeping actor, sampled from the world.
    pub sleep_rotation: Option<f32>,
}

impl ActorStatus {
    /// Whether the red damage overlay should tint the actor this frame.
    #[must_use]
    pub fn overlay_active(&self) -> bool {
        (self.hurt_time > 0 && !self.skip_red_flash) || self.dead
    }

    /// Death tip-over progress in `0..=1` at `partial_tick`, or `None` while alive.
    #[must_use]
    pub fn death_progress(&self, partial_tick: f32) -> Option<f32> {
        if !self.dead {
            return None;
        }
        let ticks = f32::from(self.death_time) + partial_tick.clamp(0.0, 1.0);
        Some((ticks / f32::from(DEATH_DURATION_TICKS)).clamp(0.0, 1.0))
    }

    pub fn tick(&mut self) {
        self.age_ticks = self.age_ticks.saturating_add(1);
        if let Some(pickup) = &mut self.pickup {
            pickup.ticks = pickup.ticks.saturating_add(1).min(PICKUP_DURATION_TICKS);
        }
        self.hurt_time = self.hurt_time.saturating_sub(1);
        if self.dead && self.death_time < DEATH_DURATION_TICKS {
            self.death_time += 1;
        }
    }

    pub fn die(&mut self) {
        self.dead = true;
        self.hurt_time = HURT_DURATION_TICKS;
        self.skip_red_flash = false;
    }

    pub fn revive(&mut self) {
        self.hurt_time = 0;
        self.hurt_direction = None;
        self.death_time = 0;
        self.dead = false;
    }
}
