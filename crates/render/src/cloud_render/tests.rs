use std::sync::Arc;

use assets::{
    AtmosphereTexture, CompiledAtmosphereAssets, RuntimeAtmosphereAssets, encode_atmosphere_blob,
};
use bevy::{
    ecs::system::RunSystemOnce,
    render::{renderer::WgpuWrapper, view::RetainedViewEntity},
};
use sha2::{Digest, Sha256};

use super::*;

fn runtime(seed: u8) -> Arc<RuntimeAtmosphereAssets> {
    let textures = [
        (AtmosphereRole::Sun, "textures/environment/sun.png", 32, 32),
        (
            AtmosphereRole::MoonPhases,
            "textures/environment/moon_phases.png",
            128,
            64,
        ),
        (
            AtmosphereRole::Clouds,
            "textures/environment/clouds.png",
            meshing::CLOUD_MASK_SIZE,
            meshing::CLOUD_MASK_SIZE,
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (role, path, width, height))| {
        let rgba8 = vec![seed; (width * height * 4) as usize];
        AtmosphereTexture {
            role,
            source_path: path.into(),
            source_bytes: 1,
            source_sha256: [index as u8 + 1; 32],
            pixels_sha256: Sha256::digest(&rgba8).into(),
            width,
            height,
            rgba8: rgba8.into_boxed_slice(),
        }
    })
    .collect::<Vec<_>>()
    .into_boxed_slice();
    let blob = encode_atmosphere_blob(&CompiledAtmosphereAssets {
        source_manifest_sha256: [0x66; 32],
        textures,
        biome_profiles: Box::new([]),
        fog_profiles: Box::new([]),
    })
    .unwrap();
    Arc::new(RuntimeAtmosphereAssets::decode(&blob).unwrap())
}

fn world() -> World {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut world = World::new();
    world.insert_resource(RenderDevice::from(device));
    world.insert_resource(RenderQueue(Arc::new(WgpuWrapper::new(queue))));
    world.insert_resource(AtmosphereFrame::from_bedrock_time(0.0, 0.0, 0.0));
    world.insert_resource(AtmosphereTextureAssets::new(runtime(255), [0x41; 32]));
    world.run_system_once(init_cloud_gpu).unwrap();
    world
}

fn spawn_view(world: &mut World, position: Vec3) -> Entity {
    let entity = world.spawn_empty().id();
    world.entity_mut(entity).insert((
        Camera3d::default(),
        ExtractedView {
            retained_view_entity: RetainedViewEntity::new(entity.into(), None, 0),
            clip_from_view: Mat4::IDENTITY,
            world_from_view: GlobalTransform::from_translation(position),
            clip_from_world: None,
            hdr: false,
            viewport: UVec4::new(0, 0, 256, 256),
            color_grading: Default::default(),
            invert_culling: false,
        },
    ));
    entity
}

fn buffer_id(world: &World, entity: Entity) -> BufferId {
    world.resource::<CloudGpu>().views[&entity]
        .record_buffer
        .as_ref()
        .unwrap()
        .id()
}

#[test]
fn viewport_records_are_identity_cached_with_exact_diagnostic_layout() {
    let mut world = world();
    let view = spawn_view(&mut world, Vec3::ZERO);
    world.run_system_once(prepare_cloud_records).unwrap();
    let gpu = world.resource::<CloudGpu>();
    let prepared = &gpu.views[&view];
    let config = CloudRenderConfig::legacy_fancy();
    let expected_count = u32::from(config.mesh_size()).pow(2) + 4 * u32::from(config.mesh_size());
    assert_eq!(prepared.record_count, expected_count);
    assert_eq!(gpu.upload_count, 1);
    let diagnostic = prepared.geometry_diagnostic.as_ref().unwrap();
    assert_eq!(diagnostic.quad_count(), expected_count);
    assert_eq!(
        diagnostic.quad_bytes(),
        expected_count * size_of::<ViewportCloudQuad>() as u32
    );
    assert_eq!(diagnostic.instance_count(), 1);
    assert_eq!(
        diagnostic.occupied_texels(),
        meshing::CLOUD_MASK_SIZE.pow(2)
    );
    assert!(
        diagnostic
            .marker_fields()
            .contains("geometry=viewport record_stride_bytes=16")
    );
    assert!(diagnostic.marker_fields().contains("calibrated=false"));
    let first = buffer_id(&world, view);
    world.insert_resource(AtmosphereTextureAssets::new(runtime(254), [0x41; 32]));
    world.run_system_once(prepare_cloud_records).unwrap();
    assert_eq!(buffer_id(&world, view), first);
    assert_eq!(world.resource::<CloudGpu>().upload_count, 1);
    world.insert_resource(AtmosphereTextureAssets::new(runtime(254), [0x42; 32]));
    world.run_system_once(prepare_cloud_records).unwrap();
    assert_eq!(buffer_id(&world, view), first);
    assert_eq!(world.resource::<CloudGpu>().upload_count, 2);
}

#[test]
fn dimension_changes_remove_clouds_and_restore_them_only_in_the_overworld() {
    let mut world = world();
    let view = spawn_view(&mut world, Vec3::ZERO);
    let base = AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0);
    world.run_system_once(prepare_cloud_records).unwrap();
    assert!(world.resource::<CloudGpu>().views[&view].record_count > 0);

    for kind in [crate::SkyKind::End, crate::SkyKind::Nether] {
        world.insert_resource(base.with_sky_kind(kind));
        world.run_system_once(prepare_cloud_records).unwrap();
        assert!(
            world.resource::<CloudGpu>().views.is_empty(),
            "clouds survived the switch to {kind:?}"
        );

        world.insert_resource(base.with_sky_kind(crate::SkyKind::Overworld));
        world.run_system_once(prepare_cloud_records).unwrap();
        assert!(world.resource::<CloudGpu>().views[&view].record_count > 0);
    }
}

#[test]
fn per_view_cache_does_not_churn_on_subpixel_moves_and_is_pruned_on_removal() {
    let mut world = world();
    let first = spawn_view(&mut world, Vec3::ZERO);
    let second = spawn_view(&mut world, Vec3::new(-1000.0, 0.0, -1000.0));
    world.run_system_once(prepare_cloud_records).unwrap();
    let first_buffer = buffer_id(&world, first);
    let second_buffer = buffer_id(&world, second);
    world
        .get_mut::<ExtractedView>(first)
        .unwrap()
        .world_from_view = GlobalTransform::from_xyz(0.125, 0.0, 0.0);
    world.run_system_once(prepare_cloud_records).unwrap();
    assert_eq!(buffer_id(&world, first), first_buffer);
    assert_eq!(buffer_id(&world, second), second_buffer);
    world
        .get_mut::<ExtractedView>(first)
        .unwrap()
        .world_from_view = GlobalTransform::from_xyz(15.125, 0.0, 0.0);
    world.run_system_once(prepare_cloud_records).unwrap();
    assert_eq!(buffer_id(&world, first), first_buffer);
    assert_eq!(world.resource::<CloudGpu>().upload_count, 3);
    assert_eq!(buffer_id(&world, second), second_buffer);
    world.despawn(first);
    world.run_system_once(prepare_cloud_records).unwrap();
    assert_eq!(world.resource::<CloudGpu>().views.len(), 1);
}

#[test]
fn scroll_is_continuous_until_native_rebuild_threshold_and_invalid_view_is_discarded() {
    let mut world = world();
    let view = spawn_view(&mut world, Vec3::ZERO);
    world.run_system_once(prepare_cloud_records).unwrap();
    let first = buffer_id(&world, view);
    world.insert_resource(
        AtmosphereFrame::from_bedrock_time(0.0, 0.0, 0.0).with_cloud_renderer_ticks(1.0),
    );
    world.run_system_once(prepare_cloud_records).unwrap();
    assert_eq!(buffer_id(&world, view), first);
    world.insert_resource(
        AtmosphereFrame::from_bedrock_time(0.0, 0.0, 0.0).with_cloud_renderer_ticks(1000.0),
    );
    world.run_system_once(prepare_cloud_records).unwrap();
    assert_eq!(buffer_id(&world, view), first);
    assert_eq!(world.resource::<CloudGpu>().upload_count, 2);
    world
        .get_mut::<ExtractedView>(view)
        .unwrap()
        .world_from_view = GlobalTransform::from_xyz(f32::NAN, 0.0, 0.0);
    world.run_system_once(prepare_cloud_records).unwrap();
    assert!(world.resource::<CloudGpu>().views.is_empty());
}

#[test]
fn window_bounds_use_admitted_sampling_centre_and_current_subpixel_scroll() {
    let viewport = CloudViewport::try_new([-0.001; 2], 2, 1, false, false).unwrap();
    let midpoint_y = (CLOUD_UNDERSIDE_Y + CLOUD_TOP_Y) * 0.5;
    assert_eq!(
        cloud_bounds_center(viewport, 0.0),
        [-16.0, midpoint_y, -16.0]
    );
    assert_eq!(
        cloud_bounds_center(viewport, 0.125),
        [-16.125, midpoint_y, -16.0]
    );
}

#[test]
fn moving_cloud_window_reuses_storage_and_bindings_with_bounded_uploads() {
    let mut world = world();
    let view = spawn_view(&mut world, Vec3::ZERO);
    world.run_system_once(prepare_cloud_records).unwrap();
    let device = world.resource::<RenderDevice>().clone();
    let buffer = world.resource::<CloudGpu>().views[&view]
        .record_buffer
        .as_ref()
        .unwrap()
        .clone();
    let layout = device.create_bind_group_layout(
        None,
        &[BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::VERTEX,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    );
    let group = device.tracked_create_bind_group(
        None,
        &layout,
        &[BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    );
    world
        .resource_mut::<CloudGpu>()
        .views
        .get_mut(&view)
        .unwrap()
        .bind_group = Some(group.clone());
    let before = crate::render_work::snapshot();
    world
        .get_mut::<ExtractedView>(view)
        .unwrap()
        .world_from_view = GlobalTransform::from_xyz(32.0, 0.0, 0.0);
    world.run_system_once(prepare_cloud_records).unwrap();
    let moved = crate::render_work::snapshot();
    let prepared = &world.resource::<CloudGpu>().views[&view];
    assert_eq!(prepared.record_buffer.as_ref().unwrap().id(), buffer.id());
    assert_eq!(prepared.bind_group.as_ref().unwrap().id(), group.id());
    assert_eq!(prepared.viewport.centre()[0], 32.0);
    assert_eq!(
        moved.delta_since(before).buffer_upload_bytes,
        u64::from(prepared.record_count) * size_of::<ViewportCloudQuad>() as u64
    );
    assert_eq!(moved.delta_since(before).bind_groups_created, 0);
    assert!(buffer.size() <= meshing::cloud_viewport::MAX_VIEWPORT_CLOUD_BYTES as u64);
    for _ in 0..10 {
        world.run_system_once(prepare_cloud_records).unwrap();
    }
    assert_eq!(
        crate::render_work::snapshot().delta_since(moved),
        Default::default()
    );
}

#[test]
fn cached_cloud_diagnostic_matches_fresh_window_without_rescanning_texture() {
    let runtime = runtime(255);
    let texture = runtime.texture(AtmosphereRole::Clouds).unwrap();
    let config = CloudRenderConfig::legacy_fancy();
    let make = |records: &[ViewportCloudQuad]| {
        CloudGeometryDiagnostic::from_viewport_layout(
            config,
            [0x41; 32],
            texture,
            records,
            CLOUD_WORLD_PERIOD as u32 * 1_000,
            (CLOUD_UNDERSIDE_Y * 1_000.0) as i32,
            (CLOUD_TOP_Y * 1_000.0) as i32,
        )
        .unwrap()
    };
    let first_viewport = CloudViewport::try_new(
        [0.0; 2],
        config.mesh_size(),
        config.grid_size(),
        false,
        false,
    )
    .unwrap();
    let next_viewport = CloudViewport::try_new(
        [-32.0, 64.0],
        config.mesh_size(),
        config.grid_size(),
        true,
        false,
    )
    .unwrap();
    let first = mesh_cloud_viewport(texture, first_viewport).unwrap();
    let next = mesh_cloud_viewport(texture, next_viewport).unwrap();
    assert_eq!(
        make(&first).with_viewport_records(&next).unwrap(),
        make(&next)
    );
}
