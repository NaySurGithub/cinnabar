use super::*;
use bevy::{
    ecs::system::RunSystemOnce,
    render::{
        render_resource::{TextureId, TextureViewId},
        renderer::WgpuWrapper,
    },
};

/// Builds only the real clock and immutable texture preparation resources on NOOP.
fn world() -> World {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let mut world = World::new();
    world.insert_resource(crate::upload_staging::BufferUploadStaging::for_tests(
        &device, 4096,
    ));
    world.insert_resource(device);
    world.insert_resource(RenderQueue(Arc::new(WgpuWrapper::new(queue))));
    world.init_resource::<ChunkAnimationClock>();
    world.init_resource::<ChunkTextureAssets>();
    world.init_resource::<ChunkTextureReload>();
    world.init_resource::<ChunkGpuTextureAssets>();
    world.init_resource::<ChunkTextureUploadStats>();
    world.run_system_once(init_chunk_gpu_arena).unwrap();
    world
        .run_system_once(init_chunk_gpu_animation_clock)
        .unwrap();
    world.run_system_once(prepare_chunk_texture_assets).unwrap();
    world
}

/// Records actual GPU identities rather than inferring retention from upload statistics.
fn prepared_ids(world: &World) -> ([BufferId; 4], [TextureId; 2], [TextureViewId; 2]) {
    let prepared = world
        .resource::<ChunkGpuTextureAssets>()
        .prepared
        .as_ref()
        .unwrap();
    (
        [
            prepared.material_buffer.id(),
            prepared.animation_buffer.id(),
            prepared.animation_frame_buffer.id(),
            prepared.model_template_buffer.id(),
        ],
        prepared._textures.each_ref().map(Texture::id),
        prepared.views.each_ref().map(TextureView::id),
    )
}

#[test]
fn animation_clock_updates_do_not_rebuild_or_reupload_texture_assets() {
    let mut world = world();
    let immutable = prepared_ids(&world);
    let clock_buffer = world.resource::<ChunkGpuAnimationClock>().buffer.id();
    let initial = *world.resource::<ChunkTextureUploadStats>();
    assert_eq!(initial.upload_count, 1);
    assert!(initial.texture_bytes_including_mips > 0);
    let before = crate::render_work::snapshot();
    world
        .run_system_once(prepare_chunk_animation_clock)
        .unwrap();
    assert_eq!(
        crate::render_work::snapshot().delta_since(before),
        Default::default()
    );

    for frame in 1..=120_u32 {
        let clock = ChunkAnimationClock::from_elapsed_seconds(f64::from(frame) / 60.0);
        world.insert_resource(clock);
        let before = crate::render_work::snapshot();
        world.run_system_once(prepare_chunk_texture_assets).unwrap();
        world
            .run_system_once(prepare_chunk_animation_clock)
            .unwrap();
        let work = crate::render_work::snapshot().delta_since(before);
        assert_eq!(work.staged_uploads, 1);
        assert_eq!(
            work.staged_upload_bytes,
            size_of::<ChunkAnimationClock>() as u64
        );
        assert_eq!(work.buffer_upload_bytes, work.staged_upload_bytes);
        assert_eq!(work.texture_upload_bytes, 0);
        assert_eq!(work.bind_groups_created, 0);
        assert_eq!(work.staging_buffers, 0);
        assert_eq!(work.fallback_uploads, 0);
        assert_eq!(work.own_queue_submits, 0);
        assert_eq!(work.readback_waits, 0);
        assert_eq!(prepared_ids(&world), immutable);
        assert_eq!(
            world.resource::<ChunkGpuAnimationClock>().buffer.id(),
            clock_buffer
        );
        assert_eq!(world.resource::<ChunkGpuAnimationClock>().uploaded, clock);
        assert_eq!(*world.resource::<ChunkTextureUploadStats>(), initial);
        let before = crate::render_work::snapshot();
        world.run_system_once(prepare_chunk_texture_assets).unwrap();
        world
            .run_system_once(prepare_chunk_animation_clock)
            .unwrap();
        assert_eq!(
            crate::render_work::snapshot().delta_since(before),
            Default::default()
        );
    }
}
