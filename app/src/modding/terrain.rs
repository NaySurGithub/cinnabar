//! Samples terrain for terrain-authorized mods before their frame callbacks.
use std::sync::Arc;

use super::gameplay::GameplayContext;
use assets::NetworkIdMode;
use bevy::prelude::DetectChanges;
use client_extensions::modding::multi::ModHosts;
use client_interaction::movement::PhysicsCollisionRegistries;
use mod_host::{
    TerrainBlockState, TerrainChunkRevision, TerrainGeometry, TerrainPhysics, TerrainPhysicsFlags,
    TerrainQuery, TerrainReadError, TerrainRefresh, TerrainRegion, TerrainShape,
    TerrainShapeBinding, TerrainShapeOverride, TerrainStamp, TerrainSurfaceResponse,
};
use sim::{BlockPhysicsFlags, PaletteWorld, SurfaceResponse};
use world::{ChunkKey, ChunkStore};

/// Each simulation physics flag beside its contract flag.
const PHYSICS_FLAGS: [(BlockPhysicsFlags, TerrainPhysicsFlags); 7] = [
    (BlockPhysicsFlags::CLIMBABLE, TerrainPhysicsFlags::CLIMBABLE),
    (BlockPhysicsFlags::WATER, TerrainPhysicsFlags::WATER),
    (BlockPhysicsFlags::LAVA, TerrainPhysicsFlags::LAVA),
    (BlockPhysicsFlags::COBWEB, TerrainPhysicsFlags::COBWEB),
    (
        BlockPhysicsFlags::POWDER_SNOW,
        TerrainPhysicsFlags::POWDER_SNOW,
    ),
    (
        BlockPhysicsFlags::SCAFFOLDING,
        TerrainPhysicsFlags::SCAFFOLDING,
    ),
    (BlockPhysicsFlags::PASSABLE, TerrainPhysicsFlags::PASSABLE),
];

/// Sampler scratch, cached block metadata and the registry generation it describes.
#[derive(Default)]
pub(super) struct TerrainSampling {
    sampler: sim::TerrainSampler,
    descriptors: mod_host::TerrainDescriptors,
    scope: Option<(u64, NetworkIdMode)>,
    generation: u64,
    rotation: usize,
}

/// The block registry's metadata in the session's runtime-ID space.
struct Metadata<'a>(&'a PhysicsCollisionRegistries, NetworkIdMode);
impl mod_host::TerrainMetadata for Metadata<'_> {
    fn identifier(&self, runtime_id: u32) -> Option<&str> {
        self.0.block_identifier(self.1, runtime_id)
    }
    fn canonical_state(&self, runtime_id: u32) -> Option<&str> {
        self.0.block_canonical_state(self.1, runtime_id)
    }
    fn tags(&self, runtime_id: u32) -> &[Arc<str>] {
        self.0.block_tags(self.1, runtime_id)
    }
}

/// The current world a frame's samples are taken from.
struct Source<'a> {
    store: &'a ChunkStore,
    registries: &'a PhysicsCollisionRegistries,
    mode: NetworkIdMode,
    observation: &'a mod_host::WorldObservation,
    generation: u64,
}

impl TerrainSampling {
    /// Refreshes last frame's committed queries of every host, sharing one sampling budget
    /// and rotating which host is served first.
    pub(super) fn refresh(
        &mut self,
        hosts: &mut ModHosts,
        context: &GameplayContext<'_>,
        observation: Option<&mod_host::WorldObservation>,
    ) {
        let world = context.world.as_ref();
        let authority = world
            .and_then(|world| world.stream.as_ref())
            .map(|stream| stream.authority());
        let source = match (observation, authority, context.collisions.as_ref()) {
            (Some(observation), Some(authority), Some(registries)) => {
                let mode = authority.network_id_mode();
                let scope = Some((observation.session, mode));
                if self.scope != scope || registries.is_changed() {
                    self.scope = scope;
                    self.generation += 1;
                }
                Some(Source {
                    store: authority.terrain(),
                    registries,
                    mode,
                    observation,
                    generation: self.generation,
                })
            }
            _ => None,
        };
        let count = hosts.host_count();
        let mut budget = mod_api::MAX_TERRAIN_FRAME_CELLS;
        self.rotation = self.rotation.wrapping_add(1);
        for offset in 0..count {
            let host = hosts.host_mut((self.rotation + offset) % count);
            if !host.is_active() || !host.grants().terrain {
                continue;
            }
            host.refresh_terrain(|query, retained| match &source {
                Some(source) => self.sample(source, query, retained, &mut budget),
                None => TerrainRefresh::Pending,
            });
        }
    }

    /// Keeps a current region, or samples within the frame budget.
    fn sample(
        &mut self,
        source: &Source<'_>,
        query: &TerrainQuery,
        retained: Option<&TerrainRegion>,
        budget: &mut usize,
    ) -> TerrainRefresh {
        let observation = source.observation;
        let position = [
            observation.position.0,
            observation.position.1,
            observation.position.2,
        ];
        let size = match mod_host::terrain_query_size(query, position) {
            Ok(size) => size,
            Err(error) => return TerrainRefresh::Failed(error),
        };
        let stamp = TerrainStamp {
            session: observation.session,
            dimension: observation.dimension,
            registry: source.generation,
            revision: 0,
        };
        if let Some(region) = retained
            && (
                region.stamp.session,
                region.stamp.dimension,
                region.stamp.registry,
            ) == (stamp.session, stamp.dimension, stamp.registry)
            && region.chunks.iter().all(|chunk| {
                let key = ChunkKey::new(stamp.dimension, chunk.x, chunk.z);
                source
                    .store
                    .collision_revision(key)
                    .map(|value| value.revision)
                    == chunk.revision
            })
        {
            return TerrainRefresh::Keep;
        }
        let cells = size
            .iter()
            .map(|&side| usize::from(side))
            .product::<usize>();
        let Some(remaining) = budget.checked_sub(cells) else {
            return TerrainRefresh::Pending;
        };
        *budget = remaining;
        let geometry = sim::TerrainGeometry {
            collision: query.geometry.contains(TerrainGeometry::COLLISION),
            selection: query.geometry.contains(TerrainGeometry::SELECTION),
        };
        let registry = source.registries.registry(source.mode);
        let world = PaletteWorld::new(source.store, registry, stamp.dimension, Default::default());
        let min = [query.min.x, query.min.y, query.min.z];
        match self
            .sampler
            .sample(&world, min, size, geometry, mod_api::MAX_TERRAIN_BOXES)
        {
            Ok(sample) => self.region(source, stamp, sample),
            Err(_) => TerrainRefresh::Failed(TerrainReadError::LimitExceeded),
        }
    }

    /// Moves a sample's bulk arrays into a response and copies metadata once per state.
    fn region(
        &mut self,
        source: &Source<'_>,
        stamp: TerrainStamp,
        sample: sim::TerrainSample,
    ) -> TerrainRefresh {
        let metadata = Metadata(source.registries, source.mode);
        let mut bytes = 0;
        let mut palette = Vec::with_capacity(sample.palette.len());
        for state in &sample.palette {
            let (description, size) =
                self.descriptors
                    .describe(source.generation, state.runtime_id, &metadata);
            bytes += size;
            if bytes > mod_api::MAX_TERRAIN_METADATA_BYTES {
                return TerrainRefresh::Failed(TerrainReadError::LimitExceeded);
            }
            palette.push(TerrainBlockState {
                physics: state.physics.map(physics),
                collision: binding(state.collision),
                selection: binding(state.selection),
                ..description.clone()
            });
        }
        TerrainRefresh::Replace(Box::new(TerrainRegion {
            stamp,
            chunks: sample
                .columns
                .iter()
                .map(|column| TerrainChunkRevision {
                    x: column.x,
                    z: column.z,
                    revision: column.revision,
                })
                .collect(),
            layer_count: sample.layer_count,
            loaded: sample.loaded,
            states: sample.states,
            palette,
            shape_offsets: sample.shape_offsets,
            shape_boxes: sample.shape_boxes,
            overrides: sample
                .overrides
                .iter()
                .map(|entry| TerrainShapeOverride {
                    slot: entry.slot,
                    collision: entry.collision.map(shape),
                    selection: entry.selection.map(shape),
                })
                .collect(),
        }))
    }
}

fn physics(facts: sim::BlockPhysicsFacts) -> TerrainPhysics {
    TerrainPhysics {
        friction: facts.friction,
        horizontal_speed_factor: facts.horizontal_speed_factor,
        vertical_speed_factor: facts.vertical_speed_factor,
        fluid_height: facts.fluid_height_blocks,
        flags: PHYSICS_FLAGS
            .iter()
            .filter(|(flag, _)| facts.flags.contains(*flag))
            .fold(TerrainPhysicsFlags::empty(), |flags, (_, flag)| {
                flags | *flag
            }),
        surface: match facts.surface_response {
            SurfaceResponse::None => TerrainSurfaceResponse::None,
            SurfaceResponse::Slime => TerrainSurfaceResponse::Slime,
            SurfaceResponse::Bed => TerrainSurfaceResponse::Bed,
            SurfaceResponse::Honey => TerrainSurfaceResponse::Honey,
            SurfaceResponse::SoulSand => TerrainSurfaceResponse::SoulSand,
            SurfaceResponse::BubbleUp => TerrainSurfaceResponse::BubbleUp,
            SurfaceResponse::BubbleDown => TerrainSurfaceResponse::BubbleDown,
        },
    }
}

fn shape(value: sim::TerrainShape) -> TerrainShape {
    match value {
        sim::TerrainShape::Unavailable => TerrainShape::Unavailable,
        sim::TerrainShape::Known(index) => TerrainShape::Known(index),
    }
}

fn binding(value: sim::TerrainShapeBinding) -> TerrainShapeBinding {
    match value {
        sim::TerrainShapeBinding::NotRequested => TerrainShapeBinding::NotRequested,
        sim::TerrainShapeBinding::Fixed(value) => TerrainShapeBinding::Fixed(shape(value)),
        sim::TerrainShapeBinding::PerCell => TerrainShapeBinding::PerCell,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every simulation flag maps to its own contract flag, so none is dropped.
    #[test]
    fn physics_flags_cover_every_simulation_flag() {
        let mut covered = 0;
        let mut seen = TerrainPhysicsFlags::empty();
        for (flag, contract) in PHYSICS_FLAGS {
            assert_eq!(flag.bits().count_ones(), 1);
            assert!(!seen.intersects(contract));
            covered |= flag.bits();
            seen |= contract;
        }
        assert_eq!(covered, BlockPhysicsFlags::KNOWN_BITS);
        assert_eq!(seen, TerrainPhysicsFlags::all());
    }
}
