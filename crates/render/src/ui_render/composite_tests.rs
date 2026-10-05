//! Composite bindings follow GPU resource identity, independent of changing pixel contents.

use super::*;

/// Builds the actual composite layout on the validation-only backend.
fn fixture() -> (
    RenderDevice,
    BindGroupLayout,
    UiLayerTexture,
    [TextureView; 2],
) {
    let (raw, _) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(raw);
    let pipeline = UiCompositePipeline::from_world(&mut World::new());
    let layout = device.create_bind_group_layout(None, &pipeline.layout.entries);
    let layer = retained_layer(&device, Extent3d::default());
    let sources = std::array::from_fn(|_| retained_layer(&device, Extent3d::default()).view);
    let layer = UiLayerTexture::detached(layer.texture, layer.view);
    (device, layout, layer, sources)
}

#[test]
fn unchanged_composites_reuse_both_scene_sources_without_device_work() {
    let (device, layout, layer, sources) = fixture();
    let before = crate::render_work::snapshot();
    let groups = sources
        .each_ref()
        .map(|source| layer.composite_bind_group(&device, source, &layout));
    let warm = crate::render_work::snapshot();
    assert_eq!(warm.delta_since(before).bind_groups_created, 2);
    for _ in 0..100 {
        // Updating the retained layer's content never changes its resource bindings.
        layer.hold(None);
        for (source, group) in sources.iter().zip(&groups) {
            assert_eq!(
                layer.composite_bind_group(&device, source, &layout).id(),
                group.id()
            );
        }
    }
    assert_eq!(
        crate::render_work::snapshot().delta_since(warm),
        Default::default()
    );
}

#[test]
fn replacement_resources_rebuild_once_and_evict_retired_scene_bindings() {
    let (device, layout, layer, sources) = fixture();
    let original = layer.composite_bind_group(&device, &sources[0], &layout);
    let retained = layer.composite_bind_group(&device, &sources[1], &layout);
    let replacement = retained_layer(&device, Extent3d::default()).view;
    let before = crate::render_work::snapshot();
    let new = layer.composite_bind_group(&device, &replacement, &layout);
    assert_eq!(
        layer
            .composite_bind_group(&device, &replacement, &layout)
            .id(),
        new.id()
    );
    assert_eq!(
        layer
            .composite_bind_group(&device, &sources[1], &layout)
            .id(),
        retained.id()
    );
    assert_eq!(
        crate::render_work::snapshot()
            .delta_since(before)
            .bind_groups_created,
        1
    );
    assert_ne!(
        layer
            .composite_bind_group(&device, &sources[0], &layout)
            .id(),
        original.id()
    );

    let pipeline = UiCompositePipeline::from_world(&mut World::new());
    let replacement_layout = device.create_bind_group_layout(None, &pipeline.layout.entries);
    let before = crate::render_work::snapshot();
    let rebound = layer.composite_bind_group(&device, &sources[0], &replacement_layout);
    assert_eq!(
        layer
            .composite_bind_group(&device, &sources[0], &replacement_layout)
            .id(),
        rebound.id()
    );
    assert_eq!(
        crate::render_work::snapshot()
            .delta_since(before)
            .bind_groups_created,
        1
    );
}
