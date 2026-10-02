//! Native application adapters for shared HUD/icon atlas construction.
use super::UiPresentationError;
use assets::{RuntimeFontCatalog, RuntimeHudCatalog, RuntimeIconCatalog};
pub(super) use render::HudTexturePages;
pub(crate) use render::IconRef;
use std::sync::Arc;

pub(super) fn font_texture_array_with_optional_hud(
    font: &Arc<RuntimeFontCatalog>,
    hud: Option<&RuntimeHudCatalog>,
) -> Result<(render::UiRenderTextureArray, u16, Option<HudTexturePages>), UiPresentationError> {
    render::font_texture_array_with_optional_hud(font, hud)
        .map_err(|_| UiPresentationError::InvalidFontTexture)
}

pub(super) fn font_texture_array_with_hud_and_icons(
    font: &Arc<RuntimeFontCatalog>,
    hud: Option<&RuntimeHudCatalog>,
    icons: Option<&RuntimeIconCatalog>,
) -> Result<render::TextureArrayWithIcons, UiPresentationError> {
    render::font_texture_array_with_hud_and_icons(font, hud, icons)
        .map_err(|_| UiPresentationError::InvalidFontTexture)
}

pub(super) fn font_texture_array(
    font: &Arc<RuntimeFontCatalog>,
) -> Result<(render::UiRenderTextureArray, u16), UiPresentationError> {
    render::font_texture_array(font).map_err(|_| UiPresentationError::InvalidFontTexture)
}
