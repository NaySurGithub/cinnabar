//! Host-independent showcase camera and ability input logic; the guest glue only moves data.

use std::collections::VecDeque;
use std::f32::consts::{PI, TAU};

use mod_api::{
    MAX_CAMERA_DELTA_RADIANS, MAX_COMMANDS_PER_FRAME, MAX_CUES_PER_FRAME, MAX_RIG_BACK_BLOCKS,
    MAX_RIG_FOV_DELTA_DEGREES, MAX_RIG_ROLL_RADIANS, MAX_RIG_SIDE_BLOCKS, MAX_RIG_VERTICAL_BLOCKS,
};

/// The showcase boss; lock-on ignores every other mob.
pub const BOSS_TYPE: &str = "cinnabar:hollow_warden";
/// Keys taken from ordinary gameplay: abilities 1-4, lock-on and parry.
pub const RESERVED_KEYS: [&str; 6] = ["Digit1", "Digit2", "Digit3", "Digit4", "KeyR", "KeyF"];

const SHOULDER: [f32; 3] = [0.75, 0.35, 3.2];
const LOCKED_SHOULDER: [f32; 3] = [0.9, 0.5, 3.8];
const RIG_BLEND_PER_SECOND: f32 = 6.0;
/// Aim point above the boss's feet; its model is about four blocks tall.
const LOCK_AIM_HEIGHT: f32 = 2.0;
const LOCK_TRACK_PER_SECOND: f32 = 10.0;
const LOCK_BREAK_BLOCKS: f32 = 40.0;
const DOUBLE_TAP_SECONDS: f32 = 0.3;
const DODGE: Kick = Kick {
    seconds: 0.45,
    roll: 0.32,
    fov: 8.0,
};
const FLASH: Kick = Kick {
    seconds: 0.35,
    roll: 0.0,
    fov: 14.0,
};
const CHARGE_FOV: f32 = 6.0;
const CHARGE_FOV_SECONDS: f32 = 1.5;
const BEAM_FOV: f32 = -6.0;
const BEAM_SHAKE: f32 = 0.05;
const CHARGE_SHAKE: f32 = 0.02;
const SLAM_SHAKE: f32 = 0.35;
const SLAM_SHAKE_SECONDS: f32 = 0.6;
const PARRY_SCALE: f32 = 0.3;
const PARRY_HOLD_SECONDS: f32 = 0.45;
const PARRY_RETURN_SECONDS: f32 = 0.3;
const METEOR_MIN_AIR_SECONDS: f32 = 0.25;
const METEOR_TIMEOUT_SECONDS: f32 = 4.0;
const FALLING_BLOCKS_PER_SECOND: f32 = -2.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Mob {
    pub runtime_id: u64,
    pub type_id: String,
    pub position: [f32; 3],
}

/// One gameplay frame; angles are actor YXZ radians (yaw left, pitch up).
#[derive(Debug, Clone, Default)]
pub struct Frame {
    pub seconds: f32,
    pub pressed: Vec<String>,
    pub held: Vec<String>,
    pub eye: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub mobs: Vec<Mob>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rig {
    pub offset: [f32; 3],
    pub roll: f32,
    pub fov_delta: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub name: &'static str,
    pub values: Vec<f32>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Output {
    pub rig: Option<Rig>,
    pub rotate: Option<(f32, f32)>,
    pub commands: Vec<String>,
    pub cues: Vec<Cue>,
    pub label: Option<&'static str>,
}

#[derive(Debug, Clone, Copy)]
struct Kick {
    seconds: f32,
    roll: f32,
    fov: f32,
}

/// A kick that rises fast and eases back; `sign` mirrors its roll.
#[derive(Debug, Clone, Copy)]
struct Pulse {
    kick: Kick,
    elapsed: f32,
    sign: f32,
}

impl Pulse {
    fn envelope(&self) -> f32 {
        let progress = (self.elapsed / self.kick.seconds).clamp(0.0, 1.0);
        if progress < 0.2 {
            let rise = progress / 0.2;
            1.0 - (1.0 - rise) * (1.0 - rise)
        } else {
            let fall = (progress - 0.2) / 0.8;
            1.0 - fall * fall * (3.0 - 2.0 * fall)
        }
    }

    fn done(&self) -> bool {
        self.elapsed >= self.kick.seconds
    }
}

#[derive(Clone, Copy)]
enum Meteor {
    Idle,
    Rising { elapsed: f32 },
    Falling { elapsed: f32 },
}

pub struct Director {
    clock: f32,
    visual_clock: f32,
    last_sneak: f32,
    lock: Option<u64>,
    offset: [f32; 3],
    charging: Option<f32>,
    beaming: bool,
    pulses: Vec<Pulse>,
    slam_shake: f32,
    parry: Option<f32>,
    meteor: Meteor,
    last_eye_y: Option<f32>,
    commands: VecDeque<String>,
    cues: VecDeque<Cue>,
}

impl Default for Director {
    fn default() -> Self {
        Self {
            clock: 0.0,
            visual_clock: 0.0,
            last_sneak: f32::NEG_INFINITY,
            lock: None,
            offset: SHOULDER,
            charging: None,
            beaming: false,
            pulses: Vec::new(),
            slam_shake: 0.0,
            parry: None,
            meteor: Meteor::Idle,
            last_eye_y: None,
            commands: VecDeque::new(),
            cues: VecDeque::new(),
        }
    }
}

fn has(keys: &[String], key: &str) -> bool {
    keys.iter().any(|candidate| candidate == key)
}

fn look(yaw: f32, pitch: f32) -> [f32; 3] {
    [
        -yaw.sin() * pitch.cos(),
        pitch.sin(),
        -yaw.cos() * pitch.cos(),
    ]
}

fn wrap(angle: f32) -> f32 {
    (angle + PI).rem_euclid(TAU) - PI
}

/// Cheap deterministic noise in [-1, 1] so shakes replay identically.
fn noise(seed: f32) -> f32 {
    let value = (seed * 12.9898).sin() * 43_758.547;
    (value - value.floor()) * 2.0 - 1.0
}

impl Director {
    #[must_use]
    pub fn locked(&self) -> Option<u64> {
        self.lock
    }

    /// Visual time scale: slowed during a parry, then eased back to real time.
    #[must_use]
    pub fn time_scale(&self) -> f32 {
        match self.parry {
            Some(elapsed) if elapsed < PARRY_HOLD_SECONDS => PARRY_SCALE,
            Some(elapsed) => {
                let back = ((elapsed - PARRY_HOLD_SECONDS) / PARRY_RETURN_SECONDS).clamp(0.0, 1.0);
                PARRY_SCALE + (1.0 - PARRY_SCALE) * back * back * (3.0 - 2.0 * back)
            }
            None => 1.0,
        }
    }

    /// Advances one gameplay frame and returns bounded host requests.
    pub fn step(&mut self, frame: &Frame) -> Output {
        let dt = if frame.seconds.is_finite() {
            frame.seconds.clamp(0.0, 0.25)
        } else {
            0.0
        };
        self.clock += dt;
        self.advance_parry(dt);
        let visual = dt * self.time_scale();
        self.visual_clock += visual;
        self.read_inputs(frame);
        let rotate = self.track_lock(frame, dt);
        self.track_meteor(frame, dt);
        let rig = self.compose_rig(frame, visual);
        let mut output = Output {
            rig: Some(rig),
            rotate,
            label: Some(if self.lock.is_some() {
                "Lock-on: Hollow Warden"
            } else {
                "Free camera"
            }),
            ..Output::default()
        };
        let commands = self.commands.len().min(MAX_COMMANDS_PER_FRAME);
        output.commands = self.commands.drain(..commands).collect();
        let cues = self.cues.len().min(MAX_CUES_PER_FRAME);
        output.cues = self.cues.drain(..cues).collect();
        output
    }

    fn cue(&mut self, name: &'static str, values: Vec<f32>) {
        self.cues.push_back(Cue { name, values });
    }

    fn command(&mut self, text: &str) {
        self.commands.push_back(text.to_owned());
    }

    fn aim_values(frame: &Frame) -> Vec<f32> {
        let dir = look(frame.yaw, frame.pitch);
        vec![
            frame.eye[0],
            frame.eye[1],
            frame.eye[2],
            dir[0],
            dir[1],
            dir[2],
        ]
    }

    fn read_inputs(&mut self, frame: &Frame) {
        let pressed = &frame.pressed;
        let held = &frame.held;
        if has(pressed, "KeyR") {
            self.toggle_lock(frame);
        }
        if has(pressed, "KeyF") {
            self.parry = Some(0.0);
            self.cue(
                "camera.parry",
                vec![PARRY_SCALE, PARRY_HOLD_SECONDS + PARRY_RETURN_SECONDS],
            );
        }
        let sneak_tap = has(pressed, "ShiftLeft");
        let double_tap = sneak_tap && self.clock - self.last_sneak <= DOUBLE_TAP_SECONDS;
        if sneak_tap {
            self.last_sneak = self.clock;
        }
        if double_tap || (has(pressed, "Space") && has(held, "ShiftLeft")) {
            let sign = if has(held, "KeyD") { -1.0 } else { 1.0 };
            self.pulses.push(Pulse {
                kick: DODGE,
                elapsed: 0.0,
                sign,
            });
            self.cue("camera.dodge", vec![sign]);
            self.last_sneak = f32::NEG_INFINITY;
        }
        if has(pressed, "Digit1") && self.charging.is_none() {
            self.charging = Some(0.0);
            self.command("/ability charge start");
            self.cue("ability.charge.start", Vec::new());
        }
        if self.charging.is_some() && !has(held, "Digit1") {
            self.charging = None;
            self.command("/ability charge stop");
            self.cue("ability.charge.stop", Vec::new());
        }
        if has(pressed, "Digit2") && !self.beaming {
            self.beaming = true;
            self.command("/ability beam start");
            self.cue("ability.beam.start", Self::aim_values(frame));
        }
        if self.beaming && !has(held, "Digit2") {
            self.beaming = false;
            self.command("/ability beam stop");
            self.cue("ability.beam.stop", Vec::new());
        }
        if has(pressed, "Digit3") {
            self.pulses.push(Pulse {
                kick: FLASH,
                elapsed: 0.0,
                sign: 1.0,
            });
            self.command("/ability flash");
            self.cue("ability.flash", Self::aim_values(frame));
        }
        if has(pressed, "Digit4") && matches!(self.meteor, Meteor::Idle) {
            self.meteor = Meteor::Rising { elapsed: 0.0 };
            self.command("/ability meteor");
            self.cue("ability.meteor", frame.eye.to_vec());
        }
    }

    fn toggle_lock(&mut self, frame: &Frame) {
        if self.lock.take().is_some() {
            self.cue("lockon.off", Vec::new());
            return;
        }
        let distance = |mob: &Mob| {
            let d = [
                mob.position[0] - frame.eye[0],
                mob.position[1] - frame.eye[1],
                mob.position[2] - frame.eye[2],
            ];
            d[0] * d[0] + d[1] * d[1] + d[2] * d[2]
        };
        let target = frame
            .mobs
            .iter()
            .filter(|mob| mob.type_id == BOSS_TYPE && distance(mob) <= LOCK_BREAK_BLOCKS.powi(2))
            .min_by(|a, b| distance(a).total_cmp(&distance(b)));
        if let Some(mob) = target {
            self.lock = Some(mob.runtime_id);
            let [x, y, z] = mob.position;
            #[allow(clippy::cast_precision_loss, reason = "a cue value, not an identity")]
            let id = mob.runtime_id as f32;
            self.cue("lockon.on", vec![id, x, y, z]);
        }
    }

    /// Eases the look toward the locked boss; a lost or distant target breaks the lock.
    fn track_lock(&mut self, frame: &Frame, dt: f32) -> Option<(f32, f32)> {
        let id = self.lock?;
        let Some(mob) = frame.mobs.iter().find(|mob| mob.runtime_id == id) else {
            self.lock = None;
            self.cue("lockon.off", Vec::new());
            return None;
        };
        let d = [
            mob.position[0] - frame.eye[0],
            mob.position[1] + LOCK_AIM_HEIGHT - frame.eye[1],
            mob.position[2] - frame.eye[2],
        ];
        let horizontal = d[0].hypot(d[2]);
        if horizontal.hypot(d[1]) > LOCK_BREAK_BLOCKS {
            self.lock = None;
            self.cue("lockon.off", Vec::new());
            return None;
        }
        if horizontal < 0.5 {
            return None;
        }
        let yaw = (-d[0]).atan2(-d[2]);
        let pitch = d[1].atan2(horizontal);
        let blend = 1.0 - (-LOCK_TRACK_PER_SECOND * dt).exp();
        let limit = MAX_CAMERA_DELTA_RADIANS;
        let yaw_delta = (wrap(yaw - frame.yaw) * blend).clamp(-limit, limit);
        let pitch_delta = ((pitch - frame.pitch) * blend).clamp(-limit, limit);
        Some((yaw_delta, pitch_delta))
    }

    /// Detects the slam landing from the eye's vertical motion after the leap.
    fn track_meteor(&mut self, frame: &Frame, dt: f32) {
        let eye_y = frame.eye[1];
        let velocity = match self.last_eye_y {
            Some(previous) if dt > 0.0 => (eye_y - previous) / dt,
            _ => 0.0,
        };
        self.last_eye_y = Some(eye_y);
        self.meteor = match self.meteor {
            Meteor::Idle => Meteor::Idle,
            Meteor::Rising { elapsed } | Meteor::Falling { elapsed }
                if elapsed + dt > METEOR_TIMEOUT_SECONDS =>
            {
                Meteor::Idle
            }
            Meteor::Rising { elapsed } => {
                if elapsed + dt >= METEOR_MIN_AIR_SECONDS && velocity < FALLING_BLOCKS_PER_SECOND {
                    Meteor::Falling {
                        elapsed: elapsed + dt,
                    }
                } else {
                    Meteor::Rising {
                        elapsed: elapsed + dt,
                    }
                }
            }
            Meteor::Falling { elapsed } => {
                if velocity >= -0.1 {
                    self.slam_shake = SLAM_SHAKE_SECONDS;
                    self.cue("ability.meteor.land", frame.eye.to_vec());
                    Meteor::Idle
                } else {
                    Meteor::Falling {
                        elapsed: elapsed + dt,
                    }
                }
            }
        };
    }

    fn advance_parry(&mut self, dt: f32) {
        if let Some(elapsed) = &mut self.parry {
            *elapsed += dt;
            if *elapsed >= PARRY_HOLD_SECONDS + PARRY_RETURN_SECONDS {
                self.parry = None;
            }
        }
    }

    /// Shoulder offset, kicks and shakes, all advanced on visual time and clamped to host bounds.
    fn compose_rig(&mut self, frame: &Frame, visual: f32) -> Rig {
        let goal = if self.lock.is_some() {
            LOCKED_SHOULDER
        } else {
            SHOULDER
        };
        let blend = 1.0 - (-RIG_BLEND_PER_SECOND * visual).exp();
        for (current, goal) in self.offset.iter_mut().zip(goal) {
            *current += (goal - *current) * blend;
        }
        let mut roll = 0.0;
        let mut fov = 0.0;
        for pulse in &mut self.pulses {
            pulse.elapsed += visual;
            let strength = pulse.envelope();
            roll += pulse.kick.roll * pulse.sign * strength;
            fov += pulse.kick.fov * strength;
        }
        self.pulses.retain(|pulse| !pulse.done());
        if let Some(elapsed) = &mut self.charging {
            *elapsed += visual;
            fov += CHARGE_FOV * (*elapsed / CHARGE_FOV_SECONDS).min(1.0);
        }
        if self.beaming {
            fov += BEAM_FOV;
        }
        self.slam_shake = (self.slam_shake - visual).max(0.0);
        let shake = SLAM_SHAKE * (self.slam_shake / SLAM_SHAKE_SECONDS).powi(2)
            + if self.beaming { BEAM_SHAKE } else { 0.0 }
            + if self.charging.is_some() {
                CHARGE_SHAKE
            } else {
                0.0
            };
        let seed = self.visual_clock * 37.0 + frame.eye[0];
        let jitter = [noise(seed), noise(seed + 1.7), noise(seed + 3.1)];
        roll += jitter[2] * shake * 0.2;
        Rig {
            offset: [
                (self.offset[0] + jitter[0] * shake)
                    .clamp(-MAX_RIG_SIDE_BLOCKS, MAX_RIG_SIDE_BLOCKS),
                (self.offset[1] + jitter[1] * shake)
                    .clamp(-MAX_RIG_VERTICAL_BLOCKS, MAX_RIG_VERTICAL_BLOCKS),
                self.offset[2].clamp(0.0, MAX_RIG_BACK_BLOCKS),
            ],
            roll: roll.clamp(-MAX_RIG_ROLL_RADIANS, MAX_RIG_ROLL_RADIANS),
            fov_delta: fov.clamp(-MAX_RIG_FOV_DELTA_DEGREES, MAX_RIG_FOV_DELTA_DEGREES),
        }
    }
}

#[cfg(test)]
#[path = "director_tests.rs"]
mod tests;
