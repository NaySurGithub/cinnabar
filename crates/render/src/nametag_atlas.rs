//! Name-tag lines rasterized at font resolution into the shared atlas the tag billboards sample.

use std::{collections::HashMap, sync::Arc};

use crate::nametag::{NAMETAG_ATLAS_SIDE, NametagAtlasRect};
use assets::RuntimeFontCatalog;
use ui::{
    FONT_DESIGN_PIXEL_TEXELS, TEXT_BASELINE_64, TEXT_LINE_HEIGHT_64, TextLayoutCache,
    TextLayoutRequest, TextStyle, UiScale,
};

/// Widest line laid out before it would wrap, in font texels.
const MAX_LINE_TEXELS: u32 = NAMETAG_ATLAS_SIDE;

/// RGBA8 texels of one UI texture page a glyph samples.
#[derive(Clone, Copy)]
pub struct GlyphPage<'a> {
    pub width: u32,
    pub height: u32,
    pub rgba8: &'a [u8],
}

/// The font's own pages, which lead the UI texture pages.
pub fn font_page(font: &RuntimeFontCatalog, page: usize) -> Option<GlyphPage<'_>> {
    font.pages().get(page).map(|page| GlyphPage {
        width: page.width,
        height: page.height,
        rgba8: &page.rgba8,
    })
}

/// One rasterized line: its atlas cell in texels and its width in font pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AtlasLine {
    pub cell: [u32; 4],
    pub width_px: f32,
    /// Font pixels from the line box's top to the cell's top (negative for tall glyphs).
    pub top_px: f32,
}

/// Shelf-packed line cells, rebuilt from scratch when a frame's lines no longer fit.
#[derive(Default)]
pub struct NametagAtlas {
    rectangles: Vec<NametagAtlasRect>,
    lines: HashMap<Arc<str>, AtlasLine>,
    shelf: [u32; 3],
    published: Option<Arc<[NametagAtlasRect]>>,
    revision: u64,
}

impl NametagAtlas {
    /// The cell of `text`, rasterizing it on first use; `None` when the font cannot lay it out.
    pub fn line<'p>(
        &mut self,
        text: &Arc<str>,
        font: &RuntimeFontCatalog,
        layouts: &mut TextLayoutCache,
        pages: &impl Fn(usize) -> Option<GlyphPage<'p>>,
    ) -> Option<AtlasLine> {
        if let Some(line) = self.lines.get(text) {
            return Some(*line);
        }
        let (width, height, top, rgba8, advance) = rasterize(text, font, layouts, pages)?;
        let origin = self.allocate(width, height)?;
        self.rectangles.push(NametagAtlasRect {
            cell: [origin[0], origin[1], width, height],
            rgba8: rgba8.into(),
        });
        let line = AtlasLine {
            cell: [origin[0], origin[1], width, height],
            width_px: advance as f32 / FONT_DESIGN_PIXEL_TEXELS as f32,
            top_px: top as f32 / FONT_DESIGN_PIXEL_TEXELS as f32,
        };
        self.lines.insert(Arc::clone(text), line);
        self.published = None;
        Some(line)
    }

    /// Forgets every line so the next frame packs only what it draws.
    pub fn reset(&mut self) {
        self.lines.clear();
        self.shelf = [0; 3];
        self.rectangles.clear();
        self.published = None;
    }

    /// Checks the retained line-count budget.
    pub fn has_room_for(&self, texts: usize) -> bool {
        self.lines.len() + texts < MAX_ATLAS_LINES
    }

    /// Retains immutable line pixels so skipped extractions can recover every update.
    pub fn publish(&mut self) -> (Arc<[NametagAtlasRect]>, u64) {
        if self.published.is_none() {
            self.revision += 1;
            self.published = Some(self.rectangles.clone().into());
        }
        (
            Arc::clone(self.published.as_ref().expect("just published")),
            self.revision,
        )
    }

    /// Reserves a non-overlapping shelf cell.
    fn allocate(&mut self, width: u32, height: u32) -> Option<[u32; 2]> {
        let side = NAMETAG_ATLAS_SIDE;
        if width > side || height > side {
            return None;
        }
        let [mut x, mut y, mut shelf_height] = self.shelf;
        if x + width > side {
            (x, y, shelf_height) = (0, y + shelf_height, 0);
        }
        if y + height > side {
            return None;
        }
        self.shelf = [x + width, y, shelf_height.max(height)];
        Some([x, y])
    }
}

/// Lines kept before the atlas is rebuilt, far above any frame's visible tags.
const MAX_ATLAS_LINES: usize = 4096;

/// `text` as one unwrapped line of font texels: `(width, height, RGBA8)`. Glyph texels keep
/// their own colour (image glyphs) times the `§` colour, white when unset.
fn rasterize<'p>(
    text: &str,
    font: &RuntimeFontCatalog,
    layouts: &mut TextLayoutCache,
    pages: &impl Fn(usize) -> Option<GlyphPage<'p>>,
) -> Option<(u32, u32, i32, Vec<u8>, u32)> {
    let layout = layouts
        .layout(TextLayoutRequest {
            text,
            style: TextStyle::default(),
            width_64: MAX_LINE_TEXELS * 64,
            line_height_64: TEXT_LINE_HEIGHT_64,
            baseline_64: TEXT_BASELINE_64,
            scale: UiScale::default(),
            font,
        })
        .ok()?;
    let advance = layout.size_64()[0].div_ceil(64).max(1);
    let glyphs = || layout.glyphs().iter().filter(|glyph| glyph.line == 0);
    let top = glyphs()
        .map(|glyph| glyph.bounds_64[1].div_euclid(64))
        .min()
        .unwrap_or(0)
        .min(0);
    let bottom = glyphs()
        .map(|glyph| (glyph.bounds_64[3] + 63).div_euclid(64))
        .max()
        .unwrap_or(0)
        .max(TEXT_LINE_HEIGHT_64.div_ceil(64) as i32);
    let right = glyphs()
        .map(|glyph| (glyph.bounds_64[2] + 63).div_euclid(64))
        .max()
        .unwrap_or(0)
        .max(advance as i32);
    let (width, height) = (right as u32, (bottom - top) as u32);
    let mut canvas = vec![0u8; (width * height * 4) as usize];
    for glyph in glyphs() {
        let Some(page) = pages(usize::from(glyph.page)) else {
            continue;
        };
        let tint = glyph.style.color.rgb().unwrap_or([255; 3]);
        // Sheet glyphs are drawn scaled into their bounds, so sample the source nearest-texel.
        let mut bounds = glyph.bounds_64.map(|value| value as f32 / 64.0);
        bounds[1] -= top as f32;
        bounds[3] -= top as f32;
        let [u0, v0, u1, v1] = glyph.uv.map(f32::from);
        // Font UVs address texel edges, with exclusive upper bounds as in the UI shader.
        let (source_width, source_height) = (u1 - u0, v1 - v0);
        let (dest_width, dest_height) = (bounds[2] - bounds[0], bounds[3] - bounds[1]);
        if dest_width <= 0.0 || dest_height <= 0.0 {
            continue;
        }
        for dy in bounds[1].floor().max(0.0) as u32..(bounds[3].ceil() as u32).min(height) {
            for dx in bounds[0].floor().max(0.0) as u32..(bounds[2].ceil() as u32).min(width) {
                let fx = (dx as f32 + 0.5 - bounds[0]) / dest_width;
                let fy = (dy as f32 + 0.5 - bounds[1]) / dest_height;
                if !(0.0..1.0).contains(&fx) || !(0.0..1.0).contains(&fy) {
                    continue;
                }
                let sx = (u0 + fx * source_width) as u32;
                let sy = (v0 + fy * source_height) as u32;
                if sx >= page.width || sy >= page.height {
                    continue;
                }
                let source = ((sy * page.width + sx) * 4) as usize;
                let Some(texel) = page.rgba8.get(source..source + 4) else {
                    continue;
                };
                if texel[3] == 0 {
                    continue;
                }
                let target = ((dy * width + dx) * 4) as usize;
                for channel in 0..3 {
                    canvas[target + channel] =
                        (u16::from(texel[channel]) * u16::from(tint[channel]) / 255) as u8;
                }
                canvas[target + 3] = texel[3];
            }
        }
    }
    Some((width, height, top, canvas, advance))
}

#[cfg(test)]
mod tests {
    use super::*;
    use assets::{FontTexturePage, GlyphMetrics, encode_font_catalog};
    use sha2::{Digest, Sha256};

    #[test]
    fn glyph_sampling_keeps_exclusive_edges_and_neighbouring_ink_outside_the_cell() {
        let mut pixels = [255, 0, 0, 255].repeat(16);
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let start = (y * 4 + x) * 4;
            pixels[start..start + 4].copy_from_slice(&[255; 4]);
        }
        let page = FontTexturePage {
            source_path: "font/edges.png".into(),
            source_bytes: pixels.len() as u32,
            source_sha256: [1; 32],
            pixels_sha256: Sha256::digest(&pixels).into(),
            width: 4,
            height: 4,
            rgba8: pixels.into_boxed_slice(),
        };
        let glyph = GlyphMetrics {
            codepoint: 'A',
            page: 0,
            uv: [0, 0, 2, 2],
            bearing: [0, 0],
            advance_64: 2 * 64,
        };
        let manifest = [7; 32];
        let bytes = encode_font_catalog(manifest, &[glyph], &[page]).unwrap();
        let font = RuntimeFontCatalog::decode(&bytes, manifest).unwrap();
        let mut layouts = TextLayoutCache::new(4, 64 * 1024);
        let mut atlas = NametagAtlas::default();
        atlas
            .line(&Arc::from("A"), &font, &mut layouts, &|page| {
                font_page(&font, page)
            })
            .unwrap();
        let (rectangles, _) = atlas.publish();
        let ink: Vec<_> = rectangles[0]
            .rgba8
            .chunks_exact(4)
            .filter(|pixel| pixel[3] != 0)
            .collect();
        assert_eq!(ink, vec![&[255; 4][..]; 4]);
    }
}
