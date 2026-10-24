//! Presented text shared by the screens and the retained HUD: chat rows resolved against the
//! language tables, and every server string bounded before layout.

use std::{borrow::Cow, sync::Arc};

use ui::{ChatMessage, ChatMessageKind};

/// The most bytes of one server string the client presents.
pub const MAX_PRESENTED_TEXT_BYTES: usize = 512;

/// The presented text for one chat row. `Translation`/command-output rows
/// resolve their key against `translate` and splice their ordered parameters
/// (an unknown key presents verbatim, as the vanilla client does); a `Chat`
/// row with a source gains the `<name>` prefix via `chat.type.text`. Other
/// kinds present their message unchanged. Bedrock `§` codes in the result are
/// parsed later at layout time.
pub fn resolve_chat_line<'a>(
    node: &'a ChatMessage,
    translate: impl Fn(&str) -> Option<Arc<str>>,
) -> Cow<'a, str> {
    match node.kind {
        ChatMessageKind::Translation => {
            let template = json_ui::localize_text(&node.message, &translate);
            let arguments = node
                .parameters
                .iter()
                .map(|parameter| {
                    protocol::localize_parameter_prefix(parameter, &translate, usize::MAX)
                        .into_owned()
                })
                .collect::<Vec<_>>();
            Cow::Owned(protocol::format_translation(&template, &arguments))
        }
        ChatMessageKind::Chat => match node.source.as_deref() {
            Some(source) if !source.is_empty() => {
                let template = translate("chat.type.text").unwrap_or_else(|| Arc::from("<%s> %s"));
                Cow::Owned(protocol::format_translation(
                    &template,
                    &[source.to_owned(), node.message.as_ref().to_owned()],
                ))
            }
            _ => Cow::Borrowed(node.message.as_ref()),
        },
        _ => Cow::Borrowed(node.message.as_ref()),
    }
}

/// `value` cut at a character boundary to [`MAX_PRESENTED_TEXT_BYTES`].
pub fn bounded_visible_text(value: &str) -> &str {
    bounded_text(value, MAX_PRESENTED_TEXT_BYTES)
}

/// The most bytes of a title or subtitle handed to the JSON UI globals
/// (`#hud_title_text_string` / `#hud_subtitle_text_string`). Packs read data out of
/// these strings, so they keep the protocol decode limit, not the presentation one.
pub const MAX_TITLE_BINDING_BYTES: usize = protocol::MAX_UI_TEXT_BYTES;

/// `value` cut at a character boundary to [`MAX_TITLE_BINDING_BYTES`].
pub fn bounded_title_text(value: &str) -> &str {
    bounded_text(value, MAX_TITLE_BINDING_BYTES)
}

/// `value` cut at a character boundary to at most `max_bytes`.
pub fn bounded_text(value: &str, max_bytes: usize) -> &str {
    &value[..value.floor_char_boundary(max_bytes)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(
        kind: ChatMessageKind,
        source: Option<&str>,
        text: &str,
        parameters: &[&str],
    ) -> ChatMessage {
        ChatMessage {
            fifo_sequence: 0,
            received_millis: 0,
            kind,
            source: source.map(Arc::from),
            message: Arc::from(text),
            parameters: parameters.iter().map(|value| Arc::from(*value)).collect(),
        }
    }

    #[test]
    fn title_text_keeps_data_beyond_the_presentation_limit() {
        let long = "x".repeat(MAX_PRESENTED_TEXT_BYTES * 4);
        assert_eq!(bounded_visible_text(&long).len(), MAX_PRESENTED_TEXT_BYTES);
        assert_eq!(bounded_title_text(&long), long);
        let oversized = "§".repeat(MAX_TITLE_BINDING_BYTES);
        let cut = bounded_title_text(&oversized);
        assert!(cut.len() <= MAX_TITLE_BINDING_BYTES);
        assert!(oversized.starts_with(cut));
    }

    #[test]
    fn translation_row_resolves_key_and_splices_ordered_parameters() {
        let node = message(
            ChatMessageKind::Translation,
            None,
            "death.attack.player",
            &["Legolas", "Gimli"],
        );
        let resolved = resolve_chat_line(&node, |key| {
            (key == "death.attack.player").then(|| Arc::from("%1$s was slain by %2$s"))
        });
        assert_eq!(resolved, "Legolas was slain by Gimli");
    }

    #[test]
    fn death_message_uses_the_active_entity_name_and_preserves_player_names() {
        let node = message(
            ChatMessageKind::Translation,
            None,
            "death.attack.mob",
            &["notchyves", "%entity.zombie.name"],
        );
        assert_eq!(
            resolve_chat_line(&node, |key| match key {
                "death.attack.mob" => Some(Arc::from("%1$s was slain by %2$s")),
                "entity.zombie.name" => Some(Arc::from("§aZombie§r")),
                "notchyves" => Some(Arc::from("must stay literal")),
                _ => None,
            }),
            "notchyves was slain by §aZombie§r"
        );
    }

    #[test]
    fn marked_translation_keys_and_arguments_resolve() {
        let node = message(
            ChatMessageKind::Translation,
            None,
            "§e%multiplayer.player.joined",
            &["Alex"],
        );
        let resolved = resolve_chat_line(&node, |key| {
            (key == "multiplayer.player.joined").then(|| Arc::from("%s joined the game"))
        });
        assert_eq!(resolved, "§eAlex joined the game");
        let node = message(
            ChatMessageKind::Translation,
            None,
            "test.key",
            &["%menu.play"],
        );
        let resolved = resolve_chat_line(&node, |key| match key {
            "test.key" => Some(Arc::from("Selected %s")),
            "menu.play" => Some(Arc::from("Play")),
            _ => None,
        });
        assert_eq!(resolved, "Selected Play");
    }

    #[test]
    fn translation_arguments_keep_unmarked_names_and_literal_percent_text() {
        let node = message(
            ChatMessageKind::Translation,
            None,
            "wrap",
            &[
                "menu.play",
                "100% literal %menu.play",
                "%missing",
                "%menu.play",
            ],
        );
        assert_eq!(
            resolve_chat_line(&node, |key| match key {
                "wrap" => Some(Arc::from("%s | %s | %s | %s")),
                "menu.play" => Some(Arc::from("Play")),
                _ => None,
            }),
            "menu.play | 100% literal %menu.play | %missing | Play"
        );
    }

    #[test]
    fn zero_argument_translations_format_percent_escapes_while_literal_rows_stay_literal() {
        let translated = message(ChatMessageKind::Translation, None, "progress", &[]);
        assert_eq!(
            resolve_chat_line(&translated, |key| {
                (key == "progress").then(|| Arc::from("100%% complete"))
            }),
            "100% complete"
        );
        for kind in [ChatMessageKind::Chat, ChatMessageKind::System] {
            let literal = message(kind, None, "100%% complete", &[]);
            assert_eq!(resolve_chat_line(&literal, |_| None), "100%% complete");
        }
    }

    #[test]
    fn unknown_translation_key_presents_verbatim() {
        let node = message(ChatMessageKind::Translation, None, "death.attack.void", &[]);
        let resolved = resolve_chat_line(&node, |_| None);
        assert_eq!(resolved, "death.attack.void");
    }

    #[test]
    fn chat_row_prepends_the_source_name() {
        let node = message(ChatMessageKind::Chat, Some("Steve"), "hello", &[]);
        let resolved = resolve_chat_line(&node, |_| None);
        assert_eq!(resolved, "<Steve> hello");
    }

    #[test]
    fn chat_row_without_a_source_is_unchanged() {
        let node = message(ChatMessageKind::Chat, None, "server broadcast", &[]);
        let resolved = resolve_chat_line(&node, |_| None);
        assert_eq!(resolved, "server broadcast");
    }

    #[test]
    fn system_row_is_never_translated() {
        let node = message(ChatMessageKind::System, None, "chat.type.text", &["x"]);
        let resolved = resolve_chat_line(&node, |_| Some(Arc::from("<%s> %s")));
        assert_eq!(resolved, "chat.type.text");
    }
}
