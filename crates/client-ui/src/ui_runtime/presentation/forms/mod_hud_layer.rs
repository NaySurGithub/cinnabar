//! A player mod's template over the gameplay HUD. It follows the package screen rules of
//! [`super::template_screen`] with its own bound data, so a HUD change never rebuilds the mod's
//! container screens. The host hides it with the HUD (hide-GUI, loading) and under every screen
//! but chat, which it draws over; it is never hit-tested.

use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

use assets::RuntimeFontCatalog;
use json_ui::{LabelShape, TextMeasure};
use server_experience::screen::{self, GuiSize, HudLayout};
use ui::{TextLayoutCache, UiNode};

use super::super::{FONT_DESIGN_PIXEL_TEXELS, TextMetrics, UiPresentationRuntime};
use super::{
    engine::{EngineInputs, ScreenArt},
    template_screen::{TemplateArt, TemplateScreen},
};
use crate::ui_runtime::{Translator, UiRuntime};

/// What a player mod draws over the HUD, as its host committed it.
pub struct ModHudInput<'a> {
    /// The mod's id, which owns its catalog and textures.
    pub id: &'a str,
    pub files: &'a Arc<screen::Files>,
    pub data: &'a screen::Modal,
}

pub(super) struct ModHudLayer {
    art: TemplateArt,
    screen: TemplateScreen,
    icon_keys: BTreeSet<i64>,
    /// The HUD as the last build laid it out for the layer; none while hidden.
    layout: Option<HudLayout>,
}

impl ModHudLayer {
    /// A build that draws no HUD leaves the layer hidden.
    pub(super) fn begin_frame(&mut self) {
        self.layout = None;
    }
}

impl UiPresentationRuntime {
    /// Follows the HUD owner's committed layer; `None` removes it and drops its catalog and
    /// textures.
    pub fn set_mod_hud(&mut self, input: Option<ModHudInput<'_>>) {
        let page = (self.textures.dynamic_start() + super::super::dynamic_textures::MOD_HUD_UI_PAGE)
            as u16;
        let slot = &mut self.form_presentation.mod_hud_layer;
        let Some(input) = input else {
            *slot = None;
            return;
        };
        if slot
            .as_ref()
            .is_none_or(|current| !current.art.is(input.id, input.files))
        {
            *slot = Some(ModHudLayer {
                art: TemplateArt::new(
                    input.id,
                    input.files,
                    page,
                    super::super::dynamic_textures::MOD_HUD_UI_PAGES,
                ),
                screen: TemplateScreen::default(),
                icon_keys: BTreeSet::new(),
                layout: None,
            });
        }
        let layer = slot.as_mut().expect("mod HUD installed");
        layer
            .screen
            .follow(input.data.template.as_ref(), input.data);
        layer.icon_keys = super::mod_screens::icon_keys(input.data);
    }

    /// The HUD the last build laid the layer out on; none while the host hides it.
    pub fn mod_hud_layout(&self) -> Option<&HudLayout> {
        self.form_presentation
            .mod_hud_layer
            .as_ref()?
            .layout
            .as_ref()
    }

    /// Why the HUD owner's template was refused, which quarantines it.
    pub fn mod_hud_failure(&self) -> Option<&str> {
        self.form_presentation.mod_hud_layer.as_ref()?.art.failure()
    }

    /// Whether the HUD layer may draw now: the HUD is not hidden (F1) and nothing but chat
    /// covers it.
    pub(in super::super) fn mod_hud_visible(
        &self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
    ) -> bool {
        let hidden_hud = self
            .form_presentation
            .chat
            .settings
            .options
            .value("hide_hud")
            != 0;
        self.loading_stage.is_none()
            && !hidden_hud
            && !self.experience_modal_open()
            && (!runtime.ui_focused(player_runtime) || runtime.chat_focused())
    }

    /// Draws the layer over the whole content area, when it may draw; otherwise records it as
    /// hidden.
    #[allow(
        clippy::too_many_arguments,
        reason = "Player authority is borrowed separately from UI state."
    )]
    pub(in super::super) fn append_mod_hud_layer(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
    ) {
        let visible = self.mod_hud_visible(player_runtime, runtime);
        let Some(keys) = self
            .form_presentation
            .mod_hud_layer
            .as_ref()
            .map(|layer| layer.icon_keys.clone())
        else {
            return;
        };
        let id_aux = super::mod_screens::item_icons(&keys, player_runtime, runtime, |id, aux| {
            self.item_icon(id, aux)
        });
        let layer = self
            .form_presentation
            .mod_hud_layer
            .as_mut()
            .expect("mod HUD checked above");
        layer.screen.frame = None;
        let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        layer.layout = visible.then(|| HudLayout {
            size: GuiSize {
                width: f64::from(content[0] / px),
                height: f64::from(content[1] / px),
                scale: f64::from(px),
            },
            boss_bars: None,
        });
        let Some(renderer) = self.form_presentation.engine.as_deref() else {
            return;
        };
        if !visible || layer.screen.template.is_none() {
            return;
        }
        let translate = |key: &str| runtime.translation(key);
        let inputs = EngineInputs {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content,
            translate: &translate,
            language: runtime.text_generation(),
        };
        let art = ScreenArt {
            id_aux: &id_aux,
            ..ScreenArt::default()
        };
        layer
            .screen
            .draw(&mut layer.art, renderer, inputs, (nodes, next), art);
        // The layer is presentation only: nothing under it is the mod's to press.
        layer.screen.frame = None;
    }

    /// Copies the HUD atlas's page images when they changed; `true` asks for a page rebuild.
    pub(super) fn refresh_mod_hud_pages(&mut self) -> bool {
        self.form_presentation
            .mod_hud_layer
            .as_mut()
            .is_some_and(|layer| layer.art.refresh_pages())
    }

    /// The HUD atlas's pages, for the dynamic pages reserved to it.
    pub(in super::super) fn mod_hud_pages(&self) -> &[render_model::UiTexturePage] {
        self.form_presentation
            .mod_hud_layer
            .as_ref()
            .map_or(&[], |layer| layer.art.pages())
    }

    /// Follows the language and GUI scale a mod's `text` reads, replacing the services only
    /// when either changed.
    pub(in super::super) fn observe_mod_text(&mut self, runtime: &UiRuntime, metrics: TextMetrics) {
        let key = (runtime.text_generation(), metrics.scale.get().to_bits());
        if self
            .form_presentation
            .mod_text
            .as_ref()
            .is_some_and(|(current, _)| *current == key)
        {
            return;
        }
        let text = ModText {
            translator: runtime.translator(),
            font: Arc::clone(&self.font),
            metrics,
            px: metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32,
            layouts: Mutex::new(TextLayoutCache::new(
                MEASURE_CACHE_ENTRIES,
                MEASURE_CACHE_BYTES,
            )),
        };
        self.form_presentation.mod_text = Some((key, Arc::new(text)));
    }

    /// The client's language and the HUD font's measure, for a mod's `text`; the same `Arc`
    /// until either changes.
    pub fn mod_text(&self) -> Option<Arc<ModText>> {
        self.form_presentation
            .mod_text
            .as_ref()
            .map(|(_, text)| Arc::clone(text))
    }
}

/// A mod measures a few lines per target; a small cache of its own keeps it off the frame's.
const MEASURE_CACHE_ENTRIES: usize = 64;
const MEASURE_CACHE_BYTES: usize = 256 * 1024;

/// A mod's text services, keyed by the text generation and GUI scale bits they follow.
pub(in super::super) type ModTextCache = (([usize; 3], u32), Arc<ModText>);

/// The text services a mod's `text` import reads: owned, so they outlive the frame.
pub struct ModText {
    translator: Translator,
    font: Arc<RuntimeFontCatalog>,
    metrics: TextMetrics,
    px: f32,
    layouts: Mutex<TextLayoutCache>,
}

impl ModText {
    /// `key` in the active language, the server's strings first.
    pub fn translate(&self, key: &str) -> Option<String> {
        self.translator.lookup(key).map(|text| text.to_string())
    }

    /// `text`'s width in GUI units as a HUD label draws it.
    pub fn width(&self, text: &str) -> f32 {
        let Ok(mut layouts) = self.layouts.lock() else {
            return 0.0;
        };
        let cell = std::cell::RefCell::new(&mut *layouts);
        let measure = super::engine::text_paint::Measure {
            layouts: &cell,
            font: &self.font,
            metrics: self.metrics,
            px: self.px,
            translate: &|_| None,
        };
        measure.label(
            text,
            None,
            LabelShape {
                scale: 1.0,
                line_padding: 0.0,
                hide_hyphen: false,
            },
        )[0] as f32
    }
}

#[cfg(test)]
mod tests;
