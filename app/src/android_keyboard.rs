//! NativeActivity IME commits feed the existing JSON-UI text editors.

#[cfg(target_os = "android")]
use bevy::window::PrimaryWindow;
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput, NativeKey, NativeKeyCode},
    },
    prelude::*,
};
#[cfg(target_os = "android")]
use jni::objects::{JString, JValue};

#[cfg(target_os = "android")]
use crate::menu::MenuRuntime;
#[cfg(target_os = "android")]
use client_ui::ui_runtime::UiRuntime;
#[cfg(target_os = "android")]
use launcher::menu::MenuField;

#[cfg(target_os = "android")]
pub(crate) fn configure_app(app: &mut App) {
    // Match OS event timing so ButtonInput and the JSON UI consume each commit
    // in the same frame; delayed Enter presses could otherwise reopen chat.
    app.add_systems(
        PreUpdate,
        drive_text_input.before(bevy::input::InputSystems),
    );
}

#[cfg(target_os = "android")]
fn drive_text_input(
    window: Single<(Entity, &Window), With<PrimaryWindow>>,
    menu: Option<Res<MenuRuntime>>,
    runtime: Option<Res<UiRuntime>>,
    mut input: MessageWriter<KeyboardInput>,
    mut enabled: Local<Option<(bool, bool)>>,
) {
    let (window_entity, window) = window.into_inner();
    let menu_view = menu.as_deref().map(MenuRuntime::view);
    let menu_focused = menu_view
        .as_ref()
        .is_some_and(|view| view.visible && view.field.is_some());
    let runtime_focused = menu_view.as_ref().is_none_or(|view| !view.visible)
        && runtime
            .as_deref()
            .is_some_and(|runtime| runtime.chat_focused() || runtime.screen_state().text_focused());
    let focus = (
        window.focused && (menu_focused || runtime_focused),
        menu_focused && menu_view.is_some_and(|view| view.field == Some(MenuField::Port)),
    );
    let result = crate::android::jni_call(|env, activity| {
        if *enabled != Some(focus) {
            env.call_method(
                activity,
                "setTextInputEnabled",
                "(ZZ)V",
                &[JValue::Bool(focus.0.into()), JValue::Bool(focus.1.into())],
            )?;
            *enabled = Some(focus);
        }
        if !focus.0 {
            return Ok(());
        }
        // Bound JNI work if the client was paused while the IME produced commits.
        for _ in 0..16 {
            let text = env
                .call_method(activity, "pollTextInput", "()Ljava/lang/String;", &[])?
                .l()?;
            if text.is_null() {
                break;
            }
            let text = env.auto_local(JString::from(text));
            let text: String = env.get_string(&text)?.into();
            write_commit(&text, window_entity, &mut input);
        }
        Ok(())
    });
    if let Err(error) = result
        && *enabled != Some(focus)
    {
        warn!("Android text input is unavailable: {error}");
        *enabled = Some(focus);
    }
}

fn write_commit(text: &str, window: Entity, input: &mut MessageWriter<KeyboardInput>) {
    // The existing editors enforce this same maximum insertion budget. Bound
    // event expansion too, since an IME may submit an arbitrarily large paste.
    let mut end = text.len().min(ui::MAX_CHAT_INPUT_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    let text = &text[..end];
    let mut start = 0;
    for (index, character) in text.char_indices() {
        let key = match character {
            '\u{8}' => KeyCode::Backspace,
            '\u{7f}' => KeyCode::Delete,
            '\n' | '\r' => KeyCode::Enter,
            _ => continue,
        };
        if start < index {
            write_key(
                KeyCode::Unidentified(NativeKeyCode::Unidentified),
                Some(&text[start..index]),
                window,
                input,
            );
        }
        write_key(key, None, window, input);
        start = index + character.len_utf8();
    }
    if start < text.len() {
        write_key(
            KeyCode::Unidentified(NativeKeyCode::Unidentified),
            Some(&text[start..]),
            window,
            input,
        );
    }
}

fn write_key(
    key_code: KeyCode,
    text: Option<&str>,
    window: Entity,
    input: &mut MessageWriter<KeyboardInput>,
) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        input.write(KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state,
            text: if state == ButtonState::Pressed {
                text.map(Into::into)
            } else {
                None
            },
            repeat: false,
            window,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::message::Messages;

    #[test]
    fn ime_commits_preserve_unicode_and_ordered_edits_without_repeating_text_on_release() {
        let mut app = App::new();
        app.add_message::<KeyboardInput>();
        let window = app.world_mut().spawn_empty().id();
        app.add_systems(Update, move |mut input: MessageWriter<KeyboardInput>| {
            write_commit("猫🦀\u{8}x\u{7f}\n", window, &mut input);
        });
        app.update();
        let events: Vec<_> = app
            .world_mut()
            .resource_mut::<Messages<KeyboardInput>>()
            .drain()
            .collect();
        let pressed: Vec<_> = events
            .iter()
            .filter(|event| event.state == ButtonState::Pressed)
            .map(|event| (event.key_code, event.text.as_deref()))
            .collect();
        assert_eq!(pressed.len(), 5);
        assert_eq!(pressed[0].1, Some("猫🦀"));
        assert_eq!(pressed[1], (KeyCode::Backspace, None));
        assert_eq!(pressed[2].1, Some("x"));
        assert_eq!(pressed[3], (KeyCode::Delete, None));
        assert_eq!(pressed[4], (KeyCode::Enter, None));
        for pair in events.chunks_exact(2) {
            assert_eq!(pair[0].key_code, pair[1].key_code);
            assert_eq!(pair[1].state, ButtonState::Released);
            assert_eq!(pair[1].text, None);
            assert_eq!(pair[1].window, window);
        }
    }
}
