//! Publishes the session state to Discord Rich Presence.

use std::time::{SystemTime, UNIX_EPOCH};

use bevy::prelude::*;
use discord_presence::{Card, Destination, PresenceWorker, Status};
use launcher::menu::settings_options::DISCORD_PRESENCE_OPTION;
use protocol::launcher_control::ConnectTarget;

use crate::menu::MenuRuntime;
use crate::runtime::world::ClientWorld;
use crate::session::SessionController;

/// Starts the presence worker when a Discord application is registered.
pub(crate) struct DiscordPresencePlugin;

impl Plugin for DiscordPresencePlugin {
    fn build(&self, app: &mut App) {
        let Some(application_id) = discord_presence::APPLICATION_ID else {
            return;
        };
        let logo_text = format!(
            "{} {} (Bedrock {})",
            launcher::PRODUCT_NAME,
            env!("CARGO_PKG_VERSION"),
            protocol::GAME_VERSION
        );
        match PresenceWorker::spawn(application_id, logo_text) {
            Ok(worker) => {
                app.insert_resource(DiscordPresence {
                    worker,
                    published: None,
                })
                .add_systems(
                    Update,
                    publish_presence.after(crate::session::drive_session),
                );
            }
            Err(error) => warn!(%error, "discord rich presence unavailable"),
        }
    }
}

#[derive(Resource)]
struct DiscordPresence {
    worker: PresenceWorker,
    /// What was last sent, so steady frames build no card.
    published: Option<Published>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Off,
    Menus,
    Joining,
    Playing,
}

/// A session generation changes the target, so it re-keys the card alongside the phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Published {
    phase: Phase,
    generation: u64,
}

fn publish_presence(
    menu: Res<MenuRuntime>,
    world: Res<ClientWorld>,
    session: Res<SessionController>,
    mut presence: ResMut<DiscordPresence>,
) {
    let phase = if menu
        .settings_snapshot()
        .0
        .value(DISCORD_PRESENCE_OPTION.name)
        == 0
    {
        Phase::Off
    } else if menu.is_connecting() {
        Phase::Joining
    } else if world.stream.is_some() {
        Phase::Playing
    } else {
        Phase::Menus
    };
    let key = Published {
        phase,
        generation: session.generation(),
    };
    if presence.published == Some(key) {
        return;
    }
    presence.published = Some(key);
    let status = match phase {
        Phase::Off => {
            presence.worker.publish(None);
            return;
        }
        Phase::Menus => Status::Menus,
        Phase::Joining => Status::Joining(destination(session.target())),
        Phase::Playing => Status::Playing {
            destination: destination(session.target()),
            since_unix_ms: unix_ms_now(),
        },
    };
    presence.worker.publish(Some(Card::for_status(&status)));
}

/// Names a server by the address the player chose; Realm and friend ids stay private.
fn destination(target: Option<(&str, bool)>) -> Destination {
    match target {
        None => Destination::Server(None),
        Some((world, true)) => Destination::LocalWorld(world.to_owned()),
        Some((address, false)) => match crate::menu::target_for(address) {
            ConnectTarget::RakNet(_) => Destination::Server(Some(address.trim().to_owned())),
            ConnectTarget::Realm(_) => Destination::Realm,
            ConnectTarget::Friend(_) => Destination::FriendWorld,
            ConnectTarget::Gathering(_) => Destination::Experience,
        },
    }
}

fn unix_ms_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn realm_friend_and_experience_ids_never_reach_the_card() {
        assert_eq!(
            destination(Some(("realm_id/123", false))),
            Destination::Realm
        );
        assert_eq!(
            destination(Some(("friend_xuid/2535", false))),
            Destination::FriendWorld
        );
        let experience = format!("{}abc", launcher::menu::EXPERIENCE_ADDRESS_PREFIX);
        assert_eq!(
            destination(Some((&experience, false))),
            Destination::Experience
        );
    }

    #[test]
    fn servers_keep_the_typed_address_and_local_worlds_their_name() {
        assert_eq!(
            destination(Some((" play.example.net ", false))),
            Destination::Server(Some("play.example.net".into()))
        );
        assert_eq!(
            destination(Some(("realm_id/x", true))),
            Destination::LocalWorld("realm_id/x".into())
        );
        assert_eq!(destination(None), Destination::Server(None));
    }
}
