//! Several local mods in one frame. Load order resolves every conflict; cues cross between mods.

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
};

use mod_host::{
    CameraDelta, ControlFrame, GameplayCameraRig, MAX_CUE_INBOX, MAX_LOADED_MODS, ModCue,
    ModGrants, ModHost,
};
use serde::Deserialize;

use super::{ModRuntime, registration::Grants};

/// Selects an ordered set of components, each with its own grants.
pub(super) const SET_ENV: &str = "CINNABAR_MOD_SET";
const MAX_SET_BYTES: usize = 16 * 1024;

/// A mod after the first, loaded only from an explicit set.
pub(super) struct Companion {
    pub host: ModHost,
    pub inbox: VecDeque<ModCue>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModSet {
    version: u32,
    mods: Vec<SetEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetEntry {
    component: PathBuf,
    #[serde(default)]
    grants: Grants,
}

/// Reads a bounded set file; its order is the load order.
pub(super) fn read_set(path: &Path) -> Result<Vec<(PathBuf, ModGrants)>, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    if bytes.len() > MAX_SET_BYTES {
        return Err("mod set exceeds its byte limit".into());
    }
    let set: ModSet =
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid mod set: {error}"))?;
    if set.version != 1 {
        return Err("unsupported mod set version".into());
    }
    if set.mods.is_empty() || set.mods.len() > MAX_LOADED_MODS {
        return Err(format!("a mod set lists 1 to {MAX_LOADED_MODS} components"));
    }
    if let Some(entry) = set.mods.iter().find(|entry| !entry.component.is_absolute()) {
        return Err(format!(
            "mod set component {} must be absolute",
            entry.component.display()
        ));
    }
    Ok(set
        .mods
        .into_iter()
        .map(|entry| (entry.component, ModGrants::from(&entry.grants)))
        .collect())
}

impl ModRuntime {
    pub(super) fn host_count(&self) -> usize {
        1 + self.companions.len()
    }

    /// Index 0 is the first-loaded mod; companions follow in load order.
    pub(super) fn host(&self, index: usize) -> &ModHost {
        match index {
            0 => &self.host,
            _ => &self.companions[index - 1].host,
        }
    }

    pub(super) fn host_mut(&mut self, index: usize) -> &mut ModHost {
        match index {
            0 => &mut self.host,
            _ => &mut self.companions[index - 1].host,
        }
    }

    fn inbox_mut(&mut self, index: usize) -> &mut VecDeque<ModCue> {
        match index {
            0 => &mut self.inbox,
            _ => &mut self.companions[index - 1].inbox,
        }
    }

    /// The first mod in load order that publishes a panel owns the panel and its events.
    pub(super) fn panel_owner(&self) -> usize {
        (0..self.host_count())
            .find(|&index| self.host(index).panel().is_some())
            .unwrap_or(0)
    }

    /// Every loaded mod's reserved keys, kept away from ordinary gameplay.
    pub(super) fn reserved_keys(&self) -> Vec<String> {
        let mut keys: Vec<_> = (0..self.host_count())
            .flat_map(|index| self.host(index).reserved_keys().iter().cloned())
            .collect();
        keys.sort();
        keys.dedup();
        keys
    }

    pub(super) fn take_inbox(&mut self, index: usize) -> Vec<ModCue> {
        self.inbox_mut(index).drain(..).collect()
    }

    /// Queues `cues` for every other mod holding the events grant, dropping the oldest on overflow.
    pub(super) fn route_cues(&mut self, from: usize, cues: &[ModCue]) {
        if cues.is_empty() {
            return;
        }
        for index in (0..self.host_count()).filter(|&index| index != from) {
            if !self.host(index).grants().events {
                continue;
            }
            let inbox = self.inbox_mut(index);
            inbox.extend(cues.iter().cloned());
            while inbox.len() > MAX_CUE_INBOX {
                inbox.pop_front();
            }
        }
    }
}

/// The controls one mod sees: keys reserved by an earlier mod are withheld, and only the
/// panel owner receives panel events.
pub(super) fn claim_controls(
    frame: &ControlFrame,
    claimed: &[String],
    panel_owner: bool,
) -> ControlFrame {
    let free = |keys: &[String]| -> Vec<String> {
        keys.iter()
            .filter(|key| !claimed.contains(key))
            .cloned()
            .collect()
    };
    ControlFrame {
        keys_pressed: free(&frame.keys_pressed),
        keys_held: free(&frame.keys_held),
        events: if panel_owner {
            frame.events.clone()
        } else {
            Vec::new()
        },
        ..frame.clone()
    }
}

/// One frame's combined output; for single-valued outputs the earliest mod wins.
#[derive(Default)]
pub(super) struct Merged {
    pub rig: Option<GameplayCameraRig>,
    pub delta: Option<CameraDelta>,
    pub time_override: Option<u32>,
    pub attack_reach: Option<f32>,
    pub attack_pulse: bool,
    pub commands: Vec<String>,
    pub cues: Vec<ModCue>,
    labels: Vec<String>,
}

impl Merged {
    /// Consumes `host`'s committed output, in load order.
    pub fn absorb(&mut self, host: &mut ModHost) -> Vec<ModCue> {
        let interaction = host.take_interaction();
        self.attack_reach = self.attack_reach.or(interaction.attack_reach);
        self.attack_pulse |= interaction.attack_pulse;
        let delta = host.take_camera_delta();
        self.delta = self.delta.or(delta);
        self.rig = self.rig.or(host.camera_rig());
        self.time_override = self.time_override.or(host.time_override());
        self.commands.extend(host.take_commands());
        if let Some(label) = host.label() {
            self.labels.push(label.to_owned());
        }
        let cues = host.take_cues();
        self.cues.extend(cues.iter().cloned());
        cues
    }

    /// Labels in load order, joined and cut to the host's plain-text limit.
    pub fn label(&self) -> Option<String> {
        if self.labels.is_empty() {
            return None;
        }
        let mut label = self.labels.join(" | ");
        if label.len() > mod_host::MAX_LABEL_BYTES {
            let mut end = mod_host::MAX_LABEL_BYTES;
            while !label.is_char_boundary(end) {
                end -= 1;
            }
            label.truncate(end);
        }
        Some(label)
    }
}

#[cfg(test)]
#[path = "multi_tests.rs"]
mod tests;
