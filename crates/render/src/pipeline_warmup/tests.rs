use super::*;
use crate::render_work::PipelineWork as _;
use bevy::{
    ecs::system::RunSystemOnce,
    render::{
        render_resource::{ColorTargetState, FragmentState, RenderPipelineDescriptor, VertexState},
        renderer::{RenderAdapter, RenderDevice, WgpuWrapper},
    },
};
use std::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};

/// Uses the validation-only backend, with no window, native GPU or timing bound.
fn cache() -> (RenderDevice, PipelineCache) {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions { enable: true },
            ..default()
        },
        ..default()
    });
    let mut context = Context::from_waker(Waker::noop());
    let Poll::Ready(Ok(adapter)) = pin!(instance.request_adapter(&default())).poll(&mut context)
    else {
        panic!("NOOP adapter must be immediate");
    };
    let Poll::Ready(Ok((device, _))) = pin!(adapter.request_device(&default())).poll(&mut context)
    else {
        panic!("NOOP device must be immediate");
    };
    let device = RenderDevice::from(device);
    let adapter = RenderAdapter(Arc::new(WgpuWrapper::new(adapter)));
    let mut cache = PipelineCache::new(device.clone(), adapter, true);
    cache.set_shader(Handle::<Shader>::default().id(), Shader::from_wgsl(
        "@vertex fn vertex() -> @builtin(position) vec4f { return vec4f(0.0); }\n@fragment fn fragment() -> @location(0) vec4f { return vec4f(1.0); }",
        "warmup-regression.wgsl"));
    (device, cache)
}

#[derive(Resource, Default)]
struct TwoModes {
    calls: usize,
}

impl PrewarmPipelines for TwoModes {
    const PROFILE: crate::render_systems::System =
        crate::render_systems::System::WarmupActorPipeline;
    /// Distinct descriptors make missing IDs observable through real cache work.
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        _view: WarmView,
        ids: &mut WarmupIds,
    ) -> Result<(), BevyError> {
        self.calls += 1;
        for mask in [
            bevy::render::render_resource::ColorWrites::RED,
            bevy::render::render_resource::ColorWrites::GREEN,
        ] {
            ids.push(
                cache.tracked_queue_render_pipeline(RenderPipelineDescriptor {
                    vertex: VertexState {
                        shader: Handle::default(),
                        entry_point: Some("vertex".into()),
                        ..default()
                    },
                    fragment: Some(FragmentState {
                        shader: Handle::default(),
                        entry_point: Some("fragment".into()),
                        targets: vec![Some(ColorTargetState {
                            format: TextureFormat::Rgba8Unorm,
                            blend: None,
                            write_mask: mask,
                        })],
                        ..default()
                    }),
                    ..default()
                }),
            );
        }
        Ok(())
    }
}

/// Reads the same aggregate gate that is published to the main world.
fn ready(world: &mut World) -> bool {
    world.resource_scope(|world, cache: Mut<PipelineCache>| {
        registered_pipelines_ready(&cache, &mut world.resource_mut::<WarmupRegistry>())
    })
}

#[test]
fn warmup_requires_every_id_and_steady_state_creates_nothing() {
    let (_, cache) = cache();
    let mut world = World::new();
    world.insert_resource(cache);
    world.init_resource::<TwoModes>();
    let mut registry = WarmupRegistry::default();
    registry.owners.insert(TypeId::of::<TwoModes>(), default());
    registry.views.push(WarmView {
        msaa: Msaa::Off,
        hdr: false,
        enhanced: false,
        main_format: TextureFormat::Rgba8Unorm,
        output_format: TextureFormat::Rgba8Unorm,
    });
    world.insert_resource(registry);
    assert!(!ready(&mut world));
    let before = crate::render_work::snapshot();
    world.run_system_once(queue_owner::<TwoModes>).unwrap();
    assert!(!ready(&mut world), "queued pipelines are not usable yet");
    let ids = world.resource::<WarmupRegistry>().owners[&TypeId::of::<TwoModes>()]
        .ids
        .clone();
    assert_eq!(ids.len(), 2);
    world.resource_mut::<PipelineCache>().process_queue();
    assert!(ready(&mut world));
    let warmed = crate::render_work::snapshot();
    assert_eq!(
        warmed.delta_since(before).render_pipelines_queued,
        ids.len() as u64
    );
    for _ in 0..3 {
        world.run_system_once(queue_owner::<TwoModes>).unwrap();
        world.resource_mut::<PipelineCache>().process_queue();
        assert!(ready(&mut world));
    }
    assert_eq!(world.resource::<TwoModes>().calls, 1);
    let steady = crate::render_work::snapshot().delta_since(warmed);
    assert_eq!(steady.render_pipelines_queued, 0);
    assert_eq!(steady.render_pipelines_created, 0);
    assert_eq!(steady.shader_modules_created, 0);
    assert_eq!(steady.buffer_upload_bytes + steady.texture_upload_bytes, 0);
    world
        .resource_mut::<PipelineCache>()
        .remove_shader(Handle::<Shader>::default().id());
    assert!(
        !ready(&mut world),
        "shader invalidation must close the gate again"
    );
}

#[test]
fn an_owner_without_a_view_cannot_release_loading() {
    let (_, cache) = cache();
    let mut registry = WarmupRegistry::default();
    registry.owners.insert(TypeId::of::<TwoModes>(), default());
    assert!(!registered_pipelines_ready(&cache, &mut registry));
    assert!(!PipelineWarmupReadiness::default().ready());
}
