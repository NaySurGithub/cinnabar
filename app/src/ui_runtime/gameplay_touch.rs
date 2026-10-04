use bevy::{input::touch::Touches, prelude::*, window::PrimaryWindow};
use semantic_input::{TouchControlState, touch};

use crate::{
    menu::MenuRuntime, player_runtime::PlayerRuntime, semantic_controls::SemanticTouchTargets,
};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};

#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_gameplay_touch_targets(
    mut touches: ResMut<Touches>,
    mut ui: ResMut<UiRuntime>,
    mut targets: ResMut<SemanticTouchTargets>,
    mut player: ResMut<PlayerRuntime>,
    mut menu: Option<ResMut<MenuRuntime>>,
    mut presentation: Option<ResMut<UiPresentationRuntime>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut state: Local<TouchControlState>,
) {
    targets.release_all();
    let Some(window) = windows.single().ok().filter(|window| window.focused) else {
        state.clear();
        if let Some(presentation) = presentation.as_mut() {
            presentation.set_touch_contacts(Vec::new());
        }
        return;
    };
    if crate::screen_policy::absorbs_input(
        &player,
        Some(&ui),
        menu.as_deref(),
        presentation.as_deref(),
    ) {
        state.clear();
        if let Some(presentation) = presentation.as_mut() {
            presentation.set_touch_contacts(Vec::new());
        }
        return;
    }
    let Some(presentation) = presentation
        .as_mut()
        .filter(|presentation| presentation.touch_controls_enabled())
    else {
        state.clear();
        return;
    };
    let size = [window.width().max(1.0), window.height().max(1.0)];
    let position = |position: Vec2| {
        [
            (position.x / size[0]).clamp(0.0, 1.0),
            (position.y / size[1]).clamp(0.0, 1.0),
        ]
    };
    for contact in touches.iter_just_canceled() {
        state.release(contact.id());
    }
    let started: Vec<_> = touches
        .iter_just_pressed()
        .map(|contact| (contact.id(), position(contact.start_position())))
        .collect();
    for &(id, start) in &started {
        if touches.just_canceled(id) {
            continue;
        }
        let region = presentation.gameplay_touch_region(start, size);
        let opened_ui = match region.map(|region| region.hit_id) {
            Some(touch::INVENTORY) => {
                ui.toggle_inventory(&mut player);
                true
            }
            Some(touch::CHAT) => {
                ui.open_chat(&mut player);
                true
            }
            Some(touch::MENU) => {
                if let Some(menu) = menu.as_mut() {
                    menu.open_pause();
                }
                true
            }
            _ => false,
        };
        if opened_ui {
            for &(id, _) in &started {
                touches.clear_just_pressed(id);
            }
            state.clear();
            presentation.set_touch_contacts(Vec::new());
            return;
        }
        state.begin(id, start, 0, region);
    }
    for contact in touches.iter().chain(touches.iter_just_released()) {
        state.move_to(contact.id(), position(contact.position()), 0);
    }
    let samples = state.sample();
    presentation.set_touch_contacts(samples.clone());
    for mut sample in samples {
        if sample.hit_id == Some(touch::LOOK_SURFACE) {
            sample.delta = [sample.delta[0] * 4.0, sample.delta[1] * 2.0];
        }
        if touches.just_released(sample.contact_id) {
            targets.pulse(sample);
        } else {
            targets.set_sample(&sample);
        }
    }
    for contact in touches.iter_just_released() {
        state.release(contact.id());
    }
}
