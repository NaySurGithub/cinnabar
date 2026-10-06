//! Device-bounded immutable neutral artwork, replaced whole when its identity changes.
use super::*;
use crate::actor::{
    ActorArtworkPageId, ActorArtworkPages, MAX_ACTOR_GPU_PIXEL_BYTES, MAX_ACTOR_TEXTURE_PAGES,
    gpu::ActorDrawSpan,
};

pub(super) struct GpuArtworkPage {
    _texture: Texture,
    pub view: TextureView,
    pub bind_group: Option<BindGroup>,
    pub color_mask: bool,
    pub multitexture: bool,
}

#[derive(Default)]
pub(super) struct GpuArtwork {
    identity: Option<([u8; 32], [u8; 32])>,
    pub pages: Vec<GpuArtworkPage>,
    rejected: bool,
    pending: Option<PendingArtwork>,
}

struct PendingArtwork {
    identity: ([u8; 32], [u8; 32]),
    pages: Vec<GpuArtworkPage>,
    source: Option<crate::actor::ActorTexturePage>,
    cursor: crate::texture_upload::TextureUploadCursor,
}

impl GpuArtwork {
    /// Uploads one allocation and a bounded texel batch without exposing partial generations.
    pub fn prepare(
        &mut self,
        pages: &ActorArtworkPages,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) -> bool {
        let identity = (pages.identity, pages.entity_identity);
        if self.identity == Some(identity) {
            self.pending = None;
            return !self.rejected;
        }
        if pages.identity == [0; 32] {
            self.pages.clear();
            self.identity = None;
            self.pending = None;
            self.rejected = false;
            return true;
        }
        let limits = device.limits();
        if self
            .pending
            .as_ref()
            .is_none_or(|pending| pending.identity != identity)
        {
            let bytes = pages
                .pages
                .iter()
                .try_fold(crate::actor::PLAYER_SKIN_BUDGET_BYTES, |total, page| {
                    total.checked_add(page.rgba8.len())
                });
            if pages.pages.len() + 1 > MAX_ACTOR_TEXTURE_PAGES
                || bytes.is_none_or(|bytes| bytes > MAX_ACTOR_GPU_PIXEL_BYTES)
                || pages
                    .pages
                    .iter()
                    .any(|page| page.layers > limits.max_texture_array_layers)
            {
                self.pending = None;
                self.identity = Some(identity);
                self.pages.clear();
                self.rejected = true;
                bevy::log::warn!(
                    "neutral actor artwork exceeds device limits; generic artwork unavailable"
                );
                return false;
            }
            self.pending = Some(PendingArtwork {
                identity,
                pages: Vec::with_capacity(pages.pages.len()),
                source: None,
                cursor: Default::default(),
            });
        }
        let pending = self.pending.as_mut().expect("validated artwork generation");
        let mut remaining = crate::texture_upload::TEXTURE_UPLOAD_BATCH_BYTES;
        let mut allocated = false;
        while pending.pages.len() < pages.pages.len() || pending.source.is_some() {
            if pending.source.is_none() {
                if allocated {
                    return false;
                }
                let page = pages.pages[pending.pages.len()]
                    .fit_within(limits.max_texture_dimension_2d)
                    .into_owned();
                let texture = device.create_texture(&TextureDescriptor {
                    label: Some("immutable neutral binary-alpha actor page"),
                    size: Extent3d {
                        width: u32::from(page.width),
                        height: u32::from(page.height),
                        depth_or_array_layers: page.layers,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: TextureFormat::Rgba8UnormSrgb,
                    usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let view = texture.create_view(&TextureViewDescriptor {
                    dimension: Some(TextureViewDimension::D2Array),
                    ..default()
                });
                pending.pages.push(GpuArtworkPage {
                    _texture: texture,
                    view,
                    bind_group: None,
                    color_mask: page.color_mask,
                    multitexture: page.multitexture,
                });
                pending.source = Some(page);
                pending.cursor = Default::default();
                allocated = true;
            }
            let page = pending.source.as_ref().expect("allocated actor page");
            let size = [u32::from(page.width), u32::from(page.height), page.layers];
            while let Some(slice) = pending.cursor.take(size, 4, &mut remaining) {
                queue.tracked_write_texture(
                    TexelCopyTextureInfo {
                        texture: &pending
                            .pages
                            .last()
                            .expect("allocated actor texture")
                            ._texture,
                        mip_level: 0,
                        origin: bevy::render::render_resource::Origin3d {
                            x: 0,
                            y: slice.row,
                            z: slice.layer,
                        },
                        aspect: default(),
                    },
                    &page.rgba8[slice.bytes],
                    TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(size[0] * 4),
                        rows_per_image: Some(size[1]),
                    },
                    Extent3d {
                        width: size[0],
                        height: slice.rows,
                        depth_or_array_layers: slice.layers,
                    },
                );
            }
            if !pending.cursor.complete(page.layers) {
                return false;
            }
            pending.source = None;
        }
        let pending = self.pending.take().expect("completed actor artwork");
        self.pages = pending.pages;
        self.identity = Some(identity);
        self.rejected = false;
        true
    }

    /// Pending artwork retains the previous complete GPU actor frame.
    pub(super) fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    pub fn invalidate_bindings(&mut self) {
        for page in &mut self.pages {
            page.bind_group = None;
        }
    }
}

/// Opaque instances share runs; blended instances retain individual sort positions.
pub(super) fn draw_spans(
    pages: &[ActorArtworkPageId],
    instances: &[crate::actor::ActorGpuInstance],
    geometry: &[crate::actor::ActorRigGeometrySpan],
) -> Vec<ActorDrawSpan> {
    let mut spans: Vec<ActorDrawSpan> = Vec::new();
    let mut last_geometry = None;
    for (index, (page, instance)) in pages.iter().copied().zip(instances).enumerate() {
        if let Some(span) = spans.last_mut().filter(|span| {
            span.page == page
                && !crate::actor::material::state(instance.material)
                    .is_some_and(|state| state.blend)
                && last_geometry == Some(instance.geometry_id)
                && span.material == instance.material
        }) {
            span.count += 1;
        } else {
            spans.push(ActorDrawSpan {
                material: instance.material,
                page,
                first: index as u32,
                count: 1,
                vertex_count: geometry
                    .get(instance.geometry_id as usize)
                    .map_or(0, |span| span.vertex_count),
            });
            last_geometry = Some(instance.geometry_id);
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn artwork_uploads_are_bounded_and_publish_only_complete_generations() {
        use bevy::render::renderer::WgpuWrapper;
        use std::sync::Arc;

        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let device = RenderDevice::from(device);
        let queue = RenderQueue(Arc::new(WgpuWrapper::new(queue)));
        let page = |side| crate::actor::ActorTexturePage {
            width: side,
            height: side,
            layers: 2,
            rgba8: vec![255; usize::from(side).pow(2) * 4 * 2].into(),
            color_mask: false,
            multitexture: false,
        };
        let mut pages = ActorArtworkPages::default();
        pages.identity = [1; 32];
        pages.pages = Arc::from([page(4)]);
        let mut gpu = GpuArtwork::default();
        assert!(gpu.prepare(&pages, &device, &queue));
        let old_id = gpu.pages[0]._texture.id();
        pages.entity_identity = [2; 32];
        pages.pages = Arc::from([page(1024), page(8), page(16)]);
        let mut allocated = 0;
        for _ in 0..32 {
            let before = crate::render_work::snapshot();
            let ready = gpu.prepare(&pages, &device, &queue);
            let work = crate::render_work::snapshot().delta_since(before);
            assert!(
                work.texture_upload_bytes
                    <= crate::texture_upload::TEXTURE_UPLOAD_BATCH_BYTES as u64
            );
            assert_eq!(work.readback_waits, 0);
            if ready {
                assert_eq!(gpu.pages.len(), 3);
                assert_ne!(gpu.pages[0]._texture.id(), old_id);
                let before = crate::render_work::snapshot();
                let ids = gpu
                    .pages
                    .iter()
                    .map(|page| page._texture.id())
                    .collect::<Vec<_>>();
                assert!(gpu.prepare(&pages, &device, &queue));
                assert_eq!(
                    gpu.pages
                        .iter()
                        .map(|page| page._texture.id())
                        .collect::<Vec<_>>(),
                    ids
                );
                assert_eq!(
                    crate::render_work::snapshot().delta_since(before),
                    Default::default()
                );
                return;
            }
            assert_eq!(gpu.pages[0]._texture.id(), old_id);
            let next = gpu.pending.as_ref().unwrap().pages.len();
            assert!(next - allocated <= 1);
            allocated = next;
        }
        panic!("bounded artwork upload must make progress");
    }

    #[test]
    fn blended_instances_keep_separate_sortable_spans_while_opaque_instances_batch() {
        let material = |blend| {
            assets::EntityRenderMaterial::Default.word(Some(assets::EntityRenderMaterialState {
                blend,
                ..Default::default()
            }))
        };
        let instances = [false, false, true, true].map(|blend| crate::actor::ActorGpuInstance {
            material: material(blend),
            ..Default::default()
        });
        let spans = draw_spans(
            &[1; 4],
            &instances,
            &[crate::actor::ActorRigGeometrySpan {
                first_vertex: 0,
                vertex_count: 36,
            }],
        );
        assert_eq!(
            spans
                .iter()
                .map(|span| (span.first, span.count))
                .collect::<Vec<_>>(),
            [(0, 2), (2, 1), (3, 1)]
        );
        assert!(spans.iter().all(|span| span.vertex_count == 36));
    }

    #[test]
    fn high_artwork_page_ids_survive_paged_build_and_draw_spans() {
        use crate::actor::{
            ActorRenderIdentity, ActorRigFrameBuilder, ActorRigRenderInput, ActorRigRoute,
            ActorRigSubmission,
        };
        let textures = (1..=u16::from(u8::MAX) + 2)
            .map(|height| assets::ActorTexture {
                source: u32::from(height),
                width: 1,
                height,
                pixel_sha256: [1; 32],
                rgba8: vec![255; usize::from(height) * 4].into(),
            })
            .collect::<Vec<_>>();
        let bindings = [0, textures.len() as u32 - 1].map(|texture| assets::ActorArtworkBinding {
            rig: texture,
            geometry_candidate: texture,
            entity_symbol: texture,
            geometry: 0,
            render_controller: 0,
            texture,
            material: "entity".into(),
            pose_mode: assets::ActorPoseMode::CompiledLiteral,
        });
        let pages = ActorArtworkPages::default().with_pack_artwork(&textures, &bindings);
        let low = pages.route(render_model::pack_rig_id(0)).unwrap();
        let high = pages
            .route(render_model::pack_rig_id(textures.len() as u32 - 1))
            .expect("valid high page retains its route");
        assert!(usize::from(high.page()) > usize::from(u8::MAX));
        let rig = render_model::pack_rig_id(0);
        let geometry =
            render_model::ActorRigGeometry::synthetic_cuboid(rig, [0.0; 3], [1.0; 3], 1).unwrap();
        let mut builder = ActorRigFrameBuilder::new([geometry]).unwrap();
        let actor = |runtime_id| ActorRigSubmission {
            material: Default::default(),
            culling_bounds: Default::default(),
            input: ActorRigRenderInput {
                identity: ActorRenderIdentity {
                    session_id: 1,
                    dimension: 0,
                    runtime_id,
                    spawn_revision: 1,
                    ingress_sequence: 1,
                    source_tick: Some(1),
                    movement_revision: 1,
                    pose_generation: 1,
                    layer: crate::ACTOR_LAYER_BODY,
                },
                rig,
                previous_bones: std::sync::Arc::from([render_model::RenderBoneTransform {
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    translation_scale: [0.0, 0.0, 0.0, 1.0],
                    axis_scale: render_model::UNIT_AXIS_SCALE,
                }]),
                current_bones: std::sync::Arc::from([render_model::RenderBoneTransform {
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    translation_scale: [0.0, 0.0, 0.0, 1.0],
                    axis_scale: render_model::UNIT_AXIS_SCALE,
                }]),
                completed_tick: 1,
                reset_generation: 1,
            },
            world_from_actor: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            texture_layer: 0,
            route: ActorRigRoute::Compiled,
            tint: 0,
            uv_anim: crate::IDENTITY_UV_ANIM,
            light: 0,
            overlay_rgba8: 0,
        };
        let page_of = |identity: &ActorRenderIdentity| {
            if identity.runtime_id == 2 {
                low.page()
            } else {
                high.page()
            }
        };
        let frame = builder.build_paged(0.5, None, [actor(1), actor(2), actor(3)], page_of);
        assert_eq!(frame.instances.len(), 3);
        let instance_pages = frame
            .manifest
            .iter()
            .map(|entry| page_of(&entry.identity))
            .collect::<Vec<_>>();
        let spans = draw_spans(&instance_pages, &frame.instances, &frame.geometry_spans);
        assert_eq!(spans.len(), 2);
        assert_eq!((spans[0].page, spans[0].count), (low.page(), 1));
        assert_eq!((spans[1].page, spans[1].count), (high.page(), 2));
    }

    #[test]
    fn coplanar_dissolve_passes_have_separate_ordered_draw_spans() {
        let instances = [
            assets::EntityRenderMaterial::DissolveDepth,
            assets::EntityRenderMaterial::DissolveColor,
        ]
        .map(|material| crate::actor::ActorGpuInstance {
            material: material as u32,
            ..Default::default()
        });
        let spans = draw_spans(
            &[1, 1],
            &instances,
            &[crate::actor::ActorRigGeometrySpan {
                first_vertex: 0,
                vertex_count: 36,
            }],
        );
        assert_eq!(spans.len(), 2);
        assert_eq!(
            (spans[0].first, spans[0].count, spans[0].material),
            (0, 1, instances[0].material)
        );
        assert_eq!(
            (spans[1].first, spans[1].count, spans[1].material),
            (1, 1, instances[1].material)
        );
    }
    #[test]
    fn spans_split_on_page_and_geometry_and_carry_exact_vertex_counts() {
        let instance = |geometry_id| crate::actor::ActorGpuInstance {
            geometry_id,
            ..Default::default()
        };
        let geometry = [
            crate::actor::ActorRigGeometrySpan {
                first_vertex: 0,
                vertex_count: 36,
            },
            crate::actor::ActorRigGeometrySpan {
                first_vertex: 36,
                vertex_count: 3024,
            },
        ];
        let spans = draw_spans(
            &[0, 1, 1, 1, 2],
            &[
                instance(0),
                instance(1),
                instance(1),
                instance(0),
                instance(0),
            ],
            &geometry,
        );
        let span = |page, first, count, vertex_count| ActorDrawSpan {
            material: 0,
            page,
            first,
            count,
            vertex_count,
        };
        assert_eq!(
            spans,
            vec![
                span(0, 0, 1, 36),
                span(1, 1, 2, 3024),
                span(1, 3, 1, 36),
                span(2, 4, 1, 36),
            ]
        );
    }

    // A tall flipbook past the device limit draws downscaled instead of blanking every page.
    #[test]
    fn a_page_past_the_device_limit_uploads_downscaled() {
        use bevy::render::renderer::WgpuWrapper;
        use std::sync::Arc;
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let device = RenderDevice::from(device);
        let queue = RenderQueue(Arc::new(WgpuWrapper::new(queue)));
        let side = device.limits().max_texture_dimension_2d;
        let height = u16::try_from(side * 2).unwrap();
        let mut pages = ActorArtworkPages::default();
        pages.identity = [1; 32];
        pages.pages = Arc::from([crate::actor::ActorTexturePage {
            width: 2,
            height,
            layers: 1,
            rgba8: vec![255; 2 * usize::from(height) * 4].into(),
            color_mask: true,
            multitexture: false,
        }]);
        let mut gpu = GpuArtwork::default();
        assert!(gpu.prepare(&pages, &device, &queue));
        assert_eq!(gpu.pages.len(), 1);
        assert!(gpu.pages[0].color_mask);
    }

    #[test]
    fn replacement_artwork_supersedes_the_previous_generation() {
        use bevy::render::renderer::WgpuWrapper;
        use std::sync::Arc;
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let device = RenderDevice::from(device);
        let queue = RenderQueue(Arc::new(WgpuWrapper::new(queue)));
        let mut pages = ActorArtworkPages::default();
        pages.identity = [1; 32];
        pages.entity_identity = [2; 32];
        pages.pages = Arc::from([crate::actor::ActorTexturePage {
            width: 16,
            height: 16,
            layers: 1,
            rgba8: vec![255; 16 * 16 * 4].into(),
            color_mask: false,
            multitexture: false,
        }]);
        let mut gpu = GpuArtwork::default();
        assert!(gpu.prepare(&pages, &device, &queue));
        assert_eq!(gpu.pages.len(), 1);
        assert!(gpu.prepare(&pages, &device, &queue));
        pages.entity_identity = [3; 32];
        assert!(gpu.prepare(&pages, &device, &queue));
        assert_eq!(gpu.pages.len(), 1);
        assert_eq!(gpu.identity, Some(([1; 32], [3; 32])));
        pages.identity = [0; 32];
        assert!(gpu.prepare(&pages, &device, &queue));
        assert!(gpu.pages.is_empty() && gpu.identity.is_none());
    }
}
