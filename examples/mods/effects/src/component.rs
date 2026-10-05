//! The standalone component: keys and the F7 panel stand in for actions and server events.

use crate::{Effects, PASSES};
use mod_api::bindings::{
    Guest,
    cinnabar::extension::{events, gameplay, hud, input, panel, render},
};
use std::cell::RefCell;

const BOSS_TYPE: &str = "cinnabar:hollow_warden";

thread_local! { static EFFECTS: RefCell<Effects> = RefCell::new(Effects::new()); }

const PANEL: &str = r#"{"title":"Effects","toggle_key":"F7","dark":true,"controls":[
{"kind":"slider","id":"health","label":"Health","value":1,"min":0,"max":1,"step":0.05},
{"kind":"toggle","id":"aura","label":"Boss aura","value":true},
{"kind":"toggle","id":"phase2","label":"Phase 2","value":false},
{"kind":"button","id":"telegraph","label":"Boss telegraph"},
{"kind":"button","id":"slam","label":"Boss slam"},
{"kind":"button","id":"die","label":"You died"},
{"kind":"button","id":"respawn","label":"Respawn"}]}"#;

struct Component;

impl Guest for Component {
    fn init() {
        for (name, order, source) in PASSES {
            let spec = render::PassSpec {
                name: name.into(),
                order,
                source: source.into(),
                depth: false,
            };
            if let Err(error) = render::register_pass(&spec) {
                // Shows shader errors in game, which matters most while hot-reloading edits.
                let text: String = format!("FX {name}: {error}")
                    .chars()
                    .map(|c| {
                        if c.is_control() || c == '\u{a7}' {
                            ' '
                        } else {
                            c
                        }
                    })
                    .take(120)
                    .collect();
                let _ = hud::set_label(&text);
                return;
            }
        }
        let _ = panel::set_content(PANEL);
        let _ = input::reserve_keys(&[
            "Digit1".into(),
            "Digit2".into(),
            "Digit3".into(),
            "Digit4".into(),
        ]);
        let _ = hud::set_label("FX: 1 charge, 2 beam, 3 flash, 4 meteor, F7 panel");
    }

    fn frame() {
        EFFECTS.with(|effects| {
            let mut fx = effects.borrow_mut();
            let mut dt = 1.0 / 60.0;
            if let Ok(controls) = input::read_controls() {
                dt = controls.seconds;
                for key in &controls.keys_pressed {
                    fx.handle_key(key);
                }
                for event in &controls.events {
                    fx.handle_panel(&event.id, event.value);
                }
            }
            let was_cue_driven = fx.cue_driven;
            for cue in events::poll() {
                fx.handle_cue(&cue.name, &cue.values);
            }
            if fx.cue_driven && !was_cue_driven {
                // The actions mod owns the ability keys now; give them back to it.
                let _ = input::reserve_keys(&[]);
            }
            match gameplay::read_frame() {
                Ok(Some(frame)) => {
                    dt = frame.frame_seconds;
                    let eye = [frame.eye.x, frame.eye.y, frame.eye.z];
                    fx.set_view(eye, frame.yaw, frame.pitch, frame.attack_held);
                }
                _ => fx.clear_view(),
            }
            if let Ok(mobs) = gameplay::read_mobs()
                && let Some(boss) = mobs.iter().find(|mob| mob.type_id == BOSS_TYPE)
            {
                fx.observe_boss(
                    [boss.position.x, boss.position.y, boss.position.z],
                    boss.health
                        .zip(boss.max_health)
                        .map(|(h, max)| h / max.max(1.0)),
                );
            }
            let out = fx.step(dt);
            let _ = render::draw(&out.primitives);
            for pass in out.passes {
                let _ = render::update_pass(pass.name, pass.enabled, &pass.params);
            }
        });
    }
}

mod_api::bindings::export!(Component with_types_in mod_api::bindings);
