use super::*;

const DT: f32 = 1.0 / 60.0;

fn keys(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn boss(x: f32, z: f32) -> Mob {
    Mob {
        runtime_id: 9,
        type_id: BOSS_TYPE.into(),
        position: [x, 64.0, z],
    }
}

fn frame(pressed: &[&str], held: &[&str]) -> Frame {
    Frame {
        seconds: DT,
        pressed: keys(pressed),
        held: keys(held),
        eye: [0.0, 65.6, 0.0],
        yaw: 0.0,
        pitch: 0.0,
        mobs: vec![boss(10.0, 0.0)],
    }
}

fn names(output: &Output) -> Vec<&'static str> {
    output.cues.iter().map(|cue| cue.name).collect()
}

#[test]
fn idle_frames_hold_an_over_the_shoulder_rig_within_host_bounds() {
    let mut director = Director::default();
    let output = director.step(&frame(&[], &[]));
    let rig = output.rig.unwrap();
    assert!(rig.offset[0] > 0.0 && rig.offset[1] > 0.0 && rig.offset[2] > 2.0);
    assert_eq!((rig.roll, rig.fov_delta), (0.0, 0.0));
    assert!(output.commands.is_empty() && output.cues.is_empty());
    assert_eq!(output.rotate, None);
}

#[test]
fn lock_on_targets_the_nearest_boss_and_eases_the_look_toward_it() {
    let mut director = Director::default();
    let mut input = frame(&["KeyR"], &[]);
    input.mobs = vec![
        Mob {
            runtime_id: 3,
            type_id: "minecraft:zombie".into(),
            position: [1.0, 64.0, 0.0],
        },
        boss(10.0, 0.0),
        Mob {
            runtime_id: 11,
            ..boss(20.0, 0.0)
        },
    ];
    let output = director.step(&input);
    assert_eq!(director.locked(), Some(9));
    assert_eq!(names(&output), ["lockon.on"]);
    // The boss is at +X; facing -Z, turning toward +X is a negative (rightward) yaw.
    let (yaw, _) = output.rotate.unwrap();
    assert!(yaw < 0.0 && yaw.abs() <= MAX_CAMERA_DELTA_RADIANS);
    let full = (-10.0_f32).atan2(0.0);
    assert!(
        yaw.abs() < full.abs(),
        "tracking eases rather than snapping"
    );
}

#[test]
fn lock_on_breaks_when_the_boss_leaves_the_snapshot_and_toggles_off() {
    let mut director = Director::default();
    director.step(&frame(&["KeyR"], &[]));
    let mut gone = frame(&[], &[]);
    gone.mobs.clear();
    let output = director.step(&gone);
    assert_eq!(director.locked(), None);
    assert_eq!(names(&output), ["lockon.off"]);

    director.step(&frame(&["KeyR"], &[]));
    let output = director.step(&frame(&["KeyR"], &[]));
    assert_eq!(director.locked(), None);
    assert_eq!(names(&output), ["lockon.off"]);
}

#[test]
fn no_boss_means_no_lock() {
    let mut director = Director::default();
    let mut input = frame(&["KeyR"], &[]);
    input.mobs[0].type_id = "minecraft:zombie".into();
    let output = director.step(&input);
    assert_eq!(director.locked(), None);
    assert!(output.cues.is_empty() && output.rotate.is_none());
}

#[test]
fn held_abilities_send_start_then_stop_on_release() {
    let mut director = Director::default();
    let start = director.step(&frame(&["Digit1", "Digit2"], &["Digit1", "Digit2"]));
    assert_eq!(
        start.commands,
        ["/ability charge start", "/ability beam start"]
    );
    assert_eq!(
        names(&start),
        ["ability.charge.start", "ability.beam.start"]
    );
    assert_eq!(start.cues[1].values.len(), 6, "beam carries eye and aim");
    let held = director.step(&frame(&[], &["Digit1", "Digit2"]));
    assert!(held.commands.is_empty());
    let stop = director.step(&frame(&[], &[]));
    assert_eq!(
        stop.commands,
        ["/ability charge stop", "/ability beam stop"]
    );
}

#[test]
fn a_same_frame_tap_of_every_ability_stays_within_the_command_budget() {
    let mut director = Director::default();
    let all = ["Digit1", "Digit2", "Digit3", "Digit4"];
    let first = director.step(&frame(&all, &[]));
    assert_eq!(first.commands.len(), MAX_COMMANDS_PER_FRAME);
    let second = director.step(&frame(&[], &[]));
    assert_eq!(
        first.commands.len() + second.commands.len(),
        6,
        "taps queue their stop requests"
    );
}

#[test]
fn flash_step_kicks_the_fov_and_eases_back() {
    let mut director = Director::default();
    let output = director.step(&frame(&["Digit3"], &[]));
    assert_eq!(output.commands, ["/ability flash"]);
    let mut peak = output.rig.unwrap().fov_delta;
    for _ in 0..6 {
        peak = peak.max(director.step(&frame(&[], &[])).rig.unwrap().fov_delta);
    }
    assert!(peak > 10.0);
    for _ in 0..60 {
        director.step(&frame(&[], &[]));
    }
    assert_eq!(director.step(&frame(&[], &[])).rig.unwrap().fov_delta, 0.0);
}

#[test]
fn dodge_on_double_tap_sneak_rolls_toward_the_side_and_returns() {
    let mut director = Director::default();
    director.step(&frame(&["ShiftLeft"], &["ShiftLeft"]));
    let output = director.step(&frame(&["ShiftLeft"], &["ShiftLeft", "KeyD"]));
    assert_eq!(names(&output), ["camera.dodge"]);
    assert_eq!(output.cues[0].values, [-1.0]);
    let mut extreme = 0.0_f32;
    for _ in 0..10 {
        extreme = extreme.min(director.step(&frame(&[], &[])).rig.unwrap().roll);
    }
    assert!(extreme < -0.2);
    for _ in 0..60 {
        director.step(&frame(&[], &[]));
    }
    assert_eq!(director.step(&frame(&[], &[])).rig.unwrap().roll, 0.0);
}

#[test]
fn slow_sneak_taps_do_not_dodge_but_sneak_jump_does() {
    let mut director = Director::default();
    director.step(&frame(&["ShiftLeft"], &[]));
    let mut late = frame(&["ShiftLeft"], &[]);
    late.seconds = 0.25;
    director.step(&late);
    late.seconds = 0.2;
    assert!(director.step(&late).cues.is_empty());
    let output = director.step(&frame(&["Space"], &["ShiftLeft"]));
    assert_eq!(names(&output), ["camera.dodge"]);
}

#[test]
fn parry_slows_visual_time_then_returns_to_real_time() {
    let mut director = Director::default();
    let output = director.step(&frame(&["KeyF"], &[]));
    assert_eq!(names(&output), ["camera.parry"]);
    assert_eq!(director.time_scale(), PARRY_SCALE);
    for _ in 0..60 {
        director.step(&frame(&[], &[]));
    }
    assert_eq!(director.time_scale(), 1.0);
}

#[test]
fn parry_stretches_a_flash_kick_over_more_real_time() {
    let kick_frames = |parry: bool| {
        let mut director = Director::default();
        if parry {
            director.step(&frame(&["KeyF"], &[]));
        }
        director.step(&frame(&["Digit3"], &[]));
        (0..200)
            .take_while(|_| director.step(&frame(&[], &[])).rig.unwrap().fov_delta > 0.0)
            .count()
    };
    assert!(kick_frames(true) > kick_frames(false));
}

#[test]
fn meteor_landing_shakes_after_the_leap_falls_and_stops() {
    let mut director = Director::default();
    director.step(&frame(&["Digit4"], &[]));
    let mut y = 65.6;
    let mut landed = Vec::new();
    for velocity in [8.0, 8.0, 4.0, 0.0]
        .into_iter()
        .chain(std::iter::repeat_n(0.0, 12))
        .chain(std::iter::repeat_n(-12.0, 10))
        .chain([0.0, 0.0])
    {
        y += velocity * DT;
        let mut input = frame(&[], &[]);
        input.eye[1] = y;
        let output = director.step(&input);
        landed.extend(names(&output));
        if !landed.is_empty() {
            let rig = output.rig.unwrap();
            assert!(rig.offset[0].abs() <= MAX_RIG_SIDE_BLOCKS);
            break;
        }
    }
    assert_eq!(landed, ["ability.meteor.land"]);
}

#[test]
fn rigs_stay_inside_host_bounds_under_stacked_effects() {
    let mut director = Director::default();
    director.step(&frame(
        &["Digit1", "Digit2", "Digit3", "KeyR"],
        &["Digit1", "Digit2"],
    ));
    for _ in 0..30 {
        let rig = director
            .step(&frame(&["ShiftLeft"], &["Digit1", "Digit2", "ShiftLeft"]))
            .rig
            .unwrap();
        assert!(rig.offset[0].abs() <= MAX_RIG_SIDE_BLOCKS);
        assert!(rig.offset[1].abs() <= MAX_RIG_VERTICAL_BLOCKS);
        assert!((0.0..=MAX_RIG_BACK_BLOCKS).contains(&rig.offset[2]));
        assert!(rig.roll.abs() <= MAX_RIG_ROLL_RADIANS);
        assert!(rig.fov_delta.abs() <= MAX_RIG_FOV_DELTA_DEGREES);
    }
}

#[test]
fn rejected_commands_are_retried_in_order_before_new_ones() {
    let mut director = Director::default();
    let start = director.step(&frame(&["Digit2"], &["Digit2"]));
    assert_eq!(start.commands, ["/ability beam start"]);
    let stop = director.step(&frame(&["Digit3"], &[]));
    assert_eq!(stop.commands, ["/ability beam stop", "/ability flash"]);
    director.requeue(stop.commands);
    let retry = director.step(&frame(&["Digit4"], &[]));
    assert_eq!(
        retry.commands,
        ["/ability beam stop", "/ability flash", "/ability meteor"]
    );
}
