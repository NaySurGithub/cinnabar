//! Showcase effects built on the render capability: telegraphs, slash trails, ability VFX
//! and screen grades. `Effects` is plain state, so another mod can embed and trigger it.

#[cfg(feature = "component")]
mod component;
mod effects;
pub mod shaders;
#[cfg(test)]
mod tests;
pub mod tuning;
mod vfx;

use effects::Timed;
use mod_api::bindings::cinnabar::extension::render::{
    Beam, Billboard, BillboardPattern, Decal, DecalStyle, Primitives, Rgba, Ribbon, Vector3,
};
use tuning::Rgb;

pub use effects::Effects;

pub type Point = [f32; 3];

/// The cue a server marker actor stands for, with the values that follow its position. The
/// local showcase server spawns these briefly at boss events the client cannot otherwise see.
pub fn marker_cue(type_id: &str) -> Option<(&'static str, &'static [f32])> {
    match type_id {
        "cinnabar:fx_telegraph" => Some(("boss.telegraph", &[5.0, 1.0])),
        "cinnabar:fx_slam" => Some(("boss.slam", &[])),
        "cinnabar:fx_stagger" => Some(("camera.parry", &[])),
        "cinnabar:fx_phase2" => Some(("boss.phase2", &[])),
        _ => None,
    }
}

/// Effect triggers; positions are world block coordinates at the feet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Event {
    ChargeStart,
    ChargeStop,
    BeamStart,
    BeamStop,
    BeamHit,
    FlashStep {
        from: Point,
    },
    MeteorLeap,
    MeteorLand {
        at: Point,
    },
    FlightStart,
    FlightStop,
    BossTelegraph {
        at: Point,
        radius: f32,
        seconds: f32,
    },
    BossSlam {
        at: Point,
    },
    BossPhase2,
    BossPosition {
        at: Point,
    },
    PlayerHealth {
        fraction: f32,
    },
    PlayerDied,
    PlayerRespawn,
}

/// The four registered passes in execution order.
pub const PASSES: [(&str, i32, &str); 4] = [
    ("boss-aura", 10, shaders::BOSS_AURA),
    ("beam-bloom", 20, shaders::BEAM_BLOOM),
    ("warp", 30, shaders::WARP),
    ("grade", 40, shaders::GRADE),
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PassUpdate {
    pub name: &'static str,
    pub enabled: bool,
    pub params: [f32; 16],
}

pub struct Frame {
    pub primitives: Primitives,
    pub passes: [PassUpdate; 4],
}

fn timed(at: Point, life: f32, radius: f32) -> Timed {
    Timed {
        at,
        age: 0.0,
        life,
        radius,
    }
}

fn params(values: &[f32]) -> [f32; 16] {
    let mut out = [0.0; 16];
    out[..values.len()].copy_from_slice(values);
    out
}

fn v3(p: Point) -> Vector3 {
    Vector3 {
        x: p[0],
        y: p[1],
        z: p[2],
    }
}

fn rgba(rgb: Rgb, a: f32) -> Rgba {
    Rgba {
        r: rgb[0],
        g: rgb[1],
        b: rgb[2],
        a: a.clamp(0.0, 1.0),
    }
}

fn decal(at: Point, radius: f32, color: Rgba, progress: f32, style: DecalStyle) -> Decal {
    Decal {
        center: v3(at),
        radius,
        color,
        progress,
        style,
    }
}

fn billboard(
    at: Point,
    size: (f32, f32),
    color: Rgba,
    pattern: BillboardPattern,
    upright: bool,
) -> Billboard {
    Billboard {
        position: v3(at),
        width: size.0,
        height: size.1,
        color,
        pattern,
        upright,
    }
}

fn add(a: Point, b: Point) -> Point {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: Point, s: f32) -> Point {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn sub(a: Point, b: Point) -> Point {
    add(a, scale(b, -1.0))
}

fn length(a: Point) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

/// Actor YXZ look basis: forward, right and up; positive yaw turns left, pitch up.
fn basis(yaw: f32, pitch: f32) -> (Point, Point, Point) {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    let forward = [-sy * cp, sp, -cy * cp];
    let right = [cy, 0.0, -sy];
    let up = [sy * sp, cp, cy * sp];
    (forward, right, up)
}

fn ribbon(points: Vec<Point>, width: f32, color: Rgba) -> Ribbon {
    Ribbon {
        points: points.into_iter().map(v3).collect(),
        width,
        color,
    }
}

fn beam_primitive(start: Point, end: Point, width: f32, color: Rgba, intensity: f32) -> Beam {
    Beam {
        start: v3(start),
        end: v3(end),
        width,
        color,
        intensity,
    }
}
