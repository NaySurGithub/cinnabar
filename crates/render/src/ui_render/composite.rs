//! The gamma-space UI layer: UI quads blend in an 8-bit sRGB-encoded offscreen
//! target, then one pass composites that layer over the scene in sRGB values,
//! as vanilla's UI blends, instead of in linear light.
//!
//! The layer is retained per view and redrawn only when what it holds changes.
//! The last composite of a frame runs in place of the output blit, writing the
//! camera's output directly.
use super::*;
use bevy::{
    camera::{CameraOutputMode, ClearColor, ClearColorConfig},
    core_pipeline::{core_3d::graph::Node3d, upscaling::UpscalingNode},
    ecs::query::QueryItem,
    math::UVec2,
    render::{
        camera::ExtractedCamera,
        render_graph::{NodeRunError, RenderGraph, RenderGraphContext, ViewNode, ViewNodeRunner},
        render_resource::{
            BindGroup, BindGroupEntries, BindGroupLayout, BindGroupLayoutId, Extent3d, LoadOp,
            Operations, RenderPassColorAttachment, RenderPassDescriptor, StoreOp, Texture,
            TextureDescriptor, TextureDimension, TextureUsages, TextureView, TextureViewDescriptor,
            TextureViewId,
        },
        renderer::RenderContext,
    },
};
use std::{
    collections::HashMap,
    ops::Range,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

pub(crate) const UI_COMPOSITE_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("f5b1f3c2-7a0e-4d0c-9c5e-3a8a4b1e6d21");
/// The UI layer's format: raw bytes, so blending happens on sRGB-encoded values.
pub(crate) const UI_LAYER_FORMAT: TextureFormat = TextureFormat::Rgba8Unorm;

/// What a retained layer was drawn from; equal content means its pixels are still valid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UiLayerContent {
    pub(crate) revision: u64,
    pub(crate) skip: Option<Range<u32>>,
    pub(crate) viewport: Option<(UVec2, UVec2)>,
    pub(crate) model_depth: bool,
}

/// A view's retained UI layer for this frame.
#[derive(Component)]
pub(crate) struct UiLayerTexture {
    pub(crate) texture: Texture,
    pub(crate) view: TextureView,
    /// The drawn content and whether it encoded any batch.
    held: Arc<Mutex<Option<(UiLayerContent, bool)>>>,
    bindings: Arc<Mutex<CompositeBindings>>,
    /// Set when the frame's final layer is left for [`UiPresentNode`] to composite.
    present: AtomicBool,
    /// The sole writer of its output with no blend, so the composite can replace the blit.
    direct_output: bool,
}

impl UiLayerTexture {
    /// Reuses both scene ping-pong bindings while keeping replaced targets bounded.
    fn composite_bind_group(
        &self,
        device: &RenderDevice,
        source: &TextureView,
        layout: &BindGroupLayout,
    ) -> BindGroup {
        let key = (layout.id(), self.view.id(), source.id());
        let mut bindings = self.bindings.lock().expect("UI composite bindings lock");
        if let Some(binding) = bindings
            .slots
            .iter()
            .flatten()
            .find(|binding| binding.key == key)
        {
            return binding.group.clone();
        }
        let group = device.tracked_create_bind_group(
            "UI composite bind group",
            layout,
            &BindGroupEntries::sequential((&self.view, source)),
        );
        let next = bindings.next;
        bindings.slots[next] = Some(CompositeBinding {
            key,
            group: group.clone(),
        });
        bindings.next = (next + 1) % bindings.slots.len();
        group
    }

    /// Whether the layer already holds `content`, and if so whether that drew anything.
    pub(crate) fn holds(&self, content: &UiLayerContent) -> Option<bool> {
        let held = self.held.lock().expect("UI layer content lock");
        held.as_ref()
            .filter(|(held, _)| held == content)
            .map(|(_, encoded)| *encoded)
    }

    /// Records what the layer now holds; `None` marks it stale.
    pub(crate) fn hold(&self, content: Option<(UiLayerContent, bool)>) {
        *self.held.lock().expect("UI layer content lock") = content;
    }

    pub(crate) fn defer_present(&self) {
        self.present.store(true, Ordering::Relaxed);
    }

    #[cfg(test)]
    pub(crate) fn detached(texture: Texture, view: TextureView) -> Self {
        Self {
            texture,
            view,
            held: Arc::default(),
            bindings: Arc::default(),
            present: AtomicBool::new(false),
            direct_output: false,
        }
    }
}

struct CompositeBinding {
    key: (BindGroupLayoutId, TextureViewId, TextureViewId),
    group: BindGroup,
}

/// A view has two scene textures; retired target bindings must not accumulate after resize.
#[derive(Default)]
struct CompositeBindings {
    slots: [Option<CompositeBinding>; 2],
    next: usize,
}

struct RetainedLayer {
    texture: Texture,
    view: TextureView,
    held: Arc<Mutex<Option<(UiLayerContent, bool)>>>,
    bindings: Arc<Mutex<CompositeBindings>>,
}

/// Per-view layers kept across frames, unlike the frame-scoped texture cache.
#[derive(Default, Resource)]
pub(crate) struct UiLayerStore {
    device: Option<wgpu::Device>,
    views: HashMap<Entity, RetainedLayer>,
}

/// Present when [`UiPresentNode`] replaced the output blit, so the final composite may wait for it.
#[derive(Resource)]
pub(crate) struct UiPresentInstalled;

pub(crate) fn prepare_ui_layers(
    mut commands: Commands,
    mut store: ResMut<UiLayerStore>,
    device: Res<RenderDevice>,
    views: Query<(Entity, &ViewTarget, Option<&ExtractedCamera>)>,
) {
    let _render_system_span = crate::render_systems::time(
        crate::render_systems::System::UiRenderCompositePrepareUiLayers,
    );
    if store.device.as_ref() != Some(device.wgpu_device()) {
        store.views.clear();
        store.device = Some(device.wgpu_device().clone());
    }
    store.views.retain(|view, _| views.contains(*view));
    let mut writers = HashMap::<_, usize>::new();
    for (_, target, _) in &views {
        *writers.entry(target.out_texture().id()).or_default() += 1;
    }
    for (entity, target, camera) in &views {
        let size = target.main_texture().size();
        let size = Extent3d {
            width: size.width,
            height: size.height,
            depth_or_array_layers: 1,
        };
        let layer = store
            .views
            .entry(entity)
            .and_modify(|layer| {
                if layer.texture.size() != size {
                    *layer = retained_layer(&device, size);
                }
            })
            .or_insert_with(|| retained_layer(&device, size));
        let unblended = camera.is_none_or(|camera| {
            matches!(
                camera.output_mode,
                CameraOutputMode::Write {
                    blend_state: None,
                    ..
                }
            )
        });
        commands.entity(entity).insert(UiLayerTexture {
            texture: layer.texture.clone(),
            view: layer.view.clone(),
            held: Arc::clone(&layer.held),
            bindings: Arc::clone(&layer.bindings),
            present: AtomicBool::new(false),
            direct_output: unblended && writers[&target.out_texture().id()] == 1,
        });
    }
}

fn retained_layer(device: &RenderDevice, size: Extent3d) -> RetainedLayer {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("retained gamma-space UI layer"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: UI_LAYER_FORMAT,
        usage: TextureUsages::RENDER_ATTACHMENT | TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor::default());
    RetainedLayer {
        texture,
        view,
        held: Arc::default(),
        bindings: Arc::default(),
    }
}

#[derive(Resource)]
pub(crate) struct UiCompositePipeline {
    pub(crate) layout: BindGroupLayoutDescriptor,
    variants: Variants<RenderPipeline, UiCompositeSpecializer>,
}

struct UiCompositeSpecializer;

/// The composite's colour target: the view's main texture or its output.
#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
pub(crate) struct UiCompositeKey {
    pub(crate) format: TextureFormat,
}

impl Specializer<RenderPipeline> for UiCompositeSpecializer {
    type Key = UiCompositeKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        crate::render_work::specialization();
        let target = descriptor.fragment.as_mut().unwrap().targets[0]
            .as_mut()
            .unwrap();
        target.format = key.format;
        Ok(key)
    }
}

impl FromWorld for UiCompositePipeline {
    fn from_world(_world: &mut World) -> Self {
        let texture = |binding| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Float { filterable: false },
                view_dimension: TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout =
            BindGroupLayoutDescriptor::new("UI composite layout", &[texture(0), texture(1)]);
        let descriptor = RenderPipelineDescriptor {
            label: Some("gamma-space UI composite".into()),
            layout: vec![layout.clone()],
            vertex: VertexState {
                shader: UI_COMPOSITE_SHADER_HANDLE,
                entry_point: Some("composite_vertex".into()),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: UI_COMPOSITE_SHADER_HANDLE,
                entry_point: Some("composite_fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::bevy_default(),
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            ..default()
        };
        Self {
            layout,
            variants: Variants::new(UiCompositeSpecializer, descriptor),
        }
    }
}

impl UiCompositePipeline {
    pub(crate) fn specialize(
        &mut self,
        cache: &PipelineCache,
        key: UiCompositeKey,
    ) -> Option<CachedRenderPipelineId> {
        self.variants.specialize(cache, key).ok()
    }
}

impl crate::pipeline_warmup::PrewarmPipelines for UiCompositePipeline {
    const PROFILE: crate::render_systems::System =
        crate::render_systems::System::WarmupUiCompositePipeline;

    /// Warms both offscreen composition and the surface's actual output format.
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        for format in [view.main_format, view.output_format] {
            ids.push(self.variants.specialize(cache, UiCompositeKey { format })?);
        }
        Ok(())
    }
}

/// A view's composite pipelines into its main texture and into its output.
#[derive(Clone, Copy)]
pub(crate) struct CompositePipelines {
    pub(crate) main: CachedRenderPipelineId,
    pub(crate) output: Option<CachedRenderPipelineId>,
}

/// Composite `layer` over the view's scene into its next main texture.
pub(crate) fn composite(
    context: &mut RenderContext,
    target: &ViewTarget,
    layer: &UiLayerTexture,
    pipeline: &RenderPipeline,
    layout: &BindGroupLayout,
) {
    let write = target.post_process_write();
    let destination = RenderPassColorAttachment {
        view: write.destination,
        depth_slice: None,
        resolve_target: None,
        ops: Operations {
            load: LoadOp::Clear(Default::default()),
            store: StoreOp::Store,
        },
    };
    let bind_group = layer.composite_bind_group(context.render_device(), write.source, layout);
    encode_composite(context, bind_group, destination, None, pipeline);
}

/// Records the composite with a retained binding for the source texture pair.
fn encode_composite(
    context: &mut RenderContext,
    bind_group: BindGroup,
    destination: RenderPassColorAttachment,
    scissor: Option<(UVec2, UVec2)>,
    pipeline: &RenderPipeline,
) {
    let attachments = [Some(destination)];
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("gamma-space UI composite"),
        color_attachments: &attachments,
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    if let Some((position, size)) = scissor {
        pass.set_scissor_rect(position.x, position.y, size.x, size.y);
    }
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, &bind_group, &[]);
    pass.draw(0..3, 0..1);
}

/// Replaces the output blit: composites a deferred final layer straight into the
/// camera output, or falls back to compositing into the main texture and blitting.
#[derive(Default)]
pub(crate) struct UiPresentNode(UpscalingNode);

impl ViewNode for UiPresentNode {
    type ViewQuery = (
        <UpscalingNode as ViewNode>::ViewQuery,
        Option<&'static UiLayerTexture>,
    );

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        context: &mut RenderContext<'w>,
        (blit, layer): QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let _render_system_span =
            crate::render_systems::time(crate::render_systems::System::UiRenderCompositeRun);
        let (target, _, camera) = blit;
        let pending = layer.filter(|layer| layer.present.load(Ordering::Relaxed));
        let pipelines = world
            .get_resource::<UiGpu>()
            .and_then(|gpu| gpu.composite_pipelines.get(&graph.view_entity()).copied());
        let (Some(layer), Some(pipelines), Some(cache), Some(composite_pipeline)) = (
            pending,
            pipelines,
            world.get_resource::<PipelineCache>(),
            world.get_resource::<UiCompositePipeline>(),
        ) else {
            return self.0.run(graph, context, blit, world);
        };
        let layout = cache.get_bind_group_layout(&composite_pipeline.layout);
        if layer.direct_output
            && let Some(pipeline) = pipelines
                .output
                .and_then(|id| cache.get_render_pipeline(id))
        {
            let clear = match camera.map(|camera| &camera.output_mode) {
                Some(CameraOutputMode::Write { clear_color, .. }) => *clear_color,
                _ => ClearColorConfig::Default,
            };
            let clear = match clear {
                ClearColorConfig::Default => Some(world.resource::<ClearColor>().0.into()),
                ClearColorConfig::Custom(color) => Some(color.into()),
                ClearColorConfig::None => None,
            };
            let scissor = camera
                .and_then(|camera| camera.viewport.as_ref())
                .map(|viewport| (viewport.physical_position, viewport.physical_size));
            let bind_group = layer.composite_bind_group(
                context.render_device(),
                target.main_texture_view(),
                &layout,
            );
            encode_composite(
                context,
                bind_group,
                target.out_texture_color_attachment(clear),
                scissor,
                pipeline,
            );
            return Ok(());
        }
        if let Some(pipeline) = cache.get_render_pipeline(pipelines.main) {
            composite(context, target, layer, pipeline, &layout);
        }
        self.0.run(graph, context, blit, world)
    }
}

#[cfg(test)]
#[path = "composite_tests.rs"]
mod tests;

/// Swaps the output blit for [`UiPresentNode`], keeping every installed edge.
pub(crate) fn install_present_node(world: &mut World) {
    if world.contains_resource::<UiPresentInstalled>() {
        return;
    }
    let runner = ViewNodeRunner::new(UiPresentNode::default(), world);
    let Some(mut graphs) = world.get_resource_mut::<RenderGraph>() else {
        return;
    };
    let Some(graph) = graphs.get_sub_graph_mut(bevy::core_pipeline::core_3d::graph::Core3d) else {
        return;
    };
    let Ok(node) = graph.get_node_state_mut(Node3d::Upscaling) else {
        return;
    };
    node.node = Box::new(runner);
    node.type_name = std::any::type_name::<ViewNodeRunner<UiPresentNode>>();
    world.insert_resource(UiPresentInstalled);
}
