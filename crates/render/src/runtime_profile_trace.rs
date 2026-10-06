//! Bounded, opt-in spans exported on the exit frame without per-frame file I/O.

use crate::runtime_profile::RuntimeStage;
use crate::runtime_profile_slow::SlowFrameEvent;
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::ThreadId,
    time::{Duration, Instant},
};

const TRACE_CAPACITY: usize = 1_048_576;
const RENDER_WORK_CAPACITY: usize = TRACE_CAPACITY / 32;

#[derive(Debug)]
struct TraceEvent {
    name: &'static str,
    started: Duration,
    elapsed: Duration,
    thread: ThreadId,
    args: TraceArgs,
}

#[derive(Debug)]
enum TraceArgs {
    None,
    Focus { focused: bool, occluded: bool },
    SlowFrame(SlowFrameEvent),
    RenderWork(u32),
}

#[derive(Debug)]
struct TraceBuffer {
    events: Vec<TraceEvent>,
    render_work: Vec<crate::runtime_profile_render::RenderWorkFrame>,
    dropped_events: u64,
    dropped_render_work: u64,
}

#[derive(Debug)]
pub(crate) struct FrameTrace {
    path: PathBuf,
    epoch: Instant,
    flushed: AtomicBool,
    events: Mutex<TraceBuffer>,
}

impl FrameTrace {
    /// Preallocates a fixed recording budget; full traces drop subsequent spans.
    pub(crate) fn new(path: PathBuf, epoch: Instant) -> Self {
        Self::with_capacities(path, epoch, TRACE_CAPACITY, RENDER_WORK_CAPACITY)
    }

    /// Allows bounded-recording tests to exercise overflow without a full capture allocation.
    #[cfg(test)]
    fn with_capacity(path: PathBuf, epoch: Instant, capacity: usize) -> Self {
        Self::with_capacities(path, epoch, capacity, capacity.div_ceil(32))
    }

    /// Keeps large per-render payloads separate from the much more frequent stage events.
    fn with_capacities(
        path: PathBuf,
        epoch: Instant,
        capacity: usize,
        work_capacity: usize,
    ) -> Self {
        Self {
            path,
            epoch,
            flushed: AtomicBool::new(false),
            events: Mutex::new(TraceBuffer {
                events: Vec::with_capacity(capacity),
                render_work: Vec::with_capacity(work_capacity),
                dropped_events: 0,
                dropped_render_work: 0,
            }),
        }
    }

    /// Records a stage on its executing thread without doing file I/O.
    pub(crate) fn record(&self, stage: RuntimeStage, started: Instant, elapsed: Duration) {
        self.push(TraceEvent {
            name: stage.name(),
            started: started.saturating_duration_since(self.epoch),
            elapsed,
            thread: std::thread::current().id(),
            args: TraceArgs::None,
        });
    }

    /// Records focus at the start of each main frame, including time between updates.
    pub(crate) fn frame(&self, focused: bool, occluded: bool) {
        self.push(TraceEvent {
            name: "frame_start",
            started: self.epoch.elapsed(),
            elapsed: Duration::ZERO,
            thread: std::thread::current().id(),
            args: TraceArgs::Focus { focused, occluded },
        });
    }

    /// Marks every slow frame, including those whose text was rate-limited.
    pub(crate) fn slow_frame(&self, event: SlowFrameEvent) {
        self.push(TraceEvent {
            name: "slow_frame",
            started: self.epoch.elapsed(),
            elapsed: Duration::ZERO,
            thread: std::thread::current().id(),
            args: TraceArgs::SlowFrame(event),
        });
    }

    /// Retains every completed render frame's counters, including frames without overruns.
    pub(crate) fn render_work(&self, work: crate::runtime_profile_render::RenderWorkFrame) {
        let mut buffer = self
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if buffer.events.len() == buffer.events.capacity()
            || buffer.render_work.len() == buffer.render_work.capacity()
        {
            buffer.dropped_render_work += 1;
            return;
        }
        let index = buffer.render_work.len() as u32;
        buffer.render_work.push(work);
        buffer.events.push(TraceEvent {
            name: "render_work",
            started: self.epoch.elapsed(),
            elapsed: Duration::ZERO,
            thread: std::thread::current().id(),
            args: TraceArgs::RenderWork(index),
        });
    }

    /// Appends only while the preallocated capacity still has room.
    fn push(&self, event: TraceEvent) {
        let mut buffer = self
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if buffer.events.len() < buffer.events.capacity() {
            buffer.events.push(event);
        } else {
            buffer.dropped_events += 1;
        }
    }
    /// Writes the buffered spans once, after updates stop and before shutdown waits.
    pub(crate) fn flush(&self) {
        if self.flushed.swap(true, Ordering::AcqRel) {
            return;
        }
        let buffer = self
            .events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let result = std::fs::File::create(&self.path).and_then(|file| {
            use std::io::Write;
            let mut writer = std::io::BufWriter::new(file);
            let events = &buffer.events;
            let capacity = events.capacity();
            let work_capacity = buffer.render_work.capacity();
            write!(
                writer,
                "{{\"capacity\":{capacity},\"render_work_capacity\":{work_capacity},\"dropped_events\":{},\"dropped_render_work\":{},\"truncated\":{},\"traceEvents\":[",
                buffer.dropped_events,
                buffer.dropped_render_work,
                buffer.dropped_events != 0 || buffer.dropped_render_work != 0
            )?;
            let mut threads = Vec::new();
            for (index, event) in events.iter().enumerate() {
                if index != 0 {
                    writer.write_all(b",")?;
                }
                let tid = match threads.iter().position(|id| *id == event.thread) {
                    Some(index) => index,
                    None => {
                        threads.push(event.thread);
                        threads.len() - 1
                    }
                };
                let mut record = json!({"name": event.name, "ph": "X", "pid": std::process::id(),
                    "tid": tid, "ts": event.started.as_secs_f64() * 1e6,
                    "dur": event.elapsed.as_secs_f64() * 1e6});
                let args = match &event.args {
                    TraceArgs::None => None,
                    TraceArgs::RenderWork(index) => {
                        let frame = &buffer.render_work[*index as usize];
                        Some(json!({
                        "render_frame_id": frame.sequence,
                        "render_pipelines_queued": frame.work.render_pipelines_queued,
                        "render_pipelines_created": frame.work.render_pipelines_created,
                        "compute_pipelines_created": frame.work.compute_pipelines_created,
                        "shader_modules": frame.work.shader_modules_created,
                        "bind_groups": frame.work.bind_groups_created,
                        "buffer_upload_bytes": frame.work.buffer_upload_bytes,
                        "texture_upload_bytes": frame.work.texture_upload_bytes,
                        "arena_migrations": frame.arena_migrations,
                        "arena_copy_bytes": frame.arena_copy_bytes,
                        "own_queue_submits": frame.work.own_queue_submits,
                        "completion_callbacks": frame.work.completion_callbacks,
                        "staged_uploads": frame.work.staged_uploads,
                        "staged_upload_bytes": frame.work.staged_upload_bytes,
                        "fallback_uploads": frame.work.fallback_uploads,
                        "fallback_upload_bytes": frame.work.fallback_upload_bytes,
                        "staging_buffers": frame.work.staging_buffers,
                        "staging_capacity_bytes": frame.work.staging_capacity_bytes,
                        "timestamp_marker_passes": frame.work.timestamp_marker_passes,
                        "readback_polls": frame.work.readback_polls,
                        "readback_waits": frame.work.readback_waits,
                        "systems": frame.systems.iter().filter(|sample| sample.calls != 0).map(|sample| json!({"name": sample.name, "ms": sample.nanos as f64 / 1e6, "calls": sample.calls})).collect::<Vec<_>>(),
                    }))
                    },
                    TraceArgs::Focus { focused, occluded } => {
                        Some(json!({"focused": focused, "occluded": occluded}))
                    }
                    TraceArgs::SlowFrame(slow) => Some(json!({
                        "violations": slow.reasons(),
                        "frame_ms": slow.frame.as_secs_f64() * 1e3,
                    })),
                };
                if let Some(args) = args {
                    record["ph"] = json!("i");
                    record["s"] = json!("t");
                    record["args"] = args;
                }
                serde_json::to_writer(&mut writer, &record).map_err(std::io::Error::other)?;
            }
            writer.write_all(b"]}")?;
            writer.flush()
        });
        if let Err(error) = result {
            eprintln!("write stage frame trace {}: {error}", self.path.display());
        }
    }
}

impl Drop for FrameTrace {
    fn drop(&mut self) {
        self.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_trace_reserves_less_than_128_mib_for_events_and_render_work() {
        let event_bytes = std::mem::size_of::<TraceEvent>() * TRACE_CAPACITY;
        let work_bytes = std::mem::size_of::<crate::runtime_profile_render::RenderWorkFrame>()
            * RENDER_WORK_CAPACITY;
        assert!(event_bytes + work_bytes <= 128 * 1024 * 1024);
        assert!(std::mem::size_of::<TraceEvent>() < 128);
        eprintln!("trace reserved bytes: events={event_bytes}, render_work={work_bytes}");
    }

    #[test]
    fn render_work_side_storage_preserves_order_and_never_grows_on_overflow() {
        use crate::runtime_profile_render::RenderWorkFrame;

        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("trace.json");
        let trace = FrameTrace::with_capacities(path.clone(), Instant::now(), 8, 2);
        let pointers = {
            let buffer = trace.events.lock().unwrap();
            (buffer.events.as_ptr(), buffer.render_work.as_ptr())
        };
        trace.frame(true, false);
        trace.render_work(RenderWorkFrame {
            sequence: 7,
            ..Default::default()
        });
        trace.record(
            RuntimeStage::WorldStream,
            Instant::now(),
            Duration::from_millis(1),
        );
        trace.render_work(RenderWorkFrame {
            sequence: 9,
            ..Default::default()
        });
        trace.render_work(RenderWorkFrame {
            sequence: 11,
            ..Default::default()
        });
        for _ in 0..8 {
            trace.frame(false, false);
        }
        {
            let buffer = trace.events.lock().unwrap();
            assert_eq!(
                (buffer.events.as_ptr(), buffer.render_work.as_ptr()),
                pointers
            );
            assert_eq!((buffer.events.len(), buffer.events.capacity()), (8, 8));
            assert_eq!(
                (buffer.render_work.len(), buffer.render_work.capacity()),
                (2, 2)
            );
            assert_eq!(buffer.dropped_events, 4);
            assert_eq!(buffer.dropped_render_work, 1);
        }
        trace.flush();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["truncated"], true);
        assert_eq!(saved["dropped_render_work"], 1);
        assert_eq!(saved["traceEvents"][0]["name"], "frame_start");
        assert_eq!(saved["traceEvents"][1]["args"]["render_frame_id"], 7);
        assert_eq!(saved["traceEvents"][2]["dur"], 1000.0);
        assert_eq!(saved["traceEvents"][3]["args"]["render_frame_id"], 9);
    }

    #[test]
    fn full_event_storage_does_not_leave_orphaned_render_payloads() {
        let root = tempfile::tempdir().unwrap();
        let trace =
            FrameTrace::with_capacities(root.path().join("trace.json"), Instant::now(), 1, 2);
        trace.frame(true, false);
        trace.render_work(Default::default());
        let buffer = trace.events.lock().unwrap();
        assert!(buffer.render_work.is_empty());
        assert_eq!(buffer.dropped_render_work, 1);
    }

    #[test]
    /// Keeps the recording bounded while preserving timing and window state.
    fn trace_preserves_thread_spans_and_focus_without_growing() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("trace.json");
        let trace = FrameTrace::with_capacity(path.clone(), Instant::now(), 16);
        trace.frame(false, false);
        trace.slow_frame(SlowFrameEvent {
            reasons: 0b1001,
            frame: Duration::from_millis(12),
        });
        trace.record(
            RuntimeStage::WorldStream,
            Instant::now(),
            Duration::from_millis(2),
        );
        for _ in 0..16 {
            trace.frame(true, false);
        }
        assert_eq!(trace.events.lock().unwrap().events.len(), 16);
        drop(trace);
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved["truncated"], true);
        assert_eq!(saved["traceEvents"][0]["args"]["focused"], false);
        assert_eq!(
            saved["traceEvents"][1]["args"]["violations"],
            "interval+gpu"
        );
        assert_eq!(saved["traceEvents"][2]["dur"], 2000.0);
        assert_eq!(
            saved["traceEvents"][0]["tid"],
            saved["traceEvents"][2]["tid"]
        );
    }
}
