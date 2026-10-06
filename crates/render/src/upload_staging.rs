//! Bounded mapped upload storage reused after asynchronous GPU completion.

#[cfg(test)]
#[path = "upload_staging/tests.rs"]
mod tests;

use crate::render_work::QueueWork as _;
use bevy::{
    prelude::*,
    render::{
        RenderApp, RenderStartup,
        graph::CameraDriverLabel,
        render_graph::{Node, NodeRunError, RenderGraph, RenderGraphContext, RenderLabel},
        renderer::{RenderContext, RenderDevice, RenderQueue},
    },
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
};

const SLOT_COUNT: usize = 4;
const SLOT_BYTES: u64 = 4 * 1024 * 1024;
const MAX_COPIES: usize = 512;
const READY: u8 = 0;
const ACTIVE: u8 = 1;
const PENDING: u8 = 2;
const FAILED: u8 = 3;

/// One destination update; a batch is admitted atomically into retained staging.
pub(crate) type BufferWrite<'a> = (&'a wgpu::Buffer, u64, &'a [u8]);

#[derive(Resource)]
struct Installed;

/// Installs one upload prefix shared by all participating renderer owners.
pub(crate) fn install(app: &mut App) {
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    if render_app.world().contains_resource::<Installed>() {
        return;
    }
    render_app
        .insert_resource(Installed)
        .add_systems(RenderStartup, initialize);
}

/// Allocates the fixed staging pool before recurring render preparation begins.
fn initialize(world: &mut World) {
    if world
        .get_resource::<RenderGraph>()
        .is_none_or(|graph| graph.get_node_state(CameraDriverLabel).is_err())
    {
        return;
    }
    let uploads = BufferUploadStaging::new(world.resource::<RenderDevice>(), SLOT_BYTES);
    world.insert_resource(uploads);
    let mut graph = world.resource_mut::<RenderGraph>();
    graph.add_node(UploadLabel, UploadNode);
    graph.add_node_edge(UploadLabel, CameraDriverLabel);
}

/// Uses retained staging when installed, preserving direct preparation in isolated tests.
pub(crate) fn write_batch(
    staging: Option<&BufferUploadStaging>,
    device: &RenderDevice,
    queue: &RenderQueue,
    writes: &[BufferWrite<'_>],
) {
    if let Some(staging) = staging {
        staging.write_batch(device, queue, writes);
    } else {
        fallback(queue, writes);
    }
}

/// Uploads a complete batch without waiting when mapped storage is unavailable.
fn fallback(queue: &RenderQueue, writes: &[BufferWrite<'_>]) {
    for &(buffer, offset, bytes) in writes.iter().filter(|write| !write.2.is_empty()) {
        crate::render_work::staging::fallback_buffer_upload(bytes.len());
        queue.tracked_write_buffer(buffer, offset, bytes);
    }
}

/// The fixed pool never waits for mapping and never grows with unfinished GPU work.
#[derive(Resource)]
pub(crate) struct BufferUploadStaging(Mutex<Pool>);

struct Slot {
    buffer: wgpu::Buffer,
    state: Arc<AtomicU8>,
    offset: u64,
}

struct Copy {
    slot: usize,
    source: u64,
    target: wgpu::Buffer,
    offset: u64,
    bytes: u64,
}

struct Pool {
    slots: [Slot; SLOT_COUNT],
    copies: Vec<Copy>,
}

impl BufferUploadStaging {
    /// Creates mapped slots once; callbacks make the same buffers writable again.
    fn new(device: &RenderDevice, slot_bytes: u64) -> Self {
        Self(Mutex::new(Pool {
            slots: std::array::from_fn(|_| {
                crate::render_work::staging::staging_buffer_allocation(slot_bytes as usize);
                Slot {
                    buffer: device.wgpu_device().create_buffer(&wgpu::BufferDescriptor {
                        label: Some("retained frame upload staging"),
                        size: slot_bytes,
                        usage: wgpu::BufferUsages::MAP_WRITE | wgpu::BufferUsages::COPY_SRC,
                        mapped_at_creation: true,
                    }),
                    state: Arc::new(AtomicU8::new(READY)),
                    offset: 0,
                }
            }),
            copies: Vec::with_capacity(MAX_COPIES),
        }))
    }

    /// Reserves every write together; fallback cannot overtake earlier overlapping copies.
    fn write_batch(&self, device: &RenderDevice, queue: &RenderQueue, writes: &[BufferWrite<'_>]) {
        let mut pool = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if pool.can_stage(writes) {
            let _span =
                crate::render_systems::time(crate::render_systems::System::GpuApiWriteBuffer);
            for &(target, offset, bytes) in writes.iter().filter(|write| !write.2.is_empty()) {
                pool.stage(target, offset, bytes);
            }
            return;
        }
        let pending = if pool.overlaps(writes) {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("ordered upload fallback"),
            });
            pool.encode(&mut encoder);
            Some(encoder.finish())
        } else {
            None
        };
        drop(pool);
        if let Some(pending) = pending {
            queue.tracked_submit([pending]);
        }
        fallback(queue, writes);
    }
}

impl Pool {
    /// Simulates reservations without modifying any slot when the full batch cannot fit.
    fn can_stage(&self, writes: &[BufferWrite<'_>]) -> bool {
        let count = writes.iter().filter(|write| !write.2.is_empty()).count();
        if self.copies.len() + count > MAX_COPIES {
            return false;
        }
        let mut offsets =
            self.slots
                .each_ref()
                .map(|slot| match slot.state.load(Ordering::Acquire) {
                    READY => Some(0),
                    ACTIVE => Some(slot.offset),
                    _ => None,
                });
        for &(_, _, bytes) in writes.iter().filter(|write| !write.2.is_empty()) {
            let Some((index, start)) = self.reserve(&offsets, bytes.len() as u64) else {
                return false;
            };
            offsets[index] = Some(start + bytes.len() as u64);
        }
        true
    }

    /// Finds mapped storage while respecting WebGPU mapped-range alignment.
    fn reserve(&self, offsets: &[Option<u64>; SLOT_COUNT], bytes: u64) -> Option<(usize, u64)> {
        for occupied in [true, false] {
            for (index, offset) in offsets.iter().enumerate() {
                let Some(offset) = offset.filter(|offset| (*offset > 0) == occupied) else {
                    continue;
                };
                let start = offset.div_ceil(wgpu::MAP_ALIGNMENT) * wgpu::MAP_ALIGNMENT;
                if start
                    .checked_add(bytes)
                    .is_some_and(|end| end <= self.slots[index].buffer.size())
                {
                    return Some((index, start));
                }
            }
        }
        None
    }

    /// Copies one admitted payload into mapped memory and records its destination.
    fn stage(&mut self, target: &wgpu::Buffer, offset: u64, bytes: &[u8]) {
        let offsets = self
            .slots
            .each_ref()
            .map(|slot| match slot.state.load(Ordering::Acquire) {
                READY => Some(0),
                ACTIVE => Some(slot.offset),
                _ => None,
            });
        let (index, start) = self
            .reserve(&offsets, bytes.len() as u64)
            .expect("batch reserved storage");
        let slot = &mut self.slots[index];
        slot.state.store(ACTIVE, Ordering::Release);
        slot.offset = start + bytes.len() as u64;
        slot.buffer
            .slice(start..slot.offset)
            .get_mapped_range_mut()
            .copy_from_slice(bytes);
        self.copies.push(Copy {
            slot: index,
            source: start,
            target: target.clone(),
            offset,
            bytes: bytes.len() as u64,
        });
        crate::render_work::staging::staged_buffer_upload(bytes.len());
    }

    /// Detects fallback writes that would otherwise run before an older staged update.
    fn overlaps(&self, writes: &[BufferWrite<'_>]) -> bool {
        self.copies.iter().any(|copy| {
            writes.iter().any(|(buffer, offset, bytes)| {
                **buffer == copy.target
                    && *offset < copy.offset + copy.bytes
                    && copy.offset < offset.saturating_add(bytes.len() as u64)
            })
        })
    }

    /// Encodes uploads ahead of draws and requests reuse only after this submission completes.
    fn encode(&mut self, encoder: &mut wgpu::CommandEncoder) {
        for slot in &self.slots {
            if slot.state.load(Ordering::Acquire) == ACTIVE {
                slot.buffer.unmap();
                slot.state.store(PENDING, Ordering::Release);
            }
        }
        for copy in self.copies.drain(..) {
            encoder.copy_buffer_to_buffer(
                &self.slots[copy.slot].buffer,
                copy.source,
                &copy.target,
                copy.offset,
                copy.bytes,
            );
        }
        for slot in &self.slots {
            if slot.offset == 0 || slot.state.load(Ordering::Acquire) != PENDING {
                continue;
            }
            let state = Arc::clone(&slot.state);
            encoder.map_buffer_on_submit(&slot.buffer, wgpu::MapMode::Write, .., move |result| {
                state.store(
                    if result.is_ok() { READY } else { FAILED },
                    Ordering::Release,
                );
            });
        }
        for slot in &mut self.slots {
            if slot.state.load(Ordering::Acquire) == PENDING {
                slot.offset = 0;
            }
        }
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
struct UploadLabel;
struct UploadNode;

impl Node for UploadNode {
    /// Places upload copies in the frame's existing command encoder before its world passes.
    fn run<'w>(
        &self,
        _: &mut RenderGraphContext,
        context: &mut RenderContext<'w>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let staging = world.resource::<BufferUploadStaging>();
        let mut pool = staging.0.lock().unwrap_or_else(|error| error.into_inner());
        if !pool.copies.is_empty() {
            pool.encode(context.command_encoder());
        }
        Ok(())
    }
}
