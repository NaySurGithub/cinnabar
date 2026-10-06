use super::*;
use crate::pipeline_warmup::{PrewarmPipelines, WarmView};

#[test]
fn vertex_lists_track_counts_without_a_device() {
    let list = VertexList::new();
    assert_eq!(list.count, 0);
    assert!(list.buffer.is_none() && list.bind_group.is_none());
}

#[test]
fn first_block_selection_reuses_its_prewarmed_pipeline() {
    let (app, _) = crate::queue_review_support::app();
    let cache = app.world().resource::<PipelineCache>();
    let mut pipeline = BlockEntityPipeline::from_world(&mut World::new());
    for msaa in [Msaa::Off, Msaa::Sample2, Msaa::Sample4, Msaa::Sample8] {
        for hdr in [false, true] {
            let mut ids = Vec::new();
            pipeline
                .prewarm(
                    cache,
                    WarmView {
                        msaa,
                        hdr,
                        enhanced: false,
                        main_format: if hdr {
                            ViewTarget::TEXTURE_FORMAT_HDR
                        } else {
                            TextureFormat::bevy_default()
                        },
                        output_format: TextureFormat::bevy_default(),
                    },
                    &mut ids,
                )
                .unwrap();
            let before = crate::render_work::snapshot();
            let outline = pipeline
                .variants
                .specialize(
                    cache,
                    BlockEntityPipelineKey {
                        mode: PipelineMode::Outline,
                        msaa,
                        hdr,
                    },
                )
                .unwrap();
            assert!(
                ids.contains(&outline),
                "block selection belongs to the loading gate"
            );
            assert_eq!(
                crate::render_work::snapshot()
                    .delta_since(before)
                    .render_pipelines_queued,
                0,
                "first targeting must not specialize a new pipeline",
            );
        }
    }
}
