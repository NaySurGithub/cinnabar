//! Resolved per-layer facts shared by terrain consumers and the native simulator.
use super::{BlockPhysicsFacts, PaletteWorld, WorldQueryError};
use crate::{Aabb, Vec3};

/// Shapes use block-local coordinates, with native random and actor shape offsets applied.
#[derive(Debug, Clone, PartialEq)]
pub struct BlockLayerObservation {
    pub runtime_id: u32,
    pub collision: Vec<Aabb>,
    pub selection: Vec<Aabb>,
    pub physics: BlockPhysicsFacts,
}
impl PaletteWorld<'_> {
    /// Reads every palette layer; missing terrain or metadata remains an explicit error.
    pub fn observe_block_layers(
        &self,
        position: [i32; 3],
    ) -> Result<Vec<BlockLayerObservation>, WorldQueryError> {
        let origin = Vec3::new(position[0] as f64, position[1] as f64, position[2] as f64);
        let query = Aabb::new(origin, origin + Vec3::ONE);
        self.runtime_ids_at(position)?
            .into_iter()
            .map(|runtime_id| {
                let physics =
                    self.registry
                        .physics(runtime_id)
                        .ok_or(WorldQueryError::UnknownRuntimeId {
                            runtime_id,
                            block: position,
                        })?;
                let collision = self.block_collision_shapes(position, physics, query)?;
                let selection = if !self.may_pick(physics) {
                    Vec::new()
                } else if physics.pick_shapes.is_some() {
                    self.selection_shapes_at(position, physics)?.into_owned()
                } else {
                    collision.clone().into_owned()
                };
                let offset = physics.shape_offset(position) - origin;
                Ok(BlockLayerObservation {
                    runtime_id,
                    collision: collision
                        .iter()
                        .map(|shape| shape.translated(offset))
                        .collect(),
                    selection: selection
                        .iter()
                        .map(|shape| shape.translated(offset))
                        .collect(),
                    physics: self.registry.movement_facts(runtime_id).ok_or(
                        WorldQueryError::UnknownRuntimeId {
                            runtime_id,
                            block: position,
                        },
                    )?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CollisionRegistry;
    use world::{ChunkKey, ChunkStore};
    #[test]
    fn unloaded_cells_never_become_air() {
        let store = ChunkStore::new();
        let mut registry = CollisionRegistry::new();
        registry.register(0, []).unwrap();
        let world = PaletteWorld::new(&store, &registry, 0, Default::default());
        assert!(matches!(
            world.observe_block_layers([0, 0, 0]),
            Err(WorldQueryError::UnloadedChunk(_))
        ));
    }
    #[test]
    fn loaded_empty_cells_have_known_empty_shapes() {
        let mut store = ChunkStore::new();
        store.mark_chunk_loaded(ChunkKey::new(0, 0, 0)).unwrap();
        let mut registry = CollisionRegistry::new();
        registry.register(0, []).unwrap();
        let world = PaletteWorld::new(&store, &registry, 0, Default::default());
        let layers = world.observe_block_layers([0, 0, 0]).unwrap();
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].runtime_id, 0);
        assert!(layers[0].collision.is_empty());
        assert!(layers[0].selection.is_empty());
    }
}
