//! `player-mod`'s HUD layer, crosshair target and text. HUD writes stage into a copy of the
//! committed layer, apart from the screens' data, so a HUD change never rebuilds them; they
//! share the callback's host-call and output budgets with every other write.

use super::{
    State,
    cinnabar::{
        extension::{hud_layer, screen},
        server_experience::ui,
        session::{target, text},
    },
    player_mod::{host_value, set_collection, set_value, value_bytes},
};
use crate::{MAX_HARVEST_CANDIDATES, MAX_TEXT_BYTES};
use anyhow::{Result, ensure};
use server_experience::{
    policy::MAX_HOST_OUTPUT,
    screen::{HudLayout, Modal},
};

impl State {
    /// Applies one HUD write of `bytes` to this callback's copy of the layer, within the output
    /// budget; a denied or refused write changes nothing.
    fn stage_hud(
        &mut self,
        bytes: usize,
        write: impl FnOnce(&mut Modal, &Self) -> Result<(), String>,
    ) -> Result<Result<(), String>> {
        self.charge()?;
        if !self.grants.hud {
            return Ok(Err("hud permission denied".into()));
        }
        if bytes > MAX_HOST_OUTPUT - self.output {
            return Ok(Err("host output budget exceeded".into()));
        }
        let mut hud = self.pending_hud.clone().unwrap_or_else(|| self.hud.clone());
        let result = write(&mut hud, self).and_then(|()| hud.check().map_err(|e| e.to_string()));
        if result.is_ok() {
            self.output += bytes;
            self.pending_hud = Some(hud);
        }
        Ok(result)
    }
}

pub(super) fn hud_layout(layout: &HudLayout) -> hud_layer::HudLayout {
    hud_layer::HudLayout {
        size: ui::GuiSize {
            width: layout.size.width,
            height: layout.size.height,
            scale: layout.size.scale,
        },
        boss_bars: layout.boss_bars.as_ref().map(|rect| screen::Rect {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        }),
    }
}

impl hud_layer::Host for State {
    fn layout(&mut self) -> Result<Option<hud_layer::HudLayout>> {
        self.charge()?;
        Ok(self.hud_layout.as_ref().map(hud_layout))
    }

    fn frame_seconds(&mut self) -> Result<f32> {
        self.charge()?;
        Ok(self.frame_seconds)
    }

    fn set_template(&mut self, template: Option<String>) -> Result<Result<(), String>> {
        self.stage_hud(0, |hud, state| {
            state.template(template.as_ref())?;
            hud.template = template;
            hud.revision += 1;
            Ok(())
        })
    }

    fn set_collection(&mut self, name: String, rows_json: Vec<u8>) -> Result<Result<(), String>> {
        let bytes = rows_json.len();
        self.stage_hud(bytes, |hud, _| set_collection(hud, name, &rows_json))
    }

    fn set_value(&mut self, name: String, value: ui::Value) -> Result<Result<(), String>> {
        let value = host_value(value);
        self.stage_hud(value_bytes(&name, &value), |hud, _| {
            set_value(hud, name, value)
        })
    }
}

impl target::Host for State {
    fn revision(&mut self) -> Result<Result<u64, String>> {
        let revision = self.target_revision;
        Ok(self.read(self.grants.target, "target")?.map(|()| revision))
    }

    fn current(&mut self) -> Result<Result<Option<target::Look>, String>> {
        let granted = self.read(self.grants.target, "target")?;
        Ok(granted.map(|()| self.target.look.clone()))
    }

    fn mining(&mut self) -> Result<Result<Option<target::MiningState>, String>> {
        let granted = self.read(self.grants.target, "target")?;
        Ok(granted.map(|()| self.target.mining.clone()))
    }

    fn harvest(
        &mut self,
        candidates: Vec<String>,
    ) -> Result<Result<Option<target::HarvestFacts>, String>> {
        let granted = self.read(self.grants.target, "target")?;
        Ok(granted.map(|()| {
            let candidates = &candidates[..candidates.len().min(MAX_HARVEST_CANDIDATES)];
            self.target
                .harvest
                .as_ref()
                .map(|rules| rules.harvest(candidates))
        }))
    }
}

impl text::Host for State {
    fn translate(&mut self, key: String) -> Result<Option<String>> {
        self.charge()?;
        ensure!(key.len() <= MAX_TEXT_BYTES, "translation key too long");
        Ok(self.text.as_ref().and_then(|text| text.translate(&key)))
    }

    fn width(&mut self, text: String) -> Result<f32> {
        self.charge()?;
        ensure!(text.len() <= MAX_TEXT_BYTES, "measured text too long");
        Ok(self.text.as_ref().map_or(0.0, |source| source.width(&text)))
    }
}
