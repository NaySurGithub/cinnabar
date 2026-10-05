//! Work assertions deliberately avoid hardware-dependent timing thresholds.

use super::*;
use crate::render_work::{DeviceWork as _, QueueWork as _};
use bevy::render::renderer::{RenderDevice, RenderQueue, WgpuWrapper};
use std::sync::Arc;

/// Uses the validation-only backend; application counters are thread-local in tests.
fn device() -> (RenderDevice, RenderQueue) {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue(Arc::new(WgpuWrapper::new(queue)));
    (device, queue)
}

#[test]
fn creations_survive_drop_and_unchanged_frames_do_no_work() {
    let (device, queue) = device();
    let before = crate::render_work::snapshot();
    let buffer = device.tracked_create_buffer_with_data(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: &[0; 32],
        usage: wgpu::BufferUsages::COPY_DST,
    });
    queue.tracked_write_buffer(&buffer, 0, &[1; 16]);
    drop(buffer);
    let after = crate::render_work::snapshot();
    assert_eq!(after.delta_since(before).buffer_upload_bytes, 48);
    assert_eq!(after.delta_since(after), Default::default());
}

#[test]
fn shader_and_bind_group_counts_include_first_use_only() {
    let (device, _) = device();
    let before = crate::render_work::snapshot();
    let shader = device.tracked_create_and_validate_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl("@compute @workgroup_size(1) fn main() {}".into()),
    });
    let pipeline = device.tracked_create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let layout = device.create_bind_group_layout(None, &[]);
    let group = device.tracked_create_bind_group(None, &layout, &[]);
    let warm = crate::render_work::snapshot();
    let first = warm.delta_since(before);
    assert_eq!(first.shader_modules_created, 1);
    assert_eq!(first.compute_pipelines_created, 1);
    assert_eq!(first.bind_groups_created, 1);
    drop((pipeline, group, shader));
    assert_eq!(
        crate::render_work::snapshot().delta_since(warm),
        Default::default()
    );
}

#[test]
fn completed_frames_drain_work_once_and_keep_their_own_sequence() {
    let (device, queue) = device();
    let mut world = World::new();
    world.insert_resource(device.clone());
    finish_work_frame(&mut world);
    let first = world.resource::<WorkBaseline>().0;
    let buffer = device.tracked_create_buffer_with_data(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: &[0; 32],
        usage: wgpu::BufferUsages::COPY_DST,
    });
    queue.tracked_write_buffer(&buffer, 0, &[1; 16]);
    finish_work_frame(&mut world);
    let second = world.resource::<WorkBaseline>().0;
    assert_eq!(second.sequence, first.sequence + 1);
    assert_eq!(second.work.delta_since(first.work).buffer_upload_bytes, 48);
    finish_work_frame(&mut world);
    let third = world.resource::<WorkBaseline>().0;
    assert_eq!(third.sequence, second.sequence + 1);
    assert_eq!(third.work.delta_since(second.work), Default::default());
}

#[test]
fn texture_upload_excludes_source_offsets_row_padding_and_unused_bytes() {
    let (device, queue) = device();
    let size = wgpu::Extent3d {
        width: 3,
        height: 2,
        depth_or_array_layers: 2,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let before = crate::render_work::snapshot();
    queue.tracked_write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &[0; 1536],
        wgpu::TexelCopyBufferLayout {
            offset: 16,
            bytes_per_row: Some(256),
            rows_per_image: Some(4),
        },
        size,
    );
    let uploaded = crate::render_work::snapshot();
    assert_eq!(uploaded.delta_since(before).texture_upload_bytes, 48);
    assert_eq!(uploaded.delta_since(before).buffer_upload_bytes, 0);
    drop(texture);
    assert_eq!(
        crate::render_work::snapshot()
            .delta_since(uploaded)
            .texture_upload_bytes,
        0
    );
}

#[test]
fn initialized_array_texture_counts_each_mip_and_layer_once_in_either_order() {
    let (device, queue) = device();
    let descriptor = wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 4,
            height: 2,
            depth_or_array_layers: 3,
        },
        mip_level_count: 3,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    };
    for order in [
        wgpu::util::TextureDataOrder::LayerMajor,
        wgpu::util::TextureDataOrder::MipMajor,
    ] {
        let before = crate::render_work::snapshot();
        let texture =
            device.tracked_create_texture_with_data(&queue, &descriptor, order, &[0; 140]);
        let uploaded = crate::render_work::snapshot();
        // Each layer contains 8, 2 and 1 RGBA texels; the trailing bytes are unused.
        assert_eq!(uploaded.delta_since(before).texture_upload_bytes, 132);
        assert_eq!(uploaded.delta_since(before).buffer_upload_bytes, 0);
        drop(texture);
        assert_eq!(
            crate::render_work::snapshot()
                .delta_since(uploaded)
                .texture_upload_bytes,
            0
        );
    }
}
