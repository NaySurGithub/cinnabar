//! Compiles the current view's built-in variants before world presentation.

use std::{
    any::TypeId,
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use bevy::{
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        render_resource::{
            CachedPipelineState, CachedRenderPipelineId, PipelineCache, TextureFormat,
        },
        view::{ExtractedView, ViewTarget},
    },
    shader::PipelineCacheError,
};

/// Shared loading gate; a missing view or pipeline keeps world presentation held.
#[derive(Resource, Clone, Default)]
pub struct PipelineWarmupReadiness(Arc<AtomicBool>);

impl PipelineWarmupReadiness {
    /// True only after all registered variants for the current views are usable.
    pub fn ready(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Only view properties that change our built-in pipeline descriptors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WarmView {
    pub msaa: Msaa,
    pub hdr: bool,
    pub enhanced: bool,
    pub main_format: TextureFormat,
    pub output_format: TextureFormat,
}

pub(crate) type WarmupIds = Vec<CachedRenderPipelineId>;

/// Implemented beside each private owner so drawing and warmup share its cache.
pub(crate) trait PrewarmPipelines: Resource {
    /// Distinguishes this owner's warmup system in render-frame attribution.
    const PROFILE: crate::render_systems::System;
    /// Queues every built-in material mode without requiring visible content.
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: WarmView,
        ids: &mut WarmupIds,
    ) -> Result<(), BevyError>;
}

#[derive(Default)]
struct OwnerPipelines {
    views: Vec<WarmView>,
    ids: WarmupIds,
    enumerated: bool,
    error_reported: bool,
}

#[derive(Resource, Default)]
struct WarmupRegistry {
    owners: HashMap<TypeId, OwnerPipelines>,
    views: Vec<WarmView>,
    failed: HashSet<CachedRenderPipelineId>,
}

#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
enum WarmupSet {
    Views,
    Queue,
}

/// Registers an owner once, even when its plugin retries installation at finish.
pub(crate) fn register<T: PrewarmPipelines>(app: &mut App) {
    app.init_resource::<PipelineWarmupReadiness>();
    let shared = app.world().resource::<PipelineWarmupReadiness>().clone();
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    if !render_app.world().contains_resource::<WarmupRegistry>() {
        render_app
            .insert_resource(shared)
            .init_resource::<WarmupRegistry>()
            .configure_sets(
                Render,
                (WarmupSet::Views, WarmupSet::Queue)
                    .chain()
                    .after(RenderSystems::ManageViews)
                    .before(RenderSystems::Queue),
            )
            .add_systems(Render, collect_views.in_set(WarmupSet::Views))
            .add_systems(Render, publish_readiness.in_set(RenderSystems::Cleanup));
    }
    let mut registry = render_app.world_mut().resource_mut::<WarmupRegistry>();
    if let std::collections::hash_map::Entry::Vacant(entry) =
        registry.owners.entry(TypeId::of::<T>())
    {
        entry.insert(OwnerPipelines::default());
        render_app.add_systems(Render, queue_owner::<T>.in_set(WarmupSet::Queue));
    }
}

/// Reuses collection capacity while collecting the current camera pipeline configurations.
fn collect_views(
    views: Query<
        (
            &ExtractedView,
            &Msaa,
            &ViewTarget,
            Option<&crate::EnhancedRendering>,
        ),
        With<Camera3d>,
    >,
    mut registry: ResMut<WarmupRegistry>,
) {
    let _render_system_span =
        crate::render_systems::time(crate::render_systems::System::PipelineWarmupCollectViews);
    registry.views.clear();
    for (view, msaa, target, enhanced) in &views {
        let key = WarmView {
            msaa: *msaa,
            hdr: view.hdr,
            enhanced: enhanced.is_some(),
            main_format: target.main_texture_format(),
            output_format: target.out_texture_view_format(),
        };
        if !registry.views.contains(&key) {
            registry.views.push(key);
        }
    }
}

/// Specializes once per view configuration; normal gameplay only checks readiness.
fn queue_owner<T: PrewarmPipelines>(
    owner: Option<ResMut<T>>,
    cache: Res<PipelineCache>,
    mut registry: ResMut<WarmupRegistry>,
) {
    let _render_system_span = crate::render_systems::time(T::PROFILE);
    let WarmupRegistry { views, owners, .. } = &mut *registry;
    let state = owners
        .get_mut(&TypeId::of::<T>())
        .expect("registered pipeline owner");
    state.enumerated = false;
    let Some(mut owner) = owner else {
        return;
    };
    for view in views.iter().copied() {
        if state.views.contains(&view) {
            continue;
        }
        if let Err(error) = owner.prewarm(&cache, view, &mut state.ids) {
            if !state.error_reported {
                error!("pipeline prewarm {}: {error}", std::any::type_name::<T>());
                state.error_reported = true;
            }
            return;
        }
        state.views.push(view);
    }
    state.enumerated = !views.is_empty();
}

/// Missing shader imports retry normally; terminal compilation errors remain visible.
fn pipeline_ready(
    cache: &PipelineCache,
    id: CachedRenderPipelineId,
    failed: &mut HashSet<CachedRenderPipelineId>,
) -> bool {
    match cache.get_render_pipeline_state(id) {
        CachedPipelineState::Ok(_) => true,
        CachedPipelineState::Err(
            PipelineCacheError::ShaderNotLoaded(_)
            | PipelineCacheError::ShaderImportNotYetAvailable,
        ) => false,
        CachedPipelineState::Err(error) => {
            if failed.insert(id) {
                error!("pipeline prewarm {id:?}: {error}");
            }
            false
        }
        _ => false,
    }
}

/// Opens the world gate only after every application pipeline variant is ready.
fn publish_readiness(
    cache: Res<PipelineCache>,
    mut registry: ResMut<WarmupRegistry>,
    shared: Res<PipelineWarmupReadiness>,
) {
    let _render_system_span =
        crate::render_systems::time(crate::render_systems::System::PipelineWarmupPublishReadiness);
    shared.0.store(
        registered_pipelines_ready(&cache, &mut registry),
        Ordering::Release,
    );
}

/// Requires every owner to enumerate and every returned ID to finish compilation.
fn registered_pipelines_ready(cache: &PipelineCache, registry: &mut WarmupRegistry) -> bool {
    let WarmupRegistry { owners, failed, .. } = registry;
    let mut ready = !owners.is_empty();
    for owner in owners.values() {
        ready &= owner.enumerated;
        for &id in &owner.ids {
            ready &= pipeline_ready(&cache, id, failed);
        }
    }
    ready
}

#[cfg(test)]
#[path = "pipeline_warmup/tests.rs"]
mod tests;
