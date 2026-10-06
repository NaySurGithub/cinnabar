use super::*;
use bevy::{ecs::system::RunSystemOnce, render::renderer::WgpuWrapper};
use std::sync::Arc;

#[test]
fn atmosphere_uniform_stages_one_changed_frame_and_skips_unchanged_input() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let mut world = World::new();
    world.insert_resource(crate::upload_staging::BufferUploadStaging::for_tests(
        &device, 1024,
    ));
    world.insert_resource(device);
    world.insert_resource(RenderQueue(Arc::new(WgpuWrapper::new(queue))));
    world.init_resource::<AtmosphereFrame>();
    world.run_system_once(init_atmosphere_gpu).unwrap();
    let buffer = world.resource::<AtmosphereGpu>().buffer.id();
    let before = crate::render_work::snapshot();
    world.run_system_once(prepare_atmosphere_uniform).unwrap();
    assert_eq!(
        crate::render_work::snapshot().delta_since(before),
        Default::default()
    );

    world.insert_resource(AtmosphereFrame::from_bedrock_time(100.0, 0.5, 0.25));
    let before = crate::render_work::snapshot();
    world.run_system_once(prepare_atmosphere_uniform).unwrap();
    let work = crate::render_work::snapshot().delta_since(before);
    assert_eq!(work.staged_uploads, 1);
    assert_eq!(
        work.staged_upload_bytes,
        std::mem::size_of::<AtmosphereFrame>() as u64
    );
    assert_eq!(work.buffer_upload_bytes, work.staged_upload_bytes);
    assert_eq!(work.fallback_uploads, 0);
    assert_eq!(work.staging_buffers, 0);
    assert_eq!(work.readback_waits, 0);
    assert_eq!(world.resource::<AtmosphereGpu>().buffer.id(), buffer);
    let before = crate::render_work::snapshot();
    world.run_system_once(prepare_atmosphere_uniform).unwrap();
    assert_eq!(
        crate::render_work::snapshot().delta_since(before),
        Default::default()
    );
}
