//! Native font-pixel layout for world-space nameplate billboards.

use crate::{
    nametag::{MAX_NAMETAG_RECORDS, NAMETAG_ATLAS_SIDE, NametagRecord, NametagScene},
    nametag_atlas::{GlyphPage, NametagAtlas},
};
use assets::RuntimeFontCatalog;
use bevy::math::Vec3;
use std::sync::Arc;
use ui::{FONT_DESIGN_PIXEL_TEXELS, TextLayoutCache};

/// Vanilla's default nameplate render distance, used until an actor streams its own.
pub const DEFAULT_NAMEPLATE_DISTANCE: f32 = 64.0;
/// Default standing player box height.
pub const DEFAULT_NAMETAG_HEIGHT: f32 = 1.8;
const SNEAKING_HEIGHT: f32 = 1.5;
/// Clearance above the actor's bounding box.
pub const NAMETAG_HEAD_CLEARANCE: f32 = 0.7;
/// Each extra line raises the whole tag by this many blocks.
pub const EXTRA_NAMETAG_LINE_RAISE: f32 = 0.125;
/// Line pitch in font pixels.
pub const NAMETAG_LINE_PITCH_PX: f32 = 10.0;
/// Native name-tag plate colour.
pub const NAMETAG_PLATE_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 0.25];
/// A sneaking actor's depth-tested text alpha.
pub const SNEAK_NAMETAG_TEXT_ALPHA: f32 = 0.125;

#[must_use]
pub fn default_nametag_box_height(sneaking: bool, scale: f32) -> f32 {
    (if sneaking {
        SNEAKING_HEIGHT
    } else {
        DEFAULT_NAMETAG_HEIGHT
    }) * scale
}

/// Vanilla splits text on newlines and drops empty lines.
#[must_use]
pub fn nametag_lines(name: &str, score: Option<&str>) -> Vec<Arc<str>> {
    name.split('\n')
        .chain(score.into_iter().flat_map(|score| score.split('\n')))
        .filter(|line| !line.is_empty())
        .map(Arc::from)
        .collect()
}

/// One actor's tag: where its first line hangs and what it draws.
#[derive(Clone, Debug, PartialEq)]
pub struct NametagAnchor {
    /// World point of the tag's font-pixel origin, already raised for extra lines.
    pub position: Vec3,
    /// Non-empty lines, top first.
    pub lines: Vec<Arc<str>>,
    /// Drawn behind walls only where visible, as for a sneaking actor.
    pub depth_tested: bool,
    pub text_alpha: f32,
    pub distance: f32,
}

/// A public player name, positioned with the native default player nameplate policy.
#[must_use]
pub fn player_nametag_anchor(
    name: &str,
    feet: Vec3,
    eye: Vec3,
    sneaking: bool,
) -> Option<NametagAnchor> {
    let distance = eye.distance(feet);
    let lines = nametag_lines(name, None);
    if !feet.is_finite()
        || !distance.is_finite()
        || distance > DEFAULT_NAMEPLATE_DISTANCE
        || lines.is_empty()
    {
        return None;
    }
    Some(NametagAnchor {
        position: feet
            + Vec3::Y
                * (default_nametag_box_height(sneaking, 1.0)
                    + NAMETAG_HEAD_CLEARANCE
                    + EXTRA_NAMETAG_LINE_RAISE * (lines.len() - 1) as f32),
        lines,
        depth_tested: sneaking,
        text_alpha: if sneaking {
            SNEAK_NAMETAG_TEXT_ALPHA
        } else {
            1.0
        },
        distance,
    })
}

/// The frame's tag quads: see-through tags first, then depth-tested ones, each farthest first,
/// every tag its plates then its text.
pub fn build_nametag_scene<'p>(
    anchors: &[NametagAnchor],
    font: &RuntimeFontCatalog,
    layouts: &mut TextLayoutCache,
    atlas: &mut NametagAtlas,
    pages: &impl Fn(usize) -> Option<GlyphPage<'p>>,
) -> NametagScene {
    let mut ordered: Vec<&NametagAnchor> = anchors.iter().collect();
    ordered.sort_by(|a, b| {
        a.depth_tested
            .cmp(&b.depth_tested)
            .then(b.distance.total_cmp(&a.distance))
    });
    let lines: usize = ordered.iter().map(|anchor| anchor.lines.len()).sum();
    if !atlas.has_room_for(lines) {
        atlas.reset();
    }
    let side = NAMETAG_ATLAS_SIDE as f32;
    let texels = FONT_DESIGN_PIXEL_TEXELS as f32;
    let mut records = Vec::new();
    let mut see_through = 0;
    for anchor in ordered {
        let placed: Vec<_> = anchor
            .lines
            .iter()
            .filter_map(|line| atlas.line(line, font, layouts, pages))
            .collect();
        if placed.is_empty() || records.len() + placed.len() * 2 > MAX_NAMETAG_RECORDS {
            continue;
        }
        // Vanilla measures whole font pixels and pads the widest half-width by one pixel.
        let half = placed
            .iter()
            .map(|line| line.width_px.round() as i32 / 2)
            .max()
            .unwrap_or(0) as f32;
        for index in 0..placed.len() {
            let top = NAMETAG_LINE_PITCH_PX * index as f32;
            records.push(NametagRecord {
                anchor: anchor.position.to_array(),
                text: 0,
                rect: [
                    -(half + 1.0),
                    top - 1.0,
                    half + 1.0,
                    top + NAMETAG_LINE_PITCH_PX - 1.0,
                ],
                uv: [0.0, 0.0, -1.0, -1.0],
                color: NAMETAG_PLATE_COLOR,
            });
        }
        for (index, line) in placed.iter().enumerate() {
            let left = -((line.width_px.round() as i32 / 2) as f32);
            let top = NAMETAG_LINE_PITCH_PX * index as f32;
            let [x, y, width, height] = line.cell.map(|value| value as f32);
            let top = top + line.top_px;
            records.push(NametagRecord {
                anchor: anchor.position.to_array(),
                text: 1,
                rect: [left, top, left + width / texels, top + height / texels],
                uv: [x / side, y / side, (x + width) / side, (y + height) / side],
                color: [1.0, 1.0, 1.0, anchor.text_alpha],
            });
        }
        if !anchor.depth_tested {
            see_through = records.len();
        }
    }
    let (atlas, atlas_revision) = atlas.publish();
    NametagScene {
        records,
        see_through,
        atlas,
        atlas_revision,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_player_names_share_native_distance_sneak_and_multiline_policy() {
        let feet = Vec3::new(2.0, 64.0, 3.0);
        let standing = player_nametag_anchor("Player\n\nRank", feet, feet, false).unwrap();
        assert_eq!(
            standing.lines.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
            ["Player", "Rank"]
        );
        assert_eq!(
            standing.position,
            feet + Vec3::Y
                * (DEFAULT_NAMETAG_HEIGHT + NAMETAG_HEAD_CLEARANCE + EXTRA_NAMETAG_LINE_RAISE)
        );
        assert!(!standing.depth_tested);
        let sneaking = player_nametag_anchor("Player", feet, feet, true).unwrap();
        assert!(sneaking.depth_tested);
        assert_eq!(sneaking.text_alpha, SNEAK_NAMETAG_TEXT_ALPHA);
        assert!(sneaking.position.y < standing.position.y);
        assert!(
            player_nametag_anchor(
                "Player",
                feet,
                feet + Vec3::X * (DEFAULT_NAMEPLATE_DISTANCE + 1.0),
                false
            )
            .is_none()
        );
        assert!(player_nametag_anchor("", feet, feet, false).is_none());
    }
}
