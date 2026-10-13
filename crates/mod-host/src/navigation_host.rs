use super::{State, cinnabar};
use crate::{AutomationControls, TerrainQuery, TerrainRegion};
use anyhow::{Result, bail};

impl cinnabar::extension::terrain::Host for State {
    fn read_region(
        &mut self,
        query: TerrainQuery,
    ) -> Result<Result<Option<TerrainRegion>, String>> {
        self.navigation.calls += 1;
        if self.navigation.calls > super::MAX_IMPORT_WRITES as usize {
            bail!("terrain read budget exhausted");
        }
        if !self.grants.terrain {
            return Ok(Err("terrain capability denied".into()));
        }
        let Some(frame) = self.snapshot.as_ref() else {
            return Ok(Ok(None));
        };
        if !crate::navigation::query_valid(&query, [frame.eye.x, frame.eye.y, frame.eye.z]) {
            return Ok(Err("invalid or out-of-range terrain query".into()));
        }
        if !self.navigation.pending_queries.contains(&query) {
            if self.navigation.pending_queries.len() == mod_api::MAX_TERRAIN_QUERIES {
                return Ok(Err("terrain query budget exhausted".into()));
            }
            self.navigation.pending_queries.push(query);
        }
        let scope = self
            .player_state
            .world
            .as_ref()
            .map_or((frame.session, frame.dimension), |world| {
                (world.session, world.dimension)
            });
        Ok(Ok(self
            .navigation
            .regions
            .iter()
            .find(|region| {
                region.query == query && region.session == scope.0 && region.dimension == scope.1
            })
            .cloned()))
    }
}
impl cinnabar::extension::automation::Host for State {
    fn set_controls(&mut self, value: Option<AutomationControls>) -> Result<Result<(), String>> {
        self.controls.budget(false)?;
        if !self.grants.automation {
            return Ok(Err("automation capability denied".into()));
        }
        if value.is_some()
            && (self.snapshot.is_none()
                || !self.controls.frame.focused
                || self.controls.frame.panel_open)
        {
            return Ok(Err("automation requires focused captured gameplay".into()));
        }
        self.navigation.pending_controls = value;
        Ok(Ok(()))
    }
    fn select_hotbar(&mut self, slot: u8) -> Result<Result<(), String>> {
        self.controls.budget(false)?;
        if !self.grants.hotbar {
            return Ok(Err("hotbar capability denied".into()));
        }
        if slot >= mod_api::AUTOMATION_HOTBAR_SLOTS {
            return Ok(Err("invalid hotbar slot".into()));
        }
        if self.snapshot.is_none() || !self.controls.frame.focused || self.controls.frame.panel_open
        {
            return Ok(Err("hotbar requires focused captured gameplay".into()));
        }
        self.navigation.pending_hotbar = Some(slot);
        Ok(Ok(()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{GameplaySnapshot, GameplayVector3, ModGrants, TerrainPosition};
    use cinnabar::extension::{automation::Host as _, terrain::Host as _};
    fn state(grants: ModGrants) -> State {
        let mut value = State::new(grants, String::new(), Default::default());
        value.snapshot = Some(GameplaySnapshot {
            session: 1,
            dimension: 0,
            eye: GameplayVector3 {
                x: 0.,
                y: 0.,
                z: 0.,
            },
            yaw: 0.,
            pitch: 0.,
            attack_held: false,
            frame_seconds: 0.016,
            players: vec![],
        });
        value.controls.frame.focused = true;
        value
    }
    fn controls() -> AutomationControls {
        AutomationControls {
            forward: true,
            backward: false,
            left: false,
            right: false,
            jump: true,
            sprint: true,
            sneak: false,
            attack: true,
            use_item: true,
        }
    }
    fn query(x: i32) -> TerrainQuery {
        TerrainQuery {
            min: TerrainPosition { x, y: 0, z: 0 },
            max: TerrainPosition {
                x: x + 1,
                y: 1,
                z: 1,
            },
        }
    }
    #[test]
    fn permissions_are_independent_and_denied_by_default() {
        let mut value = state(ModGrants::default());
        assert!(value.read_region(query(0)).unwrap().is_err());
        assert!(value.set_controls(Some(controls())).unwrap().is_err());
        assert!(value.select_hotbar(0).unwrap().is_err());
        assert!(value.navigation.pending_queries.is_empty());
    }
    #[test]
    fn automation_requires_focus_and_hotbar_rejects_invalid_slots() {
        let mut value = state(ModGrants {
            automation: true,
            hotbar: true,
            ..Default::default()
        });
        assert!(value.select_hotbar(9).unwrap().is_err());
        assert!(value.select_hotbar(8).unwrap().is_ok());
        value.controls.frame.focused = false;
        assert!(value.set_controls(Some(controls())).unwrap().is_err());
        assert!(value.select_hotbar(0).unwrap().is_err());
        assert!(value.set_controls(None).unwrap().is_ok());
    }
    #[test]
    fn region_requests_deduplicate_and_stale_sessions_never_cross_the_boundary() {
        let mut value = state(ModGrants {
            terrain: true,
            ..Default::default()
        });
        assert!(value.read_region(query(0)).unwrap().unwrap().is_none());
        assert!(value.read_region(query(0)).unwrap().unwrap().is_none());
        assert_eq!(value.navigation.pending_queries.len(), 1);
        value.navigation.regions.push(TerrainRegion {
            session: 2,
            dimension: 0,
            query: query(0),
            chunks: vec![],
            cells: vec![],
        });
        assert!(value.read_region(query(0)).unwrap().unwrap().is_none());
        value.navigation.regions[0].session = 1;
        assert!(value.read_region(query(0)).unwrap().unwrap().is_some());
    }
    #[test]
    fn terrain_uses_the_observed_world_session_instead_of_the_actor_session() {
        let mut value = state(ModGrants {
            terrain: true,
            ..Default::default()
        });
        value.player_state.world = Some(crate::WorldObservation {
            session: 7,
            dimension: 2,
            world: "fixture".into(),
            position: (0., 0., 0.),
            alive: true,
        });
        value.navigation.regions.push(TerrainRegion {
            session: 7,
            dimension: 2,
            query: query(0),
            chunks: vec![],
            cells: vec![],
        });
        assert!(value.read_region(query(0)).unwrap().unwrap().is_some());
        value.navigation.regions[0].session = 1;
        assert!(value.read_region(query(0)).unwrap().unwrap().is_none());
        value.navigation.regions[0].session = 7;
        value.navigation.regions[0].dimension = 0;
        assert!(value.read_region(query(0)).unwrap().unwrap().is_none());
    }
}
