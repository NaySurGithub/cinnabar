use super::*;
use crate::actor::{
    ActorDrawFrame, ActorPresentationGate, ActorRuntimeWitness,
    gpu::{ActorDrawSpan, ActorDrawTracker},
};
use crate::render_work::DeviceWork as _;
use bevy::{ecs::system::RunSystemOnce, prelude::World};

#[test]
fn actor_completion_fences_the_draw_without_an_extra_submission_or_poll() {
    let (device, queue) = wgpu::Device::noop(&Default::default());
    let device = RenderDevice::from(device);
    let queue = RenderQueue(Arc::new(WgpuWrapper::new(queue)));
    queue.submit([]);
    let frame = generic_actor_frame();
    let draw = ActorDrawFrame {
        artwork_identity: frame.artwork.identity(),
        skin_revision: frame.skin_revision,
        geometry_revision: frame.rig.geometry_revision,
        frame_generation: frame.rig.frame_generation,
        draw_generation: 1,
        manifest: Arc::clone(&frame.rig.manifest),
    };
    assert!(draw.is_exact());
    let tracker = ActorDrawTracker::default();
    let span = ActorDrawSpan {
        material: 0,
        page: frame.instance_pages[0],
        first: 0,
        count: 1,
        vertex_count: frame.rig.maximum_vertex_count,
    };
    assert!(tracker.begin(draw.clone(), 1, &[span]));
    tracker.record_draw(1, span);
    let gate = ActorPresentationGate::default();
    let mut world = World::new();
    world.insert_resource(queue);
    world.insert_resource(tracker);
    world.insert_resource(gate.clone());
    world.insert_resource(ActorRuntimeWitness::default());
    let before = crate::render_work::snapshot();
    world
        .run_system_once(super::super::submit_actor_presented_frame)
        .unwrap();
    let work = crate::render_work::snapshot().delta_since(before);
    assert_eq!(work.completion_callbacks, 1);
    assert_eq!(work.own_queue_submits, 0);
    assert_eq!(work.readback_polls, 0);
    assert_eq!(work.readback_waits, 0);
    device.poll_frame().unwrap();
    let acknowledgements = gate.drain();
    assert_eq!(acknowledgements.len(), 1);
    assert!(acknowledgements[0].is_exact());
    assert_eq!(acknowledgements[0].manifest, draw.manifest);
    assert_eq!(acknowledgements[0].draw_generation, draw.draw_generation);
    let before = crate::render_work::snapshot();
    world
        .run_system_once(super::super::submit_actor_presented_frame)
        .unwrap();
    assert_eq!(
        crate::render_work::snapshot().delta_since(before),
        Default::default()
    );
}
