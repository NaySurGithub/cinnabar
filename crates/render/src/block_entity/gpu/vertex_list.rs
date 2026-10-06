//! Retained vertex storage shared by the block-entity draw lists.

use super::{BlockEntityVertex, VERTEX_BYTES};
use bevy::render::{
    render_resource::{BindGroup, Buffer, BufferDescriptor, BufferUsages},
    renderer::{RenderDevice, RenderQueue},
};

const MIN_BUFFER_VERTICES: u64 = 1024;

/// One storage buffer of vertices with its live count and bind group.
pub(super) struct VertexList {
    pub(super) buffer: Option<Buffer>,
    capacity_vertices: u64,
    pub(super) count: u32,
    pub(super) bind_group: Option<BindGroup>,
}

impl VertexList {
    /// Starts with no GPU allocation until a nonempty vertex list arrives.
    pub(super) const fn new() -> Self {
        Self {
            buffer: None,
            capacity_vertices: 0,
            count: 0,
            bind_group: None,
        }
    }

    /// Uploads changed vertices into a retained destination and shared staging storage.
    pub(super) fn upload(
        &mut self,
        vertices: &[BlockEntityVertex],
        render_device: &RenderDevice,
        render_queue: &RenderQueue,
        label: &'static str,
        staging: Option<&crate::upload_staging::BufferUploadStaging>,
    ) {
        self.count = u32::try_from(vertices.len()).unwrap_or(0);
        if vertices.is_empty() {
            return;
        }
        let needed = vertices.len() as u64;
        if self.buffer.is_none() || self.capacity_vertices < needed {
            let capacity = needed.next_power_of_two().max(MIN_BUFFER_VERTICES);
            self.buffer = Some(render_device.create_buffer(&BufferDescriptor {
                label: Some(label),
                size: capacity * VERTEX_BYTES,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.capacity_vertices = capacity;
            self.bind_group = None;
        }
        if let Some(buffer) = &self.buffer {
            crate::upload_staging::write_batch(
                staging,
                render_device,
                render_queue,
                &[(buffer, 0, bytemuck::cast_slice(vertices))],
            );
        }
    }
}
