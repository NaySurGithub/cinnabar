//! Renewed ordinary digital inputs layered over a finalized physical frame.
use crate::{Action, ActionPhase, MovementButtons, finalized::FinalizedInput};

const AUTOMATED_ACTIONS: [Action; 9] = [
    Action::MoveForward,
    Action::MoveBackward,
    Action::MoveLeft,
    Action::MoveRight,
    Action::Jump,
    Action::Sprint,
    Action::Sneak,
    Action::Attack,
    Action::Use,
];

/// Authorized gameplay directions and held actions; this is not raw key injection.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Controls {
    pub movement: MovementButtons,
    pub jump: bool,
    pub sprint: bool,
    pub sneak: bool,
    pub attack: bool,
    pub use_item: bool,
}
/// Tracks synthetic edges within one session, dimension and router authority.
#[derive(Default)]
pub struct Overlay {
    held: [bool; AUTOMATED_ACTIONS.len()],
    authority: Option<crate::identity::InputAuthorityGeneration>,
}
impl Overlay {
    /// Releases all synthetic ownership; physical input remains owned by the router.
    pub fn reset(&mut self) {
        self.held = [false; AUTOMATED_ACTIONS.len()];
        self.authority = None;
    }
    /// Adds a current-frame contribution without creating input when the router has no authority.
    pub fn apply(&mut self, input: &mut FinalizedInput, controls: Option<Controls>) {
        let Some(frame) = input.0.as_mut() else {
            self.reset();
            return;
        };
        if self.authority != Some(frame.authority_generation) || frame.authority_released {
            self.reset();
        }
        self.authority = Some(frame.authority_generation);
        if controls.is_none() && !self.held.iter().any(|held| *held) {
            return;
        }
        let value = controls
            .filter(|_| !frame.authority_released)
            .unwrap_or_default();
        let buttons = value.movement;
        let actions = AUTOMATED_ACTIONS;
        let held = [
            buttons.forward,
            buttons.backward,
            buttons.left,
            buttons.right,
            value.jump,
            value.sprint,
            value.sneak,
            value.attack,
            value.use_item,
        ];
        for (index, action) in actions.into_iter().enumerate() {
            let phase = &mut frame.phases[action as usize];
            let physical_before = phase.released || (phase.held && !phase.pressed);
            let before = physical_before || self.held[index];
            let after = phase.held || held[index];
            *phase = ActionPhase {
                pressed: after && !before,
                held: after,
                released: before && !after,
            };
        }
        self.held = held;
        let digital = [
            u8::from(buttons.right) as f32 - u8::from(buttons.left) as f32,
            u8::from(buttons.forward) as f32 - u8::from(buttons.backward) as f32,
        ];
        // The router's x axis is right-positive and y axis is forward-positive.
        let raw = [
            (frame.raw_movement[0] + digital[0]).clamp(-1.0, 1.0),
            (frame.raw_movement[1] + digital[1]).clamp(-1.0, 1.0),
        ];
        frame.raw_movement = raw;
        let length = raw[0].hypot(raw[1]).max(1.0);
        frame.movement = [raw[0] / length, raw[1] / length];
        frame.movement_buttons.forward |= buttons.forward;
        frame.movement_buttons.backward |= buttons.backward;
        frame.movement_buttons.left |= buttons.left;
        frame.movement_buttons.right |= buttons.right;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ActionSnapshot, InputMode,
        identity::{InputAuthorityGeneration, InputSequence},
    };
    fn input() -> FinalizedInput {
        FinalizedInput::from_finalized(ActionSnapshot {
            frame_sequence: InputSequence::new(1),
            authority_generation: InputAuthorityGeneration::INITIAL,
            movement: [0.; 2],
            raw_movement: [0.; 2],
            analogue_movement: [0.; 2],
            movement_buttons: Default::default(),
            look_delta: [0.; 2],
            input_mode: InputMode::KeyboardMouse,
            phases: [ActionPhase::default(); Action::COUNT],
            release_reasons: [None; Action::COUNT],
            authority_released: false,
        })
    }
    #[test]
    fn no_mod_preserves_analogue_and_physical_input() {
        let mut frame = input();
        let snapshot = frame.0.as_mut().unwrap();
        snapshot.raw_movement = [0.8, 0.8];
        snapshot.movement = [0.4, 0.4];
        snapshot.analogue_movement = [0.8, 0.8];
        snapshot.phases[Action::Attack as usize] = ActionPhase {
            pressed: true,
            held: true,
            released: false,
        };
        let before = frame.snapshot().unwrap().clone();
        Overlay::default().apply(&mut frame, None);
        assert_eq!(frame.snapshot(), Some(&before));
    }
    #[test]
    fn renewed_actions_press_once_then_release_on_omission() {
        let mut overlay = Overlay::default();
        let controls = Controls {
            attack: true,
            use_item: true,
            jump: true,
            ..Default::default()
        };
        let mut first = input();
        overlay.apply(&mut first, Some(controls));
        assert!(first.phase(Action::Attack).pressed);
        let mut next = input();
        overlay.apply(&mut next, Some(controls));
        assert!(next.phase(Action::Attack).held);
        assert!(!next.phase(Action::Attack).pressed);
        let mut last = input();
        overlay.apply(&mut last, None);
        assert!(last.phase(Action::Attack).released);
        assert!(!last.phase(Action::Use).held);
    }
    #[test]
    fn physical_hold_survives_synthetic_release() {
        let mut overlay = Overlay::default();
        let mut first = input();
        overlay.apply(
            &mut first,
            Some(Controls {
                jump: true,
                ..Default::default()
            }),
        );
        let mut next = input();
        next.0.as_mut().unwrap().phases[Action::Jump as usize] = ActionPhase {
            pressed: true,
            held: true,
            released: false,
        };
        overlay.apply(&mut next, None);
        assert!(next.phase(Action::Jump).held);
        assert!(!next.phase(Action::Jump).released);
        assert!(!next.phase(Action::Jump).pressed);
    }
    #[test]
    fn movement_uses_native_axis_signs_and_unit_diagonal() {
        let mut frame = input();
        let mut overlay = Overlay::default();
        overlay.apply(
            &mut frame,
            Some(Controls {
                movement: MovementButtons {
                    right: true,
                    forward: true,
                    ..Default::default()
                },
                ..Default::default()
            }),
        );
        assert_eq!(frame.raw_movement(), [1., 1.]);
        assert!((frame.movement()[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 0.00001);
        assert!(frame.snapshot().unwrap().movement_buttons.right);
    }
    #[test]
    fn opposing_inputs_cancel_and_authority_release_cannot_restart_bot() {
        let mut frame = input();
        frame.0.as_mut().unwrap().raw_movement = [-1., 0.];
        let mut overlay = Overlay::default();
        overlay.apply(
            &mut frame,
            Some(Controls {
                movement: MovementButtons {
                    right: true,
                    ..Default::default()
                },
                ..Default::default()
            }),
        );
        assert_eq!(frame.movement(), [0., 0.]);
        let mut frame = input();
        frame.0.as_mut().unwrap().authority_released = true;
        overlay.apply(
            &mut frame,
            Some(Controls {
                attack: true,
                ..Default::default()
            }),
        );
        assert!(!frame.phase(Action::Attack).held);
    }
}
