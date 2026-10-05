//! Per-frame primitive generation for each effect family.

use crate::{tuning::*, *};

pub(super) fn ground(fx: &Effects, out: &mut Primitives) {
    for t in &fx.telegraphs {
        let progress = t.age / t.life;
        out.decals.push(decal(
            t.at,
            t.radius,
            rgba(TELEGRAPH_COLOR, 0.9),
            progress,
            DecalStyle::Telegraph,
        ));
    }
    for c in &fx.craters {
        let fade = 1.0 - (c.age / c.life).powi(3);
        out.decals.push(decal(
            c.at,
            c.radius,
            rgba(SCORCH_COLOR, 0.95 * fade),
            c.age / c.life,
            DecalStyle::Crater,
        ));
    }
    for s in &fx.shockwaves {
        out.decals.push(decal(
            s.at,
            s.radius,
            rgba([1.0, 0.85, 0.6], 1.0),
            s.age / s.life,
            DecalStyle::Shockwave,
        ));
    }
    for d in &fx.dust {
        out.decals.push(decal(
            d.at,
            d.radius,
            rgba([0.55, 0.5, 0.42], 0.8),
            d.age / d.life,
            DecalStyle::Dust,
        ));
    }
}

/// Slash arcs sweep across the view from where the attack started, head first.
pub(super) fn slashes(fx: &Effects, out: &mut Primitives) {
    for slash in &fx.slashes {
        let (forward, right, up) = basis(slash.yaw, slash.pitch);
        let pivot = sub(slash.eye, scale(up, 0.35));
        let side = if slash.mirrored { -1.0 } else { 1.0 };
        let sweep = |t: f32| {
            let eased = 1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3);
            side * (-1.25 + 2.5 * eased)
        };
        let swing = SLASH_SECONDS * 0.7;
        let head = sweep(slash.age / swing);
        let tail = sweep((slash.age - SLASH_SECONDS * 0.35) / swing);
        let points = (0..10)
            .map(|i| {
                let a = head + (tail - head) * i as f32 / 9.0;
                let reach = add(scale(forward, a.cos()), scale(right, a.sin()));
                add(
                    add(pivot, scale(reach, SLASH_REACH)),
                    scale(up, -a.sin() * 0.3 * side),
                )
            })
            .collect();
        let fade = 1.0 - (slash.age / SLASH_SECONDS).powi(2);
        out.ribbons
            .push(ribbon(points, SLASH_WIDTH, rgba(SLASH_COLOR, fade)));
    }
}

pub(super) fn abilities(fx: &Effects, out: &mut Primitives) {
    let Some(feet) = fx.feet() else { return };
    let t = fx.time;
    if let Some(age) = fx.charge {
        let rise = (age / 0.3).min(1.0);
        let flicker = 1.0 + 0.06 * (t * 23.0).sin();
        out.billboards.push(billboard(
            add(feet, [0.0, 1.1, 0.0]),
            (2.0 * flicker, 3.0 * rise),
            rgba(CHARGE_COLOR, 0.9),
            BillboardPattern::Aura,
            true,
        ));
        out.billboards.push(billboard(
            add(feet, [0.0, 1.0, 0.0]),
            (2.6, 3.6 * rise),
            rgba(CHARGE_COLOR, 0.35),
            BillboardPattern::Aura,
            true,
        ));
        for i in 0..CHARGE_PARTICLES {
            let seed = i as f32;
            let phase = (seed * 0.618 + t * 0.7).fract();
            let angle = seed * 2.4 + t * 0.5;
            let r = 0.8 + 0.35 * (seed * 1.7).sin().abs();
            out.billboards.push(billboard(
                add(feet, [angle.cos() * r, phase * 3.2, angle.sin() * r]),
                (0.2 * (1.0 - phase) + 0.05, 0.2 * (1.0 - phase) + 0.05),
                rgba([1.0, 0.95, 0.7], (1.0 - phase) * rise),
                BillboardPattern::SoftDisc,
                false,
            ));
        }
        out.decals.push(decal(
            feet,
            2.6,
            rgba([0.6, 0.55, 0.45], 0.7),
            (t * 0.8).fract(),
            DecalStyle::Dust,
        ));
        out.decals.push(decal(
            feet,
            1.6,
            rgba(CHARGE_COLOR, 0.25 * rise),
            0.0,
            DecalStyle::Disc,
        ));
    }
    if fx.flight && fx.history.len() >= 2 {
        let points: Vec<Point> = fx
            .history
            .iter()
            .rev()
            .step_by((fx.history.len() / 24).max(1))
            .take(mod_api::MAX_RIBBON_POINTS)
            .map(|(p, _)| add(*p, [0.0, 0.9, 0.0]))
            .collect();
        if points.len() >= 2 {
            out.ribbons
                .push(ribbon(points, 1.1, rgba(CHARGE_COLOR, 0.8)));
        }
        out.billboards.push(billboard(
            add(feet, [0.0, 1.0, 0.0]),
            (1.6, 2.6),
            rgba(CHARGE_COLOR, 0.45),
            BillboardPattern::Aura,
            true,
        ));
    }
    for image in &fx.afterimages {
        let fade = 1.0 - image.age / image.life;
        out.billboards.push(billboard(
            add(image.at, [0.0, 0.95, 0.0]),
            (1.1, 1.95),
            rgba(FLASH_COLOR, 0.8 * fade * image.radius),
            BillboardPattern::Silhouette,
            true,
        ));
    }
}

/// Draws the channelled beam, its impact sphere and sparks; returns its endpoints for bloom.
pub(super) fn beam(fx: &mut Effects, out: &mut Primitives) -> Option<(Point, Point)> {
    let age = fx.beam?;
    let (eye, yaw, pitch) = fx.view?;
    let (forward, right, up) = basis(yaw, pitch);
    let start = add(
        add(eye, scale(right, 0.35)),
        add(scale(up, -0.25), scale(forward, 0.6)),
    );
    let mut end = add(start, scale(forward, BEAM_RANGE));
    let boss = add(fx.boss_position(), [0.0, 1.4, 0.0]);
    let along = dot(sub(boss, start), forward);
    if (0.0..BEAM_RANGE).contains(&along) {
        let closest = add(start, scale(forward, along));
        if length(sub(boss, closest)) < BOSS_HIT_RADIUS {
            end = closest;
            if !fx.beam_hit {
                fx.beam_hit = true;
                fx.trigger(Event::BeamHit);
            }
        }
    }
    let grow = (age / 0.15).min(1.0);
    let wobble = 1.0 + 0.08 * (fx.time * 40.0).sin();
    out.beams.push(beam_primitive(
        start,
        end,
        BEAM_WIDTH * grow * wobble,
        rgba(BEAM_COLOR, 1.0),
        2.0,
    ));
    out.beams.push(beam_primitive(
        start,
        end,
        BEAM_WIDTH * 2.4 * grow,
        rgba(BEAM_COLOR, 0.35),
        0.0,
    ));
    let pulse = 1.0 + 0.15 * (fx.time * 30.0).sin();
    out.billboards.push(billboard(
        end,
        (2.4 * pulse * grow, 2.4 * pulse * grow),
        rgba(BEAM_COLOR, 1.0),
        BillboardPattern::Sphere,
        false,
    ));
    for i in 0..6 {
        let seed = i as f32 * 1.37 + (fx.time * 12.0).floor();
        let jitter = [(seed * 3.1).sin(), (seed * 5.7).sin(), (seed * 7.3).sin()];
        out.billboards.push(billboard(
            add(end, scale(jitter, 1.1)),
            (0.7, 0.7),
            rgba([1.0, 1.0, 1.0], 0.9),
            BillboardPattern::Spark,
            false,
        ));
    }
    Some((start, end))
}

/// Afterimages fill the dash path; a standing flash step trails them behind the view.
pub(super) fn spawn_afterimages(fx: &mut Effects, from: Point) {
    let Some(feet) = fx.feet() else { return };
    let mut path = sub(feet, from);
    if length(path) < 1.0
        && let Some((_, yaw, _)) = fx.view
    {
        let (forward, ..) = basis(yaw, 0.0);
        path = scale(forward, 3.5);
    }
    let origin = sub(feet, path);
    for i in 0..FLASH_IMAGES {
        let k = i as f32 / FLASH_IMAGES as f32;
        let mut image = timed(
            add(origin, scale(path, k)),
            AFTERIMAGE_SECONDS * (0.6 + 0.4 * k),
            0.0,
        );
        image.radius = 0.4 + 0.6 * k;
        fx.afterimages.push(image);
    }
}

fn dot(a: Point, b: Point) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
