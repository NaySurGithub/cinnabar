use super::*;
use bevy::{ecs::system::RunSystemOnce, render::renderer::WgpuWrapper};

#[test]
fn nametag_records_stage_bounded_payloads_and_reuse_unchanged_input() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let mut world = World::new();
    world.insert_resource(crate::upload_staging::BufferUploadStaging::for_tests(
        &device,
        (MAX_NAMETAG_RECORDS * RECORD_BYTES) as u64,
    ));
    world.insert_resource(device);
    world.insert_resource(RenderQueue(Arc::new(WgpuWrapper::new(queue))));
    world.insert_resource(NametagSceneResource(NametagScene {
        records: vec![NametagRecord::default(); MAX_NAMETAG_RECORDS + 1],
        see_through: MAX_NAMETAG_RECORDS + 1,
        ..Default::default()
    }));
    world.run_system_once(init_nametag_gpu).unwrap();
    let before = crate::render_work::snapshot();
    world.run_system_once(prepare_nametags).unwrap();
    let work = crate::render_work::snapshot().delta_since(before);
    assert_eq!(work.staged_uploads, 1);
    assert_eq!(
        work.staged_upload_bytes,
        (MAX_NAMETAG_RECORDS * RECORD_BYTES) as u64
    );
    assert_eq!(work.buffer_upload_bytes, work.staged_upload_bytes);
    assert_eq!(work.texture_upload_bytes, 0);
    assert_eq!(work.fallback_uploads, 0);
    assert_eq!(work.staging_buffers, 0);
    assert_eq!(work.readback_waits, 0);
    let gpu = world.resource::<NametagGpu>();
    let buffer = gpu.record_buffer.id();
    let records = gpu.records.as_ptr();
    let batches = gpu.batches.as_ptr();
    let capacity = gpu.records.capacity();
    assert_eq!(gpu.total as usize, MAX_NAMETAG_RECORDS);
    assert_eq!(gpu.see_through, MAX_NAMETAG_RECORDS);
    let before = crate::render_work::snapshot();
    world.run_system_once(prepare_nametags).unwrap();
    assert_eq!(
        crate::render_work::snapshot().delta_since(before),
        Default::default()
    );
    let gpu = world.resource::<NametagGpu>();
    assert_eq!(gpu.record_buffer.id(), buffer);
    assert_eq!(gpu.records.as_ptr(), records);
    assert_eq!(gpu.records.capacity(), capacity);
    assert_eq!(gpu.batches.as_ptr(), batches);

    world.resource_mut::<NametagSceneResource>().see_through = 0;
    let before = crate::render_work::snapshot();
    world.run_system_once(prepare_nametags).unwrap();
    assert_eq!(
        crate::render_work::snapshot().delta_since(before),
        Default::default()
    );
    assert!(
        world
            .resource::<NametagGpu>()
            .batches
            .iter()
            .all(|batch| batch.depth_tested)
    );

    world.resource_mut::<NametagSceneResource>().records[0].anchor[0] = 1.0;
    let before = crate::render_work::snapshot();
    world.run_system_once(prepare_nametags).unwrap();
    let work = crate::render_work::snapshot().delta_since(before);
    assert_eq!(work.staged_uploads, 1);
    assert_eq!(
        work.staged_upload_bytes,
        (MAX_NAMETAG_RECORDS * RECORD_BYTES) as u64
    );
    assert_eq!(work.fallback_uploads, 0);
    assert_eq!(world.resource::<NametagGpu>().records.as_ptr(), records);

    world.resource_mut::<NametagSceneResource>().atlas = Arc::from([NametagAtlasRect {
        cell: [0, 0, 1, 1],
        rgba8: Arc::from([255; 4]),
    }]);
    let before = crate::render_work::snapshot();
    world.run_system_once(prepare_nametags).unwrap();
    let work = crate::render_work::snapshot().delta_since(before);
    assert_eq!(work.buffer_upload_bytes, 0);
    assert_eq!(work.texture_upload_bytes, 4);
    world.resource_mut::<NametagSceneResource>().records.clear();
    let before = crate::render_work::snapshot();
    world.run_system_once(prepare_nametags).unwrap();
    assert_eq!(
        crate::render_work::snapshot().delta_since(before),
        Default::default()
    );
    assert_eq!(world.resource::<NametagGpu>().total, 0);
    assert!(world.resource::<NametagGpu>().batches.is_empty());
    assert_eq!(world.resource::<NametagGpu>().records.capacity(), capacity);
}
