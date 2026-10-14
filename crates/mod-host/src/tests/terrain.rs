use super::*;
use crate::{
    TerrainGeometry, TerrainPosition, TerrainQuery, TerrainRefresh, TerrainRegion, TerrainStamp,
};
use std::path::Path;
#[path = "terrain_guest.rs"]
mod terrain_guest;
use terrain_guest::{CHANGED, PENDING, UNCHANGED, expect_error, expect_reads, terrain_guest};

const MIN: [i32; 3] = [0, 63, 0];
const MAX: [i32; 3] = [2, 65, 2];
/// `geometry` flag bits: collision only.
const COLLISION: u8 = 1;

fn load(check: &str, terrain: bool) -> ModHost {
    ModHost::load_snapshot_with_grants(
        Path::new("terrain.wasm"),
        &terrain_guest(MIN, MAX, COLLISION, true, check),
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
/// Observes the terrain session, then runs one frame.
fn run(host: &mut ModHost) -> anyhow::Result<()> {
    let world = crate::WorldObservation {
        session: 1,
        dimension: 0,
        world: "fixture".into(),
        position: (0., 63., 0.),
        alive: true,
    };
    host.observe_presentation(Some(world), None, None, Vec::new())?;
    host.frame_with_gameplay(false, Some(snapshot()))
}
fn query() -> TerrainQuery {
    let position = |[x, y, z]: [i32; 3]| TerrainPosition { x, y, z };
    TerrainQuery {
        min: position(MIN),
        max: position(MAX),
        geometry: TerrainGeometry::COLLISION,
    }
}
fn region() -> TerrainRegion {
    TerrainRegion {
        stamp: TerrainStamp {
            session: 1,
            dimension: 0,
            registry: 3,
            revision: 0,
        },
        chunks: vec![],
        layer_count: 1,
        loaded: vec![0xff],
        states: vec![0; 8],
        palette: vec![],
        shape_offsets: vec![0, 0],
        shape_boxes: vec![],
        overrides: vec![],
    }
}
/// Supplies the committed query's next outcome, then runs one frame.
fn frame(host: &mut ModHost, refresh: TerrainRefresh) -> anyhow::Result<()> {
    host.refresh_terrain(|candidate, _| {
        assert_eq!(*candidate, query());
        refresh.clone()
    });
    run(host)
}

/// New queries are pending, then deliver a region once and an unchanged stamp while kept.
#[test]
fn polling_crosses_the_component_boundary() {
    let mut host = load(
        &expect_reads(&[PENDING, CHANGED, UNCHANGED, CHANGED, UNCHANGED, PENDING]),
        true,
    );
    run(&mut host).unwrap();
    for refresh in [
        TerrainRefresh::Replace(Box::new(region())),
        TerrainRefresh::Keep,
        TerrainRefresh::Replace(Box::new(region())),
        TerrainRefresh::Keep,
        TerrainRefresh::Pending,
    ] {
        frame(&mut host, refresh).unwrap();
    }
}

/// A read result the guest does not expect traps its frame.
#[test]
fn unexpected_results_trap() {
    let mut host = load(&expect_reads(&[CHANGED]), true);
    assert!(run(&mut host).is_err());
    assert!(!host.is_active());
}

#[test]
fn denied_or_trapped_reads_request_nothing() {
    let mut denied = load(&expect_error(0), false);
    run(&mut denied).unwrap();
    let mut requested = false;
    denied.refresh_terrain(|_, _| {
        requested = true;
        TerrainRefresh::Pending
    });
    assert!(!requested);
    let mut trapped = load("unreachable", true);
    assert!(run(&mut trapped).is_err());
    assert!(!trapped.is_active());
    trapped.refresh_terrain(|_, _| unreachable!("trapped requests are discarded"));
}

#[test]
fn terrain_compute_remains_bounded() {
    let mut host = load("(loop $forever br $forever)", true);
    assert!(run(&mut host).is_err());
    assert!(!host.is_active());
}

#[test]
fn terrain_frames_run_after_the_shared_event_lane_is_used() {
    let mut host = load(&expect_reads(&[PENDING]), true);
    let budget = CallbackBudget::new(mod_api::TOTAL_MOD_FRAME_FUEL);
    assert!(
        budget
            .reserve(server_experience::runtime::CALLBACK_FUEL)
            .is_some()
    );
    host.set_frame_authority(budget.clone(), false, false, 0, HudClaims::default());
    run(&mut host).unwrap();
    assert_eq!(budget.remaining(), mod_api::TOTAL_MOD_FRAME_FUEL);
}

/// Two terrain mods that spend fuel every frame both run every frame on the shared lane.
#[test]
fn terrain_mods_after_the_first_are_not_starved() {
    // Counts down a linear-memory counter, spending a large share of one callback's fuel.
    let burn = "i32.const 600 i32.const 60000 i32.store \
        (loop $spin i32.const 600 i32.const 600 i32.load i32.const 1 i32.sub i32.store \
        i32.const 600 i32.load br_if $spin)";
    let mut hosts = [load(burn, true), load(burn, true)];
    let budget = CallbackBudget::new(mod_api::TOTAL_MOD_FRAME_FUEL);
    for _ in 0..4 {
        budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
        for (owner, host) in hosts.iter_mut().enumerate() {
            host.set_frame_authority(budget.clone(), false, false, owner, HudClaims::default());
            run(host).unwrap();
        }
        for host in &mut hosts {
            let mut staged = false;
            host.refresh_terrain(|_, _| {
                staged = true;
                TerrainRefresh::Pending
            });
            assert!(staged, "every terrain mod ran this frame");
        }
    }
}
