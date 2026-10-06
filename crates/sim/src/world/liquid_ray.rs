//! The nearest liquid surface along a ray, which the client's own block pick never reports.
//! Each liquid cell is a box up to its surface: the caller's height for its depth, or the full
//! block when the same liquid fills the block above.

use super::{
    PaletteWorld, WorldQueryError,
    flow::{LiquidCell, liquid_cell},
    raycast::{TraversalState, validate_ray},
};
use crate::Vec3;

/// A liquid surface a ray meets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LiquidHit {
    pub block_pos: [i32; 3],
    /// The runtime id that holds the liquid: the extra layer's or the primary's.
    pub runtime_id: u32,
    /// From the ray's origin, along the normalized ray.
    pub distance: f64,
    /// Depth zero: a source rather than flowing liquid.
    pub source: bool,
}

impl PaletteWorld<'_> {
    /// The nearest liquid surface within `max_distance` along the ray. `height(depth, covered)`
    /// is the surface's height in its block, `covered` when the same liquid is above.
    pub fn liquid_ray(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f64,
        height: impl Fn(u8, bool) -> f64,
    ) -> Result<Option<LiquidHit>, WorldQueryError> {
        let direction = validate_ray(origin, direction, max_distance)?;
        let mut state = TraversalState::new(origin, direction)?;
        let within = |hit: Option<LiquidHit>| hit.filter(|hit| hit.distance <= max_distance);
        // A box lies within its cell, so the first cell in ray order with a hit is nearest.
        loop {
            if let Some(hit) = within(self.liquid_box_hit(state.cell, origin, direction, &height)?)
            {
                return Ok(Some(hit));
            }
            let next = state.next_crossing();
            if next > max_distance {
                return Ok(None);
            }
            for tied in state.advance(next)? {
                if let Some(hit) = within(self.liquid_box_hit(tied, origin, direction, &height)?) {
                    return Ok(Some(hit));
                }
            }
        }
    }

    /// Whether `point` lies under a liquid surface.
    pub fn point_in_liquid(
        &self,
        point: Vec3,
        height: impl Fn(u8, bool) -> f64,
    ) -> Result<bool, WorldQueryError> {
        let block = [point.x, point.y, point.z].map(|axis| axis.floor() as i32);
        Ok(self
            .surface(block, &height)?
            .is_some_and(|(_, _, top)| point.y - f64::from(block[1]) < top))
    }

    /// The liquid in `block` and its surface height within the block.
    fn surface(
        &self,
        block: [i32; 3],
        height: &impl Fn(u8, bool) -> f64,
    ) -> Result<Option<(u32, LiquidCell, f64)>, WorldQueryError> {
        let Some((runtime_id, cell)) = liquid_cell(self, block)? else {
            return Ok(None);
        };
        let above = [block[0], block[1].saturating_add(1), block[2]];
        let covered =
            liquid_cell(self, above)?.is_some_and(|(_, other)| other.material == cell.material);
        Ok(Some((
            runtime_id,
            cell,
            height(cell.depth, covered).clamp(0.0, 1.0),
        )))
    }

    fn liquid_box_hit(
        &self,
        block: [i32; 3],
        origin: Vec3,
        direction: Vec3,
        height: &impl Fn(u8, bool) -> f64,
    ) -> Result<Option<LiquidHit>, WorldQueryError> {
        let Some((runtime_id, cell, top)) = self.surface(block, height)? else {
            return Ok(None);
        };
        let min = [block[0], block[1], block[2]].map(f64::from);
        let max = [min[0] + 1.0, min[1] + top, min[2] + 1.0];
        Ok(
            box_entry(origin, direction, min, max).map(|distance| LiquidHit {
                block_pos: block,
                runtime_id,
                distance,
                source: cell.depth_known && cell.depth == 0,
            }),
        )
    }
}

/// Distance along a unit `direction` at which the ray enters the box; zero from inside.
fn box_entry(origin: Vec3, direction: Vec3, min: [f64; 3], max: [f64; 3]) -> Option<f64> {
    let origin = [origin.x, origin.y, origin.z];
    let direction = [direction.x, direction.y, direction.z];
    let mut near = 0.0_f64;
    let mut far = f64::INFINITY;
    for axis in 0..3 {
        if direction[axis] == 0.0 {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return None;
            }
            continue;
        }
        let first = (min[axis] - origin[axis]) / direction[axis];
        let second = (max[axis] - origin[axis]) / direction[axis];
        near = near.max(first.min(second));
        far = far.min(first.max(second));
        if near > far {
            return None;
        }
    }
    Some(near)
}
