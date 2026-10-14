//! Current-session terrain sampling for terrain-authorized mods.
use super::gameplay::GameplayContext;
use mod_host::{
    TerrainBounds, TerrainCell, TerrainChunkRevision, TerrainLayer, TerrainPosition,
    TerrainProperties, TerrainQuery, TerrainRegion,
};
use sim::{BlockPhysicsFlags as Flags, PaletteWorld};

/// Services only last frame's committed requests against this frame's current world.
pub(super) fn supply_terrain(
    host: &mut mod_host::ModHost,
    context: &GameplayContext<'_>,
    observation: Option<&mod_host::WorldObservation>,
) {
    let queries = host.take_terrain_queries();
    host.supply_terrain(Vec::new());
    if !host.is_active() || !host.grants().terrain {
        return;
    }
    let (Some(observation), Some(world), Some(registries)) = (
        observation,
        context.world.as_ref(),
        context.collisions.as_ref(),
    ) else {
        return;
    };
    let Some(stream) = world.stream.as_ref() else {
        return;
    };
    let authority = stream.authority();
    let store = authority.terrain();
    let mode = authority.network_id_mode();
    let registry = registries.registry(mode);
    let palette = PaletteWorld::new(store, registry, observation.dimension, Default::default());
    let regions = queries
        .into_iter()
        .filter(|query| in_range(query, observation.position))
        .map(|query| {
            let mut cells = Vec::new();
            for x in query.min.x..query.max.x {
                for y in query.min.y..query.max.y {
                    for z in query.min.z..query.max.z {
                        let position = TerrainPosition { x, y, z };
                        let key =
                            world::SubChunkKey::new(observation.dimension, x >> 4, y >> 4, z >> 4);
                        let loaded = store.is_sub_chunk_loaded(key);
                        let layers = if loaded {
                            let sub = store.sub_chunk(key);
                            let ids = sub
                                .as_ref()
                                .map(|sub| {
                                    (0..sub.storages().len())
                                        .map(|layer| {
                                            sub.runtime_id(
                                                layer,
                                                x.rem_euclid(16) as u8,
                                                y.rem_euclid(16) as u8,
                                                z.rem_euclid(16) as u8,
                                            )
                                            .expect("validated terrain storage")
                                        })
                                        .collect::<Vec<_>>()
                                })
                                .filter(|ids| !ids.is_empty())
                                .unwrap_or_else(|| vec![authority.air_block_id()]);
                            let resolved = palette.observe_block_layers([x, y, z]).ok();
                            ids.into_iter()
                                .enumerate()
                                .map(|(index, runtime_id)| {
                                    let properties = resolved
                                        .as_ref()
                                        .and_then(|layers| layers.get(index))
                                        .map(|layer| {
                                            let facts = layer.physics;
                                            let flags = facts.flags;
                                            TerrainProperties {
                                                collision: layer
                                                    .collision
                                                    .iter()
                                                    .map(bounds)
                                                    .collect(),
                                                selection: layer
                                                    .selection
                                                    .iter()
                                                    .map(bounds)
                                                    .collect(),
                                                friction: facts.friction,
                                                horizontal_speed_factor: facts
                                                    .horizontal_speed_factor,
                                                vertical_speed_factor: facts.vertical_speed_factor,
                                                fluid_height: facts.fluid_height_blocks,
                                                climbable: flags.contains(Flags::CLIMBABLE),
                                                water: flags.contains(Flags::WATER),
                                                lava: flags.contains(Flags::LAVA),
                                                cobweb: flags.contains(Flags::COBWEB),
                                                powder_snow: flags.contains(Flags::POWDER_SNOW),
                                                scaffolding: flags.contains(Flags::SCAFFOLDING),
                                                passable: flags.contains(Flags::PASSABLE),
                                                surface_response: format!(
                                                    "{:?}",
                                                    facts.surface_response
                                                ),
                                            }
                                        });
                                    TerrainLayer {
                                        runtime_id,
                                        tags: registries
                                            .block_tags(mode, runtime_id)
                                            .iter()
                                            .map(|tag| tag.to_string())
                                            .collect(),
                                        identifier: registries
                                            .block_identifier(mode, runtime_id)
                                            .map(str::to_owned),
                                        canonical_state: registries
                                            .block_canonical_state(mode, runtime_id)
                                            .map(str::to_owned),
                                        properties,
                                    }
                                })
                                .collect()
                        } else {
                            Vec::new()
                        };
                        cells.push(TerrainCell {
                            position,
                            loaded,
                            layers,
                        });
                    }
                }
            }
            // Include neighboring columns whose states can affect door/selection geometry.
            let mut chunks = Vec::new();
            for x in (query.min.x.saturating_sub(1) >> 4)..=(query.max.x >> 4) {
                for z in (query.min.z.saturating_sub(1) >> 4)..=(query.max.z >> 4) {
                    let key = world::ChunkKey::new(observation.dimension, x, z);
                    chunks.push(TerrainChunkRevision {
                        x,
                        z,
                        revision: store.collision_revision(key).map(|value| value.revision),
                    });
                }
            }
            TerrainRegion {
                session: observation.session,
                dimension: observation.dimension,
                query,
                chunks,
                cells,
            }
        })
        .collect();
    host.supply_terrain(regions);
}
fn bounds(value: &sim::Aabb) -> TerrainBounds {
    TerrainBounds {
        min: (value.min.x, value.min.y, value.min.z),
        max: (value.max.x, value.max.y, value.max.z),
    }
}
fn in_range(query: &TerrainQuery, position: (f32, f32, f32)) -> bool {
    let min = [query.min.x, query.min.y, query.min.z];
    let max = [query.max.x, query.max.y, query.max.z];
    let center = [position.0, position.1, position.2];
    (0..3).all(|axis| {
        (i64::from(min[axis]) - center[axis].floor() as i64).abs() <= mod_api::MAX_TERRAIN_DISTANCE
            && (i64::from(max[axis]) - 1 - center[axis].floor() as i64).abs()
                <= mod_api::MAX_TERRAIN_DISTANCE
    })
}
