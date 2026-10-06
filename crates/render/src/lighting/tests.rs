use super::*;
use bevy::{
    ecs::system::RunSystemOnce,
    render::renderer::{RenderAdapter, WgpuWrapper},
};
use std::{
    future::Future,
    pin::pin,
    sync::Arc,
    task::{Context, Poll, Waker},
};

/// Builds only the resources consumed by lightmap preparation on a validation-only device.
fn world() -> World {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions { enable: true },
            ..Default::default()
        },
        ..Default::default()
    });
    let mut context = Context::from_waker(Waker::noop());
    let Poll::Ready(Ok(adapter)) =
        pin!(instance.request_adapter(&Default::default())).poll(&mut context)
    else {
        panic!("noop adapter must be immediate");
    };
    let Poll::Ready(Ok((device, queue))) =
        pin!(adapter.request_device(&Default::default())).poll(&mut context)
    else {
        panic!("noop device must be immediate");
    };
    let device = RenderDevice::from(device);
    let mut world = World::new();
    world.insert_resource(crate::upload_staging::BufferUploadStaging::for_tests(
        &device, 4096,
    ));
    world.insert_resource(PipelineCache::new(
        device.clone(),
        RenderAdapter(Arc::new(WgpuWrapper::new(adapter))),
        true,
    ));
    world.insert_resource(device);
    world.insert_resource(RenderQueue(Arc::new(WgpuWrapper::new(queue))));
    world.init_resource::<WorldLighting>();
    world.init_resource::<WorldFullbright>();
    world
}

#[test]
fn changed_lightmap_stages_one_table_and_unchanged_input_does_no_work() {
    let mut world = world();
    world.run_system_once(prepare).unwrap();
    let buffer = world.resource::<LightmapGpu>().buffer.id();
    let binding = world.resource::<LightmapGpu>().bind_group.id();
    world.resource_mut::<WorldFullbright>().0 = true;
    let before = crate::render_work::snapshot();
    world.run_system_once(prepare).unwrap();
    let work = crate::render_work::snapshot().delta_since(before);
    assert_eq!(work.staged_uploads, 1);
    assert_eq!(
        work.staged_upload_bytes,
        size_of::<[[f32; 4]; 256]>() as u64
    );
    assert_eq!(work.buffer_upload_bytes, work.staged_upload_bytes);
    assert_eq!(work.fallback_uploads, 0);
    assert_eq!(work.staging_buffers, 0);
    assert_eq!(work.readback_waits, 0);
    assert_eq!(world.resource::<LightmapGpu>().buffer.id(), buffer);
    assert_eq!(world.resource::<LightmapGpu>().bind_group.id(), binding);
    let before = crate::render_work::snapshot();
    world.run_system_once(prepare).unwrap();
    assert_eq!(
        crate::render_work::snapshot().delta_since(before),
        Default::default()
    );
}
