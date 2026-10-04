use super::*;
use bevy::input::mouse::{MouseButtonInput, mouse_button_input_system};
use bevy::input::touch::{TouchInput, TouchPhase, touch_screen_input_system};

struct TouchHarness {
    app: App,
    window: Entity,
    state: TouchPointerState,
}

impl TouchHarness {
    fn new() -> Self {
        let mut app = App::new();
        app.add_message::<TouchInput>()
            .init_resource::<Touches>()
            .add_systems(Update, touch_screen_input_system);
        let window = app.world_mut().spawn(Window::default()).id();
        Self {
            app,
            window,
            state: TouchPointerState::default(),
        }
    }

    fn step(
        &mut self,
        target: Option<PointerTarget>,
        events: &[(u64, TouchPhase, [f32; 2])],
    ) -> PointerFrame {
        for &(id, phase, position) in events {
            self.app.world_mut().write_message(TouchInput {
                id,
                phase,
                position: Vec2::from_array(position),
                window: self.window,
                force: None,
            });
        }
        self.app.update();
        self.state
            .step(target, self.app.world().resource::<Touches>())
    }
}

#[test]
fn a_tap_ending_before_the_next_sample_delivers_both_edges() {
    let mut harness = TouchHarness::new();
    let frame = harness.step(
        Some(PointerTarget::Inventory),
        &[
            (7, TouchPhase::Started, [100.0, 80.0]),
            (7, TouchPhase::Ended, [100.0, 80.0]),
        ],
    );
    assert_eq!(frame.edges, [true, false]);
    assert_eq!(frame.cursor, Some(Vec2::new(100.0, 80.0)));
    assert!(frame.active && !frame.canceled);
    let next = harness.step(Some(PointerTarget::Inventory), &[]);
    assert!(next.edges.is_empty() && !next.active);
}

#[test]
fn cancellation_releases_capture_without_a_click_location() {
    let mut harness = TouchHarness::new();
    harness.step(
        Some(PointerTarget::Inventory),
        &[(7, TouchPhase::Started, [100.0, 80.0])],
    );
    let cancel = harness.step(
        Some(PointerTarget::Inventory),
        &[(7, TouchPhase::Canceled, [100.0, 80.0])],
    );
    assert_eq!(cancel.edges, [false]);
    assert!(cancel.active && cancel.canceled && cancel.cursor.is_none());
    let same_frame = harness.step(
        Some(PointerTarget::Inventory),
        &[
            (8, TouchPhase::Started, [100.0, 80.0]),
            (8, TouchPhase::Canceled, [100.0, 80.0]),
        ],
    );
    assert!(
        same_frame.edges.is_empty(),
        "a canceled tap must never press a widget"
    );
}

#[test]
fn a_second_finger_cannot_move_or_inherit_the_primary_pointer() {
    let mut harness = TouchHarness::new();
    harness.step(
        Some(PointerTarget::Inventory),
        &[(1, TouchPhase::Started, [10.0, 20.0])],
    );
    let other = harness.step(
        Some(PointerTarget::Inventory),
        &[(2, TouchPhase::Started, [90.0, 80.0])],
    );
    assert_eq!(other.cursor, Some(Vec2::new(10.0, 20.0)));
    assert!(other.edges.is_empty());
    harness.step(
        Some(PointerTarget::Inventory),
        &[(1, TouchPhase::Ended, [10.0, 20.0])],
    );
    let ignored = harness.step(
        Some(PointerTarget::Inventory),
        &[(2, TouchPhase::Moved, [50.0, 60.0])],
    );
    assert!(!ignored.active && ignored.edges.is_empty());
}

#[test]
fn an_opening_finger_is_excluded_until_a_new_ui_press() {
    let mut harness = TouchHarness::new();
    harness.step(None, &[(1, TouchPhase::Started, [10.0, 20.0])]);
    let opening = harness.step(
        Some(PointerTarget::Inventory),
        &[(1, TouchPhase::Moved, [90.0, 80.0])],
    );
    assert!(!opening.active && opening.edges.is_empty());
    let fresh = harness.step(
        Some(PointerTarget::Inventory),
        &[(2, TouchPhase::Started, [30.0, 40.0])],
    );
    assert_eq!(fresh.edges, [true]);
    assert_eq!(fresh.cursor, Some(Vec2::new(30.0, 40.0)));
}

#[test]
fn movement_keeps_capture_and_a_screen_change_cancels_it() {
    let mut harness = TouchHarness::new();
    harness.step(
        Some(PointerTarget::Inventory),
        &[(1, TouchPhase::Started, [10.0, 20.0])],
    );
    let moved = harness.step(
        Some(PointerTarget::Inventory),
        &[(1, TouchPhase::Moved, [90.0, 80.0])],
    );
    assert_eq!(moved.cursor, Some(Vec2::new(90.0, 80.0)));
    assert!(moved.active && moved.edges.is_empty());
    let form = PointerTarget::Form(ServerFormIdentity {
        session: 1,
        form_id: 2,
        revision: 3,
    });
    let changed = harness.step(Some(form), &[]);
    assert_eq!(changed.edges, [false]);
    assert!(changed.canceled && changed.cursor.is_none());
    assert!(!harness.step(Some(form), &[]).active);
}

#[test]
fn the_app_adapter_updates_ui_input_without_warping_the_window_cursor() {
    let mut app = App::new();
    let mut player = crate::player_runtime::PlayerRuntime::new(1);
    let mut ui = UiRuntime::new(1);
    ui.publish_local_runtime_id(&mut player, 1, 42)
        .expect("local player identity");
    ui.publish_inventory_authority(&mut player, protocol::InventoryAuthority::Server);
    ui.toggle_inventory(&mut player);
    assert!(ui.inventory_open());
    app.insert_resource(ui)
        .init_resource::<UiTouchPointer>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        .add_message::<TouchInput>()
        .add_message::<MouseButtonInput>()
        .add_systems(
            Update,
            (
                mouse_button_input_system,
                touch_screen_input_system,
                drive_ui_touch_pointer,
            )
                .chain(),
        );
    let mut window = Window {
        focused: true,
        ..Window::default()
    };
    window.set_cursor_position(Some(Vec2::new(3.0, 4.0)));
    let window = app.world_mut().spawn((window, PrimaryWindow)).id();
    app.world_mut().write_message(TouchInput {
        id: 1,
        phase: TouchPhase::Started,
        position: Vec2::new(40.0, 50.0),
        window,
        force: None,
    });
    app.update();
    assert_eq!(
        app.world().get::<Window>(window).unwrap().cursor_position(),
        Some(Vec2::new(3.0, 4.0))
    );
    assert_eq!(
        app.world().resource::<UiTouchPointer>().cursor(None),
        Some(Vec2::new(40.0, 50.0))
    );
    assert!(
        app.world()
            .resource::<ButtonInput<MouseButton>>()
            .just_pressed(MouseButton::Left)
    );
    assert!(
        !app.world().resource::<Touches>().just_pressed(1),
        "UI owns the start edge"
    );
    app.update();
    assert!(
        !app.world()
            .resource::<ButtonInput<MouseButton>>()
            .just_pressed(MouseButton::Left),
        "the backend must not replay a synthetic touch press on the next frame"
    );
}
