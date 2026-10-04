//! Touch pointer for screens whose existing controllers consume mouse edges.
use bevy::{input::touch::Touches, prelude::*, window::PrimaryWindow};
use client_ui::ui_runtime::forms::ServerFormIdentity;

use crate::{menu::MenuRuntime, ui_runtime::UiRuntime};

/// Overrides the UI cursor without asking the operating system to warp a mouse.
#[derive(Resource, Default)]
pub(crate) struct UiTouchPointer {
    cursor: Option<Vec2>,
    active: bool,
    pub(crate) canceled: bool,
    pub(crate) edges: Vec<bool>,
}

impl UiTouchPointer {
    pub(crate) fn cursor(&self, mouse: Option<Vec2>) -> Option<Vec2> {
        if self.active { self.cursor } else { mouse }
    }

    pub(crate) fn mode(&self) -> json_ui::InputMode {
        if self.active {
            json_ui::InputMode::Touch
        } else {
            json_ui::InputMode::Mouse
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PointerTarget {
    Inventory,
    Form(ServerFormIdentity),
}

#[derive(Default)]
pub(crate) struct TouchPointerState {
    target: Option<PointerTarget>,
    owner: Option<u64>,
    excluded: Vec<u64>,
}

#[derive(Default, Debug)]
struct PointerFrame {
    cursor: Option<Vec2>,
    active: bool,
    canceled: bool,
    edges: Vec<bool>,
}

impl TouchPointerState {
    fn exclude(&mut self, id: u64) {
        if self.excluded.len() < semantic_input::MAX_TOUCH_CONTACTS && !self.excluded.contains(&id)
        {
            self.excluded.push(id);
        }
    }

    fn step(&mut self, target: Option<PointerTarget>, touches: &Touches) -> PointerFrame {
        self.excluded
            .retain(|id| touches.get_pressed(*id).is_some());
        let mut frame = PointerFrame::default();
        if self.target != target {
            if let Some(owner) = self.owner.take() {
                self.exclude(owner);
                frame.active = true;
                frame.canceled = true;
                frame.edges.push(false);
            }
            self.target = target;
        }
        if target.is_none() {
            for touch in touches.iter() {
                self.exclude(touch.id());
            }
            return frame;
        }
        let previous_owner = self.owner;
        if self.owner.is_none() && !frame.canceled {
            self.owner = touches
                .iter_just_pressed()
                .filter(|touch| {
                    !touches.just_canceled(touch.id()) && !self.excluded.contains(&touch.id())
                })
                .filter(|touch| touch.position().is_finite())
                .min_by_key(|touch| touch.id())
                .map(|touch| touch.id());
            if self.owner.is_some() {
                frame.edges.push(true);
            }
        }
        for touch in touches.iter_just_pressed() {
            if Some(touch.id()) != self.owner {
                self.exclude(touch.id());
            }
        }
        let Some(owner) = self.owner else {
            return frame;
        };
        frame.active = true;
        if touches.just_canceled(owner) {
            frame.canceled = true;
        } else if let Some(touch) = touches
            .get_pressed(owner)
            .or_else(|| touches.get_released(owner))
        {
            frame.cursor = touch.position().is_finite().then_some(touch.position());
            frame.canceled = frame.cursor.is_none();
        } else {
            frame.canceled = true;
        }
        if frame.canceled || touches.just_released(owner) {
            if frame.canceled {
                frame.cursor = None;
                // A start and cancellation in one frame never presses a widget.
                if previous_owner.is_none() {
                    frame.edges.clear();
                }
            }
            if previous_owner.is_some() || !frame.edges.is_empty() {
                frame.edges.push(false);
            }
            self.owner = None;
        }
        frame
    }
}

/// Run before gameplay touch capture and before inventory/form controllers.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_ui_touch_pointer(
    mut touches: ResMut<Touches>,
    ui: Res<UiRuntime>,
    menu: Option<Res<MenuRuntime>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut pointer: ResMut<UiTouchPointer>,
    mut state: Local<TouchPointerState>,
) {
    let window = windows.single().ok();
    let target = window.filter(|window| window.focused).and_then(|_| {
        if ui.server_forms().owns_input()
            && (!menu.as_ref().is_some_and(|menu| menu.is_visible())
                || ui.server_forms().settings_form_active())
        {
            ui.server_forms()
                .active()
                .map(|entry| PointerTarget::Form(entry.identity))
        } else if ui.inventory_open()
            && !ui.chat_focused()
            && !ui.sign_editor().is_open()
            && !ui.local_sleeping()
            && !menu.as_ref().is_some_and(|menu| menu.is_visible())
        {
            Some(PointerTarget::Inventory)
        } else {
            None
        }
    });
    let frame = state.step(target, &touches);
    *pointer = UiTouchPointer {
        cursor: frame.cursor,
        active: frame.active,
        canceled: frame.canceled,
        edges: frame.edges.clone(),
    };
    if target.is_some() {
        let started: Vec<_> = touches
            .iter_just_pressed()
            .map(|touch| touch.id())
            .collect();
        for id in started {
            touches.clear_just_pressed(id);
        }
    }
    for down in frame.edges {
        if down {
            mouse.press(MouseButton::Left);
        } else {
            mouse.release(MouseButton::Left);
        }
    }
}

#[cfg(test)]
mod tests;
