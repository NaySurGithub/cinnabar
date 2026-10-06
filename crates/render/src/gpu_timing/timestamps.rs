//! Timestamp storage, lock-free frame allocation and asynchronous readback reuse.

use super::*;

/// Lock-free span allocation for the frame being recorded, shared by graph threads.
pub(super) struct FrameSpans {
    pub(super) slot: AtomicU32,
    pub(super) passes: AtomicU32,
    pub(super) draws: AtomicU32,
    pub(super) stages: [AtomicU8; SLOT_SPANS as usize],
}

pub(super) struct ReadbackSlot {
    pub(super) buffer: wgpu::Buffer,
    pub(super) state: Arc<AtomicU8>,
    pub(super) passes: u32,
    pub(super) draws: u32,
    pub(super) stages: [RuntimeStage; SLOT_SPANS as usize],
}

pub(super) struct Readbacks {
    pub(super) slots: [ReadbackSlot; SLOTS],
    pub(super) ring: ReadbackRing,
}

#[derive(Resource)]
pub(crate) struct GpuTimestamps {
    pub(super) queries: wgpu::QuerySet,
    pub(super) resolve: wgpu::Buffer,
    pub(super) readbacks: Mutex<Readbacks>,
    pub(super) period_ns: f32,
    pub(super) draw_spans: bool,
    pub(super) frame: FrameSpans,
}

impl GpuTimestamps {
    /// `None` when the device lacks `TIMESTAMP_QUERY`.
    pub(super) fn new(device: &RenderDevice, queue: &RenderQueue, profiling: bool) -> Option<Self> {
        let features = device.features();
        if !features.contains(wgpu::Features::TIMESTAMP_QUERY) {
            return None;
        }
        let device = device.wgpu_device();
        let buffer = |label, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: SLOT_BYTES,
                usage,
                mapped_at_creation: false,
            })
        };
        Some(Self {
            queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                label: Some("gpu timestamps"),
                ty: wgpu::QueryType::Timestamp,
                count: SLOTS as u32 * SLOT_SPANS * 2,
            }),
            resolve: buffer(
                "gpu timestamp resolve",
                wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            ),
            readbacks: Mutex::new(Readbacks {
                slots: std::array::from_fn(|_| ReadbackSlot {
                    buffer: buffer(
                        "gpu timestamp readback",
                        wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    ),
                    state: Arc::new(AtomicU8::new(PENDING)),
                    passes: 0,
                    draws: 0,
                    stages: [RuntimeStage::GpuFrame; SLOT_SPANS as usize],
                }),
                ring: ReadbackRing::default(),
            }),
            period_ns: queue.get_timestamp_period(),
            draw_spans: profiling
                && features.contains(wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES),
            frame: FrameSpans {
                slot: AtomicU32::new(NO_SLOT),
                passes: AtomicU32::new(0),
                draws: AtomicU32::new(0),
                stages: std::array::from_fn(|_| AtomicU8::new(0)),
            },
        })
    }

    /// Allocates one node-level pair without competing with draw category capacity.
    pub(super) fn open_pass(&self, stage: RuntimeStage) -> Option<Span<'_>> {
        self.open(stage, &self.frame.passes, 0, PASS_SPANS)
    }

    /// Allocates a category pair only when aggregate profiling requests per-draw timing.
    pub(super) fn open_draw(&self, stage: RuntimeStage) -> Option<Span<'_>> {
        self.draw_spans
            .then(|| self.open(stage, &self.frame.draws, PASS_SPANS, DRAW_SPANS))
            .flatten()
    }

    /// Claims a unique pair in the current frame and stores its category.
    fn open(
        &self,
        stage: RuntimeStage,
        counter: &AtomicU32,
        offset: u32,
        capacity: u32,
    ) -> Option<Span<'_>> {
        let slot = self.frame.slot.load(Ordering::Acquire);
        if slot == NO_SLOT {
            return None;
        }
        let index = counter.fetch_add(1, Ordering::Relaxed);
        if index >= capacity {
            return None;
        }
        let span = offset + index;
        self.frame.stages[span as usize].store(stage as u8, Ordering::Relaxed);
        Some(Span {
            queries: &self.queries,
            begin: (slot * SLOT_SPANS + span) * 2,
        })
    }

    /// Hands mapped frames to `sink` oldest first, then claims a slot for this frame.
    pub(super) fn begin(&mut self, mut sink: impl FnMut(&GpuFrameTimes)) {
        let readbacks = self
            .readbacks
            .get_mut()
            .unwrap_or_else(|error| error.into_inner());
        while let Some(index) = readbacks.ring.oldest_in_flight() {
            let slot = &readbacks.slots[index];
            match slot.state.load(Ordering::Acquire) {
                PENDING => break,
                MAPPED => {
                    let bytes = slot.buffer.slice(..).get_mapped_range();
                    let tick = |query: u32| {
                        let start = query as usize * TIMESTAMP_BYTES as usize;
                        u64::from_le_bytes(
                            bytes[start..start + TIMESTAMP_BYTES as usize]
                                .try_into()
                                .expect("timestamp is eight bytes"),
                        )
                    };
                    let passes = (0..slot.passes).map(|span| span * 2);
                    let draws = (0..slot.draws).map(|span| (PASS_SPANS + span) * 2);
                    let frame = decode_spans(
                        passes.chain(draws).map(|query| {
                            let stage = slot.stages[query as usize / 2];
                            (stage, tick(query), tick(query + 1))
                        }),
                        self.period_ns,
                    );
                    drop(bytes);
                    slot.buffer.unmap();
                    sink(&frame);
                }
                _ => {}
            }
            slot.state.store(PENDING, Ordering::Relaxed);
            readbacks.ring.release(index);
        }
        let slot = readbacks.ring.acquire().map_or(NO_SLOT, |slot| slot as u32);
        self.frame.passes.store(0, Ordering::Relaxed);
        self.frame.draws.store(0, Ordering::Relaxed);
        self.frame.slot.store(slot, Ordering::Release);
    }
}
