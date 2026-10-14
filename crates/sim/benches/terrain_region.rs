//! Host-side terrain region construction over mixed terrain.

#[path = "terrain_region/fixture.rs"]
mod fixture;

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use sim::{BlockPhysicsFlags as Flags, PaletteWorld};
use world::{ChunkKey, ChunkStore, SubChunkKey};

#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

/// Inclusive minimum and exclusive maximum crossing four columns, one absent.
const QUERY_256: ([i32; 3], [i32; 3]) = ([12, 62, 12], [20, 66, 20]);

type Bounds = ((f64, f64, f64), (f64, f64, f64));
/// Owned per-layer record with the same allocations as the nested WIT response.
#[allow(dead_code)]
struct Layer {
    runtime_id: u32,
    identifier: Option<String>,
    canonical_state: Option<String>,
    tags: Vec<String>,
    properties: Option<(Vec<Bounds>, Vec<Bounds>, [f64; 4], [bool; 7], String)>,
}

fn bounds(value: &sim::Aabb) -> Bounds {
    (
        (value.min.x, value.min.y, value.min.z),
        (value.max.x, value.max.y, value.max.z),
    )
}

/// The nested per-cell build: every layer re-resolves its cell and copies its metadata.
fn nested(
    store: &ChunkStore,
    registry: &sim::CollisionRegistry,
    (min, max): ([i32; 3], [i32; 3]),
) -> usize {
    let palette = PaletteWorld::new(store, registry, 0, Default::default());
    let mut cells = Vec::new();
    for x in min[0]..max[0] {
        for y in min[1]..max[1] {
            for z in min[2]..max[2] {
                let key = SubChunkKey::new(0, x >> 4, y >> 4, z >> 4);
                let loaded = store.is_sub_chunk_loaded(key);
                let layers: Vec<Layer> = if loaded {
                    let ids = store
                        .sub_chunk(key)
                        .map(|sub| {
                            (0..sub.storages().len())
                                .map(|layer| {
                                    sub.runtime_id(
                                        layer,
                                        x.rem_euclid(16) as u8,
                                        y.rem_euclid(16) as u8,
                                        z.rem_euclid(16) as u8,
                                    )
                                    .unwrap()
                                })
                                .collect::<Vec<_>>()
                        })
                        .filter(|ids| !ids.is_empty())
                        .unwrap_or_else(|| vec![fixture::AIR]);
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
                                    (
                                        layer.collision.iter().map(bounds).collect(),
                                        layer.selection.iter().map(bounds).collect(),
                                        [
                                            facts.friction,
                                            facts.horizontal_speed_factor,
                                            facts.vertical_speed_factor,
                                            facts.fluid_height_blocks,
                                        ],
                                        [
                                            Flags::CLIMBABLE,
                                            Flags::WATER,
                                            Flags::LAVA,
                                            Flags::COBWEB,
                                            Flags::POWDER_SNOW,
                                            Flags::SCAFFOLDING,
                                            Flags::PASSABLE,
                                        ]
                                        .map(|flag| flags.contains(flag)),
                                        format!("{:?}", facts.surface_response),
                                    )
                                });
                            let (identifier, state, tags) = fixture::metadata(runtime_id);
                            Layer {
                                runtime_id,
                                tags: tags.iter().map(|tag| tag.to_string()).collect(),
                                identifier: Some(identifier.to_owned()),
                                canonical_state: Some(state.to_owned()),
                                properties,
                            }
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                cells.push(([x, y, z], loaded, layers));
            }
        }
    }
    let mut chunks = Vec::new();
    for x in (min[0].saturating_sub(1) >> 4)..=(max[0] >> 4) {
        for z in (min[2].saturating_sub(1) >> 4)..=(max[2] >> 4) {
            chunks.push((
                x,
                z,
                store
                    .collision_revision(ChunkKey::new(0, x, z))
                    .map(|value| value.revision),
            ));
        }
    }
    black_box(&chunks);
    black_box(&cells).len()
}

fn region_benches(criterion: &mut Criterion) {
    let store = fixture::store();
    let registry = fixture::registry();
    let (cells, allocations) = test_alloc::measure(|| nested(&store, &registry, QUERY_256));
    assert_eq!(cells, 256);
    eprintln!(
        "terrain_region/nested_256: {} allocations, {} bytes per build",
        allocations.calls, allocations.bytes
    );
    let mut group = criterion.benchmark_group("terrain_region");
    group.throughput(Throughput::Elements(256));
    group.bench_function("nested_256", |bencher| {
        bencher.iter(|| nested(&store, &registry, black_box(QUERY_256)));
    });
    group.finish();
}

criterion_group!(benches, region_benches);
criterion_main!(benches);
