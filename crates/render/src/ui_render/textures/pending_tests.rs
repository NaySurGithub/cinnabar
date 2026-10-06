use super::*;
use crate::ui_render::{
    UiRenderInput, UiRenderScene, UiRenderSceneResource, UiRenderStatsResource,
    prepare_ui_resources,
};
use bevy::{ecs::system::RunSystemOnce, prelude::World};
use render_model::{UI_DYNAMIC_PAGE_SIDE, UI_MODEL_ATLAS_PAGE_OFFSET, UI_MODEL_ATLAS_SIDE};
use std::sync::Arc;

/// Provides an admitted catalog and a same-identity resize that cannot finish in one call.
fn catalogs() -> (UiTextureCatalog, UiTextureCatalog) {
    let small = UiTexturePage::owned(
        [UI_DYNAMIC_PAGE_SIDE; 2],
        vec![0; (UI_DYNAMIC_PAGE_SIDE * UI_DYNAMIC_PAGE_SIDE * 4) as usize].into(),
    )
    .unwrap();
    let mut pages = vec![UiTexturePage::owned([1; 2], vec![255; 4].into()).unwrap()];
    pages.extend(vec![small; UI_MODEL_ATLAS_PAGE_OFFSET + 1]);
    let base = UiTextureCatalog::new(pages, 1).unwrap();
    let mut dynamic = base.pages()[base.dynamic_start()..].to_vec();
    dynamic[UI_MODEL_ATLAS_PAGE_OFFSET] = UiTexturePage::owned(
        [UI_MODEL_ATLAS_SIDE; 2],
        vec![7; (UI_MODEL_ATLAS_SIDE * UI_MODEL_ATLAS_SIDE * 4) as usize].into(),
    )
    .unwrap();
    let resized = base.replace_dynamic(dynamic).unwrap();
    (base, resized)
}

/// Publishes a complete draw snapshot without retaining references outside the scene.
fn publish(world: &mut World, revision: u64, catalog: UiTextureCatalog) {
    let mut scene = world.resource::<UiRenderSceneResource>().0.clone();
    scene
        .publish(
            UiRenderInput {
                revision,
                viewport_size: [64; 2],
                safe_area: [0; 4],
                vertices: Arc::from([]),
                indices: Arc::from([]),
                batches: Arc::from([]),
                textures: Arc::new(catalog),
            },
            world.resource::<UiRenderStatsResource>(),
        )
        .unwrap();
    world.insert_resource(UiRenderSceneResource(scene));
}

/// Completes the resident catalog before beginning a private replacement.
fn world_with_pending_resize() -> (World, UiTextureCatalog, UiTextureCatalog) {
    let mut world = crate::ui_render::ordered_command_tests::binding_world();
    let (base, resized) = catalogs();
    publish(&mut world, 1, base.clone());
    for _ in 0..16 {
        world.run_system_once(prepare_ui_resources).unwrap();
        if world.resource::<UiGpu>().accepted_revision == Some(1) {
            break;
        }
    }
    assert_eq!(world.resource::<UiGpu>().accepted_revision, Some(1));
    publish(&mut world, 2, resized.clone());
    world.run_system_once(prepare_ui_resources).unwrap();
    assert!(world.resource::<UiGpu>().pending_input.is_some());
    assert!(world.resource::<UiGpu>().textures.pending.is_some());
    (world, base, resized)
}

#[test]
fn canceled_ui_scene_drops_private_uploads_and_reuses_resident_textures() {
    let (mut world, base, _) = world_with_pending_resize();
    let pending = Arc::downgrade(world.resource::<UiGpu>().pending_input.as_ref().unwrap());
    world.insert_resource(UiRenderSceneResource(UiRenderScene::default()));
    world.run_system_once(prepare_ui_resources).unwrap();
    assert!(pending.upgrade().is_none());
    assert!(world.resource::<UiGpu>().textures.pending.is_none());
    assert!(world.resource::<UiGpu>().accepted_revision.is_none());

    publish(&mut world, 3, base.clone());
    let texture_ids = world
        .resource::<UiGpu>()
        .textures
        .buckets
        .iter()
        .map(|bucket| bucket.texture.id())
        .collect::<Vec<_>>();
    let before = crate::render_work::snapshot();
    world.run_system_once(prepare_ui_resources).unwrap();
    let work = crate::render_work::snapshot().delta_since(before);
    assert_eq!(
        world
            .resource::<UiGpu>()
            .textures
            .buckets
            .iter()
            .map(|bucket| bucket.texture.id())
            .collect::<Vec<_>>(),
        texture_ids
    );
    assert_eq!(work.texture_upload_bytes, 0);
    assert_eq!(world.resource::<UiGpu>().accepted_revision, Some(3));
    assert!(world.resource::<UiGpu>().textures.resident(&base));
}

#[test]
fn device_invalidation_cancels_private_ui_uploads_without_more_gpu_work() {
    let (mut world, _, _) = world_with_pending_resize();
    let (device, _) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    world.insert_resource(RenderDevice::from(device));
    let before = crate::render_work::snapshot();
    world.run_system_once(prepare_ui_resources).unwrap();
    assert_eq!(
        crate::render_work::snapshot().delta_since(before),
        Default::default()
    );
    let gpu = world.resource::<UiGpu>();
    assert!(gpu.pending_input.is_none() && gpu.textures.pending.is_none());
    assert!(gpu.accepted_revision.is_none());
}

#[test]
fn conflicting_static_identity_cancels_pending_ui_before_publication() {
    let (mut world, base, resized) = world_with_pending_resize();
    let conflicting = UiTextureCatalog::with_source_identity(
        resized.pages().to_vec(),
        resized.dynamic_start(),
        [7; 32],
    )
    .unwrap();
    let mut input = world
        .resource::<UiRenderSceneResource>()
        .input
        .as_ref()
        .unwrap()
        .as_ref()
        .clone();
    input.revision += 1;
    input.textures = Arc::new(conflicting);
    world.resource_mut::<UiRenderSceneResource>().input = Some(Arc::new(input));
    let texture_ids = world
        .resource::<UiGpu>()
        .textures
        .buckets
        .iter()
        .map(|bucket| bucket.texture.id())
        .collect::<Vec<_>>();
    let before = crate::render_work::snapshot();
    world.run_system_once(prepare_ui_resources).unwrap();
    let work = crate::render_work::snapshot().delta_since(before);
    assert_eq!(
        world
            .resource::<UiGpu>()
            .textures
            .buckets
            .iter()
            .map(|bucket| bucket.texture.id())
            .collect::<Vec<_>>(),
        texture_ids
    );
    assert_eq!(work.texture_upload_bytes, 0);
    let gpu = world.resource::<UiGpu>();
    assert!(gpu.pending_input.is_none() && gpu.textures.pending.is_none());
    assert!(gpu.accepted_revision.is_none());
    assert!(gpu.textures.resident(&base));
    assert!(matches!(
        world
            .resource::<UiRenderStatsResource>()
            .snapshot()
            .rejected_reason,
        Some(UiRenderRejectReason::TextureIdentityConflict { .. })
    ));
}
