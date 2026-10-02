//! Native application adapter for the shared HUD renderer owner.

use serde_json::Value;
use std::collections::BTreeMap;
use ui::UiVisual;

use super::Painter;
pub(crate) use ui::native_hud::{HudPaint, SheetSprite};

impl ui::native_hud::HudPaintTarget for Painter<'_> {
    fn gui_pixel_scale(&self) -> f32 {
        self.px
    }
    fn sprite(&self, path: &str, color: [u8; 4]) -> Option<UiVisual> {
        Painter::sprite(self, path, json_ui::UvRect::full(), color)
    }
    fn push(&mut self, visual: UiVisual, bounds: [f32; 4]) {
        let _ = Painter::push(self, visual, bounds);
    }
    fn solid(&mut self, bounds: [f32; 4], color: [u8; 4]) {
        let _ = Painter::solid(self, bounds, color);
    }
}

pub(super) fn paint(
    painter: &mut Painter<'_>,
    hud: &HudPaint,
    renderer: &str,
    data: &BTreeMap<String, Value>,
    dest: [f32; 4],
    alpha: &dyn Fn([u8; 4]) -> [u8; 4],
) -> bool {
    let index = data
        .get("#collection_index")
        .and_then(Value::as_f64)
        .map_or(0, |value| value.clamp(0.0, 8.0) as usize);
    let notches = data
        .get("#bar_notches")
        .and_then(Value::as_f64)
        .map_or(0, |value| value.clamp(0.0, 64.0) as u32);
    ui::native_hud::paint(painter, hud, renderer, index, notches, dest, alpha)
}

/// `vanilla` under the built-in Java HUD pack, less its files for namespaces in
/// `withdrawn` (restyled by a server pack authored against vanilla); no Mojang footer.
pub(super) fn with_java_hud(
    vanilla: &json_ui::Catalog,
    withdrawn: &std::collections::BTreeSet<String>,
) -> json_ui::Catalog {
    let mut catalog = vanilla.clone();
    let kept = super::super::hud::JAVA_HUD_PACK
        .iter()
        .filter(|(_, namespace, _)| !withdrawn.contains(*namespace))
        .map(|(path, _, bytes)| (*path, *bytes));
    catalog.apply_pack(kept);
    catalog.apply_pack(
        [(
            "ui/cinnabar_title.json",
            super::menu_renderers::TITLE_PANEL_OVERLAY,
        )]
        .into_iter()
        .chain(super::menu_renderers::NO_COPYRIGHT_OVERLAYS),
    );
    catalog
}
