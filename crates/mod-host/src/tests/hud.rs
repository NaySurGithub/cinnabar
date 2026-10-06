//! The HUD layer, the crosshair target and text through the real probe component.

use super::screens::{action, probe, probe_with, text, value};
use crate::{
    HarvestRules, HudLayout, ModEvent, TargetBlock, TargetBlockHit, TargetBlockPos, TargetFrame,
    TargetHarvest, TargetHit, TargetLook, TargetMining, TextSource,
};
use server_experience::screen::{GuiSize, Rect, Value};
use std::sync::Arc;

fn hud_value<'a>(host: &'a crate::ModHost, name: &str) -> Option<&'a Value> {
    host.hud().values.get(name)
}

fn stone(name: &str) -> TargetLook {
    TargetLook {
        hit: Some(TargetHit::Block(TargetBlockHit {
            position: TargetBlockPos { x: 1, y: 64, z: -3 },
            face: 1,
            distance: 2.5,
            block: TargetBlock {
                identifier: "minecraft:stone".into(),
                states: Vec::new(),
                name: name.into(),
                pick: None,
                hardness: Some(1.5),
            },
        })),
        liquid: None,
        eye_in_liquid: false,
    }
}

/// Pickaxes from stone up suit the block; it needs one, and the hand cannot harvest it.
struct Pickaxes;

impl HarvestRules for Pickaxes {
    fn harvest(&self, candidates: &[String]) -> TargetHarvest {
        TargetHarvest {
            requires_tool: true,
            instant: false,
            breakable: true,
            held_can_harvest: false,
            effective: candidates
                .iter()
                .filter(|tool| !tool.contains("wooden"))
                .cloned()
                .collect(),
        }
    }
}

struct Text;

impl TextSource for Text {
    fn translate(&self, key: &str) -> Option<String> {
        (key == "probe.key").then(|| "Probe".to_owned())
    }

    fn width(&self, text: &str) -> f32 {
        6.0 * text.len() as f32
    }
}

fn frame(look: TargetLook) -> TargetFrame {
    TargetFrame {
        look: Some(look),
        mining: Some(TargetMining {
            position: TargetBlockPos { x: 1, y: 64, z: -3 },
            progress: 0.25,
            per_tick: 0.05,
            partial_tick: 0.5,
            harvestable: false,
        }),
        harvest: Some(Arc::new(Pickaxes)),
    }
}

fn hud_layout(bars: Option<Rect>) -> HudLayout {
    HudLayout {
        size: GuiSize {
            width: 480.0,
            height: 270.0,
            scale: 2.0,
        },
        boss_bars: bars,
    }
}

#[test]
fn hud_writes_commit_to_the_hud_store_and_need_the_hud_permission() {
    let (_dir, mut host) = probe();
    host.dispatch(vec![action("probe.hud")]).unwrap();
    assert_eq!(host.hud().template.as_deref(), Some("ui/hud.json"));
    assert_eq!(hud_value(&host, "#hud_set"), Some(&Value::Bool(true)));
    assert!(value(&host, "#hud_set").is_none());

    let (_dir, mut denied) = probe_with(|manifest| manifest.replace(", \"hud\"", ""));
    denied.dispatch(vec![action("probe.hud")]).unwrap();
    assert!(text(&denied, "#action").contains("hud permission denied"));
    assert!(denied.hud().template.is_none());
}

#[test]
fn a_hud_change_moves_only_the_hud_store() {
    let (_dir, mut host) = probe();
    let screens = host.screens().data.revision;
    let hud = host.hud().revision;
    let bars = Rect {
        x: 149.0,
        y: 0.0,
        width: 182.0,
        height: 20.0,
    };
    host.set_hud_layout(Some(hud_layout(Some(bars)))).unwrap();
    assert_eq!(
        hud_value(&host, "#hud_layout"),
        Some(&Value::Numbers(vec![480.0, 270.0, 20.0]))
    );
    assert_eq!(host.screens().data.revision, screens);
    assert!(host.hud().revision > hud);
    // The same layout delivers nothing; hiding delivers `none`, which the probe reports as -1.
    let hud = host.hud().revision;
    host.set_hud_layout(Some(hud_layout(Some(bars)))).unwrap();
    assert_eq!(host.hud().revision, hud);
    host.set_hud_layout(None).unwrap();
    assert_eq!(
        hud_value(&host, "#hud_layout"),
        Some(&Value::Numbers(vec![-1.0]))
    );
}

#[test]
fn target_changed_fires_on_a_changed_look_with_reads_and_text() {
    let (_dir, mut host) = probe();
    host.set_text(Arc::new(Text));
    host.set_target(frame(stone("Stone")), 0.016).unwrap();
    assert_eq!(text(&host, "#target"), "block:minecraft:stone:Stone");
    assert_eq!(value(&host, "#target_revision"), Some(&Value::Integer(1)));
    assert_eq!(
        value(&host, "#mining"),
        Some(&Value::Numbers(vec![0.25, f64::from(0.05_f32)]))
    );
    assert_eq!(text(&host, "#harvest"), "true:minecraft:iron_pickaxe");
    assert_eq!(text(&host, "#translated"), "Probe");
    assert_eq!(value(&host, "#width"), Some(&Value::Number(18.0)));
    assert_eq!(hud_value(&host, "#hud_target"), Some(&Value::Bool(true)));

    // An unchanged look, even with new mining progress, delivers nothing.
    let mut moved = frame(stone("Stone"));
    moved.mining = None;
    host.set_target(moved, 0.016).unwrap();
    assert_eq!(value(&host, "#target_revision"), Some(&Value::Integer(1)));
    host.set_target(frame(stone("Smooth Stone")), 0.016)
        .unwrap();
    assert_eq!(value(&host, "#target_revision"), Some(&Value::Integer(2)));
    host.set_target(TargetFrame::default(), 0.016).unwrap();
    assert_eq!(text(&host, "#target"), "outside");
}

#[test]
fn without_the_target_permission_nothing_is_delivered_or_read() {
    let (_dir, mut host) = probe_with(|manifest| manifest.replace(", \"target\"", ""));
    host.set_target(frame(stone("Stone")), 0.016).unwrap();
    assert!(value(&host, "#target").is_none());
    assert!(host.dispatch(vec![ModEvent::TargetChanged]).is_err());
    assert!(host.is_active());
}

#[test]
fn a_trap_removes_the_hud_layer() {
    let (_dir, mut host) = probe();
    host.dispatch(vec![action("probe.hud")]).unwrap();
    let trap = ModEvent::Key {
        id: "probe.trap".into(),
        hovered: None,
        row: None,
    };
    assert!(host.dispatch(vec![trap]).is_err());
    assert!(host.hud().template.is_none());
    assert!(host.hud().values.is_empty());
}

#[test]
fn coalescing_keeps_the_latest_hud_layout_and_one_target_change() {
    let events = vec![
        ModEvent::TargetChanged,
        ModEvent::HudChanged(None),
        ModEvent::TargetChanged,
        ModEvent::HudChanged(Some(hud_layout(None))),
    ];
    assert_eq!(
        ModEvent::coalesce(events),
        vec![
            ModEvent::HudChanged(Some(hud_layout(None))),
            ModEvent::TargetChanged,
        ]
    );
}
