use super::*;
use bevy::render::{render_graph::RenderGraph, renderer::WgpuWrapper};

fn noop_device(features: wgpu::Features) -> (RenderDevice, RenderQueue) {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::NOOP,
        backend_options: wgpu::BackendOptions {
            noop: wgpu::NoopBackendOptions { enable: true },
            ..Default::default()
        },
        ..Default::default()
    });
    let adapter =
        bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .unwrap();
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: features,
        ..Default::default()
    }))
    .unwrap();
    let device = RenderDevice::from(device);
    (
        device.clone(),
        RenderQueue(Arc::new(WgpuWrapper::new(queue))),
    )
}

struct CountingNode(Arc<AtomicU32>);

impl Node for CountingNode {
    fn run<'w>(
        &self,
        _: &mut RenderGraphContext,
        _: &mut RenderContext<'w>,
        _: &'w World,
    ) -> Result<(), NodeRunError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

/// Defers draw-span allocation just as the opaque phase defers command encoding.
struct DeferredDrawNode;

impl Node for DeferredDrawNode {
    fn run<'w>(
        &self,
        _: &mut RenderGraphContext,
        context: &mut RenderContext<'w>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let timestamps = world.resource::<GpuTimestamps>();
        context.add_command_buffer_generation_task(move |device| {
            let span = timestamps
                .open_draw(RuntimeStage::GpuActors)
                .expect("draw recording retains its frame slot until graph finish");
            let mut encoder = device.create_command_encoder(&Default::default());
            encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                    query_set: span.queries,
                    beginning_of_pass_write_index: Some(span.begin),
                    end_of_pass_write_index: Some(span.begin + 1),
                }),
            });
            encoder.finish()
        });
        Ok(())
    }
}

/// A Core3d graph whose opaque node counts runs, wrapped as the plugin does.
fn timed_world() -> (World, Arc<AtomicU32>) {
    let runs = Arc::new(AtomicU32::new(0));
    let mut core = RenderGraph::default();
    core.add_node(Node3d::MainOpaquePass, CountingNode(runs.clone()));
    let mut graph = RenderGraph::default();
    graph.add_sub_graph(Core3d, core);
    let mut world = World::new();
    world.insert_resource(graph);
    wrap_timed_nodes(&mut world);
    (world, runs)
}

/// Runs the wrapped opaque node once and returns the command buffers it recorded.
fn run_opaque(world: &World, device: &RenderDevice) -> Vec<wgpu::CommandBuffer> {
    let graph = world
        .resource::<RenderGraph>()
        .get_sub_graph(Core3d)
        .unwrap();
    let state = graph.get_node_state(Node3d::MainOpaquePass).unwrap();
    assert!(state.node.downcast_ref::<TimedNode>().is_some());
    let mut outputs = [];
    let mut graph_context = RenderGraphContext::new(graph, state, &[], &mut outputs);
    let mut render_context = RenderContext::new(device.clone(), None);
    state
        .node
        .run(&mut graph_context, &mut render_context, world)
        .unwrap();
    resolve::ResolveNode
        .run(&mut graph_context, &mut render_context, world)
        .unwrap();
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    render_context.finish().0
}

#[test]
fn missing_timestamp_feature_still_records_cpu_node_work() {
    let (device, queue) = noop_device(wgpu::Features::empty());
    assert!(GpuTimestamps::new(&device, &queue, true).is_none());
    let (mut world, runs) = timed_world();
    let profiler = RuntimeStageProfiler::new(true);
    world.insert_resource(profiler.clone());
    assert!(run_opaque(&world, &device).is_empty());
    assert_eq!(runs.load(Ordering::Relaxed), 1);
    let snapshot = profiler
        .take_snapshot_if_due(std::time::Duration::ZERO)
        .unwrap();
    assert_eq!(snapshot.samples[RuntimeStage::CpuOpaque as usize].count, 1);
    assert_eq!(snapshot.samples[RuntimeStage::GpuOpaque as usize].count, 0);
}

#[test]
fn timed_frame_is_read_back_on_a_later_frame_without_waiting() {
    let (device, queue) = noop_device(wgpu::Features::TIMESTAMP_QUERY);
    let (mut world, runs) = timed_world();
    let mut timestamps = GpuTimestamps::new(&device, &queue, true).unwrap();
    assert!(!timestamps.draw_spans);
    let mut frames = Vec::new();
    timestamps.begin(|frame| frames.push(*frame));
    world.insert_resource(timestamps);

    let before = crate::render_work::snapshot();
    let buffers = run_opaque(&world, &device);
    assert_eq!(buffers.len(), 1, "begin and end markers share one encoder");
    queue.submit(buffers);
    let mut timestamps = world.remove_resource::<GpuTimestamps>().unwrap();
    assert_eq!(timestamps.frame.passes.load(Ordering::Relaxed), 1);
    assert!(
        timestamps
            .readbacks
            .lock()
            .unwrap()
            .ring
            .oldest_in_flight()
            .is_some()
    );
    let work = crate::render_work::snapshot().delta_since(before);
    assert_eq!(
        work.own_queue_submits, 0,
        "readback shares the frame submission"
    );
    assert_eq!(work.readback_waits, 0);
    assert_eq!(
        work.timestamp_marker_passes, 2,
        "pass boundaries use two isolated markers"
    );
    assert!(frames.is_empty());

    device.poll(wgpu::PollType::Poll).unwrap();
    timestamps.begin(|frame| frames.push(*frame));
    assert_eq!(runs.load(Ordering::Relaxed), 1);
    assert_eq!(frames.len(), 1);
    // The NOOP backend writes no ticks, so the frame decodes to no durations.
    assert_eq!(frames[0].iter().count(), 0);
    assert!(
        timestamps
            .readbacks
            .lock()
            .unwrap()
            .ring
            .oldest_in_flight()
            .is_none()
    );
}

#[test]
fn encoder_capability_keeps_isolated_markers_and_gameplay_diagnostics() {
    let features =
        wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
    let (device, queue) = noop_device(features);
    let (mut world, runs) = timed_world();
    let profiler = RuntimeStageProfiler::for_gameplay(false, None);
    world.insert_resource(profiler.clone());
    let mut timestamps = GpuTimestamps::new(&device, &queue, false).unwrap();
    timestamps.begin(|_| unreachable!("first frame has no readback"));
    world.insert_resource(timestamps);
    let before = crate::render_work::snapshot();
    let buffers = run_opaque(&world, &device);
    assert_eq!(buffers.len(), 1);
    queue.submit(buffers);
    device.poll(wgpu::PollType::Poll).unwrap();
    world
        .resource_mut::<GpuTimestamps>()
        .begin(|frame| profiler.record_gpu_frame(frame));
    assert_eq!(runs.load(Ordering::Relaxed), 1);
    assert!(
        profiler.latest_gpu_frame().is_some(),
        "normal play keeps the F3 GPU sample"
    );
    let work = crate::render_work::snapshot().delta_since(before);
    assert_eq!(
        work.timestamp_marker_passes, 2,
        "markers must remain isolated from scene passes"
    );
    assert_eq!(work.own_queue_submits, 0);
    assert_eq!(work.readback_waits, 0);
}

#[test]
fn deferred_draw_spans_resolve_once_after_command_generation() {
    use bevy::ecs::system::RunSystemOnce;
    let features = wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES;
    let (device, queue) = noop_device(features);
    let (mut world, _) = timed_world();
    world
        .resource_mut::<RenderGraph>()
        .get_sub_graph_mut(Core3d)
        .unwrap()
        .add_node(
            Node3d::MainOpaquePass,
            TimedNode {
                inner: Box::new(DeferredDrawNode),
                stage: RuntimeStage::GpuOpaque,
            },
        );
    let mut timestamps = GpuTimestamps::new(&device, &queue, true).unwrap();
    timestamps.begin(|_| unreachable!("first frame has no readback"));
    world.insert_resource(timestamps);
    world.insert_resource(device.clone());
    world.insert_resource(queue.clone());

    let buffers = run_opaque(&world, &device);
    let timestamps = world.resource::<GpuTimestamps>();
    assert_eq!(timestamps.frame.draws.load(Ordering::Relaxed), 1);
    assert_ne!(timestamps.frame.slot.load(Ordering::Acquire), NO_SLOT);
    queue.submit(buffers);
    let before = crate::render_work::snapshot();
    world.run_system_once(resolve::submit_draw_frame).unwrap();
    let work = crate::render_work::snapshot().delta_since(before);
    assert_eq!(
        work.own_queue_submits, 1,
        "per-draw profiling resolves after deferred work"
    );
    assert_eq!(work.readback_waits, 0);
    {
        let readbacks = world.resource::<GpuTimestamps>().readbacks.lock().unwrap();
        let index = readbacks.ring.oldest_in_flight().unwrap();
        let slot = &readbacks.slots[index];
        assert_eq!((slot.passes, slot.draws), (1, 1));
        assert_eq!(slot.stages[PASS_SPANS as usize], RuntimeStage::GpuActors);
    }
    world.run_system_once(resolve::submit_draw_frame).unwrap();
    assert_eq!(
        crate::render_work::snapshot()
            .delta_since(before)
            .own_queue_submits,
        1
    );
    device.poll(wgpu::PollType::Poll).unwrap();
    let mut completed = 0;
    world
        .resource_mut::<GpuTimestamps>()
        .begin(|_| completed += 1);
    assert_eq!(completed, 1);
}

#[test]
fn empty_profiled_frames_release_slots_without_submitting() {
    use bevy::ecs::system::RunSystemOnce;
    let features = wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES;
    let (device, queue) = noop_device(features);
    let mut world = World::new();
    world.insert_resource(GpuTimestamps::new(&device, &queue, true).unwrap());
    world.insert_resource(device);
    world.insert_resource(queue);
    let before = crate::render_work::snapshot();
    for _ in 0..SLOTS * 2 {
        world
            .resource_mut::<GpuTimestamps>()
            .begin(|_| unreachable!("empty frame"));
        world.run_system_once(resolve::submit_draw_frame).unwrap();
        let timestamps = world.resource::<GpuTimestamps>();
        assert_eq!(timestamps.frame.slot.load(Ordering::Acquire), NO_SLOT);
        assert!(
            timestamps
                .readbacks
                .lock()
                .unwrap()
                .ring
                .oldest_in_flight()
                .is_none()
        );
    }
    assert_eq!(
        crate::render_work::snapshot()
            .delta_since(before)
            .own_queue_submits,
        0
    );
}

#[test]
fn frame_without_spans_releases_its_slot() {
    let (device, queue) = noop_device(wgpu::Features::TIMESTAMP_QUERY);
    let mut timestamps = GpuTimestamps::new(&device, &queue, false).unwrap();
    for _ in 0..SLOTS * 2 {
        timestamps.begin(|_| unreachable!("no frame was submitted"));
        let mut encoder = device.create_command_encoder(&Default::default());
        timestamps.encode(&mut encoder);
        assert!(
            timestamps
                .readbacks
                .lock()
                .unwrap()
                .ring
                .oldest_in_flight()
                .is_none()
        );
    }
}

#[test]
fn draw_overflow_drops_categories_but_keeps_passes() {
    let features = wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES;
    let (device, queue) = noop_device(features);
    assert!(
        !GpuTimestamps::new(&device, &queue, false)
            .unwrap()
            .draw_spans,
        "per-draw spans stay off without aggregate profiling"
    );
    let mut timestamps = GpuTimestamps::new(&device, &queue, true).unwrap();
    timestamps.begin(|_| {});
    assert!(timestamps.open_pass(RuntimeStage::GpuOpaque).is_some());
    let draws = (0..=DRAW_SPANS)
        .filter(|_| timestamps.open_draw(RuntimeStage::GpuActors).is_some())
        .count();
    assert_eq!(draws, DRAW_SPANS as usize);
    let mut encoder = device.create_command_encoder(&Default::default());
    timestamps.encode(&mut encoder);
    let readbacks = timestamps.readbacks.lock().unwrap();
    let slot = readbacks.ring.oldest_in_flight().unwrap();
    assert_eq!(
        (readbacks.slots[slot].passes, readbacks.slots[slot].draws),
        (1, 0)
    );
    assert_eq!(readbacks.slots[slot].stages[0], RuntimeStage::GpuOpaque);
}

/// Pipelined rendering removes the render app during cleanup, before later plugins' hooks.
#[test]
fn nodes_are_wrapped_inside_the_render_app_under_pipelined_rendering() {
    use bevy::render::pipelined_rendering::{PipelinedRenderingPlugin, RenderAppChannels};
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    let (device, queue) = noop_device(wgpu::Features::empty());
    let mut core = RenderGraph::default();
    core.add_node(Node3d::MainOpaquePass, CountingNode(Arc::default()));
    let mut graph = RenderGraph::default();
    graph.add_sub_graph(Core3d, core);
    let mut render_app = bevy::app::SubApp::new();
    render_app
        .add_schedule(bevy::ecs::schedule::Schedule::new(RenderStartup))
        .insert_resource(graph)
        .insert_resource(device.clone())
        .insert_resource(queue);
    let mut app = App::new();
    app.insert_sub_app(RenderApp, render_app);
    app.insert_resource(RuntimeStageProfiler::new(false));
    app.add_plugins((PipelinedRenderingPlugin, GpuTimingPlugin));
    app.finish();
    app.cleanup();
    assert!(app.get_sub_app(RenderApp).is_none());

    let mut channels = app
        .world_mut()
        .remove_resource::<RenderAppChannels>()
        .unwrap();
    let mut render_app = bevy::tasks::block_on(channels.recv()).unwrap();
    render_app.world_mut().run_schedule(RenderStartup);
    let graph = render_app.world().resource::<RenderGraph>();
    let state = graph
        .get_sub_graph(Core3d)
        .unwrap()
        .get_node_state(Node3d::MainOpaquePass)
        .unwrap();
    assert!(state.node.downcast_ref::<TimedNode>().is_some());
}
