//! After the frame's shared crosshair pick: the target, mining and harvest facts and the
//! client's text for every package granted them, and the HUD owner's layer for the
//! presentation. The look is rebuilt only when the pick or the targeted actor's shown facts
//! change, so an unchanged target costs a key comparison and delivers nothing.

use std::{sync::Arc, time::Instant};

use bevy::prelude::*;
use client_ui::ui_runtime::{
    UiRuntime,
    presentation::{ModHudInput, ModText, UiPresentationRuntime},
};
use mod_host::{
    GuestItemKey, GuestStack, HarvestRules, ModHost, TargetActorHit, TargetBlock, TargetBlockHit,
    TargetBlockPos, TargetBlockState, TargetFrame, TargetGameMode, TargetHarvest, TargetHit,
    TargetLiquidHit, TargetLook, TargetMining, TargetStateValue, TextSource,
};
use protocol::{ActorKind, PlayerGameMode};
use server_experience::session_data::SessionData;
use sim::{BlockDestroyInfo, HeldTool, PaletteWorld};

use super::ModRuntime;
use crate::{
    block_selection::{CrosshairPick, CrosshairTarget},
    movement::PhysicsCollisionRegistries,
    runtime::world::ClientWorld,
    survival_mining::SurvivalMiningRuntime,
};

/// How long one simulation tick lasts, for the mining progress's partial tick.
const TICK_SECONDS: f32 = 1.0 / sim::TICKS_PER_SECOND as f32;

/// What the adapter keeps between frames.
#[derive(Default)]
pub(super) struct TargetState {
    /// The look last built and the key it was built for.
    look: Option<(LookKey, Option<TargetLook>)>,
    /// The destroy progress last seen and when it moved there.
    progress: Option<(f64, Instant)>,
    /// The text services last handed to the mods.
    text: Option<Arc<ModText>>,
}

/// What a look depends on: the pick, the liquid on its ray, the eye, the game mode and the
/// targeted actor's shown facts.
#[derive(Clone, Debug, PartialEq)]
struct LookKey {
    block: Option<([i32; 3], u32)>,
    liquid: Option<([i32; 3], u32)>,
    eye_in_liquid: bool,
    game_mode: Option<PlayerGameMode>,
    actor: Option<(u64, Option<Arc<str>>, Option<(f32, f32)>, Option<i32>)>,
}

/// One frame for the packages: the HUD owner's layer and layout, and the target and text for
/// every package granted `target`.
#[allow(
    clippy::too_many_arguments,
    reason = "Player authority is borrowed separately from UI state."
)]
pub(super) fn publish(
    player: Res<crate::player_runtime::PlayerRuntime>,
    extension: Option<ResMut<ModRuntime>>,
    mut crosshair: ResMut<CrosshairTarget>,
    world: Res<ClientWorld>,
    collisions: Res<PhysicsCollisionRegistries>,
    ui: Res<UiRuntime>,
    mining: Res<SurvivalMiningRuntime>,
    mut presentation: ResMut<UiPresentationRuntime>,
    time: Res<Time<Real>>,
) {
    let Some(mut extension) = extension else {
        return;
    };
    publish_hud(&mut extension, &mut presentation);
    let readers: Vec<usize> = (0..extension.host_count())
        .filter(|&index| {
            let host = extension.host(index);
            host.package().is_some() && host.is_active() && host.grants().target
        })
        .collect();
    if crosshair.wants_actor != !readers.is_empty() {
        crosshair.wants_actor = !readers.is_empty();
    }
    if let Some(text) = presentation.mod_text()
        && extension
            .target
            .text
            .as_ref()
            .is_none_or(|current| !Arc::ptr_eq(current, &text))
    {
        extension.target.text = Some(Arc::clone(&text));
        for index in 0..extension.host_count() {
            if extension.host(index).package().is_some() {
                extension
                    .host_mut(index)
                    .set_text(Arc::new(AppText(Arc::clone(&text))));
            }
        }
    }
    if readers.is_empty() {
        extension.target.look = None;
        return;
    }
    let facts = Facts {
        player: &player,
        world: &world,
        collisions: &collisions,
        ui: &ui,
    };
    let frame = target_frame(
        &mut extension.target,
        &facts,
        crosshair.pick.as_ref(),
        &mining,
    );
    let seconds = time.delta_secs();
    for index in readers {
        if let Err(error) = extension.host_mut(index).set_target(frame.clone(), seconds) {
            eprintln!("Cinnabar mod target-changed failed: {error:#}");
        }
    }
}

/// The HUD owner by the screens' rule, its layout from the last build, and its layer for this
/// one; a refused template quarantines it.
fn publish_hud(extension: &mut ModRuntime, presentation: &mut UiPresentationRuntime) {
    let owner = extension.hud_owner();
    let layout = presentation.mod_hud_layout().cloned();
    for index in 0..extension.host_count() {
        let host = extension.host_mut(index);
        if !host.grants().hud || host.package().is_none() {
            continue;
        }
        let shown = (Some(index) == owner).then(|| layout.clone()).flatten();
        if let Err(error) = host.set_hud_layout(shown) {
            eprintln!("Cinnabar mod hud-changed failed: {error:#}");
        }
    }
    let Some(owner) = owner else {
        presentation.set_mod_hud(None);
        return;
    };
    let host = extension.host_mut(owner);
    if let Some(reason) = presentation.mod_hud_failure() {
        eprintln!("Cinnabar mod HUD refused, mod quarantined: {reason}");
        host.quarantine();
    }
    publish_layer(host, presentation);
}

fn publish_layer(host: &ModHost, presentation: &mut UiPresentationRuntime) {
    let (Some(package), true) = (host.package(), host.is_active()) else {
        presentation.set_mod_hud(None);
        return;
    };
    presentation.set_mod_hud(Some(ModHudInput {
        id: &package.id,
        files: &package.files,
        data: host.hud(),
    }));
}

/// The client's text as a mod's `text` import reads it.
struct AppText(Arc<ModText>);

impl TextSource for AppText {
    fn translate(&self, key: &str) -> Option<String> {
        self.0.translate(key)
    }

    fn width(&self, text: &str) -> f32 {
        self.0.width(text)
    }
}

/// What the look is assembled from.
struct Facts<'a> {
    player: &'a crate::player_runtime::PlayerRuntime,
    world: &'a ClientWorld,
    collisions: &'a PhysicsCollisionRegistries,
    ui: &'a UiRuntime,
}

fn target_frame(
    state: &mut TargetState,
    facts: &Facts<'_>,
    pick: Option<&CrosshairPick>,
    mining: &SurvivalMiningRuntime,
) -> TargetFrame {
    let Some((pick, stream)) = pick.zip(facts.world.stream.as_ref()) else {
        state.look = None;
        return TargetFrame::default();
    };
    let mode = stream.network_id_mode();
    let palette = PaletteWorld::new(
        stream.collision_store(),
        facts.collisions.registry(mode),
        stream.current_dimension(),
    );
    let height = |depth: u8, covered: bool| {
        if covered {
            1.0
        } else {
            meshing::LiquidLevel::from_variant(u32::from(depth)).map_or(1.0, |level| {
                f64::from(level.height()) / f64::from(meshing::LiquidLevel::FULL_HEIGHT)
            })
        }
    };
    let liquid = palette
        .liquid_ray(pick.origin, pick.direction, pick.reach, height)
        .ok()
        .flatten();
    let eye_in_liquid = palette
        .point_in_liquid(pick.origin, height)
        .unwrap_or(false);
    let authority = stream.authority();
    let actor = pick.actor.and_then(|hit| authority.actor(hit.runtime_id));
    let key = LookKey {
        block: pick
            .block
            .as_ref()
            .map(|hit| (hit.block_pos, hit.runtime_id)),
        liquid: liquid.map(|hit| (hit.block_pos, hit.runtime_id)),
        eye_in_liquid,
        game_mode: facts.player.facts.player_game_mode(),
        actor: actor.map(|actor| {
            (
                actor.runtime_id,
                authority.actor_name_tag(actor.unique_id),
                authority.actor_health_by_unique(actor.unique_id),
                authority
                    .dropped_item_stack(actor.runtime_id)
                    .map(|item| item.identity.network_id),
            )
        }),
    };
    if state
        .look
        .as_ref()
        .is_none_or(|(current, _)| *current != key)
    {
        let look = build_look(facts, pick, liquid, eye_in_liquid);
        state.look = Some((key, look));
    }
    let look = state.look.as_ref().and_then(|(_, look)| look.clone());
    let held = held_tool(facts);
    let harvest = pick
        .block
        .as_ref()
        .and_then(|hit| destroy_info(facts, hit.runtime_id))
        .and_then(|block| {
            let requires_tool = block.requires_tool()?;
            let breakable = block.hardness >= 0.0
                && facts.player.facts.player_game_mode() != Some(PlayerGameMode::Adventure);
            Some(Arc::new(Harvest {
                block,
                requires_tool,
                breakable,
                held,
            }) as Arc<dyn HarvestRules>)
        });
    TargetFrame {
        look,
        mining: mining_state(state, facts, &palette, mining, held),
        harvest,
    }
}

/// The local break in progress, with how far this frame is into its tick: the time since the
/// progress last moved, in ticks.
fn mining_state(
    state: &mut TargetState,
    facts: &Facts<'_>,
    palette: &PaletteWorld<'_>,
    mining: &SurvivalMiningRuntime,
    held: Option<HeldTool>,
) -> Option<TargetMining> {
    let Some(progress) = mining.destroy_progress() else {
        state.progress = None;
        return None;
    };
    let now = Instant::now();
    let moved = match state.progress {
        Some((value, at)) if value == progress.progress => at,
        _ => now,
    };
    state.progress = Some((progress.progress, moved));
    let partial_tick = ((now - moved).as_secs_f32() / TICK_SECONDS).min(1.0);
    let harvestable = palette
        .primary_runtime_id(progress.position)
        .ok()
        .and_then(|runtime_id| destroy_info(facts, runtime_id))
        .is_some_and(|block| block.harvestable_with(held));
    let [x, y, z] = progress.position;
    Some(TargetMining {
        position: TargetBlockPos { x, y, z },
        progress: progress.progress as f32,
        per_tick: progress.per_tick as f32,
        partial_tick,
        harvestable,
    })
}

fn destroy_info(facts: &Facts<'_>, runtime_id: u32) -> Option<BlockDestroyInfo> {
    let mode = facts.world.stream.as_ref()?.network_id_mode();
    facts
        .collisions
        .block_identifier(mode, runtime_id)
        .and_then(sim::block_destroy_info)
}

/// The held item's tool class, as survival mining classifies it.
fn held_tool(facts: &Facts<'_>) -> Option<HeldTool> {
    let selection = crate::mining::hand_interaction_selection(facts.player)?;
    let network_id = selection.item.network_id();
    if network_id == 0 {
        return None;
    }
    let ledger = facts.ui.inventory_ledger(facts.player);
    let entry = ledger.negotiated_item_entry(network_id)?;
    HeldTool::from_identifier(&entry.identifier)
}

/// The targeted block's harvest rules, which the `target.harvest` import asks.
struct Harvest {
    block: BlockDestroyInfo,
    requires_tool: bool,
    breakable: bool,
    held: Option<HeldTool>,
}

impl HarvestRules for Harvest {
    fn harvest(&self, candidates: &[String]) -> TargetHarvest {
        TargetHarvest {
            requires_tool: self.requires_tool,
            instant: self.block.hardness == 0.0,
            breakable: self.breakable,
            held_can_harvest: self.block.harvestable_with(self.held),
            effective: candidates
                .iter()
                .filter(|tool| {
                    HeldTool::from_identifier(tool).is_some_and(|tool| self.block.suits(tool))
                })
                .cloned()
                .collect(),
        }
    }
}

fn build_look(
    facts: &Facts<'_>,
    pick: &CrosshairPick,
    liquid: Option<sim::LiquidHit>,
    eye_in_liquid: bool,
) -> Option<TargetLook> {
    let hit = match (pick.actor, &pick.block) {
        (Some(actor), _) => actor_hit(facts, actor).map(TargetHit::Actor),
        (None, Some(block)) => Some(TargetHit::Block(TargetBlockHit {
            position: position(block.block_pos),
            face: block.face,
            distance: block.distance as f32,
            block: block_facts(facts, block.runtime_id)?,
        })),
        (None, None) => None,
    };
    let liquid = liquid.and_then(|hit| {
        Some(TargetLiquidHit {
            position: position(hit.block_pos),
            distance: hit.distance as f32,
            block: block_facts(facts, hit.runtime_id)?,
            source: hit.source,
        })
    });
    Some(TargetLook {
        hit,
        liquid,
        eye_in_liquid,
        game_mode: match facts.player.facts.player_game_mode() {
            Some(PlayerGameMode::Creative) => TargetGameMode::Creative,
            Some(PlayerGameMode::Adventure) => TargetGameMode::Adventure,
            Some(PlayerGameMode::Spectator) => TargetGameMode::Spectator,
            _ => TargetGameMode::Survival,
        },
    })
}

const fn position([x, y, z]: [i32; 3]) -> TargetBlockPos {
    TargetBlockPos { x, y, z }
}

/// A block's identifier, states, display name, picked item and hardness. The name and the
/// picked item are the block's own identifier as an item, provisionally.
fn block_facts(facts: &Facts<'_>, runtime_id: u32) -> Option<TargetBlock> {
    let mode = facts.world.stream.as_ref()?.network_id_mode();
    let identifier = facts
        .collisions
        .block_identifier(mode, runtime_id)?
        .to_owned();
    let states = facts
        .collisions
        .block_canonical_state(mode, runtime_id)
        .map(states)
        .unwrap_or_default();
    Some(TargetBlock {
        name: facts.ui.localized_item_name(&identifier),
        pick: item_stack(facts, &identifier, 0, 1),
        hardness: sim::block_destroy_info(&identifier).map(|info| info.hardness),
        identifier,
        states,
    })
}

/// Canonical state JSON, plain or typed, as named values.
fn states(canonical: &str) -> Vec<TargetBlockState> {
    let Ok(serde_json::Value::Object(states)) = serde_json::from_str(canonical) else {
        return Vec::new();
    };
    states
        .into_iter()
        .filter_map(|(name, value)| {
            let value = value.get("value").cloned().unwrap_or(value);
            let value = match value {
                serde_json::Value::Bool(value) => TargetStateValue::Boolean(value),
                serde_json::Value::Number(number) => {
                    TargetStateValue::Int(i32::try_from(number.as_i64()?).ok()?)
                }
                serde_json::Value::String(text) => TargetStateValue::Text(text),
                _ => return None,
            };
            Some(TargetBlockState { name, value })
        })
        .collect()
}

/// The registry's item named `identifier`, as a session stack with its icon key.
fn item_stack(facts: &Facts<'_>, identifier: &str, aux: u16, count: u8) -> Option<GuestStack> {
    let ledger = facts.ui.inventory_ledger(facts.player);
    let entry = ledger
        .negotiated_item_registry()?
        .values()
        .find(|entry| entry.identifier.as_ref() == identifier)?;
    Some(GuestStack {
        key: GuestItemKey {
            identifier: identifier.to_owned(),
            aux,
        },
        icon: SessionData::icon_key(entry.network_id, aux),
        count,
    })
}

fn actor_hit(facts: &Facts<'_>, hit: gameplay::melee::ActorHit) -> Option<TargetActorHit> {
    let authority = facts.world.stream.as_ref()?.authority();
    let actor = authority.actor(hit.runtime_id)?;
    let type_id = match &actor.kind {
        ActorKind::Player { .. } => "minecraft:player".to_owned(),
        ActorKind::Entity { identifier } => identifier.to_string(),
    };
    let path = type_id
        .split_once(':')
        .map_or(type_id.as_str(), |(_, path)| path);
    let type_name = facts
        .ui
        .translator()
        .lookup(&format!("entity.{path}.name"))
        .map_or_else(|| type_id.clone(), |name| name.to_string());
    let item = authority
        .dropped_item_stack(hit.runtime_id)
        .and_then(|item| {
            let identity = &item.identity;
            let identifier = item.identifier.as_deref()?;
            let aux = u16::try_from(identity.metadata).ok()?;
            let count = u8::try_from(identity.count).unwrap_or(u8::MAX);
            item_stack(facts, identifier, aux, count)
        });
    let health = authority.actor_health_by_unique(actor.unique_id);
    Some(TargetActorHit {
        runtime_id: hit.runtime_id,
        type_id,
        type_name,
        name_tag: authority
            .actor_name_tag(actor.unique_id)
            .map(|name| name.to_string()),
        item,
        health: health.map(|(current, _)| current),
        max_health: health.map(|(_, max)| max),
        distance: hit.distance as f32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_states_read_plain_and_typed_values() {
        let states = states(
            r#"{"age": 3, "open_bit": {"type": "byte", "value": 1},
                "wood_type": "oak", "powered": true, "odd": [1]}"#,
        );
        let value = |name: &str| {
            states
                .iter()
                .find(|state| state.name == name)
                .map(|state| state.value.clone())
        };
        assert_eq!(value("age"), Some(TargetStateValue::Int(3)));
        assert_eq!(value("open_bit"), Some(TargetStateValue::Int(1)));
        assert_eq!(
            value("wood_type"),
            Some(TargetStateValue::Text("oak".into()))
        );
        assert_eq!(value("powered"), Some(TargetStateValue::Boolean(true)));
        assert_eq!(value("odd"), None);
        assert!(super::states("not json").is_empty());
    }
}
