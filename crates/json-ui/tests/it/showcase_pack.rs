//! The local server's showcase HUD pack bound over the vanilla HUD: its bars read the sidebar
//! channel, and without that channel the vanilla HUD is untouched.

use crate::support;

use json_ui::{
    Catalog, Context, Draw, DrawNode, HUD_SCREEN, HudModel, LayoutEnv, Sidebar, TextMeasure,
    TextureMeta, TextureSource, ViewState, hud_context, hud_data_source, render_screen,
};

const PACK_HUD: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tools/localserver/showcase/pack/ui/hud_screen.json"
);
const SOLID: &str = "textures/ui/cinnabar_solid";

struct FixedText;
impl TextMeasure for FixedText {
    fn extent(&self, text: &str) -> [f64; 2] {
        [text.chars().count() as f64 * 6.0, 9.0]
    }
}

struct SquareTextures;
impl TextureSource for SquareTextures {
    fn texture(&self, _path: &str) -> Option<TextureMeta> {
        Some(TextureMeta {
            base_size: [16.0, 16.0],
            pixels: [16.0, 16.0],
            nineslice: None,
        })
    }
}

fn render(sidebar: Option<&str>) -> Option<Vec<DrawNode>> {
    let dir = support::vanilla_pack().join("ui");
    if !dir.is_dir() {
        return None;
    }
    let mut catalog = Catalog::load_dir(&dir).expect("vanilla ui loads");
    let hud = std::fs::read(PACK_HUD).expect("showcase pack hud_screen.json");
    let before = catalog.diagnostics().len();
    catalog.apply_pack([("ui/hud_screen.json", hud.as_slice())]);
    let notes = &catalog.diagnostics()[before..];
    assert!(notes.is_empty(), "showcase pack diagnostics: {notes:?}");
    let model = HudModel {
        survival_ui: true,
        armor_visible: true,
        hotbar_visible: true,
        sidebar: sidebar.map(|title| Sidebar {
            title: title.into(),
            rows: vec![("Steve".into(), "3".into())],
            background_opacity: 0.3,
            title_background_opacity: 0.4,
        }),
        ..HudModel::default()
    };
    let env = LayoutEnv {
        text: &FixedText,
        textures: &SquareTextures,
    };
    let render = render_screen(
        HUD_SCREEN,
        &catalog,
        &hud_context(&Context::desktop()),
        &hud_data_source(&model),
        [480.0, 270.0],
        &env,
        &ViewState::default(),
    )
    .expect("hud renders");
    Some(render.nodes)
}

fn renderers(nodes: &[DrawNode]) -> Vec<&str> {
    nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Custom { renderer, .. } => Some(renderer.as_str()),
            _ => None,
        })
        .collect()
}

fn texts(nodes: &[DrawNode]) -> Vec<&str> {
    nodes
        .iter()
        .filter_map(|node| match &node.draw {
            Draw::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

fn showcase_nodes(nodes: &[DrawNode]) -> usize {
    nodes
        .iter()
        .filter(|node| matches!(&node.draw, Draw::Sprite { texture, .. } if texture == SOLID))
        .count()
}

/// The visible fraction of the named status bar: its fill's width over its track's.
fn bar_fraction(nodes: &[DrawNode], bar: &str) -> f64 {
    let width = |part: &str| {
        nodes
            .iter()
            .find(|node| {
                node.key.contains(&format!("{bar}/{part}"))
                    || node.key.ends_with(&format!("{bar}.{part}"))
            })
            .unwrap_or_else(|| panic!("{bar} {part} not drawn"))
            .dest
            .w
    };
    width("fill") / width("back")
}

#[test]
fn showcase_channel_drives_the_bars_and_hides_the_vanilla_status() {
    // Stamina 75, health 90%, energy 40, flash ready, slam half cooled, charging.
    let Some(nodes) = render(Some("cnb175190140100150101")) else {
        return;
    };
    let shown = renderers(&nodes);
    for renderer in ["heart_renderer", "armor_renderer", "hunger_renderer"] {
        assert!(
            !shown.contains(&renderer),
            "{renderer} drawn under the showcase"
        );
    }
    assert!(
        !texts(&nodes).contains(&"Steve"),
        "vanilla sidebar drawn under the showcase"
    );
    for (bar, want) in [("health", 0.9), ("stamina", 0.75), ("energy", 0.4)] {
        let got = bar_fraction(&nodes, bar);
        assert!((got - want).abs() < 1e-3, "{bar} shows {got}, want {want}");
    }
}

#[test]
fn without_the_channel_the_vanilla_hud_is_untouched() {
    for sidebar in [None, Some("Kills")] {
        let Some(nodes) = render(sidebar) else {
            return;
        };
        let shown = renderers(&nodes);
        for renderer in ["heart_renderer", "armor_renderer", "hunger_renderer"] {
            assert!(
                shown.contains(&renderer),
                "{renderer} hidden with sidebar {sidebar:?}"
            );
        }
        assert_eq!(
            showcase_nodes(&nodes),
            0,
            "showcase HUD drawn with sidebar {sidebar:?}"
        );
        if sidebar.is_some() {
            assert!(texts(&nodes).contains(&"Steve"), "vanilla sidebar hidden");
        }
    }
}
