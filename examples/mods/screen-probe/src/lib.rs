//! The host tests' `player-mod` package. Each callback reports what it received or read as a bound
//! value, so a test reads the guest's view of the host from the committed screens. A few inputs
//! misbehave on purpose: the `probe.trap` key traps, the text `loop` never returns,
//! `probe.flood` binds until the host refuses, and `probe.read_all` reads the whole session in
//! an ordinary event, as `data-changed` does on the load budget. `probe.hud` sets the HUD
//! template, and `target-changed` reports the target, mining, harvest and text reads.

use mod_api::PlayerMod;
use mod_api::player_mod::{
    DataSource, HudLayout, ScreenLayout, Stack,
    cinnabar::extension::{hud_layer, screen},
    cinnabar::server_experience::ui::Value,
    cinnabar::session::{items, recipes, target, text as host_text},
};

mod declared {
    experience_sdk::include_declarations!();
}

struct Probe;

thread_local! {
    /// How often the host closed the view.
    static VIEW_CLOSED: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
}

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

/// Reads every item and recipe page by page, as a mod loading the session does, and reports
/// how many it read.
fn read_all() {
    let mut items = 0;
    while let Ok(page) = items::page(items, 256) {
        if page.is_empty() {
            break;
        }
        items += page.len() as u32;
    }
    let mut recipes = 0;
    while let Ok(page) = recipes::page(recipes, 256) {
        if page.is_empty() {
            break;
        }
        recipes += page.len() as u32;
    }
    report("#items_read", Value::Integer(items.into()));
    report("#recipes_read", Value::Integer(recipes.into()));
}

impl PlayerMod for Probe {
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
        } else if id == declared::actions::HUD {
            hud_layer::set_template(Some(declared::templates::HUD))
                .and_then(|()| hud_layer::set_value("#hud_set", &Value::Boolean(true)))
        } else if id == declared::actions::READ_ALL {
            read_all();
            Ok(())
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

    fn scrolled(delta: f64, x: f64, y: f64, modifiers: screen::Modifiers) {
        let held = |modifier| f64::from(u8::from(modifiers.contains(modifier)));
        let (ctrl, shift, alt) = (
            held(screen::Modifiers::CTRL),
            held(screen::Modifiers::SHIFT),
            held(screen::Modifiers::ALT),
        );
        report("#scrolled", Value::Numbers(vec![delta, x, y]));
        report("#scroll_modifiers", Value::Numbers(vec![ctrl, shift, alt]));
    }

    fn text_changed(control: String, text_now: String) {
        if text_now == "loop" {
            #[allow(clippy::empty_loop, reason = "the host's fuel must stop this")]
            loop {}
        }
        text("#search", format!("{control}={text_now}"));
    }

    fn key(id: String, hovered: Option<Stack>, row: Option<(String, u32)>) {
        if id == declared::keys::TRAP {
            panic!("probe trap");
        }
        let hovered = hovered.map_or_else(String::new, |stack| {
            format!(
                "{}@{}x{}#{}",
                stack.key.identifier, stack.key.aux, stack.count, stack.icon
            )
        });
        text("#key", format!("{id}:{hovered}:{row:?}"));
    }

    fn data_changed(sources: Vec<DataSource>) {
        let names: Vec<&str> = sources
            .iter()
            .map(|source| match source {
                DataSource::Items => "items",
                DataSource::Recipes => "recipes",
            })
            .collect();
        text("#data_sources", names.join(","));
        read_session();
        read_all();
    }

    fn target_changed() {
        let look = match target::current() {
            Ok(Some(look)) => {
                let hit = match look.hit {
                    Some(target::Hit::Block(hit)) => {
                        format!("block:{}:{}", hit.block.identifier, hit.block.name)
                    }
                    Some(target::Hit::Actor(hit)) => {
                        format!("actor:{}:{:?}", hit.type_id, hit.health)
                    }
                    None => "none".into(),
                };
                let liquid = look.liquid.map_or_else(String::new, |liquid| {
                    format!("+{}", liquid.block.identifier)
                });
                hit + &liquid
            }
            Ok(None) => "outside".into(),
            Err(error) => error,
        };
        text("#target", look);
        if let Ok(revision) = target::revision() {
            report("#target_revision", Value::Integer(revision as i64));
        }
        if let Ok(Some(mining)) = target::mining() {
            report(
                "#mining",
                Value::Numbers(vec![mining.progress.into(), mining.per_tick.into()]),
            );
        }
        let candidates = [
            "minecraft:wooden_pickaxe".to_owned(),
            "minecraft:iron_pickaxe".to_owned(),
        ];
        if let Ok(Some(harvest)) = target::harvest(&candidates) {
            text(
                "#harvest",
                format!("{}:{}", harvest.requires_tool, harvest.effective.join(",")),
            );
        }
        text(
            "#translated",
            host_text::translate("probe.key").unwrap_or_default(),
        );
        report("#width", Value::Number(host_text::width("abc").into()));
        let _ = hud_layer::set_value("#hud_target", &Value::Boolean(true));
    }

    fn hud_changed(layout: Option<HudLayout>) {
        let numbers = layout.map_or_else(
            || vec![-1.0],
            |layout| {
                let bars = layout.boss_bars.map_or(-1.0, |bars| bars.height);
                vec![layout.size.width, layout.size.height, bars]
            },
        );
        // HUD-only writes, so a test sees the screens' data stay put.
        let _ = hud_layer::set_value("#hud_layout", &Value::Numbers(numbers));
        let seconds = hud_layer::frame_seconds().into();
        let _ = hud_layer::set_value("#frame_seconds", &Value::Number(seconds));
    }

    fn view_closed() {
        let closed = VIEW_CLOSED.get() + 1;
        VIEW_CLOSED.set(closed);
        report("#view_closed", Value::Integer(closed));
    }
}

mod_api::export_player_mod!(Probe);
