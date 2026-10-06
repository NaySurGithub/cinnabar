//! The crosshair's liquid ray: the nearest liquid surface along the pick ray, by vanilla's
//! liquid lookup (the extra layer unless air, then the primary layer).

use sim::{
    Aabb, BlockPhysicsFlags, CollisionIdSpace, CollisionRegistry, CollisionRegistryIdentity,
    FlowBlockFacts, PaletteWorld, SurfaceResponse, Vec3,
};
use world::{BlockUpdate, ChunkKey, ChunkStore, SubChunkKey};

const AIR: u32 = 0;
const STONE: u32 = 1;
const WATER: u32 = 2;
const FLOWING: u32 = 3;

/// Surface height as a test stand-in for the mesher's: (8 - depth) / 9, full when covered.
fn height(depth: u8, covered: bool) -> f64 {
    if covered {
        1.0
    } else {
        f64::from(8 - depth.min(7)) / 9.0
    }
}

fn registry() -> CollisionRegistry {
    let mut registry = CollisionRegistry::with_identity(CollisionRegistryIdentity {
        protocol: 2193,
        id_space: CollisionIdSpace::Sequential,
        preg_sha256: [0x5a; 32],
    });
    registry.register(AIR, []).unwrap();
    registry
        .register(STONE, [Aabb::new(Vec3::ZERO, Vec3::ONE)])
        .unwrap();
    for (runtime_id, depth) in [(WATER, 0), (FLOWING, 3)] {
        registry
            .register_physics(
                runtime_id,
                [],
                0.6,
                1.0,
                1.0,
                0.875,
                BlockPhysicsFlags::WATER,
                SurfaceResponse::None,
            )
            .unwrap();
        registry.set_flow_facts(
            runtime_id,
            FlowBlockFacts {
                blocks_motion: false,
                is_solid: false,
                blocked_faces: 0,
                allowed_faces: 0x3f,
                liquid_depth: Some(depth),
            },
        );
    }
    registry
}

fn store(blocks: &[([i32; 3], u32, u32)]) -> ChunkStore {
    let mut store = ChunkStore::new();
    for x in -2..=2 {
        for z in -2..=2 {
            for y in -3..=3 {
                let key = SubChunkKey::from_chunk(ChunkKey::new(0, x, z), y);
                store.apply_request_mode_air(key).unwrap();
                store.mark_sub_chunk_loaded(key).unwrap();
            }
        }
    }
    for &(block, layer, runtime_id) in blocks {
        let key = SubChunkKey::new(0, block[0] >> 4, block[1] >> 4, block[2] >> 4);
        let local = block.map(|axis| axis.rem_euclid(16) as u8);
        store
            .update_block(
                key,
                BlockUpdate::new(local[0], local[1], local[2], layer, runtime_id),
                0,
            )
            .unwrap();
    }
    store
}

fn ray(
    blocks: &[([i32; 3], u32, u32)],
    origin: Vec3,
    direction: Vec3,
    reach: f64,
) -> Option<sim::LiquidHit> {
    let store = store(blocks);
    let registry = registry();
    PaletteWorld::new(&store, &registry, 0)
        .liquid_ray(origin, direction, reach, height)
        .unwrap()
}

#[test]
fn a_source_surface_is_hit_from_above() {
    // Surface at 8/9: from y 2 down, the ray enters at 2 - 8/9.
    let hit = ray(
        &[([0, 0, 0], 0, WATER)],
        Vec3::new(0.5, 2.0, 0.5),
        Vec3::new(0.0, -1.0, 0.0),
        3.0,
    )
    .unwrap();
    assert_eq!(hit.block_pos, [0, 0, 0]);
    assert_eq!(hit.runtime_id, WATER);
    assert!((hit.distance - (2.0 - 8.0 / 9.0)).abs() < 1e-9);
    assert!(hit.source);
}

#[test]
fn flowing_water_is_lower_and_not_a_source() {
    // Depth 3: surface at 5/9.
    let hit = ray(
        &[([0, 0, 0], 0, FLOWING)],
        Vec3::new(0.5, 2.0, 0.5),
        Vec3::new(0.0, -1.0, 0.0),
        3.0,
    )
    .unwrap();
    assert!((hit.distance - (2.0 - 5.0 / 9.0)).abs() < 1e-9);
    assert!(!hit.source);
}

#[test]
fn waterlogged_water_is_found_in_the_extra_layer() {
    let hit = ray(
        &[([0, 0, 0], 0, STONE), ([0, 0, 0], 1, WATER)],
        Vec3::new(0.5, 2.0, 0.5),
        Vec3::new(0.0, -1.0, 0.0),
        3.0,
    )
    .unwrap();
    assert_eq!(hit.runtime_id, WATER);
}

#[test]
fn a_ray_above_an_uncovered_surface_misses_but_a_covered_block_is_full() {
    // Horizontally at y 0.95, above the 8/9 surface: no hit.
    let across = |blocks: &[([i32; 3], u32, u32)]| {
        ray(
            blocks,
            Vec3::new(-1.5, 0.95, 0.5),
            Vec3::new(1.0, 0.0, 0.0),
            4.0,
        )
    };
    assert_eq!(across(&[([0, 0, 0], 0, WATER)]), None);
    // The same liquid above fills the block: entered at x 0, 1.5 along.
    let hit = across(&[([0, 0, 0], 0, WATER), ([0, 1, 0], 0, WATER)]).unwrap();
    assert_eq!(hit.block_pos, [0, 0, 0]);
    assert!((hit.distance - 1.5).abs() < 1e-9);
}

#[test]
fn liquids_beyond_reach_or_behind_are_ignored() {
    let blocks = [([0, 0, 0], 0, WATER)];
    assert_eq!(
        ray(
            &blocks,
            Vec3::new(0.5, 5.0, 0.5),
            Vec3::new(0.0, -1.0, 0.0),
            3.0
        ),
        None
    );
    assert_eq!(
        ray(
            &blocks,
            Vec3::new(0.5, 2.0, 0.5),
            Vec3::new(0.0, 1.0, 0.0),
            3.0
        ),
        None
    );
}

#[test]
fn an_eye_under_the_surface_is_in_liquid() {
    let store = store(&[([0, 0, 0], 0, WATER)]);
    let registry = registry();
    let world = PaletteWorld::new(&store, &registry, 0);
    assert!(
        world
            .point_in_liquid(Vec3::new(0.5, 0.5, 0.5), height)
            .unwrap()
    );
    assert!(
        !world
            .point_in_liquid(Vec3::new(0.5, 0.95, 0.5), height)
            .unwrap()
    );
    assert!(
        !world
            .point_in_liquid(Vec3::new(0.5, 1.5, 0.5), height)
            .unwrap()
    );
}
