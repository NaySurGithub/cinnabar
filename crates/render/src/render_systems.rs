//! Fixed-capacity timings for our render systems and render graph nodes.
//! `extract_*` labels identify resource types whose changed snapshots are cloned.
//! `gpu_api_*` spans overlap their callers and include time spent waiting inside GPU APIs.
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

macro_rules! systems {
    ($($variant:ident => $name:literal),+ $(,)?) => {
        #[derive(Debug, Clone, Copy)]
        #[allow(dead_code, reason = "some render systems are platform-specific")]
        #[repr(usize)]
        pub(crate) enum System { $($variant),+ }
        const NAMES: &[&str] = &[$($name),+];
    };
}
systems! {
    ChunkPublishGraphicsRuntimeMetadata => "chunk_publish_graphics_runtime_metadata",
    GpuApiCreateBindGroup => "gpu_api_create_bind_group",
    GpuApiCreateBufferWithData => "gpu_api_create_buffer_with_data",
    GpuApiCreateTextureWithData => "gpu_api_create_texture_with_data",
    GpuApiCreateShaderModule => "gpu_api_create_shader_module",
    GpuApiCreateComputePipeline => "gpu_api_create_compute_pipeline",
    GpuApiCreateRenderPipeline => "gpu_api_create_render_pipeline",
    GpuApiPoll => "gpu_api_poll",
    GpuApiSubmit => "gpu_api_submit",
    GpuApiWriteBuffer => "gpu_api_write_buffer",
    GpuApiWriteTexture => "gpu_api_write_texture",

    WarmupItemPipeline => "warmup_item_pipeline",
    WarmupCloudPipeline => "warmup_cloud_pipeline",
    WarmupParticlePipeline => "warmup_particle_pipeline",
    WarmupHandGpu => "warmup_hand_gpu",
    WarmupMediaScreenPipeline => "warmup_media_screen_pipeline",
    WarmupOverlayPipeline => "warmup_overlay_pipeline",
    WarmupNametagPipeline => "warmup_nametag_pipeline",
    WarmupWeatherPipeline => "warmup_weather_pipeline",
    WarmupPanoramaPipeline => "warmup_panorama_pipeline",
    WarmupAtmospherePipeline => "warmup_atmosphere_pipeline",
    WarmupHandRigGpu => "warmup_hand_rig_gpu",
    WarmupLightningPipeline => "warmup_lightning_pipeline",
    WarmupPrimitivePipeline => "warmup_primitive_pipeline",
    WarmupActorPipeline => "warmup_actor_pipeline",
    WarmupBlockEntityPipeline => "warmup_block_entity_pipeline",
    WarmupEnhancedPostPipelines => "warmup_enhanced_post_pipelines",
    WarmupEnhancedShadowPipelines => "warmup_enhanced_shadow_pipelines",
    WarmupUiPipeline => "warmup_ui_pipeline",
    WarmupUiCompositePipeline => "warmup_ui_composite_pipeline",
    WarmupChunkPipeline => "warmup_chunk_pipeline",

    ExtractAtmosphereFrame => "extract_atmosphere_frame",
    ExtractLightningScene => "extract_lightning_scene",
    ExtractWeatherTextureAssets => "extract_weather_texture_assets",
    ExtractPrecipitationScene => "extract_precipitation_scene",
    ExtractWorldLighting => "extract_world_lighting",
    ExtractCloudVisibility => "extract_cloud_visibility",
    ExtractScreenOverlayScene => "extract_screen_overlay_scene",
    ExtractParticleGpuFrame => "extract_particle_gpu_frame",
    ExtractAtmosphereViewInputs => "extract_atmosphere_view_inputs",
    ExtractAtmosphereTextureAssets => "extract_atmosphere_texture_assets",
    ExtractPanoramaScene => "extract_panorama_scene",
    ExtractActorRenderFrame => "extract_actor_render_frame",
    ExtractMediaScreenScene => "extract_media_screen_scene",
    ExtractUiRenderSceneResource => "extract_ui_render_scene_resource",
    ExtractNametagSceneResource => "extract_nametag_scene_resource",
    ExtractVisibilityDiagnosticsInput => "extract_visibility_diagnostics_input",
    ExtractDroppedItemScene => "extract_dropped_item_scene",
    ExtractViewmodelScene => "extract_viewmodel_scene",
    ExtractHandRigScene => "extract_hand_rig_scene",
    ExtractChunkTextureReload => "extract_chunk_texture_reload",
    ExtractChunkUploadBudget => "extract_chunk_upload_budget",
    ExtractChunkGpuRemovalQueue => "extract_chunk_gpu_removal_queue",
    ExtractBlockEntityFrame => "extract_block_entity_frame",
    ExtractBlockSelectionFrame => "extract_block_selection_frame",
    ExtractImmediateTerrainMeshPublications => "extract_immediate_terrain_mesh_publications",
    ExtractUiGlintSettings => "extract_ui_glint_settings",
    ExtractTransparentWitnessRequest => "extract_transparent_witness_request",
    ExtractModelWitnessRequest => "extract_model_witness_request",

    ActorRenderQueueActors => "actor_render_queue_actors",
    ActorRenderPrepareActorResources => "actor_render_prepare_actor_resources",
    ActorRenderPrepareActorBindGroup => "actor_render_prepare_actor_bind_group",
    ActorRenderSubmitActorPresentedFrame => "actor_render_submit_actor_presented_frame",
    AtmosphereRenderPrepareAtmosphereUniform => "atmosphere_render_prepare_atmosphere_uniform",
    AtmosphereRenderPrepareAtmosphereTextures => "atmosphere_render_prepare_atmosphere_textures",
    AtmosphereRenderPrepareAtmosphereBindGroup => "atmosphere_render_prepare_atmosphere_bind_group",
    AtmosphereRenderQueueAtmosphere => "atmosphere_render_queue_atmosphere",
    BlockEntityGpuPrepareResources => "block_entity_gpu_prepare_resources",
    BlockEntityGpuPrepareBindGroups => "block_entity_gpu_prepare_bind_groups",
    BlockEntityGpuQueueSolid => "block_entity_gpu_queue_solid",
    BlockEntityGpuQueueOverlay => "block_entity_gpu_queue_overlay",
    BlockEntityGpuQueueOutline => "block_entity_gpu_queue_outline",
    BlockEntityGpuQueueCrack => "block_entity_gpu_queue_crack",
    BlockEntityGpuQueueAdditive => "block_entity_gpu_queue_additive",
    BlockEntityGpuQueueBlended => "block_entity_gpu_queue_blended",
    ChunkBiomeTintsExtractResource => "chunk_biome_tints_extract_resource",
    ChunkDrawQueueChunks => "chunk_draw_queue_chunks",
    ChunkDrawQueueTransparentChunks => "chunk_draw_queue_transparent_chunks",
    ChunkExtractExtractChunkRenderInstances => "chunk_extract_extract_chunk_render_instances",
    ChunkGpuBindGroupsPrepareChunkBiomeTints => "chunk_gpu_bind_groups_prepare_chunk_biome_tints",
    ChunkGpuBindGroupsPrepareChunkAnimationClock => "chunk_gpu_bind_groups_prepare_chunk_animation_clock",
    ChunkGpuBindGroupsPrepareChunkTextureAssets => "chunk_gpu_bind_groups_prepare_chunk_texture_assets",
    ChunkGpuBindGroupsPrepareChunkBindGroup => "chunk_gpu_bind_groups_prepare_chunk_bind_group",
    ChunkGpuUploadPrepareGpuChunks => "chunk_gpu_upload_prepare_gpu_chunks",
    ChunkGpuCullDirectPrepareDirectOcclusion => "chunk_gpu_cull_direct_prepare_direct_occlusion",
    ChunkGpuCullDirectSubmitDirectOcclusion => "chunk_gpu_cull_direct_submit_direct_occlusion",
    ChunkGpuCullDirectResetDirectOcclusionFrame => "chunk_gpu_cull_direct_reset_direct_occlusion_frame",
    ChunkGpuCullDirectRun => "chunk_gpu_cull_direct_run",
    ChunkGpuCullModResetGpuCullFrame => "chunk_gpu_cull_mod_reset_gpu_cull_frame",
    ChunkGpuCullNodeRun => "chunk_gpu_cull_node_run",
    ChunkGpuCullNodeRun2 => "chunk_gpu_cull_node_run_2",
    ChunkGpuCullPrepareExtractHiddenChunks => "chunk_gpu_cull_prepare_extract_hidden_chunks",
    ChunkGpuCullPreparePrepareGpuCull => "chunk_gpu_cull_prepare_prepare_gpu_cull",
    ChunkPipelineCommandsPrepareChunkIndirectBatches => "chunk_pipeline_commands_prepare_chunk_indirect_batches",
    ChunkPresentationFrameProbeSubmitPresentedFrameProbe => "chunk_presentation_frame_probe_submit_presented_frame_probe",
    ChunkTexturesExtractResource => "chunk_textures_extract_resource",
    ChunkTexturesExtractResource2 => "chunk_textures_extract_resource_2",
    ChunkTransparentGammaPassTargetPrepareGammaTargets => "chunk_transparent_gamma_pass_target_prepare_gamma_targets",
    ChunkTransparentGammaPassRun => "chunk_transparent_gamma_pass_run",
    ChunkTransparentModelPrepareTransparentModelSorts => "chunk_transparent_model_prepare_transparent_model_sorts",
    ChunkTransparentSortPreparePrepareTransparentSorts => "chunk_transparent_sort_prepare_prepare_transparent_sorts",
    CloudRenderPrepareCloudRecords => "cloud_render_prepare_cloud_records",
    CloudMeshViewport => "cloud_mesh_viewport",
    CloudGeometryDiagnostic => "cloud_geometry_diagnostic",
    CloudUploadRecords => "cloud_upload_records",
    CloudRenderPrepareCloudColour => "cloud_render_prepare_cloud_colour",
    CloudRenderPrepareCloudBindGroup => "cloud_render_prepare_cloud_bind_group",
    CloudRenderQueueClouds => "cloud_render_queue_clouds",
    DroppedItemRenderTerrainItemsBeginFrame => "dropped_item_render_terrain_items_begin_frame",
    DroppedItemRenderPrepareItems => "dropped_item_render_prepare_items",
    DroppedItemRenderPrepareBindGroup => "dropped_item_render_prepare_bind_group",
    DroppedItemRenderQueueItems => "dropped_item_render_queue_items",
    EnhancedGpuPrepareEnhancedMaterials => "enhanced_gpu_prepare_enhanced_materials",
    EnhancedGpuPrepareEnhancedViews => "enhanced_gpu_prepare_enhanced_views",
    EnhancedPostRun => "enhanced_post_run",
    EnhancedShadowsRun => "enhanced_shadows_run",
    EnhancedSnapshotRun => "enhanced_snapshot_run",
    GpuTimingUpdate => "gpu_timing_update",
    GpuTimingBeginGpuFrame => "gpu_timing_begin_gpu_frame",
    GpuTimingSubmitGpuFrame => "gpu_timing_submit_gpu_frame",
    HandRigRenderNodeRun => "hand_rig_render_node_run",
    HandRigRenderPrepare => "hand_rig_render_prepare",
    LightingPrepare => "lighting_prepare",
    LightningRenderPrepareLightningRecords => "lightning_render_prepare_lightning_records",
    LightningRenderPrepareLightningBindGroup => "lightning_render_prepare_lightning_bind_group",
    LightningRenderQueueLightning => "lightning_render_queue_lightning",
    MediaScreenPrepareMediaScreens => "media_screen_prepare_media_screens",
    MediaScreenQueueMediaScreens => "media_screen_queue_media_screens",
    ModRenderPassesPrepare => "mod_render_passes_prepare",
    ModRenderPassesRun => "mod_render_passes_run",
    ModRenderPrimitivesPrepare => "mod_render_primitives_prepare",
    ModRenderPrimitivesPrepareBindGroup => "mod_render_primitives_prepare_bind_group",
    ModRenderPrimitivesQueue => "mod_render_primitives_queue",
    ModRenderExtractResource => "mod_render_extract_resource",
    NametagRenderPrepareNametags => "nametag_render_prepare_nametags",
    NametagRenderPrepareNametagBindGroup => "nametag_render_prepare_nametag_bind_group",
    NametagRenderQueueNametags => "nametag_render_queue_nametags",
    OpaquePhaseResetOpaquePhases => "opaque_phase_reset_opaque_phases",
    PanoramaRenderSyncSceneAntialiasing => "panorama_render_sync_scene_antialiasing",
    PanoramaRenderPreparePanorama => "panorama_render_prepare_panorama",
    PanoramaRenderPrepareBindGroup => "panorama_render_prepare_bind_group",
    PanoramaRenderQueuePanorama => "panorama_render_queue_panorama",
    ParticleRenderPrepareParticleResources => "particle_render_prepare_particle_resources",
    ParticleRenderPrepareParticleBindGroup => "particle_render_prepare_particle_bind_group",
    ParticleRenderQueueParticles => "particle_render_queue_particles",
    PipelineWarmupCollectViews => "pipeline_warmup_collect_views",
    PipelineWarmupPublishReadiness => "pipeline_warmup_publish_readiness",
    PresentModeApplyDx12PresentModePolicy => "present_mode_apply_dx12_present_mode_policy",
    ScreenOverlayRenderPrepareOverlay => "screen_overlay_render_prepare_overlay",
    ScreenOverlayRenderPrepareBindGroup => "screen_overlay_render_prepare_bind_group",
    ScreenOverlayRenderQueueOverlay => "screen_overlay_render_queue_overlay",
    SurfaceLifecycleReleaseOrphanTargets => "surface_lifecycle_release_orphan_targets",
    UiRenderCompositePrepareUiLayers => "ui_render_composite_prepare_ui_layers",
    UiRenderCompositeRun => "ui_render_composite_run",
    UiRenderOverlayUpdate => "ui_render_overlay_update",
    UiRenderOverlayRun => "ui_render_overlay_run",
    UiRenderOverlayQueueUiOverlay => "ui_render_overlay_queue_ui_overlay",
    UiRenderOverlayRun2 => "ui_render_overlay_run_2",
    UiRenderTexturesPrepareUiBindGroup => "ui_render_textures_prepare_ui_bind_group",
    UiRenderWorldRun => "ui_render_world_run",
    UiRenderPrepareUiResources => "ui_render_prepare_ui_resources",
    ViewmodelRenderNodeRun => "viewmodel_render_node_run",
    ViewmodelRenderPrepare => "viewmodel_render_prepare",
    ViewmodelRenderSubmitCompletion => "viewmodel_render_submit_completion",
    WeatherRenderPrepareWeatherRecords => "weather_render_prepare_weather_records",
    WeatherRenderPrepareWeatherBindGroup => "weather_render_prepare_weather_bind_group",
    WeatherRenderQueueWeather => "weather_render_queue_weather",
}
static NANOS: [AtomicU64; NAMES.len()] = [const { AtomicU64::new(0) }; NAMES.len()];
static CALLS: [AtomicU64; NAMES.len()] = [const { AtomicU64::new(0) }; NAMES.len()];

pub(crate) struct Span {
    system: System,
    started: Instant,
}
/// Starts a named application span without allocation or profiler-resource lookups.
pub(crate) fn time(system: System) -> Span {
    Span {
        system,
        started: Instant::now(),
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        let index = self.system as usize;
        NANOS[index].fetch_add(self.started.elapsed().as_nanos() as u64, Ordering::Relaxed);
        CALLS[index].fetch_add(1, Ordering::Relaxed);
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Sample {
    pub name: &'static str,
    pub nanos: u64,
    pub calls: u64,
}
pub(crate) const TOP_COUNT: usize = 12;
/// Drains completed systems once per render frame and retains the twelve largest spans.
pub(crate) fn finish_frame() -> [Sample; TOP_COUNT] {
    let mut top = [Sample::default(); TOP_COUNT];
    for (index, name) in NAMES.iter().enumerate() {
        let sample = Sample {
            name,
            nanos: NANOS[index].swap(0, Ordering::Relaxed),
            calls: CALLS[index].swap(0, Ordering::Relaxed),
        };
        insert_sample(&mut top, sample);
    }
    top
}
/// Maintains descending attribution order with bounded stack storage.
fn insert_sample(top: &mut [Sample; TOP_COUNT], sample: Sample) {
    if sample.nanos == 0 {
        return;
    }
    if let Some(position) = top.iter().position(|old| old.nanos < sample.nanos) {
        top.copy_within(position..TOP_COUNT - 1, position + 1);
        top[position] = sample;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reports_largest_completed_systems_without_growing() {
        let mut top = [Sample::default(); TOP_COUNT];
        for index in 1..30 {
            insert_sample(
                &mut top,
                Sample {
                    name: "system",
                    nanos: index,
                    calls: 1,
                },
            );
        }
        assert_eq!(top[0].nanos, 29);
        assert_eq!(top[TOP_COUNT - 1].nanos, 30 - TOP_COUNT as u64);
    }
}

/// Times the clone performed by Bevy's standard extraction, preserving its change detection.
macro_rules! extract_resource {
    ($resource:ty, $system:ident) => {
        impl bevy::render::extract_resource::ExtractResource for $resource {
            type Source = Self;
            /// Publishes the changed resource with the same clone contract as derived extraction.
            fn extract_resource(source: &Self) -> Self {
                let _span = crate::render_systems::time(crate::render_systems::System::$system);
                <Self as Clone>::clone(source)
            }
        }
    };
}
pub(crate) use extract_resource;
