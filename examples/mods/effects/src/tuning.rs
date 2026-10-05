//! Effect constants: edit, rebuild and repack to see them change in a running client.

pub type Rgb = [f32; 3];

pub const TELEGRAPH_COLOR: Rgb = [1.0, 0.18, 0.08];
pub const TELEGRAPH_RADIUS: f32 = 6.0;
pub const TELEGRAPH_SECONDS: f32 = 1.6;

pub const SLASH_COLOR: Rgb = [0.75, 0.9, 1.0];
pub const SLASH_SECONDS: f32 = 0.24;
pub const SLASH_REACH: f32 = 1.9;
pub const SLASH_WIDTH: f32 = 0.55;

pub const CHARGE_COLOR: Rgb = [1.0, 0.82, 0.25];
pub const CHARGE_SECONDS: f32 = 4.0;
pub const CHARGE_PARTICLES: usize = 24;

pub const BEAM_COLOR: Rgb = [0.35, 0.7, 1.0];
pub const BEAM_SECONDS: f32 = 3.0;
pub const BEAM_RANGE: f32 = 32.0;
pub const BEAM_WIDTH: f32 = 1.1;

pub const FLASH_COLOR: Rgb = [0.55, 0.85, 1.0];
pub const FLASH_IMAGES: usize = 5;
pub const AFTERIMAGE_SECONDS: f32 = 0.55;

pub const METEOR_AIR_SECONDS: f32 = 0.7;
pub const CRATER_RADIUS: f32 = 4.5;
pub const CRATER_SECONDS: f32 = 8.0;
pub const SHOCKWAVE_RADIUS: f32 = 9.0;
pub const SHOCKWAVE_SECONDS: f32 = 0.7;
pub const SCORCH_COLOR: Rgb = [0.08, 0.05, 0.04];

pub const BOSS_AURA_COLOR: Rgb = [0.55, 0.25, 1.0];
pub const BOSS_PHASE2_COLOR: Rgb = [1.0, 0.15, 0.1];
/// Used until a boss position arrives from an entity snapshot or event.
pub const BOSS_FALLBACK_POSITION: [f32; 3] = [0.5, 64.0, 16.5];
pub const BOSS_HIT_RADIUS: f32 = 1.8;
pub const PHASE2_PULSE_SECONDS: f32 = 1.2;
pub const PHASE2_PULSE_PERIOD: f32 = 3.5;

pub const LOW_HEALTH_START: f32 = 0.35;
pub const DEATH_FADE_SECONDS: f32 = 1.2;
pub const IMPACT_FRAMES: u8 = 3;
pub const MAX_SHAKE_PIXELS: f32 = 14.0;
pub const TRAUMA_DECAY_PER_SECOND: f32 = 1.6;
