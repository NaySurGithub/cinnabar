use std::collections::{BTreeMap, VecDeque};

use assets::NetworkIdMode;
use meshing::{BlockClassifier, ChunkMesh, PackedBiomeRecord, mesh_sub_chunk_in_neighbourhood};
use world::{BlockUpdate, ChunkStore, MeshNeighbourhood, SubChunkKey};

use crate::{TerrainAssets, canonical, model::Arena};

const SIDE: i32 = 16;
const MAX_SUB_CHUNKS: usize = 4096;
const MAX_MESH_BYTES: u64 = 64 * 1024 * 1024;

/// Uses the native mesher's entire cube/model/liquid streams and 26-neighbour snapshot.
pub(super) fn prepare(
    arena: &Arena,
    assets: &TerrainAssets,
) -> Result<VecDeque<(SubChunkKey, ChunkMesh)>, String> {
    let ids = canonical::palette_ids(&assets.canonical, &arena.palette)?;
    let mut batches = BTreeMap::<SubChunkKey, Vec<BlockUpdate>>::new();
    for &[x, y, z, palette] in &arena.blocks {
        let key = SubChunkKey::new(
            0,
            x.div_euclid(SIDE),
            y.div_euclid(SIDE),
            z.div_euclid(SIDE),
        );
        batches.entry(key).or_default().push(BlockUpdate::new(
            x.rem_euclid(SIDE) as u8,
            y.rem_euclid(SIDE) as u8,
            z.rem_euclid(SIDE) as u8,
            0,
            ids[palette as usize],
        ));
        if batches.len() > MAX_SUB_CHUNKS {
            return Err("arena exceeds the 4096-subchunk browser limit".into());
        }
    }
    let mut world = ChunkStore::new();
    let keys = batches.keys().copied().collect::<Vec<_>>();
    for (key, updates) in batches {
        world
            .update_sub_chunk_blocks(key, &updates, assets.air)
            .map_err(|error| format!("invalid arena block batch: {error}"))?;
    }
    let mut meshes = VecDeque::new();
    let mut total_bytes = 0_u64;
    for key in keys {
        let Some(center) = world.sub_chunk(key) else {
            continue;
        };
        let neighbours = MeshNeighbourhood::adjacent_offsets()
            .map(|offset| {
                let key = SubChunkKey::new(
                    0,
                    key.x + i32::from(offset[0]),
                    key.y + i32::from(offset[1]),
                    key.z + i32::from(offset[2]),
                );
                (offset, world.sub_chunk(key))
            })
            .collect::<Vec<_>>();
        let mut snapshot = MeshNeighbourhood::new(&center);
        for (offset, chunk) in &neighbours {
            if let Some(chunk) = chunk {
                let _ = snapshot.insert(*offset, chunk);
            }
        }
        let mesh = mesh_sub_chunk_in_neighbourhood(
            &BlockClassifier::new(assets.air),
            &assets.runtime,
            NetworkIdMode::Sequential,
            &snapshot,
        );
        total_bytes = total_bytes.saturating_add(meshing::mesh_output_byte_len(
            &mesh,
            &PackedBiomeRecord::fallback(),
        ));
        if total_bytes > MAX_MESH_BYTES {
            return Err("arena native mesh exceeds the 64 MiB browser budget".into());
        }
        if !mesh.is_empty() {
            meshes.push_back((key, mesh));
        }
    }
    Ok(meshes)
}
