//! Native adapter for the shared nametag atlas owner.

pub(crate) use render::{
    NametagAtlas, NametagGlyphPage as GlyphPage, nametag_font_page as font_page,
};

#[cfg(test)]
use render::{NAMETAG_ATLAS_SIDE, NametagAtlasRect};
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use ui::TextLayoutCache;

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    /// Builds the labels used by the original full-atlas timing fixture.
    fn sample_atlas() -> (NametagAtlas, std::time::Duration) {
        let font = super::super::tests::fixture_font();
        let mut layouts = TextLayoutCache::new(256, 1 << 20);
        let mut atlas = NametagAtlas::default();
        let started = std::time::Instant::now();
        for index in 0..100 {
            let text = Arc::from(format!("Player {index}"));
            atlas
                .line(&text, &font, &mut layouts, &|page| font_page(&font, page))
                .unwrap();
            std::hint::black_box(atlas.publish());
        }
        let elapsed = started.elapsed();
        (atlas, elapsed)
    }

    /// Measures changing labels and fingerprints their published pixels.
    #[test]
    #[ignore = "release performance measurement"]
    fn frame_cost_bench_nametag_updates() {
        let (mut atlas, elapsed) = sample_atlas();
        let (rectangles, _) = atlas.publish();
        let mut pixels = vec![0; (NAMETAG_ATLAS_SIDE * NAMETAG_ATLAS_SIDE * 4) as usize];
        apply_rectangles(&mut pixels, &rectangles, &[]);
        let bytes: usize = rectangles
            .iter()
            .map(|rectangle| rectangle.rgba8.len())
            .sum();
        eprintln!(
            "NAMETAG_BENCH updates=100 ms={:.3} upload_bytes={bytes} sha256={:x}",
            elapsed.as_secs_f64() * 1000.0,
            Sha256::digest(&pixels)
        );
    }
    #[test]
    fn atlas_pixels_match_full_publication_baseline() {
        let (mut atlas, _) = sample_atlas();
        let mut pixels = vec![0; (NAMETAG_ATLAS_SIDE * NAMETAG_ATLAS_SIDE * 4) as usize];
        apply_rectangles(&mut pixels, &atlas.publish().0, &[]);
        // Includes the final glyph row under the exclusive UV-edge sampling contract;
        // render's neighbouring-ink regression covers that boundary directly.
        assert_eq!(
            format!("{:x}", Sha256::digest(&pixels)),
            "8e783b2f6d5e95928b2ae44fbd31a8a8779a1a3df613799960e4974ee02516f8"
        );
    }

    /// Applies the same dirty rectangles submitted by the renderer.
    fn apply_rectangles(
        pixels: &mut [u8],
        current: &[NametagAtlasRect],
        previous: &[NametagAtlasRect],
    ) {
        for rectangle in NametagAtlasRect::updates(current, previous) {
            let [x, y, width, height] = rectangle.cell;
            for row in 0..height as usize {
                let target = (((y as usize + row) * NAMETAG_ATLAS_SIDE as usize) + x as usize) * 4;
                let source = row * width as usize * 4;
                pixels[target..target + width as usize * 4]
                    .copy_from_slice(&rectangle.rgba8[source..source + width as usize * 4]);
            }
        }
    }

    #[test]
    fn rectangles_preserve_skipped_publications_reset_and_old_readers() {
        let font = super::super::tests::fixture_font();
        let mut layouts = TextLayoutCache::new(8, 1 << 20);
        let mut atlas = NametagAtlas::default();
        assert!(atlas.publish().0.is_empty());
        let pages = |page| font_page(&font, page);
        atlas
            .line(&Arc::from("Player 0"), &font, &mut layouts, &pages)
            .unwrap();
        let first = atlas.publish().0;
        atlas
            .line(&Arc::from("§cPlayer 1"), &font, &mut layouts, &pages)
            .unwrap();
        let skipped = atlas.publish().0;
        atlas
            .line(&Arc::from("Player 2"), &font, &mut layouts, &pages)
            .unwrap();
        let latest = atlas.publish().0;
        assert_eq!(NametagAtlasRect::updates(&latest, &first).count(), 2);
        assert_eq!(NametagAtlasRect::updates(&latest, &latest).count(), 0);
        let mut incremental = vec![0; (NAMETAG_ATLAS_SIDE * NAMETAG_ATLAS_SIDE * 4) as usize];
        apply_rectangles(&mut incremental, &first, &[]);
        apply_rectangles(&mut incremental, &latest, &first);
        let mut full = vec![0; incremental.len()];
        apply_rectangles(&mut full, &latest, &[]);
        assert_eq!(incremental, full);
        atlas.reset();
        atlas
            .line(&Arc::from("New"), &font, &mut layouts, &pages)
            .unwrap();
        let reset = atlas.publish().0;
        assert_eq!(NametagAtlasRect::updates(&reset, &latest).count(), 1);
        assert_eq!(first.len(), 1);
        assert_eq!(skipped.len(), 2);
        assert!(Arc::ptr_eq(&first[0].rgba8, &latest[0].rgba8));
        assert!(
            NametagAtlasRect::updates(&reset, &latest)
                .next()
                .unwrap()
                .rgba8
                .len()
                < incremental.len()
        );
    }
}
