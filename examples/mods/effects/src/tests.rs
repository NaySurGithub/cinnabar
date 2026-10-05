use super::*;
use tuning::*;

const EYE: Point = [0.0, 65.62, 0.0];

fn host_check(primitives: &Primitives) -> Result<(), String> {
    let p = |v: &Vector3| [v.x, v.y, v.z];
    let c = |c: &Rgba| [c.r, c.g, c.b, c.a];
    let converted = mod_render::Primitives {
        decals: primitives
            .decals
            .iter()
            .map(|d| mod_render::Decal {
                center: p(&d.center),
                radius: d.radius,
                color: c(&d.color),
                progress: d.progress,
                style: mod_render::DecalStyle::Disc,
            })
            .collect(),
        ribbons: primitives
            .ribbons
            .iter()
            .map(|r| mod_render::Ribbon {
                points: r.points.iter().map(p).collect(),
                width: r.width,
                color: c(&r.color),
            })
            .collect(),
        beams: primitives
            .beams
            .iter()
            .map(|b| mod_render::Beam {
                start: p(&b.start),
                end: p(&b.end),
                width: b.width,
                color: c(&b.color),
                intensity: b.intensity,
            })
            .collect(),
        billboards: primitives
            .billboards
            .iter()
            .map(|b| mod_render::Billboard {
                position: p(&b.position),
                width: b.width,
                height: b.height,
                color: c(&b.color),
                pattern: mod_render::BillboardPattern::Solid,
                upright: b.upright,
            })
            .collect(),
    };
    mod_render::Primitives::default().append_checked(converted)
}

#[test]
fn every_pass_passes_the_host_sandbox() {
    for (name, _, source) in PASSES {
        mod_render::shader::compose(source, false).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(mod_render::pass_name_valid(name));
    }
}

#[test]
fn idle_effects_disable_every_costly_pass_but_the_boss_aura() {
    let mut fx = Effects::new();
    fx.set_view(EYE, 0.0, 0.0, false);
    let frame = fx.step(1.0 / 60.0);
    let enabled: Vec<_> = frame.passes.iter().map(|p| p.enabled).collect();
    assert_eq!(enabled, [true, false, false, false]);
    assert!(frame.primitives.decals.is_empty() && frame.primitives.billboards.is_empty());
    fx.handle_panel("aura", 0.0);
    assert!(fx.step(1.0 / 60.0).passes.iter().all(|p| !p.enabled));
}

#[test]
fn telegraph_expands_then_slams_into_crater_and_shockwave() {
    let mut fx = Effects::new();
    fx.trigger(Event::BossTelegraph {
        at: [3.0, 64.0, 3.0],
        radius: 6.0,
        seconds: 1.0,
    });
    let early = fx.step(0.25).primitives.decals;
    assert_eq!(early.len(), 1);
    assert_eq!(early[0].style, DecalStyle::Telegraph);
    fx.step(0.25);
    let late = fx.step(0.25).primitives.decals[0].progress;
    assert!(late > early[0].progress);
    let slam = fx.step(0.25).primitives.decals;
    let styles: Vec<_> = slam.iter().map(|d| d.style).collect();
    assert!(
        styles.contains(&DecalStyle::Crater) && styles.contains(&DecalStyle::Shockwave),
        "{styles:?}"
    );
    assert!(!styles.contains(&DecalStyle::Telegraph));
    assert!(
        fx.step(0.01).passes[2].enabled,
        "the slam shakes the screen"
    );
}

#[test]
fn impact_frame_flashes_for_exactly_three_frames() {
    let mut fx = Effects::new();
    fx.set_view(EYE, 0.0, 0.0, false);
    fx.trigger(Event::MeteorLand {
        at: [0.0, 64.0, 0.0],
    });
    let modes: Vec<_> = (0..5)
        .map(|_| {
            let grade = fx.step(1.0 / 60.0).passes[3];
            (grade.enabled && grade.params[2] > 0.5, grade.params[3])
        })
        .collect();
    assert_eq!(modes[..3], [(true, 0.0), (true, 1.0), (true, 1.0)]);
    assert!(!modes[3].0 && !modes[4].0);
}

#[test]
fn slashes_start_on_the_attack_edge_only() {
    let mut fx = Effects::new();
    fx.set_view(EYE, 0.3, -0.1, true);
    let first = fx.step(0.05).primitives.ribbons;
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].points.len(), 10);
    fx.set_view(EYE, 0.3, -0.1, true);
    assert_eq!(
        fx.step(0.05).primitives.ribbons.len(),
        1,
        "holding does not restart"
    );
    fx.set_view(EYE, 0.3, -0.1, false);
    fx.step(SLASH_SECONDS);
    fx.set_view(EYE, 0.3, -0.1, false);
    assert!(fx.step(0.01).primitives.ribbons.is_empty());
}

#[test]
fn beam_stops_at_the_boss_and_triggers_one_impact() {
    let mut fx = Effects::new();
    fx.trigger(Event::BossPosition {
        at: [0.0, 64.0, -10.0],
    });
    // Yaw zero looks towards -Z, straight at the boss.
    fx.set_view([0.0, 65.4, 0.0], 0.0, 0.0, false);
    fx.trigger(Event::BeamStart);
    let frame = fx.step(0.2);
    let beam = &frame.primitives.beams[0];
    assert!((beam.end.z + 10.0).abs() < 0.7, "{:?}", beam.end);
    assert!(frame.passes[1].enabled, "beam bloom follows the beam");
    assert!(
        frame.passes[3].params[2] > 0.5,
        "first hit flashes an impact frame"
    );
    for _ in 0..4 {
        fx.set_view([0.0, 65.4, 0.0], 0.0, 0.0, false);
        fx.step(0.016);
    }
    fx.set_view([0.0, 65.4, 0.0], 0.0, 0.0, false);
    assert!(
        fx.step(0.016).passes[3].params[2] < 0.5,
        "continued contact does not repeat it"
    );
}

#[test]
fn every_effect_at_once_stays_within_host_budgets() {
    let mut fx = Effects::new();
    for i in 0..60 {
        fx.set_view(
            [i as f32 * 0.4, 65.62, 0.0],
            i as f32 * 0.05,
            0.1,
            i % 2 == 0,
        );
        if i == 0 {
            for event in [
                Event::ChargeStart,
                Event::BeamStart,
                Event::FlightStart,
                Event::BossPhase2,
                Event::PlayerHealth { fraction: 0.1 },
                Event::PlayerDied,
                Event::MeteorLeap,
            ] {
                fx.trigger(event);
            }
            fx.handle_key("Digit3");
            for _ in 0..8 {
                fx.handle_panel("telegraph", 1.0);
            }
        }
        let frame = fx.step(1.0 / 30.0);
        host_check(&frame.primitives).unwrap();
        assert!(
            frame
                .passes
                .iter()
                .all(|p| p.params.iter().all(|v| v.is_finite()))
        );
    }
}

#[test]
fn low_health_and_death_drive_the_grade() {
    let mut fx = Effects::new();
    fx.handle_panel("health", 0.2);
    let grade = fx.step(0.1).passes[3];
    assert!(grade.enabled && grade.params[0] > 0.5 && grade.params[1] == 0.0);
    fx.handle_panel("die", 1.0);
    for _ in 0..12 {
        fx.step(0.25);
    }
    assert_eq!(fx.step(0.01).passes[3].params[1], 1.0);
    fx.handle_panel("respawn", 1.0);
    assert!(!fx.step(0.01).passes[3].enabled);
}

#[test]
fn showcase_cues_drive_abilities_and_silence_own_keys() {
    let mut fx = Effects::new();
    fx.set_view(EYE, 0.0, 0.0, false);
    fx.handle_cue("ability.charge.start", &[]);
    assert!(!fx.step(0.016).primitives.billboards.is_empty());
    fx.handle_cue("ability.charge.stop", &[]);
    fx.handle_key("Digit1");
    assert!(
        fx.step(0.016).primitives.billboards.is_empty(),
        "keys yield to cues"
    );
    fx.handle_cue("ability.meteor.land", &[2.0, 65.62, 2.0]);
    let decals = fx.step(0.016).primitives.decals;
    assert!(
        decals
            .iter()
            .any(|d| d.style == DecalStyle::Crater && (d.center.y - 64.0).abs() < 1e-4)
    );
    fx.handle_cue("lockon.on", &[7.0, 1.0, 64.0, 9.0]);
    assert_eq!(fx.step(0.016).passes[0].params[..3], [1.0, 64.0, 9.0]);
    fx.handle_cue("unknown.cue", &[f32::MAX]);
    fx.observe_boss([0.0, 64.0, 0.0], Some(0.4));
    assert!(
        fx.step(0.016).passes[2].enabled,
        "half health pulses phase 2"
    );
}

#[test]
fn server_markers_raise_their_boss_cues() {
    let mut fx = Effects::new();
    let (name, extra) = crate::marker_cue("cinnabar:fx_telegraph").unwrap();
    let mut values = vec![1.0, 2.0, 3.0];
    values.extend_from_slice(extra);
    fx.handle_cue(name, &values);
    assert_eq!(fx.telegraphs.len(), 1);
    assert!(crate::marker_cue("cinnabar:hollow_warden").is_none());
}
