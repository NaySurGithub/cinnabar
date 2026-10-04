//! The host tests' extension 0.2 mod. Each callback reports what it received or read as a bound
//! value, so a test reads the guest's view of the host from the committed screens. A few inputs
//! misbehave on purpose: the `probe.trap` key traps, the text `loop` never returns, and
//! `probe.flood` binds until the host refuses.

use mod_api::v0_2::{
    Guest, ScreenLayout, Stack,
    cinnabar::extension0_2_0::screen,
    cinnabar::server_experience::ui::Value,
    cinnabar::session::{items, recipes},
};

mod declared {
    experience_sdk::include_declarations!();
}

struct Probe;

fn report(name: &str, value: Value) {
    let _ = screen::set_value(name, &value);
}

fn text(name: &str, text: impl Into<String>) {
    report(name, Value::Text(text.into()));
}

/// What the session reads return now: counts, revisions and the first item's name.
fn read_session() {
    match items::count() {
        Ok(count) => report("#items", Value::Integer(count.into())),
        Err(error) => text("#items_error", error),
    }
    match items::page(0, 100_000) {
        Ok(page) => {
            report("#page", Value::Integer(page.len() as i64));
            if let Some(first) = page.first() {
                text("#first", first.name.clone());
            }
        }
        Err(error) => text("#items_error", error),
    }
    if let Ok(revision) = items::revision() {
        report("#item_revision", Value::Integer(revision as i64));
    }
    match recipes::page(0, 10) {
        Ok(page) => {
            report("#recipes", Value::Integer(page.len() as i64));
            if let Some(first) = page.first() {
                report(
                    "#first_cells",
                    Value::Integer(first.ingredients.len() as i64),
                );
            }
        }
        Err(error) => text("#recipes_error", error),
    }
}

impl Guest for Probe {
    fn init() {
        let _ = screen::set_overlay(Some(declared::templates::OVERLAY));
        read_session();
    }

    fn frame() {}

    fn screen_changed(layout: Option<ScreenLayout>) {
        match layout {
            Some(layout) => {
                text("#screen", layout.screen);
                report(
                    "#gui",
                    Value::Numbers(vec![layout.gui.x, layout.gui.y, layout.gui.width]),
                );
                report(
                    "#exclusions",
                    Value::Integer(layout.exclusions.len() as i64),
                );
            }
            None => text("#screen", ""),
        }
        let open = screen::layout().is_some();
        report("#layout_open", Value::Boolean(open));
    }

    fn action(id: String, collection_index: Option<u32>) {
        let result = if id == declared::actions::VIEW {
            screen::open_view(Some(declared::templates::VIEW))
        } else if id == declared::actions::CLOSE {
            screen::open_view(None)
        } else if id == declared::actions::FLOOD {
            let row = format!(
                r##"{{"#n":{{"type":"text","value":"{}"}}}}"##,
                "x".repeat(400)
            );
            let rows = format!("[{}]", vec![row; 200].join(","));
            let mut result = Ok(());
            for index in 0..64 {
                result = screen::set_collection(&format!("flood{index}"), rows.as_bytes());
                if result.is_err() {
                    break;
                }
            }
            result
        } else {
            Ok(())
        };
        text("#action", format!("{id}:{collection_index:?}:{result:?}"));
    }

    fn secondary_action(id: String, collection_index: Option<u32>) {
        text("#secondary", format!("{id}:{collection_index:?}"));
    }

    fn scrolled(delta: f64, x: f64, y: f64) {
        report("#scrolled", Value::Numbers(vec![delta, x, y]));
    }

    fn text_changed(control: String, text_now: String) {
        if text_now == "loop" {
            #[allow(clippy::empty_loop, reason = "the host's fuel must stop this")]
            loop {}
        }
        text("#search", format!("{control}={text_now}"));
    }

    fn key(id: String, hovered: Option<Stack>) {
        if id == declared::keys::TRAP {
            panic!("probe trap");
        }
        let hovered = hovered.map_or_else(String::new, |stack| {
            format!(
                "{}@{}x{}#{}",
                stack.key.identifier, stack.key.aux, stack.count, stack.icon
            )
        });
        text("#key", format!("{id}:{hovered}"));
    }

    fn data_changed() {
        read_session();
    }
}

mod_api::v0_2::export_player_mod!(Probe with_types_in mod_api::v0_2);
