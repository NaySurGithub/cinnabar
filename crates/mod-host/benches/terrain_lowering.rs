//! Component-boundary cost of delivering one terrain region to a guest per frame.

#[path = "../src/tests/terrain_guest.rs"]
mod terrain_guest;

use std::{
    hint::black_box,
    path::Path,
    time::{Duration, Instant},
};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use mod_host::{
    GameplaySnapshot, GameplayVector3, ModGrants, ModHost, TerrainBounds, TerrainCell,
    TerrainChunkRevision, TerrainLayer, TerrainPosition, TerrainProperties, TerrainQuery,
    TerrainRegion,
};

#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

const MIN: [i32; 3] = [12, 62, 12];
const MAX: [i32; 3] = [20, 66, 20];

fn snapshot() -> GameplaySnapshot {
    GameplaySnapshot {
        session: 1,
        dimension: 0,
        eye: GameplayVector3 {
            x: 16.,
            y: 64.,
            z: 16.,
        },
        yaw: 0.,
        pitch: 0.,
        attack_held: false,
        frame_seconds: 0.016,
        players: vec![],
    }
}

fn position([x, y, z]: [i32; 3]) -> TerrainPosition {
    TerrainPosition { x, y, z }
}

/// A 256-cell nested response with representative strings, tags and shapes per layer.
fn nested_region() -> TerrainRegion {
    let cube = TerrainBounds {
        min: (0., 0., 0.),
        max: (1., 1., 1.),
    };
    let layer = |identifier: &str, state: &str, tags: &[&str], shapes: usize| TerrainLayer {
        runtime_id: 1,
        identifier: Some(identifier.into()),
        canonical_state: Some(state.into()),
        tags: tags.iter().map(|tag| (*tag).into()).collect(),
        properties: Some(TerrainProperties {
            collision: vec![cube; shapes],
            selection: vec![cube; shapes],
            friction: 0.6,
            horizontal_speed_factor: 1.,
            vertical_speed_factor: 1.,
            fluid_height: 0.,
            climbable: false,
            water: false,
            lava: false,
            cobweb: false,
            powder_snow: false,
            scaffolding: false,
            passable: false,
            surface_response: "None".into(),
        }),
    };
    let mut cells = Vec::new();
    for x in MIN[0]..MAX[0] {
        for y in MIN[1]..MAX[1] {
            for z in MIN[2]..MAX[2] {
                let loaded = !(x >= 16 && z >= 16);
                let layers = match (loaded, y) {
                    (false, _) => vec![],
                    (true, ..64) => vec![layer(
                        "minecraft:dirt",
                        r#"{"dirt_type":{"type":"string","value":"normal"}}"#,
                        &["minecraft:is_shovel_item_destructible", "dirt"],
                        1,
                    )],
                    (true, 64) => vec![
                        layer(
                            "minecraft:oak_stairs",
                            r#"{"upside_down_bit":{"type":"byte","value":0},"weirdo_direction":{"type":"int","value":2}}"#,
                            &["minecraft:is_axe_item_destructible", "wood", "stairs"],
                            2,
                        ),
                        layer(
                            "minecraft:water",
                            r#"{"liquid_depth":{"type":"int","value":0}}"#,
                            &["water"],
                            0,
                        ),
                    ],
                    _ => vec![layer("minecraft:air", "{}", &[], 0)],
                };
                cells.push(TerrainCell {
                    position: position([x, y, z]),
                    loaded,
                    layers,
                });
            }
        }
    }
    TerrainRegion {
        session: 1,
        dimension: 0,
        query: TerrainQuery {
            min: position(MIN),
            max: position(MAX),
        },
        chunks: (0..=1)
            .flat_map(|x| {
                (0..=1).map(move |z| TerrainChunkRevision {
                    x,
                    z,
                    revision: Some(7),
                })
            })
            .collect(),
        cells,
    }
}

/// Times only the guest frame; supplying the next response happens outside the measurement.
fn frames(host: &mut ModHost, region: Option<&TerrainRegion>, iterations: u64) -> Duration {
    let mut total = Duration::ZERO;
    for _ in 0..iterations {
        host.supply_terrain(region.cloned().into_iter().collect());
        let start = Instant::now();
        host.frame_with_gameplay(false, Some(snapshot())).unwrap();
        total += start.elapsed();
        black_box(host.take_terrain_queries());
    }
    total
}

fn lowering_benches(criterion: &mut Criterion) {
    let grants = ModGrants {
        terrain: true,
        ..Default::default()
    };
    let load = |present: bool| {
        ModHost::load_snapshot_with_grants(
            Path::new("terrain.wasm"),
            &terrain_guest::terrain_guest(MIN, MAX, &terrain_guest::expect_region(present)),
            grants.clone(),
        )
        .unwrap()
    };
    let region = nested_region();
    let mut empty = load(false);
    let mut nested = load(true);
    nested.supply_terrain(vec![region.clone()]);
    let ((), allocations) =
        test_alloc::measure(|| nested.frame_with_gameplay(false, Some(snapshot())).unwrap());
    eprintln!(
        "terrain_lowering/nested_256: {} allocations, {} bytes per delivered frame",
        allocations.calls, allocations.bytes
    );
    let mut group = criterion.benchmark_group("terrain_lowering");
    group.throughput(Throughput::Elements(256));
    group.bench_function("no_region", |bencher| {
        bencher.iter_custom(|iterations| frames(&mut empty, None, iterations));
    });
    group.bench_function("nested_256", |bencher| {
        bencher.iter_custom(|iterations| frames(&mut nested, Some(&region), iterations));
    });
    group.finish();
}

criterion_group!(benches, lowering_benches);
criterion_main!(benches);
