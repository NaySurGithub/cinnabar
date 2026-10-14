use super::*;
use crate::{TerrainPosition, TerrainQuery, TerrainRegion};
#[path = "terrain_guest.rs"]
mod terrain_guest;
use terrain_guest::{expect_region, terrain_guest};

const MIN: [i32; 3] = [0, 63, 0];
const MAX: [i32; 3] = [2, 65, 2];

fn load(check: &str, terrain: bool) -> ModHost {
    ModHost::load_snapshot_with_grants(
        Path::new("terrain.wasm"),
        &terrain_guest(MIN, MAX, check),
        ModGrants {
            terrain,
            ..Default::default()
        },
    )
    .unwrap()
}
fn snapshot() -> GameplaySnapshot {
    GameplaySnapshot {
        session: 1,
        dimension: 0,
        eye: GameplayVector3 {
            x: 0.,
            y: 64.,
            z: 0.,
        },
        yaw: 0.,
        pitch: 0.,
        attack_held: false,
        frame_seconds: 0.016,
        players: vec![],
    }
}
fn query() -> TerrainQuery {
    let position = |[x, y, z]: [i32; 3]| TerrainPosition { x, y, z };
    TerrainQuery {
        min: position(MIN),
        max: position(MAX),
    }
}
fn region() -> TerrainRegion {
    TerrainRegion {
        session: 1,
        dimension: 0,
        query: query(),
        chunks: vec![],
        cells: vec![],
    }
}

#[test]
fn requests_and_regions_cross_the_component_boundary() {
    let mut host = load(&expect_region(false), true);
    host.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert_eq!(host.take_terrain_queries(), [query()]);
    let mut host = load(&expect_region(true), true);
    host.supply_terrain(vec![region()]);
    host.frame_with_gameplay(false, Some(snapshot())).unwrap();
}

#[test]
fn denied_or_trapped_reads_request_nothing() {
    let mut denied = load(&expect_region(false), false);
    assert!(denied.frame_with_gameplay(false, Some(snapshot())).is_err());
    assert!(denied.take_terrain_queries().is_empty());
    let mut trapped = load("unreachable", true);
    assert!(
        trapped
            .frame_with_gameplay(false, Some(snapshot()))
            .is_err()
    );
    assert!(!trapped.is_active());
    assert!(trapped.take_terrain_queries().is_empty());
}

#[test]
fn terrain_compute_remains_bounded() {
    let mut host = load("(loop $forever br $forever)", true);
    assert!(host.frame_with_gameplay(false, Some(snapshot())).is_err());
    assert!(!host.is_active());
    assert!(host.take_terrain_queries().is_empty());
}

#[test]
fn terrain_frames_run_after_the_shared_event_lane_is_used() {
    let mut host = load(&expect_region(false), true);
    let budget = CallbackBudget::new(mod_api::TOTAL_MOD_FRAME_FUEL);
    assert!(
        budget
            .reserve(server_experience::runtime::CALLBACK_FUEL)
            .is_some()
    );
    host.set_frame_authority(budget.clone(), false, false, 0, HudClaims::default());
    host.frame_with_gameplay(false, Some(snapshot())).unwrap();
    assert_eq!(host.take_terrain_queries(), [query()]);
    assert_eq!(budget.remaining(), mod_api::TOTAL_MOD_FRAME_FUEL);
}
