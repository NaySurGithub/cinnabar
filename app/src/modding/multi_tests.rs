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

fn runtime(directory: &Path, events: [bool; 3]) -> ModRuntime {
    let mut hosts = events.into_iter().enumerate().map(|(index, events)| {
        let grants = ModGrants {
            events,
            ..Default::default()
        };
        ModHost::load_with_grants(&component(directory, &format!("{index}.wat")), grants).unwrap()
    });
    let host = hosts.next().unwrap();
    ModRuntime {
        host,
        companions: hosts
            .map(|host| Companion {
                host,
                inbox: VecDeque::new(),
            })
            .collect(),
        inbox: VecDeque::new(),
        last_reload: std::time::Instant::now(),
        controls: mod_host::empty_controls(),
        reload_on_main: true,
        registration_identity: None,
        registration_request: None,
        suspended: false,
    }
}

fn cue(name: &str) -> ModCue {
    ModCue {
        name: name.into(),
        values: Vec::new(),
    }
}

#[test]
fn cues_reach_every_other_mod_with_the_events_grant_exactly_once() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = runtime(directory.path(), [true, false, true]);
    runtime.route_cues(0, &[cue("ability.flash")]);
    runtime.route_cues(2, &[cue("boss.slam")]);
    assert_eq!(runtime.take_inbox(0), [cue("boss.slam")]);
    assert!(runtime.take_inbox(1).is_empty(), "no events grant");
    assert_eq!(runtime.take_inbox(2), [cue("ability.flash")]);
    assert!(runtime.take_inbox(2).is_empty());
}

#[test]
fn inbox_overflow_drops_the_oldest_cues() {
    let directory = tempfile::tempdir().unwrap();
    let mut runtime = runtime(directory.path(), [true, true, false]);
    for index in 0..=MAX_CUE_INBOX {
        runtime.route_cues(1, &[cue(&format!("c{index}"))]);
    }
    let inbox = runtime.take_inbox(0);
    assert_eq!(inbox.len(), MAX_CUE_INBOX);
    assert_eq!(inbox[0], cue("c1"));
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
            {"component":"/mods/effects.wasm","grants":{"events":true}}]}"#,
    );
    let mods = read_set(&set).unwrap();
    assert_eq!(mods[0].0, Path::new("/mods/camera.wasm"));
    assert!(mods[0].1.camera && !mods[0].1.events);
    assert_eq!(mods[0].1.commands, ["ability"]);
    assert!(mods[1].1.events && !mods[1].1.camera);

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
                    events: true,
                    ..Default::default()
                },
            ),
        ],
    );
    let runtime = app.world().resource::<ModRuntime>();
    assert_eq!(runtime.host_count(), 2);
    assert!(runtime.host(0).grants().camera);
    assert!(runtime.host(1).grants().events);
}
