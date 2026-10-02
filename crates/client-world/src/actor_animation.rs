//! Native actor snapshots feed the shared compiled animation owner.
pub use actor_animation::{
    ACTOR_SWING_TICKS, ACTOR_TICK_DURATION, ActorAnimationStats, ActorAnimationStore,
    ActorAnimationView, ActorLifetimeId, ActorRigSnapshot, ActorTickContext, AnimationActor,
    BoneTransform, EntityRigId, HandPhase, MAX_ACTOR_ACTION_HISTORY,
    MAX_CONTROLLER_TRANSITIONS_PER_TICK, MAX_MOLANG_OPS_PER_ACTOR_TICK,
    MAX_MOLANG_OPS_PER_RENDER_FRAME, MAX_MOLANG_OPS_PER_WORLD_TICK, MAX_RUNTIME_BONES_PER_RIG,
    RenderTextureLayer, SkinRenderLayer, WornArmor,
};

use crate::actor_store::ActorSnapshot;

impl AnimationActor for ActorSnapshot {
    fn runtime_id(&self) -> u64 {
        self.runtime_id
    }
    fn spawn_revision(&self) -> u64 {
        self.spawn_revision
    }
    fn kind(&self) -> &render_data::ActorKind {
        &self.kind
    }
    fn position(&self) -> [f32; 3] {
        self.position
    }
    fn previous_position(&self) -> [f32; 3] {
        self.previous_pose.position
    }
    fn velocity(&self) -> [f32; 3] {
        self.velocity
    }
    fn pitch(&self) -> f32 {
        self.pitch
    }
    fn yaw(&self) -> f32 {
        self.yaw
    }
    fn head_yaw(&self) -> f32 {
        self.head_yaw
    }
    fn body_yaw(&self) -> f32 {
        self.body_yaw
    }
    fn on_ground(&self) -> Option<bool> {
        self.on_ground
    }
    fn metadata(&self) -> &std::collections::HashMap<u32, render_data::ActorMetadataValue> {
        &self.metadata
    }
    fn int_properties(&self) -> &std::collections::HashMap<u32, i32> {
        &self.int_properties
    }
    fn float_properties(&self) -> &std::collections::HashMap<u32, f32> {
        &self.float_properties
    }
    fn status(&self) -> &render_data::ActorStatus {
        &self.status
    }
    fn health(&self) -> Option<f32> {
        self.attributes
            .get("minecraft:health")
            .map(|health| health.current)
    }
    fn max_health(&self) -> Option<f32> {
        self.attributes
            .get("minecraft:health")
            .map(|health| health.max)
    }
    fn player_is_sleeping(&self) -> bool {
        self.player_is_sleeping()
    }
    fn target_rotation_is_absolute(&self) -> bool {
        self.target_rotation_is_absolute()
    }
    fn render_scale(&self) -> f32 {
        self.render_scale()
    }
}
