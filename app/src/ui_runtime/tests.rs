//! Integration tests for app-owned input, ordering and inventory adapters.

use super::*;

mod forms_fixture;
mod forms_interaction_tests;
mod inventory_overlay_tests;
pub(crate) mod menu_input_tests;

#[test]
fn chat_focus_clears_stale_gameplay_touch_targets() {
    let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    use crate::semantic_controls::SemanticTouchTargets;
    use crate::ui_runtime::gameplay_touch::drive_gameplay_touch_targets;
    use bevy::{
        input::touch::Touches,
        prelude::{App, Update},
    };

    let mut runtime = UiRuntime::new(1);
    runtime.open_chat(&mut player_runtime);
    let mut app = App::new();
    let mut targets = SemanticTouchTargets::default();
    targets.set(7, semantic_input::touch::JUMP);
    app.insert_resource(runtime)
        .insert_resource(player_runtime.clone())
        .init_resource::<Touches>()
        .insert_resource(targets)
        .add_systems(Update, drive_gameplay_touch_targets);

    app.update();

    assert_eq!(
        app.world().resource::<SemanticTouchTargets>().target(7),
        None
    );
}
