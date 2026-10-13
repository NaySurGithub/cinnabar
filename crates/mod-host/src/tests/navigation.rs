use super::*;
fn load(granted: bool, trap: bool) -> (tempfile::TempDir, ModHost) {
    let package = include_str!("../../../mod-api/wit/extension.wit")
        .lines()
        .next()
        .unwrap()
        .trim_start_matches("package ")
        .trim_end_matches(';');
    let (name, version) = package.split_once('@').unwrap();
    let frame = format!(
        "i32.const 1 i32.const 1 i32.const 0 i32.const 0 i32.const 0 i32.const 1 i32.const 0 i32.const 0 i32.const 1 i32.const 1 i32.const 128 call $controls i32.const 3 i32.const 256 call $hotbar {}",
        if trap { "unreachable" } else { "" }
    );
    let source = include_str!("navigation.wat")
        .replace("$AUTOMATION", &format!("{name}/automation@{version}"))
        .replace("$FRAME", &frame);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("navigation.wat");
    std::fs::write(&path, source).unwrap();
    let host = ModHost::load_with_grants(
        &path,
        ModGrants {
            automation: granted,
            hotbar: granted,
            ..Default::default()
        },
    )
    .unwrap();
    (directory, host)
}
fn run(host: &mut ModHost) -> anyhow::Result<()> {
    let snapshot = GameplaySnapshot {
        session: 1,
        dimension: 0,
        eye: GameplayVector3 {
            x: 0.,
            y: 64.,
            z: 0.,
        },
        yaw: 0.,
        pitch: 0.,
        attack_held: false,
        frame_seconds: 0.016,
        players: vec![],
    };
    let mut controls = empty_controls();
    controls.focused = true;
    controls.gameplay = true;
    host.frame_with_controls(false, Some(snapshot), controls)
}
#[test]
fn automation_and_equipment_cross_the_real_component_boundary() {
    let (_dir, mut host) = load(true, false);
    run(&mut host).unwrap();
    let value = host.take_automation().unwrap();
    assert!(value.forward && value.jump && value.attack && value.use_item);
    assert_eq!(host.take_hotbar(), Some(3));
    assert!(host.take_hotbar().is_none());
    assert!(host.take_automation().is_none());
}
#[test]
fn denied_or_trapped_callbacks_publish_no_synthetic_input() {
    let (_dir, mut denied) = load(false, false);
    run(&mut denied).unwrap();
    assert!(denied.take_automation().is_none());
    assert!(denied.take_hotbar().is_none());
    let (_dir, mut trapped) = load(true, true);
    assert!(run(&mut trapped).is_err());
    assert!(!trapped.is_active());
    assert!(trapped.take_automation().is_none());
    assert!(trapped.take_hotbar().is_none());
}
