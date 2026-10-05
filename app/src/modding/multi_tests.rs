use super::*;

const EMPTY_COMPONENT: &str = r#"(component
    (core module $m (func (export "init")) (func (export "frame")))
    (core instance $i (instantiate $m))
    (func (export "init") (canon lift (core func $i "init")))
    (func (export "frame") (canon lift (core func $i "frame"))))"#;

fn component(directory: &Path, name: &str) -> PathBuf {
    let path = directory.join(name);
    std::fs::write(&path, EMPTY_COMPONENT).unwrap();
    path
}

#[test]
fn keys_reserved_by_an_earlier_mod_are_withheld_and_panel_events_go_to_the_owner() {
    let mut frame = mod_host::empty_controls();
    frame.keys_pressed = vec!["Digit1".into(), "KeyQ".into()];
    frame.keys_held = vec!["Digit1".into(), "ShiftLeft".into()];
    frame.events = vec![mod_host::ControlEvent {
        id: "enabled".into(),
        value: 1.0,
    }];
    let later = claim_controls(&frame, &["Digit1".into()], false);
    assert_eq!(later.keys_pressed, ["KeyQ"]);
    assert_eq!(later.keys_held, ["ShiftLeft"]);
    assert!(later.events.is_empty());
    let owner = claim_controls(&frame, &[], true);
    assert_eq!(owner.keys_pressed, frame.keys_pressed);
    assert_eq!(owner.events.len(), 1);
}

#[test]
fn labels_join_in_load_order_within_the_plain_text_limit() {
    let mut merged = Merged::default();
    assert_eq!(merged.label(), None);
    merged.labels = vec!["Lock-on".into(), "FX".into()];
    assert_eq!(merged.label().as_deref(), Some("Lock-on | FX"));
    merged.labels = vec!["é".repeat(mod_host::MAX_LABEL_BYTES)];
    let label = merged.label().unwrap();
    assert!(label.len() <= mod_host::MAX_LABEL_BYTES && label.chars().all(|c| c == 'é'));
}

#[test]
fn set_files_keep_order_and_per_mod_grants() {
    let directory = tempfile::tempdir().unwrap();
    let set = directory.path().join("mods.json");
    let write = |json: &str| std::fs::write(&set, json).unwrap();
    write(
        r#"{"version":1,"mods":[
            {"component":"/mods/camera.wasm","grants":{"camera":true,"commands":["ability"]}},
            {"component":"/mods/effects.wasm","grants":{"render":true}}]}"#,
    );
    let mods = read_set(&set).unwrap();
    assert_eq!(mods[0].0, Path::new("/mods/camera.wasm"));
    assert!(mods[0].1.camera && !mods[0].1.render);
    assert_eq!(mods[0].1.commands, ["ability"]);
    assert!(mods[1].1.render && !mods[1].1.camera);

    for invalid in [
        r#"{"version":2,"mods":[{"component":"/a.wasm"}]}"#,
        r#"{"version":1,"mods":[]}"#,
        r#"{"version":1,"mods":[{"component":"relative.wasm"}]}"#,
        r#"{"version":1,"mods":[{"component":"/a.wasm","grants":{"root":true}}]}"#,
    ] {
        write(invalid);
        assert!(read_set(&set).is_err(), "{invalid}");
    }
    let many = format!(
        r#"{{"version":1,"mods":[{}]}}"#,
        vec![r#"{"component":"/a.wasm"}"#; MAX_LOADED_MODS + 1].join(",")
    );
    write(&many);
    assert!(read_set(&set).is_err());
}

#[test]
fn a_set_loads_every_valid_component_in_order_and_skips_broken_ones() {
    let directory = tempfile::tempdir().unwrap();
    let broken = directory.path().join("broken.wat");
    std::fs::write(&broken, "(component").unwrap();
    let mut app = bevy::prelude::App::new();
    super::super::configure_set(
        &mut app,
        vec![
            (broken, ModGrants::default()),
            (
                component(directory.path(), "camera.wat"),
                ModGrants {
                    camera: true,
                    ..Default::default()
                },
            ),
            (
                component(directory.path(), "effects.wat"),
                ModGrants {
                    render: true,
                    ..Default::default()
                },
            ),
        ],
    );
    let runtime = app.world().resource::<ModRuntime>();
    assert_eq!(runtime.host_count(), 2);
    assert!(runtime.host(0).grants().camera);
    assert!(runtime.host(1).grants().render);
    assert_eq!(runtime.panel_owner(), 0);
    assert!(runtime.reserved_keys().is_empty());
}

/// The packaged camera and effects mods run side by side: cues from one drive the other's render.
#[test]
fn packaged_camera_cues_drive_the_effects_mod() {
    let (Some(camera), Some(effects)) = (
        std::env::var_os("CINNABAR_SHOWCASE_COMPONENT"),
        std::env::var_os("CINNABAR_EFFECTS_COMPONENT"),
    ) else {
        eprintln!(
            "skipping packaged_camera_cues_drive_the_effects_mod: fixture unavailable; requires \
            CINNABAR_SHOWCASE_COMPONENT and CINNABAR_EFFECTS_COMPONENT (packaged example mods)"
        );
        return;
    };
    let mut app = bevy::prelude::App::new();
    super::super::configure_set(
        &mut app,
        vec![
            (
                PathBuf::from(camera),
                ModGrants {
                    players: true,
                    camera: true,
                    controls: true,
                    entities: true,
                    commands: vec!["ability".into()],
                    ..Default::default()
                },
            ),
            (
                PathBuf::from(effects),
                ModGrants {
                    players: true,
                    controls: true,
                    entities: true,
                    render: true,
                    ..Default::default()
                },
            ),
        ],
    );
    let mut runtime = app.world_mut().resource_mut::<ModRuntime>();
    assert_eq!(runtime.host_count(), 2);
    let snapshot = mod_host::GameplaySnapshot {
        session: 1,
        dimension: 0,
        eye: mod_host::GameplayVector3 {
            x: 0.0,
            y: 65.6,
            z: 0.0,
        },
        yaw: 0.0,
        pitch: 0.0,
        attack_held: false,
        frame_seconds: 1.0 / 60.0,
        players: Vec::new(),
    };
    let mut frame = mod_host::empty_controls();
    frame.seconds = 1.0 / 60.0;
    frame.gameplay = true;
    frame.keys_pressed = vec!["Digit3".into()];
    let mut cues = Vec::new();
    let mut drew = false;
    for _ in 0..30 {
        let mut claimed = Vec::new();
        let mut merged = Merged::default();
        for index in 0..runtime.host_count() {
            let controls = claim_controls(&frame, &claimed, index == 0);
            claimed.extend(runtime.host(index).reserved_keys().iter().cloned());
            let host = runtime.host_mut(index);
            host.deliver_cues(cues.clone());
            host.frame_with_world(false, Some(snapshot.clone()), Vec::new(), controls)
                .unwrap();
            merged.absorb(host);
        }
        cues = merged.cues;
        frame.keys_pressed.clear();
        drew |= !runtime.host(1).render().0.primitives.is_empty();
        if merged.commands.is_empty() {
            assert!(claimed.contains(&"Digit3".to_owned()));
        } else {
            assert_eq!(merged.commands, ["/ability flash"]);
        }
    }
    assert!(runtime.host(0).is_active() && runtime.host(1).is_active());
    assert!(drew, "the flash cue reached the effects mod");
}
