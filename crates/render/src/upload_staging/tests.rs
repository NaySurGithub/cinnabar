use super::*;
use bevy::render::renderer::WgpuWrapper;

impl BufferUploadStaging {
    /// Supplies a bounded mapped pool to deterministic owner-upload tests.
    pub(crate) fn for_tests(device: &RenderDevice, slot_bytes: u64) -> Self {
        Self::new(device, slot_bytes)
    }
}

/// Creates a validation-only device and a deliberately small reusable upload pool.
fn setup() -> (RenderDevice, RenderQueue, BufferUploadStaging, wgpu::Buffer) {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue(Arc::new(WgpuWrapper::new(queue)));
    let staging = BufferUploadStaging::for_tests(&device, 64);
    let target = device.wgpu_device().create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 1024,
        usage: wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    (device, queue, staging, target)
}

#[test]
fn complete_batches_preserve_bytes_and_order_in_bounded_storage() {
    let (device, queue, staging, target) = setup();
    let before = crate::render_work::snapshot();
    staging.write_batch(
        &device,
        &queue,
        &[(&target, 8, &[1; 12]), (&target, 16, &[2; 8])],
    );
    let pool = staging.0.lock().unwrap();
    let mut result = [0; 32];
    for copy in &pool.copies {
        let bytes = pool.slots[copy.slot]
            .buffer
            .slice(copy.source..copy.source + copy.bytes)
            .get_mapped_range();
        result[copy.offset as usize..(copy.offset + copy.bytes) as usize].copy_from_slice(&bytes);
    }
    assert_eq!(&result[8..16], &[1; 8]);
    assert_eq!(&result[16..24], &[2; 8]);
    assert_eq!(
        pool.slots
            .iter()
            .map(|slot| slot.buffer.size())
            .sum::<u64>(),
        SLOT_COUNT as u64 * 64
    );
    let work = crate::render_work::snapshot().delta_since(before);
    assert_eq!(work.staged_uploads, 2);
    assert_eq!(work.staged_upload_bytes, 20);
    assert_eq!(work.fallback_uploads, 0);
    assert_eq!(work.staging_buffers, 0);
}

#[test]
fn insufficient_capacity_falls_back_for_the_whole_batch_without_partial_staging() {
    let (device, queue, staging, target) = setup();
    let before = crate::render_work::snapshot();
    staging.write_batch(
        &device,
        &queue,
        &[(&target, 0, &[1; 16]), (&target, 16, &[2; 80])],
    );
    let pool = staging.0.lock().unwrap();
    assert!(pool.copies.is_empty());
    assert!(
        pool.slots
            .iter()
            .all(|slot| slot.state.load(Ordering::Acquire) == READY)
    );
    let work = crate::render_work::snapshot().delta_since(before);
    assert_eq!(work.staged_uploads, 0);
    assert_eq!(work.fallback_uploads, 2);
    assert_eq!(work.fallback_upload_bytes, 96);
    assert_eq!(work.buffer_upload_bytes, 96);
    assert_eq!(work.readback_waits, 0);
}

#[test]
fn repeated_target_fallback_submits_older_staged_bytes_first() {
    let (device, queue, staging, target) = setup();
    staging.write_batch(&device, &queue, &[(&target, 0, &[1; 16])]);
    let before = crate::render_work::snapshot();
    staging.write_batch(&device, &queue, &[(&target, 8, &[2; 80])]);
    assert!(staging.0.lock().unwrap().copies.is_empty());
    let work = crate::render_work::snapshot().delta_since(before);
    assert_eq!(work.own_queue_submits, 1);
    assert_eq!(work.fallback_upload_bytes, 80);
    assert_eq!(work.readback_waits, 0);
}

#[test]
fn unfinished_slots_do_not_grow_or_wait_and_disjoint_fallback_does_not_submit() {
    let (device, queue, staging, target) = setup();
    staging.write_batch(
        &device,
        &queue,
        &[
            (&target, 0, &[1; 64]),
            (&target, 64, &[2; 64]),
            (&target, 128, &[3; 64]),
            (&target, 192, &[4; 64]),
        ],
    );
    let before = crate::render_work::snapshot();
    staging.write_batch(&device, &queue, &[(&target, 256, &[5; 16])]);
    let mut encoder = device.create_command_encoder(&Default::default());
    staging.0.lock().unwrap().encode(&mut encoder);
    staging.write_batch(&device, &queue, &[(&target, 272, &[6; 16])]);
    let work = crate::render_work::snapshot().delta_since(before);
    assert_eq!(work.own_queue_submits, 0);
    assert_eq!(work.fallback_uploads, 2);
    assert_eq!(work.staging_buffers, 0);
    assert_eq!(work.staging_capacity_bytes, 0);
    assert_eq!(work.readback_polls, 0);
    assert_eq!(work.readback_waits, 0);
}

#[test]
fn completed_slots_reuse_allocations_and_empty_frames_create_no_work() {
    let (device, queue, staging, target) = setup();
    let before = crate::render_work::snapshot();
    for value in 0..8 {
        staging.write_batch(&device, &queue, &[(&target, 0, &[value; 64])]);
        let mut encoder = device.create_command_encoder(&Default::default());
        staging.0.lock().unwrap().encode(&mut encoder);
        queue.submit([encoder.finish()]);
        device.poll(wgpu::PollType::Poll).unwrap();
    }
    let work = crate::render_work::snapshot().delta_since(before);
    assert_eq!(work.staged_uploads, 8);
    assert_eq!(work.staged_upload_bytes, 512);
    assert_eq!(work.staging_buffers, 0);
    assert_eq!(work.fallback_uploads, 0);
    let before = crate::render_work::snapshot();
    staging.write_batch(&device, &queue, &[(&target, 0, &[])]);
    assert!(staging.0.lock().unwrap().copies.is_empty());
    assert_eq!(
        crate::render_work::snapshot().delta_since(before),
        Default::default()
    );
}

#[test]
fn copy_metadata_capacity_falls_back_without_growing_or_flushing_disjoint_writes() {
    let (device, queue, _, target) = setup();
    let staging = BufferUploadStaging::for_tests(&device, 4096);
    let payload = [1; 4];
    let writes = vec![(&target, 0, payload.as_slice()); MAX_COPIES];
    staging.write_batch(&device, &queue, &writes);
    let before = crate::render_work::snapshot();
    staging.write_batch(&device, &queue, &[(&target, 8, &[2; 4])]);
    let pool = staging.0.lock().unwrap();
    assert_eq!(pool.copies.len(), MAX_COPIES);
    assert_eq!(pool.copies.capacity(), MAX_COPIES);
    let work = crate::render_work::snapshot().delta_since(before);
    assert_eq!(work.staged_uploads, 0);
    assert_eq!(work.fallback_uploads, 1);
    assert_eq!(work.fallback_upload_bytes, 4);
    assert_eq!(work.staging_buffers, 0);
    assert_eq!(work.staging_capacity_bytes, 0);
    assert_eq!(work.own_queue_submits, 0);
    assert_eq!(work.readback_waits, 0);
}

#[test]
fn failed_mapping_slot_is_quarantined_without_replacement_or_waiting() {
    let (device, queue, staging, target) = setup();
    staging.0.lock().unwrap().slots[0]
        .state
        .store(FAILED, Ordering::Release);
    let before = crate::render_work::snapshot();
    staging.write_batch(
        &device,
        &queue,
        &[
            (&target, 0, &[1; 64]),
            (&target, 64, &[2; 64]),
            (&target, 128, &[3; 64]),
        ],
    );
    staging.write_batch(&device, &queue, &[(&target, 192, &[4; 4])]);
    let pool = staging.0.lock().unwrap();
    assert_eq!(pool.slots[0].state.load(Ordering::Acquire), FAILED);
    assert_eq!(pool.slots[0].offset, 0);
    assert_eq!(pool.copies.len(), SLOT_COUNT - 1);
    assert!(pool.copies.iter().all(|copy| copy.slot != 0));
    let work = crate::render_work::snapshot().delta_since(before);
    assert_eq!(work.staged_uploads, 3);
    assert_eq!(work.staged_upload_bytes, 192);
    assert_eq!(work.fallback_uploads, 1);
    assert_eq!(work.fallback_upload_bytes, 4);
    assert_eq!(work.staging_buffers, 0);
    assert_eq!(work.staging_capacity_bytes, 0);
    assert_eq!(work.own_queue_submits, 0);
    assert_eq!(work.readback_polls, 0);
    assert_eq!(work.readback_waits, 0);
}
