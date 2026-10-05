//! Client part of the localserver showcase: places the intro screen and reports playback.

use mod_api::server_bundle::{
    Guest,
    cinnabar::server_experience::{media, messaging, scene},
};
use serde_json::{Value, json};

/// Indexed descriptor of the intro video; the host binds its frames to quads textured with it.
pub const DESCRIPTOR: &str = "media/intro.json";
const SCREEN_CHANNEL: &str = "showcase.screen";
const EVENT_CHANNEL: &str = "showcase.media";
const HOST_EVENT_CHANNEL: &str = "cinnabar.media";
const SCHEMA: u16 = 1;
const SCREEN_OBJECT: u32 = 1;

#[derive(Debug, PartialEq)]
pub enum Action {
    Play {
        centre: [f32; 3],
        size: [f32; 2],
        yaw_degrees: f32,
    },
    Pause,
    Stop,
}

/// Reads a `showcase.screen` record; the host has already checked it against the manifest.
pub fn parse_screen(record: &[u8]) -> Option<Action> {
    let fields: Vec<Value> = serde_json::from_slice(record).ok()?;
    let integer = |index: usize| fields.get(index)?.get("value")?.as_i64();
    let blocks = |index: usize| integer(index).map(|cm| cm as f32 / 100.0);
    let action = fields.get(6)?.get("value")?.as_u64()?;
    Some(match action {
        0 => Action::Play {
            centre: [blocks(0)?, blocks(1)?, blocks(2)?],
            size: [blocks(3)?, blocks(4)?],
            yaw_degrees: integer(5)? as f32,
        },
        1 => Action::Pause,
        2 => Action::Stop,
        _ => return None,
    })
}

/// A media-textured quad whose local +Z face looks along Minecraft yaw `yaw_degrees`.
pub fn quad(centre: [f32; 3], size: [f32; 2], yaw_degrees: f32) -> Vec<u8> {
    let half = -yaw_degrees.to_radians() / 2.0;
    json!({
        "kind": "quad",
        "texture": DESCRIPTOR,
        "transform": [centre[0], centre[1], centre[2], 0.0, half.sin(), 0.0, half.cos(), 1.0, 1.0, 1.0],
        "size": size,
    })
    .to_string()
    .into_bytes()
}

/// Turns a host `[text id, choice event, integer ms]` record into a `showcase.media` record.
pub fn forward_event(record: &[u8]) -> Option<Vec<u8>> {
    let fields: Vec<Value> = serde_json::from_slice(record).ok()?;
    if fields.get(0)?.get("value")?.as_str()? != DESCRIPTOR {
        return None;
    }
    let event = fields.get(1)?.get("value")?.as_u64()?;
    let position = fields.get(2)?.get("value")?.as_i64()?;
    Some(
        json!([
            {"type": "choice", "value": event},
            {"type": "integer", "value": position},
        ])
        .to_string()
        .into_bytes(),
    )
}

struct Screen;

impl Guest for Screen {
    /// Nothing is shown until the server places the screen.
    fn init() {}

    /// Denied host calls leave the server's timeout as the fallback.
    fn dispatch(channel: String, record_json: Vec<u8>) {
        match channel.as_str() {
            SCREEN_CHANNEL => match parse_screen(&record_json) {
                Some(Action::Play {
                    centre,
                    size,
                    yaw_degrees,
                }) => {
                    let _ = scene::put(SCREEN_OBJECT, Some(&quad(centre, size, yaw_degrees)));
                    let _ = media::control(DESCRIPTOR, media::Operation::Prepare, 0);
                    let _ = media::control(DESCRIPTOR, media::Operation::Play, 0);
                }
                Some(Action::Pause) => {
                    let _ = media::control(DESCRIPTOR, media::Operation::Pause, 0);
                }
                Some(Action::Stop) => {
                    let _ = media::control(DESCRIPTOR, media::Operation::Stop, 0);
                    let _ = scene::put(SCREEN_OBJECT, None);
                }
                None => {}
            },
            HOST_EVENT_CHANNEL => {
                if let Some(record) = forward_event(&record_json) {
                    let _ = messaging::send(EVENT_CHANNEL, SCHEMA, &record);
                }
            }
            _ => {}
        }
    }
}

mod_api::server_bundle::export_server_bundle!(Screen with_types_in mod_api::server_bundle);

#[cfg(test)]
mod tests {
    use super::*;

    fn integer(value: i64) -> Value {
        json!({"type": "integer", "value": value})
    }

    #[test]
    fn play_record_becomes_blocks_and_yaw() {
        let record = json!([
            integer(150),
            integer(-6400),
            integer(225),
            integer(2400),
            integer(1350),
            integer(90),
            {"type": "choice", "value": 0},
        ]);
        assert_eq!(
            parse_screen(record.to_string().as_bytes()),
            Some(Action::Play {
                centre: [1.5, -64.0, 2.25],
                size: [24.0, 13.5],
                yaw_degrees: 90.0,
            })
        );
    }

    #[test]
    fn quad_rotation_turns_local_front_toward_the_yaw() {
        let object: Value = serde_json::from_slice(&quad([0.0; 3], [24.0, 13.5], 90.0)).unwrap();
        let t: Vec<f32> = object["transform"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap() as f32)
            .collect();
        let (y, w) = (t[4], t[6]);
        assert!((y * y + w * w - 1.0).abs() < 1e-6);
        // Rotating +Z by the quaternion about Y: (2yw, 0, w² − y²); yaw 90 looks toward −X.
        assert!((2.0 * y * w + 1.0).abs() < 1e-6);
        assert!((w * w - y * y).abs() < 1e-6);
        assert_eq!(object["texture"], DESCRIPTOR);
    }

    #[test]
    fn host_events_forward_only_for_the_intro() {
        let record = json!([
            {"type": "text", "value": DESCRIPTOR},
            {"type": "choice", "value": 3},
            integer(52000),
        ]);
        let forwarded: Value =
            serde_json::from_slice(&forward_event(record.to_string().as_bytes()).unwrap()).unwrap();
        assert_eq!(
            forwarded,
            json!([{"type": "choice", "value": 3}, integer(52000)])
        );
        let other = json!([{"type": "text", "value": "media/other.json"}, {"type": "choice", "value": 3}, integer(0)]);
        assert!(forward_event(other.to_string().as_bytes()).is_none());
    }
}
