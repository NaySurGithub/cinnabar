//! Work performed by Cinnabar at its GPU API boundaries.
//! API wall times include driver waits and overlap their enclosing application-system spans.
use bevy::render::{
    render_resource::*,
    renderer::{RenderDevice, RenderQueue},
};
use std::sync::atomic::{AtomicU64, Ordering};

macro_rules! counters {
    ($($field:ident),+ $(,)?) => {
        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
        pub(crate) struct WorkSnapshot { $(pub $field: u64),+ }
        impl WorkSnapshot {
            /// Subtracts a previous observation without carrying work across resets.
            pub(crate) fn delta_since(self, old: Self) -> Self {
                Self { $($field: self.$field.saturating_sub(old.$field)),+ }
            }
        }
        struct Counters { $($field: AtomicU64),+ }
        static WORK: Counters = Counters { $($field: AtomicU64::new(0)),+ };
        /// Reads cumulative application work without consulting Bevy internals.
        pub(crate) fn snapshot() -> WorkSnapshot {
            #[cfg(test)] { return TEST_WORK.with(std::cell::Cell::get); }
            #[cfg(not(test))] { WorkSnapshot { $($field: WORK.$field.load(Ordering::Relaxed)),+ } }
        }
    }
}
counters!(
    render_pipelines_queued,
    render_pipelines_created,
    compute_pipelines_created,
    shader_modules_created,
    bind_groups_created,
    buffer_upload_bytes,
    texture_upload_bytes,
    readback_polls,
    readback_waits
);

#[cfg(test)]
thread_local! { static TEST_WORK: std::cell::Cell<WorkSnapshot> = const { std::cell::Cell::new(WorkSnapshot {
    render_pipelines_queued: 0, render_pipelines_created: 0,
    compute_pipelines_created: 0, shader_modules_created: 0, bind_groups_created: 0,
    buffer_upload_bytes: 0, texture_upload_bytes: 0, readback_polls: 0, readback_waits: 0,
}) }; }
macro_rules! record {
    ($field:ident, $value:expr) => {{
        let value = $value as u64;
        WORK.$field.fetch_add(value, Ordering::Relaxed);
        #[cfg(test)]
        TEST_WORK.with(|counter| {
            let mut work = counter.get();
            work.$field += value;
            counter.set(work);
        });
    }};
}

/// Counts a cache-miss specializer callback, including keys that canonicalize together.
pub(crate) fn specialization() {
    record!(render_pipelines_queued, 1);
}

/// Counts initialized or updated texture payload without row padding or source offsets.
fn texture_bytes(format: TextureFormat, aspect: wgpu::TextureAspect, size: wgpu::Extent3d) -> u64 {
    let (width, height) = format.block_dimensions();
    u64::from(size.width.div_ceil(width))
        * u64::from(size.height.div_ceil(height))
        * u64::from(size.depth_or_array_layers)
        * u64::from(format.block_copy_size(Some(aspect)).unwrap_or(0))
}

/// Instrumented device operations only count calls made by our renderer.
pub(crate) trait DeviceWork {
    /// Creates and counts an application bind group.
    fn tracked_create_bind_group<'a>(
        &self,
        label: impl Into<wgpu::Label<'a>>,
        layout: &'a BindGroupLayout,
        entries: &'a [BindGroupEntry<'a>],
    ) -> BindGroup;
    /// Counts the initial payload when creating an application buffer.
    fn tracked_create_buffer_with_data(&self, desc: &wgpu::util::BufferInitDescriptor) -> Buffer;
    /// Counts all initialized texture mips and layers once.
    fn tracked_create_texture_with_data(
        &self,
        queue: &RenderQueue,
        desc: &wgpu::TextureDescriptor,
        order: wgpu::util::TextureDataOrder,
        data: &[u8],
    ) -> Texture;
    /// Counts shader modules created directly by our renderer.
    fn tracked_create_and_validate_shader_module(
        &self,
        desc: wgpu::ShaderModuleDescriptor,
    ) -> wgpu::ShaderModule;
    /// Counts compute pipelines created directly by our renderer.
    fn tracked_create_compute_pipeline(
        &self,
        desc: &wgpu::ComputePipelineDescriptor,
    ) -> ComputePipeline;
    /// Counts render pipelines created directly by our renderer.
    fn tracked_create_render_pipeline(&self, desc: &RawRenderPipelineDescriptor) -> RenderPipeline;
    /// Distinguishes nonblocking readback polling from explicit device waits.
    fn tracked_poll(&self, poll: wgpu::PollType) -> Result<wgpu::PollStatus, wgpu::PollError>;
}
impl DeviceWork for RenderDevice {
    fn tracked_create_bind_group<'a>(
        &self,
        label: impl Into<wgpu::Label<'a>>,
        layout: &'a BindGroupLayout,
        entries: &'a [BindGroupEntry<'a>],
    ) -> BindGroup {
        record!(bind_groups_created, 1);
        let _api_span =
            crate::render_systems::time(crate::render_systems::System::GpuApiCreateBindGroup);
        self.create_bind_group(label, layout, entries)
    }
    fn tracked_create_buffer_with_data(&self, desc: &wgpu::util::BufferInitDescriptor) -> Buffer {
        record!(buffer_upload_bytes, desc.contents.len());
        let _api_span =
            crate::render_systems::time(crate::render_systems::System::GpuApiCreateBufferWithData);
        self.create_buffer_with_data(desc)
    }
    fn tracked_create_texture_with_data(
        &self,
        queue: &RenderQueue,
        desc: &wgpu::TextureDescriptor,
        order: wgpu::util::TextureDataOrder,
        data: &[u8],
    ) -> Texture {
        let bytes: u64 = (0..desc.mip_level_count)
            .map(|mip| {
                texture_bytes(
                    desc.format,
                    wgpu::TextureAspect::All,
                    desc.mip_level_size(mip).unwrap(),
                )
            })
            .sum();
        record!(texture_upload_bytes, bytes);
        let _api_span =
            crate::render_systems::time(crate::render_systems::System::GpuApiCreateTextureWithData);
        self.create_texture_with_data(queue, desc, order, data)
    }
    fn tracked_create_and_validate_shader_module(
        &self,
        desc: wgpu::ShaderModuleDescriptor,
    ) -> wgpu::ShaderModule {
        record!(shader_modules_created, 1);
        let _api_span =
            crate::render_systems::time(crate::render_systems::System::GpuApiCreateShaderModule);
        self.create_and_validate_shader_module(desc)
    }
    fn tracked_create_compute_pipeline(
        &self,
        desc: &wgpu::ComputePipelineDescriptor,
    ) -> ComputePipeline {
        record!(compute_pipelines_created, 1);
        let _api_span =
            crate::render_systems::time(crate::render_systems::System::GpuApiCreateComputePipeline);
        self.create_compute_pipeline(desc)
    }
    fn tracked_create_render_pipeline(&self, desc: &RawRenderPipelineDescriptor) -> RenderPipeline {
        record!(render_pipelines_created, 1);
        let _api_span =
            crate::render_systems::time(crate::render_systems::System::GpuApiCreateRenderPipeline);
        self.create_render_pipeline(desc)
    }
    fn tracked_poll(&self, poll: wgpu::PollType) -> Result<wgpu::PollStatus, wgpu::PollError> {
        record!(readback_polls, 1);
        if matches!(poll, wgpu::PollType::Wait { .. }) {
            record!(readback_waits, 1);
        }
        let _api_span = crate::render_systems::time(crate::render_systems::System::GpuApiPoll);
        self.poll(poll)
    }
}

/// Instrumented queue uploads retain the original payload and submission behavior.
pub(crate) trait QueueWork {
    /// Counts only the bytes passed for this buffer update.
    fn tracked_write_buffer(&self, buffer: &wgpu::Buffer, offset: u64, data: &[u8]);
    /// Counts a directly initialized staging allocation for this buffer update.
    fn tracked_write_buffer_with(
        &self,
        buffer: &wgpu::Buffer,
        offset: u64,
        size: std::num::NonZeroU64,
    ) -> Option<wgpu::QueueWriteBufferView>;
    /// Counts texel payload independently of source row padding.
    fn tracked_write_texture(
        &self,
        texture: wgpu::TexelCopyTextureInfo<'_>,
        data: &[u8],
        layout: wgpu::TexelCopyBufferLayout,
        size: wgpu::Extent3d,
    );
}
impl QueueWork for RenderQueue {
    fn tracked_write_buffer(&self, buffer: &wgpu::Buffer, offset: u64, data: &[u8]) {
        record!(buffer_upload_bytes, data.len());
        let _api_span =
            crate::render_systems::time(crate::render_systems::System::GpuApiWriteBuffer);
        self.write_buffer(buffer, offset, data);
    }
    fn tracked_write_buffer_with(
        &self,
        buffer: &wgpu::Buffer,
        offset: u64,
        size: std::num::NonZeroU64,
    ) -> Option<wgpu::QueueWriteBufferView> {
        let _api_span =
            crate::render_systems::time(crate::render_systems::System::GpuApiWriteBuffer);
        let view = self.write_buffer_with(buffer, offset, size)?;
        record!(buffer_upload_bytes, size.get());
        Some(view)
    }
    fn tracked_write_texture(
        &self,
        texture: wgpu::TexelCopyTextureInfo<'_>,
        data: &[u8],
        layout: wgpu::TexelCopyBufferLayout,
        size: wgpu::Extent3d,
    ) {
        record!(
            texture_upload_bytes,
            texture_bytes(texture.texture.format(), texture.aspect, size)
        );
        let _api_span =
            crate::render_systems::time(crate::render_systems::System::GpuApiWriteTexture);
        self.write_texture(texture, data, layout, size);
    }
}

/// Counts descriptors queued directly by our application pipeline caches.
pub(crate) trait PipelineWork {
    /// Counts a cache miss before handing its descriptor to Bevy.
    fn tracked_queue_render_pipeline(
        &self,
        descriptor: RenderPipelineDescriptor,
    ) -> CachedRenderPipelineId;
}
impl PipelineWork for PipelineCache {
    fn tracked_queue_render_pipeline(
        &self,
        descriptor: RenderPipelineDescriptor,
    ) -> CachedRenderPipelineId {
        specialization();
        self.queue_render_pipeline(descriptor)
    }
}
