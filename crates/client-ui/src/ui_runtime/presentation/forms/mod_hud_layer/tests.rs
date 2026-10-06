use super::super::{ServerUiPack, snapshot, tests::mini_engine_presentation};
use super::*;
use render_model::UiRenderInput;
use std::collections::BTreeMap;
use ui::DpiScale;

/// A real engine over a small HUD fixture, independent of local carrier files.
fn presentation() -> UiPresentationRuntime {
    let mut presentation = mini_engine_presentation();
    presentation.set_server_ui_pack(&ServerUiPack {
        ui_layers: vec![vec![
            (
                "ui/_ui_defs.json".to_owned(),
                br#"{"ui_defs":["ui/hud_screen.json"]}"#.to_vec(),
            ),
            (
                "ui/hud_screen.json".to_owned(),
                br#"{
                "namespace": "hud",
                "hud_screen": { "type": "screen", "controls": [{ "label": {
                    "type": "label", "size": [100, 12],
                    "anchor_from": "bottom_middle", "anchor_to": "bottom_middle",
                    "text": "Base HUD" } }] }
            }"#
                .to_vec(),
            ),
        ]],
        ..Default::default()
    });
    presentation
}

fn files() -> Arc<screen::Files> {
    Arc::new(screen::Files {
        namespace: "probe".into(),
        templates: [(
            "ui/hud.json".to_owned(),
            br##"{"namespace": "probe", "hud": {"type": "panel", "controls": [{"text": {
                "type": "label", "size": [120, 12], "text": "#label",
                "bindings": [{"binding_name": "#label"}]}}]}}"##
                .to_vec(),
        )]
        .into_iter()
        .collect(),
        textures: Vec::new(),
    })
}

fn data(label: &str, revision: u64) -> screen::Modal {
    screen::Modal {
        template: Some("ui/hud.json".into()),
        values: BTreeMap::from([("#label".to_owned(), screen::Value::Text(label.to_owned()))]),
        revision,
        ..screen::Modal::default()
    }
}

fn build(presentation: &mut UiPresentationRuntime, runtime: &UiRuntime) -> UiRenderInput {
    presentation
        .build(
            &player_state::PlayerState::new(1),
            runtime,
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap()
}

#[test]
fn the_layer_draws_over_the_hud_and_reports_its_layout() {
    let runtime = UiRuntime::new(1);
    let mut presentation = presentation();
    let before = snapshot::rasterize(&build(&mut presentation, &runtime));
    let files = files();
    presentation.set_mod_hud(Some(ModHudInput {
        id: "probe",
        files: &files,
        data: &data("Stone", 1),
    }));
    let drawn = build(&mut presentation, &runtime);
    assert!(presentation.mod_hud_failure().is_none());
    assert_ne!(before, snapshot::rasterize(&drawn));
    let layout = presentation.mod_hud_layout().unwrap();
    assert!(layout.size.valid());
    // Unchanged data draws the same frame; removing the layer restores the vanilla HUD.
    assert_eq!(drawn, build(&mut presentation, &runtime));
    presentation.set_mod_hud(None);
    assert_eq!(
        before,
        snapshot::rasterize(&build(&mut presentation, &runtime))
    );
    assert!(presentation.mod_hud_layout().is_none());
}

#[test]
fn the_layer_hides_under_screens_but_not_under_chat() {
    let files = files();
    for (inventory, chat, shown) in [(true, false, false), (false, true, true)] {
        let mut runtime = UiRuntime::new(1);
        runtime.inventory_open = inventory;
        runtime.chat_focused = chat;
        let mut presentation = presentation();
        let before = snapshot::rasterize(&build(&mut presentation, &runtime));
        presentation.set_mod_hud(Some(ModHudInput {
            id: "probe",
            files: &files,
            data: &data("Stone", 1),
        }));
        let after = snapshot::rasterize(&build(&mut presentation, &runtime));
        assert_eq!(before != after, shown, "inventory {inventory}, chat {chat}");
        assert_eq!(presentation.mod_hud_layout().is_some(), shown);
    }
}

#[test]
fn new_data_redraws_the_layer() {
    let runtime = UiRuntime::new(1);
    let mut presentation = presentation();
    let files = files();
    presentation.set_mod_hud(Some(ModHudInput {
        id: "probe",
        files: &files,
        data: &data("Stone", 1),
    }));
    let stone = build(&mut presentation, &runtime);
    presentation.set_mod_hud(Some(ModHudInput {
        id: "probe",
        files: &files,
        data: &data("Dirt", 2),
    }));
    assert_ne!(stone.vertices, build(&mut presentation, &runtime).vertices);
}

#[test]
fn hiding_the_hud_hides_the_layer() {
    use crate::menu::settings_options::{SETTINGS_OPTIONS, SettingsOptions};
    let runtime = UiRuntime::new(1);
    let mut presentation = presentation();
    let files = files();
    presentation.set_mod_hud(Some(ModHudInput {
        id: "probe",
        files: &files,
        data: &data("Stone", 1),
    }));
    build(&mut presentation, &runtime);
    assert!(presentation.mod_hud_layout().is_some());
    let mut options = SettingsOptions::default();
    let index = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "hide_hud")
        .unwrap();
    options.set(index, 1);
    presentation.set_chat_settings_snapshot((Arc::new(options), None));
    build(&mut presentation, &runtime);
    assert!(presentation.mod_hud_layout().is_none());
}
