use std::time::Instant;

use bevy::{
    prelude::{App, IntoScheduleConfigs, MinimalPlugins, Res, ResMut, Resource, Update},
    time::TimeUpdateStrategy,
};
use render::{
    ChunkRenderApplySet, ChunkRenderPlugin, ChunkRenderQueue, ChunkTextureReload,
    ChunkUploadPriority, ChunkUploadToken,
};

use super::{Duration, PublicationController, PublicationFrameWork, PublicationServiceConfig};
use crate::runtime::publication::{begin_publication_frame, configure_publication_frame_systems};

#[derive(Resource, Default)]
struct PendingFixturePublication(bool);

fn record_fixture_publication(
    mut pending: ResMut<PendingFixturePublication>,
    mut controller: ResMut<PublicationController>,
    queue: Res<ChunkRenderQueue>,
) {
    if !std::mem::take(&mut pending.0) {
        return;
    }
    controller.finish_frame(PublicationFrameWork {
        mesh_jobs_dispatched: 7,
        mesh_changes_published: queue.retained_len(),
        mesh_payloads_published: queue.pending_len(),
        mesh_bytes_published: queue.pending_bytes(),
        pending_mesh_jobs: 123,
        in_flight_mesh_jobs: 11,
        upload_queue_items: queue.retained_len(),
        upload_queue_bytes: queue.pending_bytes(),
        stream_pending_mesh_changes: 3,
        cohort_expected: 1_089,
        cohort_loaded: 900,
        resident_meshes: 850,
        cave_visible_meshes: 700,
        frustum_visible_meshes: 410,
        submitted_meshes: 410,
        gpu_completed_meshes: 410,
        ..PublicationFrameWork::healthy()
    });
}

fn full_batch_app(blocked: bool) -> App {
    let config = PublicationServiceConfig::PHASE2_GATE;
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            125,
        )))
        .init_resource::<PublicationController>()
        .init_resource::<PendingFixturePublication>()
        .add_plugins(ChunkRenderPlugin::default());
    configure_publication_frame_systems(&mut app);
    app.add_systems(
        Update,
        record_fixture_publication
            .after(begin_publication_frame)
            .before(ChunkRenderApplySet),
    );
    // Time<Real>'s first update establishes its origin; the second accrues a frame.
    app.update();
    app.update();
    let allowance = app.world().resource::<PublicationController>().allowance();
    let mesh = super::publication_fixture_mesh(&assets::RuntimeAssets::diagnostic());
    assert!(!mesh.is_empty());
    let mut queue = app.world_mut().resource_mut::<ChunkRenderQueue>();
    let payloads = config.maximum_frame_items - 2;
    for index in 0..config.maximum_frame_items {
        let key = world::SubChunkKey::new(0, index as i32, 0, 0);
        let token = ChunkUploadToken {
            generation: index as u64 + 1,
            dirty_since: Instant::now(),
        };
        if index < payloads {
            let biome = meshing::PackedBiomeRecord::fallback();
            let permit = allowance
                .try_admit_payload(ChunkRenderQueue::upload_byte_len(&mesh, &biome))
                .unwrap();
            queue
                .try_update_tracked_with_biome_identity_permitted(
                    key,
                    mesh.clone(),
                    biome,
                    meshing::ChunkBiomeTintIdentity::default(),
                    ChunkUploadPriority::new(0.0),
                    token,
                    permit,
                )
                .unwrap();
        } else {
            queue
                .try_remove_tracked_permitted(
                    key,
                    ChunkUploadPriority::new(0.0),
                    token,
                    allowance.try_admit_zero_byte().unwrap(),
                )
                .unwrap();
        }
    }
    app.world_mut()
        .resource_mut::<PendingFixturePublication>()
        .0 = true;
    if blocked {
        let reload = ChunkTextureReload::default();
        reload.hold_geometry();
        app.insert_resource(reload);
    }
    app
}

#[test]
fn applied_full_batch_preserves_next_frame_service_caps() {
    let config = PublicationServiceConfig::PHASE2_GATE;
    let mut app = full_batch_app(false);
    app.update();
    assert_eq!(app.world().resource::<ChunkRenderQueue>().retained_len(), 0);
    let work = app
        .world()
        .resource::<PublicationController>()
        .diagnostics()
        .last_work;
    app.update();
    let controller = app.world().resource::<PublicationController>();
    assert_eq!(
        controller.budget().max_per_frame,
        config.maximum_frame_items
    );
    assert_eq!(controller.diagnostics().multiplicative_decreases, 0);
    assert_eq!(work.upload_queue_items, 0);
    assert_eq!(work.upload_queue_bytes, 0);
    assert_eq!(work.mesh_changes_published, config.maximum_frame_items);
    assert_eq!(work.mesh_payloads_published, config.maximum_frame_items - 2);
    assert!(work.mesh_bytes_published > 0);
    assert_eq!(work.pending_mesh_jobs, 123);
    assert_eq!(work.stream_pending_mesh_changes, 3);
    assert_eq!(work.gpu_completed_meshes, 410);
}

#[test]
fn blocked_full_batch_remains_genuine_pressure() {
    let config = PublicationServiceConfig::PHASE2_GATE;
    let mut app = full_batch_app(true);
    app.update();
    assert_eq!(
        app.world().resource::<ChunkRenderQueue>().retained_len(),
        config.maximum_frame_items
    );
    let work = app
        .world()
        .resource::<PublicationController>()
        .diagnostics()
        .last_work;
    app.update();
    let controller = app.world().resource::<PublicationController>();
    assert_eq!(
        controller.budget().max_per_frame,
        config.maximum_frame_items / 2
    );
    assert_eq!(controller.diagnostics().multiplicative_decreases, 1);
    assert_eq!(work.upload_queue_items, config.maximum_frame_items);
}
