//! Transactional, bounded terrain requests and their retained samples.
use std::{collections::HashMap, sync::Arc};

use wasmtime::component::ComponentType;

use crate::{
    TerrainBlockState, TerrainQuery, TerrainReadError, TerrainRegion, TerrainShapeBinding,
};

/// What the client supplies for one query committed last frame.
#[derive(Debug, Clone, PartialEq)]
pub enum TerrainRefresh {
    /// The retained region is still current.
    Keep,
    /// A new sample; the host assigns its stamp revision.
    Replace(TerrainRegion),
    /// No sample this frame, for example because the frame's sampling budget is spent.
    Pending,
    Failed(TerrainReadError),
}

/// The latest outcome for one query.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Sample {
    Pending,
    Region(TerrainRegion),
    Failed(TerrainReadError),
}

#[derive(Default)]
pub(crate) struct TerrainState {
    pub samples: Vec<(TerrainQuery, Sample)>,
    pub queries: Vec<TerrainQuery>,
    pub pending_queries: Vec<TerrainQuery>,
    pub calls: usize,
    revision: u64,
}
impl TerrainState {
    pub fn begin_frame(&mut self) {
        self.pending_queries.clear();
        self.calls = 0;
    }
    pub fn commit(&mut self) {
        self.queries = std::mem::take(&mut self.pending_queries);
    }
    /// Keeps samples only for queries read again, refreshing each through `refresh`.
    pub fn refresh(
        &mut self,
        mut refresh: impl FnMut(&TerrainQuery, Option<&TerrainRegion>) -> TerrainRefresh,
    ) {
        let mut previous = std::mem::take(&mut self.samples);
        for query in std::mem::take(&mut self.queries) {
            let retained = previous
                .iter()
                .position(|(candidate, _)| *candidate == query)
                .map(|index| previous.swap_remove(index).1);
            let region = match &retained {
                Some(Sample::Region(region)) => Some(region),
                _ => None,
            };
            let sample = match refresh(&query, region) {
                TerrainRefresh::Keep => retained.unwrap_or(Sample::Pending),
                TerrainRefresh::Replace(region)
                    if terrain_region_bytes(&region) > mod_api::MAX_TERRAIN_RESPONSE_BYTES =>
                {
                    Sample::Failed(TerrainReadError::LimitExceeded)
                }
                TerrainRefresh::Replace(mut region) => {
                    self.revision += 1;
                    region.stamp.revision = self.revision;
                    Sample::Region(region)
                }
                TerrainRefresh::Pending => Sample::Pending,
                TerrainRefresh::Failed(error) => Sample::Failed(error),
            };
            self.samples.push((query, sample));
        }
    }
}

/// Bytes `region` occupies in guest memory once lowered, excluding alignment padding.
pub fn terrain_region_bytes(region: &TerrainRegion) -> usize {
    fn list<T: ComponentType>(items: &[T]) -> usize {
        items.len() * T::SIZE32
    }
    let strings = region
        .palette
        .iter()
        .map(|state| {
            state.identifier.as_ref().map_or(0, String::len)
                + state.canonical_state.as_ref().map_or(0, String::len)
                + list(&state.tags)
                + state.tags.iter().map(String::len).sum::<usize>()
        })
        .sum::<usize>();
    TerrainRegion::SIZE32
        + list(&region.chunks)
        + list(&region.loaded)
        + list(&region.states)
        + list(&region.palette)
        + list(&region.shape_offsets)
        + list(&region.shape_boxes)
        + list(&region.overrides)
        + strings
}

/// Returns the query's extent per axis when it is well formed and near `position`.
pub fn terrain_query_size(
    query: &TerrainQuery,
    position: [f32; 3],
) -> Result<[u8; 3], TerrainReadError> {
    let min = [query.min.x, query.min.y, query.min.z];
    let max = [query.max.x, query.max.y, query.max.z];
    let mut size = [0; 3];
    for axis in 0..3 {
        let extent = i64::from(max[axis]) - i64::from(min[axis]);
        if !(1..=i64::from(mod_api::MAX_TERRAIN_EXTENT)).contains(&extent) {
            return Err(TerrainReadError::InvalidQuery);
        }
        size[axis] = extent as u8;
    }
    for axis in 0..3 {
        if !position[axis].is_finite() {
            return Err(TerrainReadError::OutOfRange);
        }
        let center = position[axis].floor() as i64;
        if (i64::from(min[axis]) - center).abs() > mod_api::MAX_TERRAIN_DISTANCE
            || (i64::from(max[axis]) - 1 - center).abs() > mod_api::MAX_TERRAIN_DISTANCE
        {
            return Err(TerrainReadError::OutOfRange);
        }
    }
    Ok(size)
}

/// Registry metadata of one runtime ID, as the client's block registry reports it.
pub trait TerrainMetadata {
    fn identifier(&self, runtime_id: u32) -> Option<&str>;
    fn canonical_state(&self, runtime_id: u32) -> Option<&str>;
    fn tags(&self, runtime_id: u32) -> &[Arc<str>];
}

/// Block metadata copied once per registry generation and runtime ID; bounded in size.
#[derive(Default)]
pub struct TerrainDescriptors {
    generation: u64,
    entries: HashMap<u32, (TerrainBlockState, usize)>,
}

impl TerrainDescriptors {
    /// Entries retained before the cache starts over.
    const CAPACITY: usize = 8192;

    /// Returns `runtime_id`'s metadata without physics or geometry, and its string bytes.
    pub fn describe(
        &mut self,
        generation: u64,
        runtime_id: u32,
        metadata: &impl TerrainMetadata,
    ) -> (&TerrainBlockState, usize) {
        if generation != self.generation || self.entries.len() == Self::CAPACITY {
            self.generation = generation;
            self.entries.clear();
        }
        let entry = self.entries.entry(runtime_id).or_insert_with(|| {
            let identifier = metadata.identifier(runtime_id).map(str::to_owned);
            let canonical_state = metadata.canonical_state(runtime_id).map(str::to_owned);
            let tags: Vec<String> = metadata
                .tags(runtime_id)
                .iter()
                .map(|tag| tag.to_string())
                .collect();
            let bytes = identifier.as_ref().map_or(0, String::len)
                + canonical_state.as_ref().map_or(0, String::len)
                + tags.iter().map(String::len).sum::<usize>();
            let state = TerrainBlockState {
                runtime_id,
                identifier,
                canonical_state,
                tags,
                physics: None,
                collision: TerrainShapeBinding::NotRequested,
                selection: TerrainShapeBinding::NotRequested,
            };
            (state, bytes)
        });
        (&entry.0, entry.1)
    }
}

impl crate::ModHost {
    /// Refreshes last frame's committed queries; samples of queries not read again are dropped.
    pub fn refresh_terrain(
        &mut self,
        refresh: impl FnMut(&TerrainQuery, Option<&TerrainRegion>) -> TerrainRefresh,
    ) {
        self.instance.refresh_terrain(refresh);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TerrainGeometry, TerrainPosition, TerrainStamp};
    fn query(min: i32, max: i32) -> TerrainQuery {
        TerrainQuery {
            min: TerrainPosition { x: min, y: 0, z: 0 },
            max: TerrainPosition { x: max, y: 1, z: 1 },
            geometry: TerrainGeometry::empty(),
        }
    }
    fn region() -> TerrainRegion {
        TerrainRegion {
            stamp: TerrainStamp {
                session: 1,
                dimension: 0,
                registry: 0,
                revision: 0,
            },
            chunks: vec![],
            layer_count: 1,
            loaded: vec![],
            states: vec![],
            palette: vec![],
            shape_offsets: vec![],
            shape_boxes: vec![],
            overrides: vec![],
        }
    }
    #[test]
    fn bounds_reject_overflow_empty_volume_distance_and_oversized_regions() {
        assert_eq!(terrain_query_size(&query(-1, 1), [0.; 3]), Ok([2, 1, 1]));
        assert_eq!(
            terrain_query_size(&query(1, 1), [0.; 3]),
            Err(TerrainReadError::InvalidQuery)
        );
        assert_eq!(
            terrain_query_size(&query(i32::MIN, i32::MAX), [0.; 3]),
            Err(TerrainReadError::InvalidQuery)
        );
        assert_eq!(
            terrain_query_size(&query(0, 17), [0.; 3]),
            Err(TerrainReadError::InvalidQuery)
        );
        assert_eq!(
            terrain_query_size(&query(129, 130), [0.; 3]),
            Err(TerrainReadError::OutOfRange)
        );
        assert_eq!(
            terrain_query_size(&query(0, 1), [f32::NAN; 3]),
            Err(TerrainReadError::OutOfRange)
        );
    }
    #[test]
    fn queries_publish_on_commit_and_samples_follow_reads() {
        let mut state = TerrainState::default();
        state.begin_frame();
        state.pending_queries.push(query(0, 1));
        assert!(state.queries.is_empty());
        state.commit();
        assert_eq!(state.queries.len(), 1);
        state.refresh(|_, retained| {
            assert!(retained.is_none());
            TerrainRefresh::Replace(region())
        });
        let Sample::Region(first) = &state.samples[0].1 else {
            panic!("replaced");
        };
        assert_eq!(first.stamp.revision, 1);
        state.queries = vec![query(0, 1)];
        state.refresh(|_, retained| {
            assert_eq!(retained.map(|region| region.stamp.revision), Some(1));
            TerrainRefresh::Keep
        });
        assert!(
            matches!(&state.samples[0].1, Sample::Region(region) if region.stamp.revision == 1)
        );
        state.refresh(|_, _| unreachable!("no query was read again"));
        assert!(state.samples.is_empty());
    }
}
