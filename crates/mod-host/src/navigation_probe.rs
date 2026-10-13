//! Offline component boundary and fuel probe; it does not establish live movement parity.
use anyhow::{Context, Result, ensure};
use mod_host::{
    GameplaySnapshot, GameplayVector3, ModGrants, ModHost, MotionObservation, TerrainBounds,
    TerrainCell, TerrainLayer, TerrainPosition, TerrainProperties, TerrainQuery, TerrainRegion,
    WorldObservation,
};
use std::path::Path;
/// Exercises a navigation package against a deterministic loaded flat-world fixture.
pub fn probe(path: &Path) -> Result<()> {
    let grants = ModGrants {
        terrain: true,
        automation: true,
        hotbar: true,
        player_state: true,
        camera: true,
        controls: true,
        hud: true,
        render: true,
        settings: true,
        screen: true,
        keys: true,
        ..Default::default()
    };
    let mut host = ModHost::load_package(path, grants)?;
    let mut feet = [0.5_f32, 64., 0.5];
    let mut yaw = 0_f32;
    let mut pitch = 0_f32;
    let mut peak = 0;
    let mut input_frames = 0;
    for frame in 0..2400 {
        let responses = host
            .take_terrain_queries()
            .into_iter()
            .map(region)
            .collect();
        host.supply_terrain(responses);
        host.observe_presentation(
            Some(WorldObservation {
                session: 1,
                dimension: 0,
                world: "probe".into(),
                position: (feet[0], feet[1], feet[2]),
                alive: true,
            }),
            None,
            None,
            vec![],
        )?;
        host.observe_modules(mod_host::module_observations::ModuleObservations {
            motion: Some(MotionObservation {
                session: 1,
                dimension: 0,
                yaw: yaw.to_degrees(),
                pitch: pitch.to_degrees(),
                velocity: (0., 0., 0.),
                on_ground: true,
            }),
            ..Default::default()
        })?;
        let mut controls = mod_host::empty_controls();
        controls.focused = true;
        controls.gameplay = true;
        controls.seconds = 1. / 60.;
        if frame == 0 {
            controls.events.push(mod_host::ControlEvent {
                id: "target_x".into(),
                value: 3.,
            });
            controls.events.push(mod_host::ControlEvent {
                id: "goto".into(),
                value: 1.,
            });
        }
        let snapshot = GameplaySnapshot {
            session: 1,
            dimension: 0,
            eye: GameplayVector3 {
                x: feet[0],
                y: feet[1] + 1.62,
                z: feet[2],
            },
            yaw,
            pitch,
            attack_held: false,
            frame_seconds: 1. / 60.,
            players: vec![],
        };
        host.frame_with_controls(false, Some(snapshot), controls)
            .with_context(|| {
                format!(
                    "navigation fixture frame {frame}, fuel={}",
                    host.last_fuel_used()
                )
            })?;
        peak = peak.max(host.last_fuel_used());
        if let Some(delta) = host.take_camera_delta() {
            yaw += delta.yaw;
            pitch += delta.pitch;
        }
        if let Some(input) = host.take_automation() {
            if input.forward {
                input_frames += 1;
                feet[0] -= yaw.sin() * 0.07;
                feet[2] -= yaw.cos() * 0.07;
            }
        }
        if host
            .label()
            .is_some_and(|label| label.contains("Objectif atteint"))
        {
            println!(
                "frames={frame} peak_fuel={peak} input_frames={input_frames} position={feet:?}"
            );
            return Ok(());
        }
    }
    ensure!(
        input_frames > 0,
        "navigation fixture produced no movement; label={:?}, peak_fuel={peak}",
        host.label()
    );
    anyhow::bail!(
        "navigation fixture did not complete; label={:?}, peak_fuel={peak}, position={feet:?}",
        host.label()
    )
}
fn region(query: TerrainQuery) -> TerrainRegion {
    let mut cells = Vec::new();
    for x in query.min.x..query.max.x {
        for y in query.min.y..query.max.y {
            for z in query.min.z..query.max.z {
                let solid = y < 64;
                let shape = TerrainBounds {
                    min: (0., 0., 0.),
                    max: (1., 1., 1.),
                };
                let properties = TerrainProperties {
                    collision: if solid { vec![shape] } else { vec![] },
                    selection: if solid { vec![shape] } else { vec![] },
                    friction: 0.6,
                    horizontal_speed_factor: 1.,
                    vertical_speed_factor: 1.,
                    fluid_height: 0.,
                    climbable: false,
                    water: false,
                    lava: false,
                    cobweb: false,
                    powder_snow: false,
                    scaffolding: false,
                    passable: !solid,
                    surface_response: "None".into(),
                };
                cells.push(TerrainCell {
                    position: TerrainPosition { x, y, z },
                    loaded: true,
                    layers: vec![TerrainLayer {
                        runtime_id: u32::from(solid),
                        identifier: Some(
                            if solid {
                                "minecraft:stone"
                            } else {
                                "minecraft:air"
                            }
                            .into(),
                        ),
                        canonical_state: None,
                        tags: vec![],
                        properties: Some(properties),
                    }],
                });
            }
        }
    }
    TerrainRegion {
        session: 1,
        dimension: 0,
        query,
        chunks: vec![],
        cells,
    }
}
