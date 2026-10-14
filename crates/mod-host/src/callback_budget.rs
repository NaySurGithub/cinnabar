//! Deterministic frame, event and session-loading allowances shared across local instances.

use server_experience::runtime::LOAD_FUEL;
use std::sync::{Arc, Mutex};

/// Expensive event and loading lanes each admit one complete callback per frame.
const LARGE_CALLBACKS_PER_FRAME: usize = 1;

#[derive(Default)]
struct Allowance {
    frame: u64,
    terrain: u64,
    /// Terrain callers admitted this frame, and those deferred this and last frame.
    terrain_admitted: Vec<usize>,
    terrain_deferred: Vec<usize>,
    terrain_waiting: Vec<usize>,
    events: usize,
    loads: usize,
}

/// Whether a guest callback ran or must be delivered in a later frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackOutcome {
    Executed,
    Deferred,
}

/// Preserves each callback's full fuel limit instead of trapping on a partial frame allowance.
#[derive(Clone)]
pub struct CallbackBudget(Arc<Mutex<Allowance>>);
impl CallbackBudget {
    /// Creates the shared regular-frame allowance and bounded event/loading lanes.
    pub fn new(fuel: u64) -> Self {
        let budget = Self(Arc::new(Mutex::new(Allowance::default())));
        budget.reset(fuel);
        budget
    }
    /// Starts a new frame before any guest input or lifecycle callback.
    pub fn reset(&self, fuel: u64) {
        let mut allowance = self.0.lock().expect("mod callback budget");
        let waiting = std::mem::take(&mut allowance.terrain_deferred);
        *allowance = Allowance {
            frame: fuel,
            terrain: crate::TERRAIN_FRAME_FUEL,
            terrain_waiting: waiting,
            events: LARGE_CALLBACKS_PER_FRAME,
            loads: LARGE_CALLBACKS_PER_FRAME,
            ..Allowance::default()
        };
    }
    /// Reserves a full terrain callback for load-order position `owner`, keeping room for
    /// callers deferred last frame so no terrain mod waits more than a frame behind others.
    pub(crate) fn reserve_terrain(&self, owner: usize) -> Option<u64> {
        let fuel = crate::TERRAIN_CALLBACK_FUEL;
        let mut remaining = self.0.lock().expect("mod callback budget");
        let held = remaining
            .terrain_waiting
            .iter()
            .filter(|&&waiting| waiting != owner && !remaining.terrain_admitted.contains(&waiting))
            .count() as u64;
        if remaining.terrain < fuel * (1 + held) {
            if !remaining.terrain_deferred.contains(&owner) {
                remaining.terrain_deferred.push(owner);
            }
            return None;
        }
        remaining.terrain -= fuel;
        remaining.terrain_admitted.push(owner);
        Some(fuel)
    }
    /// Reserves a full callback allowance, or defers it without reducing its guest fuel limit.
    pub(crate) fn reserve(&self, fuel: u64) -> Option<u64> {
        let mut remaining = self.0.lock().expect("mod callback budget");
        if fuel == LOAD_FUEL {
            if remaining.loads == 0 {
                return None;
            }
            remaining.loads -= 1;
        } else if fuel > crate::FRAME_FUEL {
            if remaining.events == 0 {
                return None;
            }
            remaining.events -= 1;
        } else {
            if remaining.frame < fuel {
                return None;
            }
            remaining.frame -= fuel;
        }
        Some(fuel)
    }
    /// Returns regular-frame fuel which a completed callback did not use.
    pub(crate) fn refund(&self, fuel: u64) {
        self.0.lock().expect("mod callback budget").frame += fuel;
    }
    /// Returns unused terrain fuel without increasing the ordinary frame allowance.
    pub(crate) fn refund_terrain(&self, fuel: u64) {
        self.0.lock().expect("mod callback budget").terrain += fuel;
    }
    /// Whether the loading lane can admit another hot replacement this frame.
    pub(crate) fn can_load(&self) -> bool {
        self.0.lock().expect("mod callback budget").loads != 0
    }
    /// Reports regular-frame fuel remaining after completed frame callbacks.
    pub fn remaining(&self) -> u64 {
        self.0.lock().expect("mod callback budget").frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terrain_lane_preserves_event_and_regular_allowances() {
        let budget = CallbackBudget::new(mod_api::TOTAL_MOD_FRAME_FUEL);
        let terrain = crate::TERRAIN_CALLBACK_FUEL;
        for owner in 0..2 {
            assert_eq!(budget.reserve_terrain(owner), Some(terrain));
        }
        assert_eq!(budget.reserve_terrain(2), None);
        assert_eq!(budget.remaining(), mod_api::TOTAL_MOD_FRAME_FUEL);
        let event = server_experience::runtime::CALLBACK_FUEL;
        assert_eq!(budget.reserve(event), Some(event));
        assert_eq!(budget.reserve(event), None);
        assert_eq!(budget.reserve(crate::FRAME_FUEL), Some(crate::FRAME_FUEL));
        budget.refund_terrain(terrain - 1);
        assert_eq!(budget.reserve_terrain(2), None);
        budget.refund_terrain(1);
        assert_eq!(budget.reserve_terrain(2), Some(terrain));
    }
    /// Callers that spend fuel every frame never starve the ones after them in load order.
    #[test]
    fn terrain_callers_progress_at_least_every_other_frame() {
        let budget = CallbackBudget::new(mod_api::TOTAL_MOD_FRAME_FUEL);
        let terrain = crate::TERRAIN_CALLBACK_FUEL;
        for callers in 2..=4 {
            let mut last = vec![0; callers];
            for frame in 1..=8 {
                budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
                for (owner, last) in last.iter_mut().enumerate() {
                    if budget.reserve_terrain(owner).is_some() {
                        budget.refund_terrain(terrain / 4);
                        *last = frame;
                    }
                }
                let limit = if callers == 2 { 0 } else { 1 };
                assert!(
                    last.iter().all(|&ran| frame - ran <= limit),
                    "{callers}: {last:?}"
                );
            }
        }
    }
    #[test]
    fn reservations_never_turn_a_partial_frame_allowance_into_a_guest_fuel_limit() {
        let budget = CallbackBudget::new(crate::FRAME_FUEL - 1);
        assert_eq!(budget.reserve(crate::FRAME_FUEL), None);
        assert_eq!(budget.remaining(), crate::FRAME_FUEL - 1);
        assert_eq!(budget.reserve(LOAD_FUEL), Some(LOAD_FUEL));
        assert_eq!(budget.reserve(LOAD_FUEL), None);
        let event = server_experience::runtime::CALLBACK_FUEL;
        assert_eq!(budget.reserve(event), Some(event));
        assert_eq!(budget.reserve(event), None);
        budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
        assert_eq!(budget.reserve(LOAD_FUEL), Some(LOAD_FUEL));
    }
}
