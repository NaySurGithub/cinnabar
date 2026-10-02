//! Read-only actor observations: network snapshots and web streams retain their own state.
use render_data::{ActorKind, ActorMetadataValue, ActorStatus};
use std::collections::HashMap;

pub trait AnimationActor {
    fn runtime_id(&self) -> u64;
    fn spawn_revision(&self) -> u64;
    fn kind(&self) -> &ActorKind;
    fn position(&self) -> [f32; 3];
    fn previous_position(&self) -> [f32; 3];
    fn velocity(&self) -> [f32; 3];
    fn pitch(&self) -> f32;
    fn yaw(&self) -> f32;
    fn head_yaw(&self) -> f32;
    fn body_yaw(&self) -> f32;
    fn on_ground(&self) -> Option<bool>;
    fn metadata(&self) -> &HashMap<u32, ActorMetadataValue>;
    fn int_properties(&self) -> &HashMap<u32, i32>;
    fn float_properties(&self) -> &HashMap<u32, f32>;
    fn status(&self) -> &ActorStatus;
    fn health(&self) -> Option<f32>;
    fn max_health(&self) -> Option<f32>;
    fn player_is_sleeping(&self) -> bool {
        render_data::player_is_sleeping(self.metadata())
    }
    fn target_rotation_is_absolute(&self) -> bool {
        render_data::target_rotation_is_absolute(self.kind())
    }
    fn render_scale(&self) -> f32 {
        render_data::actor_render_scale(self.metadata())
    }
    fn flag(&self, bit: u32) -> bool {
        render_data::actor_flag(self.metadata(), bit)
    }
}
