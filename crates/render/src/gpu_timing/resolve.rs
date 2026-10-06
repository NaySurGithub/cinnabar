use super::*;
use crate::render_work::QueueWork as _;
use bevy::render::graph::CameraDriverLabel;

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct ResolveLabel;

/// Places pass-only timestamp resolution after every view in the existing frame submission.
pub(super) fn install(world: &mut World) {
    let mut graph = world.resource_mut::<RenderGraph>();
    graph.add_node(ResolveLabel, ResolveNode);
    if graph.get_node_state(CameraDriverLabel).is_ok() {
        graph.add_node_edge(CameraDriverLabel, ResolveLabel);
    }
}

pub(super) struct ResolveNode;

impl Node for ResolveNode {
    /// Resolves pass-only timestamps; deferred per-draw spans finish after graph traversal.
    fn run<'w>(
        &self,
        _: &mut RenderGraphContext,
        context: &mut RenderContext<'w>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let _span =
            crate::render_systems::time(crate::render_systems::System::GpuTimingSubmitGpuFrame);
        if let Some(timestamps) = world.get_resource::<GpuTimestamps>()
            && !timestamps.draw_spans
        {
            timestamps.encode(context.command_encoder());
        }
        Ok(())
    }
}

/// Deferred opaque commands allocate draw spans during graph finishing, after node traversal.
pub(super) fn submit_draw_frame(
    timestamps: Option<Res<GpuTimestamps>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
) {
    let Some(timestamps) = timestamps.filter(|timestamps| timestamps.draw_spans) else {
        return;
    };
    if timestamps.release_empty_frame() {
        return;
    }
    let _span = crate::render_systems::time(crate::render_systems::System::GpuTimingSubmitGpuFrame);
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("completed GPU draw timestamp readback"),
    });
    timestamps.encode(&mut encoder);
    queue.tracked_submit([encoder.finish()]);
}

impl GpuTimestamps {
    /// Releases an unused slot after all deferred draw encoders have finished.
    fn release_empty_frame(&self) -> bool {
        if self.frame.slot.load(Ordering::Acquire) == NO_SLOT {
            return true;
        }
        if self.frame.passes.load(Ordering::Relaxed) != 0
            || self.frame.draws.load(Ordering::Relaxed) != 0
        {
            return false;
        }
        let slot = self.frame.slot.swap(NO_SLOT, Ordering::AcqRel);
        self.readbacks
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .ring
            .release(slot as usize);
        true
    }

    /// Resolves this frame's spans and maps them asynchronously; nothing waits on the GPU.
    pub(super) fn encode(&self, encoder: &mut wgpu::CommandEncoder) {
        let slot = self.frame.slot.swap(NO_SLOT, Ordering::AcqRel);
        if slot == NO_SLOT {
            return;
        }
        let passes = self.frame.passes.load(Ordering::Relaxed).min(PASS_SPANS);
        let draws = self.frame.draws.load(Ordering::Relaxed);
        let draws = if draws > DRAW_SPANS { 0 } else { draws };
        let index = slot as usize;
        let mut readbacks = self
            .readbacks
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if passes == 0 && draws == 0 {
            readbacks.ring.release(index);
            return;
        }
        let base = slot * SLOT_SPANS * 2;
        if passes > 0 {
            encoder.resolve_query_set(&self.queries, base..base + passes * 2, &self.resolve, 0);
        }
        if draws > 0 {
            let first = base + PASS_SPANS * 2;
            encoder.resolve_query_set(
                &self.queries,
                first..first + draws * 2,
                &self.resolve,
                u64::from(PASS_SPANS) * 2 * TIMESTAMP_BYTES,
            );
        }
        let target = &mut readbacks.slots[index];
        encoder.copy_buffer_to_buffer(&self.resolve, 0, &target.buffer, 0, SLOT_BYTES);
        target.passes = passes;
        target.draws = draws;
        for span in (0..passes).chain(PASS_SPANS..PASS_SPANS + draws) {
            let stage = self.frame.stages[span as usize].load(Ordering::Relaxed);
            target.stages[span as usize] = RuntimeStage::ALL[stage as usize];
        }
        let state = target.state.clone();
        encoder.map_buffer_on_submit(&target.buffer, wgpu::MapMode::Read, .., move |result| {
            state.store(
                if result.is_ok() { MAPPED } else { FAILED },
                Ordering::Release,
            );
        });
        readbacks.ring.submit(index);
    }
}
