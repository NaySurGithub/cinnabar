//! Native application adapter for shared JSON-UI atlas page insertion.
use super::super::UiPresentationError;
use assets::RuntimeUiAssets;
use render::UiRenderTextureArray;

pub(super) fn with_ui_pages(
    textures: &UiRenderTextureArray,
    assets: &RuntimeUiAssets,
) -> Result<(UiRenderTextureArray, u16), UiPresentationError> {
    render::with_ui_pages(textures, assets).map_err(|_| UiPresentationError::InvalidFontTexture)
}
