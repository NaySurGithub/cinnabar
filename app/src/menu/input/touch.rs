//! A scroll gesture holds the row selection until release; dragging never activates it.
use bevy::input::touch::Touches;
use client_ui::ui_runtime::presentation::UiPresentationRuntime;
use ui::UiPoint;

use super::super::{MenuAction, MenuDialog, MenuRuntime, MenuScreen};

#[derive(Default)]
pub(crate) struct MenuTouch {
    owner: Option<u64>,
    screen: Option<(MenuScreen, Option<MenuDialog>)>,
    action: Option<MenuAction>,
}

impl MenuTouch {
    pub(super) fn cancel(&mut self, presentation: &mut UiPresentationRuntime) {
        presentation.cancel_menu_touch();
        *self = Self::default();
    }

    pub(super) fn step(
        &mut self,
        touches: &Touches,
        menu: &MenuRuntime,
        presentation: &mut UiPresentationRuntime,
    ) -> Option<(UiPoint, MenuAction)> {
        let screen = (menu.screen(), menu.dialog);
        if self.screen.is_some_and(|previous| previous != screen) {
            self.cancel(presentation);
            return None;
        }
        if self.owner.is_none() {
            let touch = touches.iter_just_pressed().min_by_key(|touch| touch.id())?;
            if touches.just_canceled(touch.id()) {
                return None;
            }
            let position = touch.position();
            let point = UiPoint::new(position.x, position.y).ok()?;
            let action = presentation.hit_test_menu(point);
            if !presentation.begin_menu_touch(point) {
                return action.map(|action| (point, action));
            }
            self.owner = Some(touch.id());
            self.screen = Some(screen);
            self.action = action;
        }
        let owner = self.owner?;
        if touches.just_canceled(owner) {
            self.cancel(presentation);
            return None;
        }
        let Some(touch) = touches
            .get_pressed(owner)
            .or_else(|| touches.get_released(owner))
        else {
            self.cancel(presentation);
            return None;
        };
        let position = touch.position();
        let Ok(point) = UiPoint::new(position.x, position.y) else {
            self.cancel(presentation);
            return None;
        };
        presentation.move_menu_touch(point);
        if !touches.just_released(owner) {
            return None;
        }
        let tap = presentation.end_menu_touch();
        let action = self.action.take();
        self.owner = None;
        self.screen = None;
        action
            .filter(|action| tap && presentation.hit_test_menu(point) == Some(*action))
            .map(|action| (point, action))
    }
}
