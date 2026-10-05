//! Showcase client mod: over-the-shoulder camera, boss lock-on, dodge and parry camera
//! feel, and ability requests the local server authorises.

pub mod director;

use director::{Director, Frame, Mob};
use mod_api::bindings::{
    Guest,
    cinnabar::extension::{events, gameplay, hud, input},
};
use std::cell::RefCell;

thread_local! { static DIRECTOR: RefCell<Director> = RefCell::new(Director::default()); }

struct ShowcaseCamera;

impl Guest for ShowcaseCamera {
    fn init() {
        let keys = director::RESERVED_KEYS.map(str::to_owned).to_vec();
        if input::reserve_keys(&keys).is_err() {
            let _ = hud::set_label("Showcase camera: controls grant missing");
        }
    }

    /// Acts only on a current gameplay frame; the rig is retained by the host otherwise.
    fn frame() {
        let (Ok(Some(snapshot)), Ok(controls)) = (gameplay::read_frame(), input::read_controls())
        else {
            return;
        };
        let mobs = gameplay::read_mobs()
            .unwrap_or_default()
            .into_iter()
            .map(|mob| Mob {
                runtime_id: mob.runtime_id,
                type_id: mob.type_id,
                position: [mob.position.x, mob.position.y, mob.position.z],
            })
            .collect();
        let frame = Frame {
            seconds: snapshot.frame_seconds,
            pressed: controls.keys_pressed,
            held: controls.keys_held,
            eye: [snapshot.eye.x, snapshot.eye.y, snapshot.eye.z],
            yaw: snapshot.yaw,
            pitch: snapshot.pitch,
            mobs,
        };
        let output = DIRECTOR.with(|director| director.borrow_mut().step(&frame));
        if let Some(rig) = output.rig {
            let [x, y, z] = rig.offset;
            let _ = gameplay::set_camera_rig(Some(gameplay::CameraRig {
                offset: gameplay::Vector3 { x, y, z },
                roll: rig.roll,
                fov_delta: rig.fov_delta,
            }));
        }
        if let Some((yaw, pitch)) = output.rotate {
            let _ = gameplay::rotate(yaw, pitch);
        }
        let mut commands = output.commands.into_iter();
        while let Some(command) = commands.next() {
            if gameplay::request_command(&command).is_err() {
                // Retried in order next frame, so a rate-limited stop is never lost.
                let rest = std::iter::once(command).chain(commands).collect();
                DIRECTOR.with(|director| director.borrow_mut().requeue(rest));
                break;
            }
        }
        for cue in output.cues {
            let _ = events::emit(cue.name, &cue.values);
        }
        if let Some(label) = output.label {
            let _ = hud::set_label(label);
        }
    }
}

mod_api::bindings::export!(ShowcaseCamera with_types_in mod_api::bindings);
