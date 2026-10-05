//! Keeps the cause of a terminal runtime failure available after app teardown.

use std::sync::{Arc, Mutex};

use bevy::prelude::{App, AppExit, IntoScheduleConfigs, Last, MessageReader, Res, Resource};

use crate::runtime::world::{ClientWorld, arm_shutdown_watchdog};

#[derive(Clone, Default, Resource)]
struct TerminalCause(Arc<Mutex<Option<String>>>);

pub(super) fn run(app: &mut App) -> (AppExit, Option<String>) {
    let cause = TerminalCause::default();
    app.insert_resource(cause.clone())
        .add_systems(Last, capture_cause.before(arm_shutdown_watchdog));
    let exit = app.run();
    let fatal_error = cause
        .0
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    (exit, fatal_error)
}

fn capture_cause(
    mut exits: MessageReader<AppExit>,
    world: Res<ClientWorld>,
    cause: Res<TerminalCause>,
) {
    if !exits.read().any(AppExit::is_error) {
        return;
    }
    let Some(error) = world.fatal_error.as_ref().filter(|error| !error.is_empty()) else {
        return;
    };
    let mut captured = cause.0.lock().unwrap_or_else(|error| error.into_inner());
    if captured.is_none() {
        *captured = Some(error.clone());
    }
}

pub(super) fn message(fatal_error: Option<&str>, panic: Option<&str>) -> String {
    if let Some(panic) = panic {
        return format!("Client runtime failed.\n\nCaptured panic:\n{panic}");
    }
    if let Some(cause) = fatal_error.filter(|cause| !cause.is_empty()) {
        return format!("Client runtime failed.\n\n{cause}");
    }
    "Bevy app exited after a fatal runtime error".into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::{MessageWriter, Update};

    #[test]
    fn terminal_cause_survives_app_run_replacing_the_caller() {
        let cause = "world FIFO rejected LevelChunk: truncated framing";
        let mut world = ClientWorld::new(Arc::new(assets::RuntimeAssets::diagnostic()));
        world.fatal_error = Some(cause.into());
        let mut app = App::new();
        app.insert_resource(world)
            .add_systems(Update, |mut exits: MessageWriter<AppExit>| {
                exits.write(AppExit::error());
            });

        let (exit, fatal_error) = run(&mut app);

        assert!(exit.is_error());
        assert!(app.world().get_resource::<ClientWorld>().is_none());
        assert!(message(fatal_error.as_deref(), None).contains(cause));
    }

    #[test]
    fn successful_exit_does_not_capture_a_stale_world_failure() {
        let mut world = ClientWorld::new(Arc::new(assets::RuntimeAssets::diagnostic()));
        world.fatal_error = Some("a stale session failure".into());
        let mut app = App::new();
        app.insert_resource(world)
            .add_systems(Update, |mut exits: MessageWriter<AppExit>| {
                exits.write(AppExit::Success);
            });

        let (exit, fatal_error) = run(&mut app);

        assert!(exit.is_success());
        assert!(fatal_error.is_none());
    }

    #[test]
    fn first_terminal_cause_survives_later_world_failures() {
        let first = "failed to encode PlayerAuthInput";
        let later = "network command channel is closed";
        let mut world = ClientWorld::new(Arc::new(assets::RuntimeAssets::diagnostic()));
        world.fatal_error = Some(first.into());
        let mut app = App::new();
        app.insert_resource(world)
            .add_systems(Update, |mut exits: MessageWriter<AppExit>| {
                exits.write(AppExit::error());
            })
            .set_runner(move |mut owned| {
                owned.finish();
                owned.cleanup();
                owned.update();
                owned.world_mut().resource_mut::<ClientWorld>().fatal_error = Some(later.into());
                owned.update();
                owned.should_exit().expect("terminal updates request exit")
            });

        let (exit, fatal_error) = run(&mut app);

        assert!(exit.is_error());
        assert_eq!(fatal_error.as_deref(), Some(first));
    }

    #[test]
    fn non_panic_runtime_failure_retains_its_cause() {
        let cause = "world FIFO rejected LevelChunk: truncated framing";
        let displayed = message(Some(cause), None);
        assert!(displayed.contains(cause));
        assert!(!displayed.contains("Captured panic"));
    }

    #[test]
    fn first_captured_panic_takes_precedence_over_a_session_failure() {
        let panic = "panicked at renderer.rs:42: invalid texture descriptor";
        let cause = "network command channel is closed";
        let displayed = message(Some(cause), Some(panic));
        assert!(displayed.contains(panic));
        assert!(displayed.contains("Captured panic"));
        assert!(!displayed.contains(cause));
    }

    #[test]
    fn unexplained_runtime_exit_keeps_the_generic_fallback() {
        let displayed = message(None, None);
        assert!(displayed.contains("Bevy app exited"));
        assert!(displayed.contains("fatal runtime error"));
    }

    #[test]
    fn empty_runtime_cause_keeps_the_generic_fallback() {
        assert_eq!(message(Some(""), None), message(None, None));
    }
}
