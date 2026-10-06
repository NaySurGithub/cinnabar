//! Per-pass GPU timing from timestamp queries, read back without stalling the CPU.
//!
//! Render-graph nodes use isolated timestamp-only compute passes. Draw categories inside
//! a pass are also timed when the adapter supports
//! `TIMESTAMP_QUERY_INSIDE_PASSES` and aggregate profiling is on, since per-draw timestamps
//! perturb the workload and require one resolve submission after deferred draw encoding.
//! Adapters without timestamps leave every `gpu_*` stage empty.

pub(crate) mod readback;
mod resolve;
mod timestamps;
pub(crate) use timestamps::GpuTimestamps;
#[cfg(test)]
mod tests;

use crate::{RuntimeStage, RuntimeStageProfiler};
use bevy::{
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    ecs::system::{SystemParamItem, lifetimeless::SRes},
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        render_graph::{
            EmptyNode, InternedRenderLabel, Node, NodeRunError, RenderGraph, RenderGraphContext,
            RenderLabel, SlotInfo,
        },
        render_phase::{PhaseItem, RenderCommand, RenderCommandResult, TrackedRenderPass},
        renderer::{RenderContext, RenderDevice, RenderQueue},
    },
};
use readback::{ReadbackRing, SLOTS};
use std::{
    marker::PhantomData,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU8, AtomicU32, Ordering},
    },
};

pub use readback::GpuFrameTimes;
pub(crate) use readback::decode_spans;

/// Node-level spans per frame, ahead of the draw pool so draws can never starve them.
const PASS_SPANS: u32 = 64;
/// Per-draw spans per frame; a frame that needs more drops its draw categories.
const DRAW_SPANS: u32 = 448;
const SLOT_SPANS: u32 = PASS_SPANS + DRAW_SPANS;
const SLOT_BYTES: u64 = SLOT_SPANS as u64 * 2 * TIMESTAMP_BYTES;
const TIMESTAMP_BYTES: u64 = 8;
const NO_SLOT: u32 = u32::MAX;

const PENDING: u8 = 0;
const MAPPED: u8 = 1;
const FAILED: u8 = 2;

/// Feeds `gpu_*` stages into the [`RuntimeStageProfiler`] already present in the app.
pub struct GpuTimingPlugin;

impl Plugin for GpuTimingPlugin {
    fn build(&self, app: &mut App) {
        let Some(profiler) = app.world().get_resource::<RuntimeStageProfiler>().cloned() else {
            return;
        };
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .insert_resource(profiler)
            // RenderStartup runs inside the render app after every plugin has built the graph,
            // and still reaches it after pipelined rendering moves the app to its own thread.
            .add_systems(
                RenderStartup,
                (init_gpu_timestamps, wrap_timed_nodes, resolve::install),
            )
            .add_systems(
                Render,
                (
                    begin_gpu_frame.in_set(RenderSystems::PrepareResources),
                    resolve::submit_draw_frame
                        .in_set(RenderSystems::Render)
                        .after(bevy::render::renderer::render_system),
                ),
            );
    }
}

/// The timed Core3d nodes; absent labels are skipped.
fn timed_nodes() -> Vec<(InternedRenderLabel, RuntimeStage)> {
    use crate::ui_render::{UiOverlayLabel, UiWorldLabel, overlay::UiOverlayPostLabel};
    let mut nodes = vec![
        (Node3d::MainOpaquePass.intern(), RuntimeStage::GpuOpaque),
        (
            crate::chunk::TerrainPassLabel.intern(),
            RuntimeStage::GpuOpaque,
        ),
        (
            Node3d::MainTransparentPass.intern(),
            RuntimeStage::GpuTransparent,
        ),
        (UiWorldLabel.intern(), RuntimeStage::GpuUi),
        (UiOverlayLabel.intern(), RuntimeStage::GpuUi),
        (UiOverlayPostLabel.intern(), RuntimeStage::GpuUi),
        (
            crate::viewmodel_render::HandLabel.intern(),
            RuntimeStage::GpuHand,
        ),
        (
            crate::hand_rig_render::HandRigLabel.intern(),
            RuntimeStage::GpuHand,
        ),
        (Node3d::Tonemapping.intern(), RuntimeStage::GpuTonemapping),
        (Node3d::Fxaa.intern(), RuntimeStage::GpuFxaa),
    ];
    nodes.push((Node3d::Upscaling.intern(), RuntimeStage::GpuBlit));
    #[cfg(feature = "enhanced")]
    nodes.extend(crate::enhanced::graph::timed_nodes());
    nodes
}

fn wrap_timed_nodes(world: &mut World) {
    let Some(mut graphs) = world.get_resource_mut::<RenderGraph>() else {
        return;
    };
    let Some(graph) = graphs.get_sub_graph_mut(Core3d) else {
        return;
    };
    for (label, stage) in timed_nodes() {
        let Ok(state) = graph.get_node_state_mut(label) else {
            continue;
        };
        // Replacing the node alone preserves its slots and edges.
        let inner = std::mem::replace(&mut state.node, Box::new(EmptyNode));
        state.node = Box::new(TimedNode { inner, stage });
    }
}

struct TimedNode {
    inner: Box<dyn Node>,
    stage: RuntimeStage,
}

impl Node for TimedNode {
    fn input(&self) -> Vec<SlotInfo> {
        self.inner.input()
    }

    fn output(&self) -> Vec<SlotInfo> {
        self.inner.output()
    }

    fn update(&mut self, world: &mut World) {
        let _render_system_span =
            crate::render_systems::time(crate::render_systems::System::GpuTimingUpdate);
        self.inner.update(world);
    }

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let _cpu = world
            .get_resource::<RuntimeStageProfiler>()
            .and_then(|profiler| {
                crate::runtime_profile_render::cpu_stage(self.stage)
                    .map(|stage| profiler.time(stage))
            });
        let span = world
            .get_resource::<GpuTimestamps>()
            .filter(|_| !(cfg!(target_os = "macos") && self.stage == RuntimeStage::GpuBlit))
            .and_then(|timestamps| timestamps.open_pass(self.stage));
        if let Some(span) = &span {
            mark(render_context, span, span.begin);
        }
        let result = self.inner.run(graph, render_context, world);
        if let Some(span) = &span {
            mark(render_context, span, span.begin + 1);
        }
        result
    }
}

/// Times `record` as one node-level span of `stage`, for nodes that record several passes.
pub(crate) fn timed<'w, R>(
    world: &World,
    context: &mut RenderContext<'w>,
    stage: RuntimeStage,
    record: impl FnOnce(&mut RenderContext<'w>) -> R,
) -> R {
    let span = world
        .get_resource::<GpuTimestamps>()
        .and_then(|timestamps| timestamps.open_pass(stage));
    if let Some(span) = &span {
        mark(context, span, span.begin);
    }
    let result = record(context);
    if let Some(span) = &span {
        mark(context, span, span.begin + 1);
    }
    result
}

/// Keeps timestamp attachments in their own pass so later scene passes remain unaffected.
fn mark(context: &mut RenderContext, span: &Span<'_>, index: u32) {
    crate::render_work::timestamp_marker_pass();
    context
        .command_encoder()
        .begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("gpu timestamp"),
            timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                query_set: span.queries,
                beginning_of_pass_write_index: None,
                end_of_pass_write_index: Some(index),
            }),
        });
}

/// Times draw command `C` as stage `STAGE` (a [`RuntimeStage`] index) inside its pass.
pub(crate) struct GpuDrawSpan<const STAGE: usize, C>(PhantomData<fn() -> C>);

impl<P: PhaseItem, const STAGE: usize, C: RenderCommand<P>> RenderCommand<P>
    for GpuDrawSpan<STAGE, C>
{
    type Param = (Option<SRes<GpuTimestamps>>, C::Param);
    type ViewQuery = C::ViewQuery;
    type ItemQuery = C::ItemQuery;

    fn render<'w>(
        item: &P,
        view: bevy::ecs::query::ROQueryItem<'w, '_, Self::ViewQuery>,
        entity: Option<bevy::ecs::query::ROQueryItem<'w, '_, Self::ItemQuery>>,
        (timestamps, param): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let span = timestamps
            .map(|timestamps| timestamps.into_inner())
            .and_then(|timestamps| timestamps.open_draw(RuntimeStage::ALL[STAGE]));
        if let Some(span) = &span {
            pass.wgpu_pass().write_timestamp(span.queries, span.begin);
        }
        let result = C::render(item, view, entity, param, pass);
        if let Some(span) = &span {
            pass.wgpu_pass()
                .write_timestamp(span.queries, span.begin + 1);
        }
        result
    }
}

/// A begin/end query pair; the end index is `begin + 1`.
struct Span<'a> {
    queries: &'a wgpu::QuerySet,
    begin: u32,
}

fn init_gpu_timestamps(
    mut commands: Commands,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    profiler: Res<RuntimeStageProfiler>,
) {
    match GpuTimestamps::new(&device, &queue, profiler.enabled()) {
        Some(timestamps) => commands.insert_resource(timestamps),
        None => info!("GPU timestamps unsupported by this adapter; gpu_* stages stay empty"),
    }
}

fn begin_gpu_frame(timestamps: Option<ResMut<GpuTimestamps>>, profiler: Res<RuntimeStageProfiler>) {
    let _render_system_span =
        crate::render_systems::time(crate::render_systems::System::GpuTimingBeginGpuFrame);
    let Some(mut timestamps) = timestamps else {
        return;
    };
    timestamps.begin(|frame| profiler.record_gpu_frame(frame));
}
