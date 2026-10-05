//! Render schedule attribution; spans overlap where Bevy allows concurrent work.

use crate::{RuntimeStage, RuntimeStageProfiler, begin_stage_span, end_stage_span};
use bevy::{
    app::SubApp,
    prelude::*,
    render::{Render, RenderSystems},
};

/// Brackets render sets without changing the order of systems within each set.
fn bracket<const S: usize>(
    app: &mut SubApp,
    set: RenderSystems,
    previous: Option<RenderSystems>,
    next: RenderSystems,
) {
    let begin = begin_stage_span::<S>.before(set.clone());
    let begin = if let Some(previous) = previous {
        begin.after(previous)
    } else {
        begin
    };
    app.add_systems(Render, (begin, end_stage_span::<S>.after(set).before(next)));
}

/// Measures extraction, schedule work and presentation independently of GPU timestamps.
pub(crate) fn install_surface_trace(app: &mut SubApp) {
    use bevy::render::view::window::prepare_windows;
    const SURFACE: usize = RuntimeStage::SurfacePreparation as usize;
    const FRAME: usize = RuntimeStage::RenderFrame as usize;
    const SUBMISSION: usize = RuntimeStage::RenderSubmission as usize;
    app.init_resource::<crate::RuntimeStageSpans>();
    app.add_systems(
        Render,
        finish_work_frame
            .after(RenderSystems::Cleanup)
            .before(crate::runtime_profile::end_render_frame_span),
    );
    let mut extract = app.take_extract();
    app.set_extract(move |main, render| {
        let profiler = render.get_resource::<RuntimeStageProfiler>().cloned();
        let _span = profiler
            .as_ref()
            .map(|p| p.time(RuntimeStage::RenderExtract));
        if let Some(extract) = &mut extract {
            extract(main, render);
        }
    });
    bracket::<{ RuntimeStage::RenderExtractCommands as usize }>(
        app,
        RenderSystems::ExtractCommands,
        None,
        RenderSystems::PrepareAssets,
    );
    bracket::<{ RuntimeStage::RenderPrepareAssets as usize }>(
        app,
        RenderSystems::PrepareAssets,
        Some(RenderSystems::ExtractCommands),
        RenderSystems::PrepareMeshes,
    );
    bracket::<{ RuntimeStage::RenderPrepareMeshes as usize }>(
        app,
        RenderSystems::PrepareMeshes,
        Some(RenderSystems::PrepareAssets),
        RenderSystems::ManageViews,
    );
    bracket::<{ RuntimeStage::RenderManageViews as usize }>(
        app,
        RenderSystems::ManageViews,
        Some(RenderSystems::PrepareMeshes),
        RenderSystems::Queue,
    );
    bracket::<{ RuntimeStage::RenderQueue as usize }>(
        app,
        RenderSystems::Queue,
        Some(RenderSystems::ManageViews),
        RenderSystems::PhaseSort,
    );
    bracket::<{ RuntimeStage::RenderPhaseSort as usize }>(
        app,
        RenderSystems::PhaseSort,
        Some(RenderSystems::Queue),
        RenderSystems::Prepare,
    );
    bracket::<{ RuntimeStage::RenderPrepareResources as usize }>(
        app,
        RenderSystems::PrepareResources,
        Some(RenderSystems::PhaseSort),
        RenderSystems::PrepareResourcesCollectPhaseBuffers,
    );
    bracket::<{ RuntimeStage::RenderPrepareCollect as usize }>(
        app,
        RenderSystems::PrepareResourcesCollectPhaseBuffers,
        Some(RenderSystems::PrepareResources),
        RenderSystems::PrepareResourcesFlush,
    );
    bracket::<{ RuntimeStage::RenderPrepareFlush as usize }>(
        app,
        RenderSystems::PrepareResourcesFlush,
        Some(RenderSystems::PrepareResourcesCollectPhaseBuffers),
        RenderSystems::PrepareBindGroups,
    );
    bracket::<{ RuntimeStage::RenderPrepareBindGroups as usize }>(
        app,
        RenderSystems::PrepareBindGroups,
        Some(RenderSystems::PrepareResourcesFlush),
        RenderSystems::Render,
    );
    bracket::<{ RuntimeStage::RenderCleanup as usize }>(
        app,
        RenderSystems::Cleanup,
        Some(RenderSystems::Render),
        RenderSystems::PostCleanup,
    );
    app.add_systems(
        Render,
        (
            begin_stage_span::<SURFACE>.before(prepare_windows),
            end_stage_span::<SURFACE>.after(prepare_windows),
        )
            .in_set(RenderSystems::ManageViews),
    );
    app.add_systems(
        Render,
        (
            begin_stage_span::<FRAME>.before(RenderSystems::ExtractCommands),
            crate::runtime_profile::end_render_frame_span
                .after(RenderSystems::Cleanup)
                .before(RenderSystems::PostCleanup),
        ),
    );
    app.add_systems(
        Render,
        (
            begin_stage_span::<SUBMISSION>
                .after(RenderSystems::Prepare)
                .before(RenderSystems::Render),
            end_stage_span::<SUBMISSION>
                .after(RenderSystems::Render)
                .before(RenderSystems::Cleanup),
        ),
    );
}

/// Maps graph pass categories to their CPU recording spans, also on Metal.
pub(crate) fn cpu_stage(stage: RuntimeStage) -> Option<RuntimeStage> {
    Some(match stage {
        RuntimeStage::GpuShadows => RuntimeStage::CpuShadows,
        RuntimeStage::GpuOpaque => RuntimeStage::CpuOpaque,
        RuntimeStage::GpuTransparent => RuntimeStage::CpuTransparent,
        RuntimeStage::GpuUi => RuntimeStage::CpuUi,
        RuntimeStage::GpuHand => RuntimeStage::CpuHand,
        RuntimeStage::GpuPost => RuntimeStage::CpuPost,
        RuntimeStage::GpuTonemapping => RuntimeStage::CpuTonemapping,
        RuntimeStage::GpuFxaa => RuntimeStage::CpuFxaa,
        RuntimeStage::GpuBlit => RuntimeStage::CpuBlit,
        _ => return None,
    })
}

/// One completed render frame's attributable work, independent of main-thread cadence.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RenderWorkFrame {
    pub sequence: u64,
    pub work: crate::render_work::WorkSnapshot,
    pub arena_migrations: u64,
    pub arena_copy_bytes: u64,
    pub systems: [crate::render_systems::Sample; crate::render_systems::TOP_COUNT],
}

impl std::fmt::Display for RenderWorkFrame {
    /// Formats only on a rate-limited slow line, never on the fast-frame path.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let w = self.work;
        write!(
            f,
            "render_frame_id={} pipeline_specializations={} own_render_pipelines_created={} own_compute_pipelines_created={} own_shader_modules={} bind_groups={} buffer_upload_bytes={} texture_upload_bytes={} arena_migrations={} arena_copy_bytes={} readback_polls={} readback_waits={} render_systems=[",
            self.sequence,
            w.render_pipelines_queued,
            w.render_pipelines_created,
            w.compute_pipelines_created,
            w.shader_modules_created,
            w.bind_groups_created,
            w.buffer_upload_bytes,
            w.texture_upload_bytes,
            self.arena_migrations,
            self.arena_copy_bytes,
            w.readback_polls,
            w.readback_waits
        )?;
        for (index, sample) in self
            .systems
            .iter()
            .filter(|sample| sample.calls != 0)
            .enumerate()
        {
            if index != 0 {
                write!(f, ",")?;
            }
            write!(
                f,
                "{}:{:.3}/{}",
                sample.name,
                sample.nanos as f64 / 1e6,
                sample.calls
            )?;
        }
        write!(f, "]")
    }
}

/// Publishes cumulative-counter deltas once, retaining creation-and-drop work.
fn finish_work_frame(world: &mut World) {
    let counters = crate::render_work::snapshot();
    let (migrations, copied) = crate::chunk::arena_work(world);
    let previous = world
        .get_resource::<WorkBaseline>()
        .map_or(RenderWorkFrame::default(), |previous| previous.0);
    let frame = RenderWorkFrame {
        sequence: previous.sequence + 1,
        work: counters.delta_since(previous.work),
        arena_migrations: migrations.saturating_sub(previous.arena_migrations),
        arena_copy_bytes: copied.saturating_sub(previous.arena_copy_bytes),
        systems: crate::render_systems::finish_frame(),
    };
    if let Some(profiler) = world.get_resource::<RuntimeStageProfiler>() {
        profiler.record_render_work(frame);
    }
    world.insert_resource(WorkBaseline(RenderWorkFrame {
        sequence: frame.sequence,
        work: counters,
        arena_migrations: migrations,
        arena_copy_bytes: copied,
        systems: frame.systems,
    }));
}

#[derive(Resource)]
struct WorkBaseline(RenderWorkFrame);

#[cfg(test)]
#[path = "runtime_profile_render_tests.rs"]
mod tests;
