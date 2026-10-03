//! Dense local-world terrain through request, decode, lighting, mesh and upload acknowledgement.
use super::*;

/// Scripted network delay must not consume response retries while workers make progress.
#[test]
fn held_network_exchange_preserves_retry_budget() {
    let (stream, keys, request) = stream_with_unsent_sub_chunks(1);
    let mut harness = Harness {
        stream,
        sequence: 2,
        ..Harness::for_tests()
    };
    harness.withheld.insert(keys[0].chunk());
    assert!(harness.stream.retry_request_front(request).is_ok());
    harness.answer_requests();
    assert_eq!(harness.held.len(), 1);

    harness.stream.expire_sub_chunk_deadlines(
        Instant::now() + SUB_CHUNK_RESPONSE_TIMEOUT * u32::from(MAX_SUB_CHUNK_RETRIES + 1),
    );
    assert_eq!(harness.stream.stats().sub_chunk_timeouts, 0);
    assert_eq!(harness.stream.stats().sub_chunk_retries_scheduled, 0);
    assert_eq!(harness.stream.outstanding_sub_chunk_count(), 1);
    assert_eq!(harness.stream.transport_pending_requests, 1);

    harness.withheld.clear();
    harness.deliver();
    assert!(harness.held.is_empty());
    assert_eq!(harness.stream.transport_pending_requests, 0);
    assert_eq!(harness.stream.stats().phase2_stages.requests_sent, 1);
    assert_eq!(harness.stream.stats().phase2_stages.responses_admitted, 1);
    assert_eq!(harness.stream.stats().phase2_outcomes.stale, 0);
}

/// Distant network exchanges may stay pending after the spawn neighborhood is presentable.
#[test]
fn bds_local_startup_completes_with_distant_replies_withheld() {
    let mut harness = Harness::for_tests();
    let center = ChunkKey::new(0, 0, 0);
    harness.send_view(center, false);
    harness.withheld.insert(ChunkKey::new(0, RADIUS, 0));
    assert!(!harness.stream.local_terrain_ready());
    let started = Instant::now();
    harness.step_until(|h| h.stream.local_terrain_ready());
    let cohort = harness.stream.committed_view_cohort().unwrap();
    assert!(!harness.stream.cohort_status(cohort).target_is_complete());
    assert!(!harness.idle());
    assert!(!harness.presented.is_empty());
    eprintln!(
        "local startup {:?}, distant stream still pending",
        started.elapsed()
    );
    harness.withheld.clear();
    harness.step_until(Harness::idle);
    assert!(harness.stream.local_terrain_ready());
}

/// Replays local terrain occupancy encoded as `(x,y,z,length,payload)` records.
#[test]
fn bds_saved_terrain_drains_without_camera_motion() {
    let Some(path) = std::env::var_os("CINNABAR_BDS_TERRAIN") else {
        return;
    };
    let bytes = std::fs::read(path).unwrap();
    let mut remaining = bytes.as_slice();
    let mut harness = Harness::for_tests();
    harness.terrain = |_| false;
    harness.highest = vanilla_dimension_range(0).unwrap().sub_chunk_count as u16;
    while !remaining.is_empty() {
        let word = |offset| i32::from_le_bytes(remaining[offset..offset + 4].try_into().unwrap());
        let key = SubChunkKey::new(0, word(0), word(4), word(8));
        let size = word(12) as usize;
        harness
            .payloads
            .insert(key, remaining[16..16 + size].to_vec());
        remaining = &remaining[16 + size..];
    }
    harness.send_view(ChunkKey::new(0, 1, 1), false);
    harness.camera = [17.0, 63.62, 23.0];
    let started = Instant::now();
    let report = harness.run();
    eprintln!(
        "saved terrain {:?}: {report:?}; pending light={} mesh={} loaded={}/{}",
        started.elapsed(),
        harness.stream.pending_light.len(),
        harness.stream.pending_mesh.len(),
        harness.stream.loaded_columns.len(),
        harness.stream.required_columns.len()
    );
    assert!(harness.idle(), "saved-world load stranded work");
    assert!(!harness.presented.is_empty());
}

#[test]
fn bds_join_dense_columns_drain_with_a_stationary_camera() {
    let mut harness = Harness::for_tests();
    // Full-height worker jobs need frame time, not the small fixture's 1 ms spin.
    harness.frame_sleep = FRAME;
    harness.terrain = |key| key.y <= 4;
    harness.highest = vanilla_dimension_range(0).unwrap().sub_chunk_count as u16;
    harness.send_view(ChunkKey::new(0, 1, 1), false);
    let started = Instant::now();
    let report = harness.run();
    eprintln!(
        "bds join {:?}: {report:?}; pending light={} mesh={} light heaps={}/{} waiters={} loaded={}/{}",
        started.elapsed(),
        harness.stream.pending_light.len(),
        harness.stream.pending_mesh.len(),
        harness.stream.pending_light_ready.len(),
        harness.stream.pending_light_deferred.len(),
        harness.stream.light_waiters.len(),
        harness.stream.loaded_columns.len(),
        harness.stream.required_columns.len()
    );
    for key in harness.stream.pending_light.keys().take(8) {
        eprintln!(
            "pending {key:?} highest={:?} context={} ready={} current={}",
            harness
                .stream
                .highest_pending_light_in_column(*key)
                .map(|(key, _)| key),
            harness.stream.original_light_column_context_ready(*key),
            harness.stream.light_dispatch_ready(*key),
            harness.stream.light_is_current(*key)
        );
    }
    assert!(harness.idle(), "local-world join stranded pending work");
    assert!(!harness.presented.is_empty());
}
