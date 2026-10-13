//! Transactional, bounded terrain requests and ordinary input output.
use crate::{AutomationControls, TerrainQuery, TerrainRegion};

#[derive(Default)]
pub(crate) struct NavigationState {
    pub regions: Vec<TerrainRegion>,
    pub queries: Vec<TerrainQuery>,
    pub controls: Option<AutomationControls>,
    pub hotbar: Option<u8>,
    pub pending_queries: Vec<TerrainQuery>,
    pub pending_controls: Option<AutomationControls>,
    pub pending_hotbar: Option<u8>,
    pub calls: usize,
}
impl NavigationState {
    pub fn begin_frame(&mut self) {
        self.pending_queries.clear();
        self.pending_controls = None;
        self.pending_hotbar = None;
        self.controls = None;
        self.hotbar = None;
        self.calls = 0;
    }
    pub fn commit(&mut self) {
        self.queries = std::mem::take(&mut self.pending_queries);
        self.controls = self.pending_controls.take();
        self.hotbar = self.pending_hotbar.take();
    }
}

/// Validates exclusive bounds without overflow or allocating cells.
pub(crate) fn query_valid(query: &TerrainQuery, eye: [f32; 3]) -> bool {
    let min = [query.min.x, query.min.y, query.min.z];
    let max = [query.max.x, query.max.y, query.max.z];
    let mut count = 1_i64;
    for axis in 0..3 {
        let extent = i64::from(max[axis]) - i64::from(min[axis]);
        if extent <= 0 || extent > mod_api::MAX_TERRAIN_CELLS as i64 {
            return false;
        }
        let center = eye[axis].floor() as i64;
        if (i64::from(min[axis]) - center).abs() > mod_api::MAX_TERRAIN_DISTANCE
            || (i64::from(max[axis]) - 1 - center).abs() > mod_api::MAX_TERRAIN_DISTANCE
        {
            return false;
        }
        count *= extent;
        if count > mod_api::MAX_TERRAIN_CELLS as i64 {
            return false;
        }
    }
    true
}

impl crate::ModHost {
    /// Consumes successful requests to sample against current terrain before the next frame.
    pub fn take_terrain_queries(&mut self) -> Vec<TerrainQuery> {
        self.instance.take_terrain_queries()
    }
    /// Supplies current-session responses. Missing or stale responses remain unknown to the guest.
    pub fn supply_terrain(&mut self, regions: Vec<TerrainRegion>) {
        self.instance.supply_terrain(regions);
    }
    /// Consumes one frame's ordinary input contribution.
    pub fn take_automation(&mut self) -> Option<AutomationControls> {
        self.instance.take_automation()
    }
    /// Consumes one frame's ordinary equipment selection.
    pub fn take_hotbar(&mut self) -> Option<u8> {
        self.instance.take_hotbar()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TerrainPosition;
    fn query(min: i32, max: i32) -> TerrainQuery {
        TerrainQuery {
            min: TerrainPosition { x: min, y: 0, z: 0 },
            max: TerrainPosition { x: max, y: 1, z: 1 },
        }
    }
    #[test]
    fn bounds_reject_overflow_empty_volume_distance_and_oversized_regions() {
        assert!(query_valid(&query(-1, 1), [0.; 3]));
        assert!(!query_valid(&query(1, 1), [0.; 3]));
        assert!(!query_valid(&query(i32::MIN, i32::MAX), [0.; 3]));
        assert!(!query_valid(&query(129, 130), [0.; 3]));
        let mut value = query(0, 16);
        value.max.y = 16;
        value.max.z = 2;
        assert!(!query_valid(&value, [0.; 3]));
    }
    #[test]
    fn input_and_queries_only_publish_on_commit_and_do_not_retain_input() {
        let mut state = NavigationState::default();
        state.begin_frame();
        state.pending_hotbar = Some(3);
        state.pending_queries.push(query(0, 1));
        assert!(state.hotbar.is_none());
        assert!(state.queries.is_empty());
        state.commit();
        assert_eq!(state.hotbar, Some(3));
        assert_eq!(state.queries.len(), 1);
        state.begin_frame();
        assert!(state.hotbar.is_none());
        state.commit();
        assert!(state.queries.is_empty());
    }
}
