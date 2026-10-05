//! Effect state: timers advanced per frame, turned into primitives and pass parameters.

use crate::*;
use std::collections::VecDeque;
use tuning::*;

const EYE_HEIGHT: f32 = 1.62;
const HISTORY_SECONDS: f32 = 0.6;

#[derive(Clone, Copy, Debug)]
pub(crate) struct Timed {
    pub(crate) at: Point,
    pub(crate) age: f32,
    pub(crate) life: f32,
    pub(crate) radius: f32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Slash {
    pub(crate) eye: Point,
    pub(crate) yaw: f32,
    pub(crate) pitch: f32,
    pub(crate) age: f32,
    pub(crate) mirrored: bool,
}

#[derive(Default)]
pub struct Effects {
    pub(crate) time: f32,
    pub(crate) view: Option<(Point, f32, f32)>,
    pub(crate) history: VecDeque<(Point, f32)>,
    pub(crate) attack_was_held: bool,
    pub(crate) telegraphs: Vec<Timed>,
    pub(crate) craters: Vec<Timed>,
    pub(crate) shockwaves: Vec<Timed>,
    pub(crate) dust: Vec<Timed>,
    pub(crate) afterimages: Vec<Timed>,
    pub(crate) slashes: Vec<Slash>,
    pub(crate) pending_flash: Option<(Point, f32)>,
    pub(crate) pending_land: Option<f32>,
    /// Set once another mod drives abilities through cues; own key bindings then stand down.
    pub(crate) cue_driven: bool,
    pub(crate) charge: Option<f32>,
    pub(crate) beam: Option<f32>,
    pub(crate) beam_hit: bool,
    pub(crate) flight: bool,
    pub(crate) boss: Option<Point>,
    pub(crate) boss_aura: bool,
    pub(crate) health: f32,
    pub(crate) death: Option<f32>,
    phase2: Option<f32>,
    pub(crate) trauma: f32,
    pub(crate) impact_frames: u8,
    pub(crate) speed_lines: f32,
    pub(crate) last_space: f32,
}

impl Effects {
    pub fn new() -> Self {
        Self {
            boss_aura: true,
            health: 1.0,
            last_space: f32::NEG_INFINITY,
            ..Default::default()
        }
    }

    pub(crate) fn feet(&self) -> Option<Point> {
        self.view
            .map(|(eye, ..)| [eye[0], eye[1] - EYE_HEIGHT, eye[2]])
    }

    pub(crate) fn boss_position(&self) -> Point {
        self.boss.unwrap_or(BOSS_FALLBACK_POSITION)
    }

    pub fn trigger(&mut self, event: Event) {
        match event {
            Event::ChargeStart => self.charge = Some(0.0),
            Event::ChargeStop => self.charge = None,
            Event::BeamStart => {
                self.beam = Some(0.0);
                self.beam_hit = false;
            }
            Event::BeamStop => self.beam = None,
            Event::BeamHit => {
                self.impact_frames = IMPACT_FRAMES;
                self.trauma = self.trauma.max(0.8);
            }
            Event::FlashStep { from } => {
                self.pending_flash = Some((from, 0.12));
                self.speed_lines = 1.0;
                self.trauma = self.trauma.max(0.3);
            }
            Event::MeteorLeap => {
                if let Some(feet) = self.feet() {
                    self.dust.push(timed(feet, 1.0, 3.0));
                }
                if !self.cue_driven {
                    self.pending_land = Some(METEOR_AIR_SECONDS);
                }
            }
            Event::MeteorLand { at } => {
                self.pending_land = None;
                self.slam(at, CRATER_RADIUS, SHOCKWAVE_RADIUS);
                self.impact_frames = IMPACT_FRAMES;
                self.trauma = 1.0;
            }
            Event::FlightStart => self.flight = true,
            Event::FlightStop => self.flight = false,
            Event::BossTelegraph {
                at,
                radius,
                seconds,
            } => {
                self.telegraphs.push(timed(at, seconds.max(0.1), radius));
            }
            Event::BossSlam { at } => {
                self.slam(at, CRATER_RADIUS * 0.8, SHOCKWAVE_RADIUS * 0.8);
                self.trauma = self.trauma.max(0.7);
            }
            Event::BossPhase2 => self.phase2 = Some(0.0),
            Event::BossPosition { at } => self.boss = Some(at),
            Event::PlayerHealth { fraction } => self.health = fraction.clamp(0.0, 1.0),
            Event::PlayerDied => self.death = Some(0.0),
            Event::PlayerRespawn => {
                self.death = None;
                self.health = 1.0;
            }
        }
    }

    fn slam(&mut self, at: Point, crater: f32, shockwave: f32) {
        self.craters.push(timed(at, CRATER_SECONDS, crater));
        self.shockwaves
            .push(timed(at, SHOCKWAVE_SECONDS, shockwave));
        self.dust.push(timed(at, 1.2, crater * 1.2));
    }

    /// Standalone bindings: 1 charge, 2 beam, 3 flash step, 4 meteor, Space twice for flight.
    pub fn handle_key(&mut self, key: &str) {
        if self.cue_driven && key != "Space" {
            return;
        }
        match key {
            "Digit1" => self.trigger(if self.charge.is_some() {
                Event::ChargeStop
            } else {
                Event::ChargeStart
            }),
            "Digit2" => self.trigger(if self.beam.is_some() {
                Event::BeamStop
            } else {
                Event::BeamStart
            }),
            "Digit3" => {
                if let Some(from) = self.feet() {
                    self.trigger(Event::FlashStep { from });
                }
            }
            "Digit4" => self.trigger(Event::MeteorLeap),
            "Space" => {
                if self.time - self.last_space < 0.3 {
                    self.trigger(if self.flight {
                        Event::FlightStop
                    } else {
                        Event::FlightStart
                    });
                    self.last_space = f32::NEG_INFINITY;
                } else {
                    self.last_space = self.time;
                }
            }
            _ => {}
        }
    }

    /// Maps showcase cues (see SPEC.md) to events; values are world coordinates.
    pub fn handle_cue(&mut self, name: &str, values: &[f32]) {
        let point = |offset: usize| -> Option<Point> {
            values.get(offset..offset + 3).map(|v| [v[0], v[1], v[2]])
        };
        let feet = |offset: usize| point(offset).map(|p| [p[0], p[1] - EYE_HEIGHT, p[2]]);
        if name.starts_with("ability.") {
            self.cue_driven = true;
        }
        let event = match name {
            "ability.charge.start" => Event::ChargeStart,
            "ability.charge.stop" => Event::ChargeStop,
            "ability.beam.start" => Event::BeamStart,
            "ability.beam.stop" => Event::BeamStop,
            "ability.beam.hit" => Event::BeamHit,
            "ability.flash" => match feet(0).or(self.feet()) {
                Some(from) => Event::FlashStep { from },
                None => return,
            },
            "ability.meteor" => Event::MeteorLeap,
            "ability.meteor.land" => match feet(0).or(self.feet()) {
                Some(at) => Event::MeteorLand { at },
                None => return,
            },
            "ability.flight.start" => Event::FlightStart,
            "ability.flight.stop" => Event::FlightStop,
            "camera.dodge" => {
                self.speed_lines = self.speed_lines.max(0.6);
                return;
            }
            "camera.parry" => Event::BeamHit,
            "lockon.on" | "boss.position" => {
                let offset = usize::from(name == "lockon.on");
                match point(offset) {
                    Some(at) => Event::BossPosition { at },
                    None => return,
                }
            }
            "boss.telegraph" => match (point(0), values.get(3), values.get(4)) {
                (Some(at), Some(&radius), Some(&seconds)) => Event::BossTelegraph {
                    at,
                    radius: radius.clamp(0.5, 32.0),
                    seconds,
                },
                _ => return,
            },
            "boss.slam" => match point(0) {
                Some(at) => Event::BossSlam { at },
                None => return,
            },
            "boss.phase2" => Event::BossPhase2,
            "player.health" => match values.first() {
                Some(&fraction) => Event::PlayerHealth { fraction },
                None => return,
            },
            "player.died" => Event::PlayerDied,
            "player.respawn" => Event::PlayerRespawn,
            _ => return,
        };
        self.trigger(event);
    }

    /// Forgets the boss once it leaves the entity snapshot, so its aura goes with it.
    pub fn lose_boss(&mut self) {
        self.boss = None;
    }

    /// Tracks the boss from an entity snapshot; crossing half health starts phase 2.
    pub fn observe_boss(&mut self, at: Point, health: Option<f32>) {
        self.boss = Some(at);
        if health.is_some_and(|fraction| fraction > 0.0 && fraction <= 0.5) && self.phase2.is_none()
        {
            self.trigger(Event::BossPhase2);
        }
    }

    /// Debug-panel stand-ins for server events.
    pub fn handle_panel(&mut self, id: &str, value: f32) {
        let boss = self.boss_position();
        match id {
            "health" => self.trigger(Event::PlayerHealth { fraction: value }),
            "telegraph" => self.trigger(Event::BossTelegraph {
                at: boss,
                radius: TELEGRAPH_RADIUS,
                seconds: TELEGRAPH_SECONDS,
            }),
            "slam" => self.trigger(Event::BossSlam { at: boss }),
            "phase2" if value > 0.5 => self.trigger(Event::BossPhase2),
            "phase2" => self.phase2 = None,
            "aura" => self.boss_aura = value > 0.5,
            "die" => self.trigger(Event::PlayerDied),
            "respawn" => self.trigger(Event::PlayerRespawn),
            _ => {}
        }
    }

    /// Current eye pose; a rising attack edge starts a slash.
    pub fn set_view(&mut self, eye: Point, yaw: f32, pitch: f32, attack_held: bool) {
        self.view = Some((eye, yaw, pitch));
        if attack_held && !self.attack_was_held {
            let mirrored = self.slashes.len() % 2 == 1;
            self.slashes.push(Slash {
                eye,
                yaw,
                pitch,
                age: 0.0,
                mirrored,
            });
        }
        self.attack_was_held = attack_held;
    }

    pub fn clear_view(&mut self) {
        self.view = None;
        self.attack_was_held = false;
    }

    /// Advances every effect by `dt` seconds and returns this frame's draw data.
    pub fn step(&mut self, dt: f32) -> Frame {
        let dt = dt.clamp(0.0, 0.25);
        self.time += dt;
        self.advance(dt);
        let mut primitives = Primitives {
            decals: Vec::new(),
            ribbons: Vec::new(),
            beams: Vec::new(),
            billboards: Vec::new(),
        };
        vfx::ground(self, &mut primitives);
        vfx::slashes(self, &mut primitives);
        vfx::abilities(self, &mut primitives);
        let beam = vfx::beam(self, &mut primitives);
        let passes = self.passes(beam);
        if self.impact_frames > 0 {
            self.impact_frames -= 1;
        }
        Frame { primitives, passes }
    }

    fn advance(&mut self, dt: f32) {
        if let Some(feet) = self.feet() {
            self.history.push_back((feet, self.time));
        }
        while self
            .history
            .front()
            .is_some_and(|(_, t)| self.time - t > HISTORY_SECONDS)
        {
            self.history.pop_front();
        }
        let mut slams = Vec::new();
        for list in [
            &mut self.telegraphs,
            &mut self.craters,
            &mut self.shockwaves,
            &mut self.dust,
            &mut self.afterimages,
        ] {
            for item in list.iter_mut() {
                item.age += dt;
            }
        }
        self.telegraphs.retain(|t| {
            let done = t.age >= t.life;
            if done {
                slams.push(t.at);
            }
            !done
        });
        for at in slams {
            self.trigger(Event::BossSlam { at });
        }
        for list in [
            &mut self.craters,
            &mut self.shockwaves,
            &mut self.dust,
            &mut self.afterimages,
        ] {
            list.retain(|item| item.age < item.life);
        }
        for slash in &mut self.slashes {
            slash.age += dt;
        }
        self.slashes.retain(|slash| slash.age < SLASH_SECONDS);
        self.charge = self.charge.map(|t| t + dt).filter(|t| *t < CHARGE_SECONDS);
        self.beam = self.beam.map(|t| t + dt).filter(|t| *t < BEAM_SECONDS);
        self.death = self.death.map(|t| t + dt);
        self.phase2 = self.phase2.map(|t| t + dt);
        self.trauma = (self.trauma - TRAUMA_DECAY_PER_SECOND * dt).max(0.0);
        if self.charge.is_some() {
            self.trauma = self.trauma.max(0.15);
        }
        if self.beam.is_some() {
            self.trauma = self.trauma.max(0.3);
        }
        self.speed_lines = (self.speed_lines - dt / 0.4).max(0.0);
        if let Some((from, delay)) = self.pending_flash {
            let delay = delay - dt;
            if delay <= 0.0 {
                self.pending_flash = None;
                vfx::spawn_afterimages(self, from);
            } else {
                self.pending_flash = Some((from, delay));
            }
        }
        if let Some(remaining) = self.pending_land {
            let remaining = remaining - dt;
            match (remaining <= 0.0, self.feet()) {
                (true, Some(at)) => self.trigger(Event::MeteorLand { at }),
                (true, None) => self.pending_land = None,
                _ => self.pending_land = Some(remaining),
            }
        }
    }

    fn passes(&self, beam: Option<(Point, Point)>) -> [PassUpdate; 4] {
        let boss = self.boss_position();
        let phase2 = self.phase2.is_some();
        let color = if phase2 {
            BOSS_PHASE2_COLOR
        } else {
            BOSS_AURA_COLOR
        };
        let aura = params(&[
            boss[0],
            boss[1],
            boss[2],
            if phase2 { 1.6 } else { 1.0 },
            color[0],
            color[1],
            color[2],
            1.8,
        ]);
        let bloom = beam.map_or([0.0; 16], |(start, end)| {
            params(&[
                start[0], start[1], start[2], 1.0, end[0], end[1], end[2], 26.0,
            ])
        });
        let pulse = self.phase2.map_or(0.0, |t| {
            let cycle = if t < PHASE2_PULSE_SECONDS {
                t
            } else {
                (t - PHASE2_PULSE_SECONDS) % PHASE2_PULSE_PERIOD
            };
            if cycle < PHASE2_PULSE_SECONDS {
                cycle / PHASE2_PULSE_SECONDS
            } else {
                0.0
            }
        });
        let pulse_strength = match self.phase2 {
            Some(t) if t < PHASE2_PULSE_SECONDS => 1.0,
            Some(_) if pulse > 0.0 => 0.45,
            _ => 0.0,
        } * (1.0 - pulse);
        let shake = self.trauma * self.trauma * MAX_SHAKE_PIXELS;
        let haze = if self.charge.is_some() { 1.0 } else { 0.0 };
        let warp = params(&[
            pulse,
            pulse_strength,
            boss[0],
            boss[1] + 1.4,
            boss[2],
            shake,
            self.time.floor(),
            haze,
            self.speed_lines,
        ]);
        let low = ((LOW_HEALTH_START - self.health) / (LOW_HEALTH_START - 0.1)).clamp(0.0, 1.0);
        let death = self
            .death
            .map_or(0.0, |t| (t / DEATH_FADE_SECONDS).min(1.0));
        let heartbeat = (self.time * std::f32::consts::TAU * 1.2)
            .sin()
            .max(0.0)
            .powi(4);
        let impact = self.impact_frames > 0;
        let impact_mode = if self.impact_frames == IMPACT_FRAMES {
            0.0
        } else {
            1.0
        };
        let grade = params(&[
            low,
            death,
            f32::from(u8::from(impact)),
            impact_mode,
            heartbeat,
        ]);
        let enabled = [
            self.boss_aura && self.boss.is_some(),
            beam.is_some(),
            pulse_strength > 0.0 || shake > 0.01 || haze > 0.0 || self.speed_lines > 0.0,
            low > 0.0 || death > 0.0 || impact,
        ];
        let values = [aura, bloom, warp, grade];
        std::array::from_fn(|i| PassUpdate {
            name: PASSES[i].0,
            enabled: enabled[i],
            params: if enabled[i] { values[i] } else { [0.0; 16] },
        })
    }
}
