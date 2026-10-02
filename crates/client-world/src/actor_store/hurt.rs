use protocol::{ActorMetadataValue, ActorStatusEvent, ActorStatusKind, ActorTakeItemEvent};

use super::{ActorApplyResult, ActorSnapshot, ActorStore};

pub use render_data::{
    DEATH_DURATION_TICKS, HURT_DURATION_TICKS, HURT_OVERLAY_ALPHA, PICKUP_DURATION_TICKS,
};

/// Sequences a knockback impulse stays attributable to a hurt event; needs measurement.
const KNOCKBACK_FRESH_SEQUENCES: u64 = 32;

const HURT_DIRECTION_METADATA_KEY: u32 = 12;

/// Most undrained status notices retained; further ones are dropped.
pub const MAX_STATUS_NOTICES: usize = 256;

/// A decoded actor status event with the actor's pose at the time, for particle and sound consumers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorStatusNotice {
    pub runtime_id: u64,
    pub kind: ActorStatusKind,
    pub data: i32,
    /// Actor feet position.
    pub position: [f32; 3],
    /// Bounding-box height, when the actor streams one.
    pub height: Option<f32>,
}

pub use render_data::{ActorPickup, ActorStatus};

impl ActorSnapshot {
    /// Marks the actor dead when its health attribute reaches zero and alive when it recovers.
    pub(super) fn sync_status_from_health(&mut self) {
        let Some(health) = self.attributes.get("minecraft:health") else {
            return;
        };
        if !health.current.is_finite() {
            return;
        }
        if health.current <= 0.0 {
            if !self.status.dead {
                self.status.die();
            }
        } else if self.status.dead {
            self.status.revive();
        }
    }

    fn streamed_hurt_direction(&self) -> Option<f32> {
        match self.metadata.get(&HURT_DIRECTION_METADATA_KEY)? {
            ActorMetadataValue::Byte(value) => Some(f32::from(*value)),
            ActorMetadataValue::Short(value) => Some(f32::from(*value)),
            ActorMetadataValue::Int(value) => Some(*value as f32),
            _ => None,
        }
    }
}

impl ActorStore {
    pub(super) fn apply_status(&mut self, event: ActorStatusEvent) -> ActorApplyResult {
        let Some(actor) = self.actors.get_mut(&event.runtime_id) else {
            return ActorApplyResult::MissingActor;
        };
        if self.status_notices.len() < MAX_STATUS_NOTICES {
            self.status_notices.push(ActorStatusNotice {
                runtime_id: event.runtime_id,
                kind: event.kind,
                data: event.data,
                position: actor.position,
                height: actor.bounding_box().map(|(min, max)| max[1] - min[1]),
            });
        }
        match event.kind {
            ActorStatusKind::Hurt | ActorStatusKind::HurtWithoutDamage => {
                actor.status.hurt_time = HURT_DURATION_TICKS;
                actor.status.skip_red_flash = event.kind == ActorStatusKind::HurtWithoutDamage;
                actor.status.hurt_direction = actor.streamed_hurt_direction();
            }
            ActorStatusKind::Death => {
                if !actor.status.dead {
                    actor.status.die();
                }
            }
            ActorStatusKind::SpawnAlive => actor.status.revive(),
            // Particle-only kinds have no retained actor state.
            _ => {}
        }
        ActorApplyResult::Updated
    }

    pub(super) fn apply_take_item(&mut self, event: ActorTakeItemEvent) -> ActorApplyResult {
        let Some(item) = self.actors.get_mut(&event.item_runtime_id) else {
            return ActorApplyResult::MissingActor;
        };
        item.status.pickup.get_or_insert(ActorPickup {
            collector_runtime_id: event.collector_runtime_id,
            ticks: 0,
        });
        ActorApplyResult::Updated
    }

    /// Drains the status events decoded since the last call, in arrival order.
    pub(crate) fn take_status_notices(&mut self) -> Vec<ActorStatusNotice> {
        std::mem::take(&mut self.status_notices)
    }

    /// Remembers the latest horizontal knockback impulse the local player received.
    pub(crate) fn note_local_knockback(&mut self, sequence: u64, motion: [f32; 3]) {
        if motion[0].hypot(motion[2]) > f32::EPSILON {
            self.local_knockback = Some((sequence, [motion[0], motion[2]]));
        }
    }

    /// Direction toward the damage source: opposite the recent knockback, if one is fresh.
    pub(crate) fn hurt_source_direction(&self, sequence: u64) -> Option<[f32; 2]> {
        let (noted, [x, z]) = self.local_knockback?;
        let length = x.hypot(z);
        (sequence.saturating_sub(noted) <= KNOCKBACK_FRESH_SEQUENCES && length > f32::EPSILON)
            .then(|| [-x / length, -z / length])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hurt_counts_down_and_death_saturates() {
        let mut status = ActorStatus {
            hurt_time: HURT_DURATION_TICKS,
            ..ActorStatus::default()
        };
        for _ in 0..HURT_DURATION_TICKS {
            assert!(status.overlay_active());
            status.tick();
        }
        assert!(!status.overlay_active());

        status.die();
        for _ in 0..(DEATH_DURATION_TICKS + 5) {
            status.tick();
        }
        assert_eq!(status.death_time, DEATH_DURATION_TICKS);
        assert_eq!(status.death_progress(0.9), Some(1.0));
        assert!(status.overlay_active());
    }

    fn spawn() -> protocol::ActorEvent {
        protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
            dimension: 0,
            unique_id: 5,
            runtime_id: 7,
            kind: protocol::ActorKind::Entity {
                identifier: "minecraft:cow".into(),
            },
            position: [0.0; 3],
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            held_item: Default::default(),
            metadata: std::sync::Arc::from([]),
            attributes: std::sync::Arc::from([]),
            properties: std::sync::Arc::from([]),
            links: std::sync::Arc::from([]),
        })
    }

    fn status(kind: ActorStatusKind) -> protocol::ActorEvent {
        protocol::ActorEvent::Status(ActorStatusEvent {
            runtime_id: 7,
            kind,
            data: 0,
        })
    }

    #[test]
    fn hurt_event_arms_the_countdown_and_ticks_down() {
        let mut store = ActorStore::new(1, 0);
        store.apply(1, 1, spawn());
        assert_eq!(
            store.apply(1, 2, status(ActorStatusKind::Hurt)),
            ActorApplyResult::Updated
        );
        assert_eq!(store.get(7).unwrap().status.hurt_time, HURT_DURATION_TICKS);
        store.advance_interpolation_ticks(3);
        assert_eq!(
            store.get(7).unwrap().status.hurt_time,
            HURT_DURATION_TICKS - 3
        );
    }

    /// Event 81 arms the hurt countdown but never the red damage flash; a real hit restores it.
    #[test]
    fn hurt_without_damage_skips_the_red_flash() {
        let mut store = ActorStore::new(1, 0);
        store.apply(1, 1, spawn());
        store.apply(1, 2, status(ActorStatusKind::HurtWithoutDamage));
        let status_now = store.get(7).unwrap().status;
        assert_eq!(status_now.hurt_time, HURT_DURATION_TICKS);
        assert!(!status_now.overlay_active());
        store.apply(1, 3, status(ActorStatusKind::Hurt));
        assert!(store.get(7).unwrap().status.overlay_active());
    }

    #[test]
    fn take_item_starts_pickup_and_saturates() {
        let mut store = ActorStore::new(1, 0);
        store.apply(1, 1, spawn());
        let take = protocol::ActorEvent::TakeItem(ActorTakeItemEvent {
            item_runtime_id: 7,
            collector_runtime_id: 99,
        });
        assert_eq!(store.apply(1, 2, take), ActorApplyResult::Updated);
        store.advance_interpolation_ticks(u32::from(PICKUP_DURATION_TICKS) + 4);
        let status = store.get(7).unwrap().status;
        assert_eq!(
            status.pickup.map(|pickup| pickup.ticks),
            Some(PICKUP_DURATION_TICKS)
        );
        assert_eq!(
            status.pickup.map(|pickup| pickup.collector_runtime_id),
            Some(99)
        );
    }

    #[test]
    fn hurt_source_is_opposite_a_fresh_knockback() {
        let mut store = ActorStore::new(1, 0);
        assert_eq!(store.hurt_source_direction(1), None);
        store.note_local_knockback(5, [2.0, 0.3, 0.0]);
        assert_eq!(store.hurt_source_direction(6), Some([-1.0, 0.0]));
        assert_eq!(
            store.hurt_source_direction(5 + KNOCKBACK_FRESH_SEQUENCES + 1),
            None
        );
    }

    #[test]
    fn death_event_for_unknown_actor_is_missing() {
        let mut store = ActorStore::new(1, 0);
        assert_eq!(
            store.apply(1, 1, status(ActorStatusKind::Death)),
            ActorApplyResult::MissingActor
        );
    }

    #[test]
    fn revive_clears_death() {
        let mut status = ActorStatus::default();
        status.die();
        status.revive();
        assert_eq!(status.death_progress(0.0), None);
        assert!(!status.overlay_active());
    }
}
