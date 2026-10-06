//! What the activity card says for each client state.

/// Where a session is played.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Destination {
    /// A dedicated server, by the address the player joined when known.
    Server(Option<String>),
    Realm,
    FriendWorld,
    Experience,
    /// A local world, by name.
    LocalWorld(String),
}

/// The client state the card describes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Menus,
    Joining(Destination),
    Playing {
        destination: Destination,
        /// Unix milliseconds when the session went live, for Discord's elapsed timer.
        since_unix_ms: i64,
    },
}

/// One published activity card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    /// First line.
    pub details: String,
    /// Second line, omitted when there is nothing to add.
    pub state: Option<String>,
    pub started_unix_ms: Option<i64>,
}

/// Discord rejects card text outside 2..=128 characters.
const TEXT_MIN_CHARS: usize = 2;
const TEXT_MAX_CHARS: usize = 128;

impl Card {
    pub fn for_status(status: &Status) -> Self {
        match status {
            Status::Menus => Self {
                details: "In the menus".to_owned(),
                state: None,
                started_unix_ms: None,
            },
            Status::Joining(destination) => {
                let details = match destination {
                    Destination::Server(_) => "Joining a server",
                    Destination::Realm => "Joining a Realm",
                    Destination::FriendWorld => "Joining a friend's world",
                    Destination::Experience => "Joining an experience",
                    Destination::LocalWorld(_) => "Loading a world",
                };
                Self {
                    details: details.to_owned(),
                    state: place(destination),
                    started_unix_ms: None,
                }
            }
            Status::Playing {
                destination,
                since_unix_ms,
            } => {
                let details = match destination {
                    Destination::Server(_) => "Playing on a server",
                    Destination::Realm => "Playing on a Realm",
                    Destination::FriendWorld => "Playing in a friend's world",
                    Destination::Experience => "Playing an experience",
                    Destination::LocalWorld(_) => "Playing singleplayer",
                };
                Self {
                    details: details.to_owned(),
                    state: place(destination),
                    started_unix_ms: Some(*since_unix_ms),
                }
            }
        }
    }
}

/// The second line naming the server or world, fitted to Discord's limits.
fn place(destination: &Destination) -> Option<String> {
    let name = match destination {
        Destination::Server(address) => address.as_deref()?,
        Destination::LocalWorld(name) => name,
        Destination::Realm | Destination::FriendWorld | Destination::Experience => return None,
    }
    .trim();
    if name.chars().count() < TEXT_MIN_CHARS {
        return None;
    }
    Some(match name.char_indices().nth(TEXT_MAX_CHARS) {
        Some((end, _)) => name[..end].to_owned(),
        None => name.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing(destination: Destination) -> Card {
        Card::for_status(&Status::Playing {
            destination,
            since_unix_ms: 1_700_000_000_000,
        })
    }

    #[test]
    fn menus_card_has_no_place_or_timer() {
        let card = Card::for_status(&Status::Menus);
        assert_eq!(card.details, "In the menus");
        assert_eq!((card.state, card.started_unix_ms), (None, None));
    }

    #[test]
    fn server_and_local_world_name_the_place_while_realms_and_friends_stay_private() {
        let server = playing(Destination::Server(Some("play.example.net".into())));
        assert_eq!(server.state.as_deref(), Some("play.example.net"));
        assert_eq!(server.started_unix_ms, Some(1_700_000_000_000));
        let world = playing(Destination::LocalWorld("My World".into()));
        assert_eq!(world.state.as_deref(), Some("My World"));
        assert_eq!(playing(Destination::Realm).state, None);
        assert_eq!(playing(Destination::FriendWorld).state, None);
        assert_eq!(playing(Destination::Server(None)).state, None);
    }

    #[test]
    fn joining_has_no_timer() {
        let card = Card::for_status(&Status::Joining(Destination::Server(Some("a.b".into()))));
        assert_eq!(card.started_unix_ms, None);
        assert_eq!(card.state.as_deref(), Some("a.b"));
    }

    #[test]
    fn place_text_fits_discord_limits() {
        assert_eq!(playing(Destination::LocalWorld(" x ".into())).state, None);
        let long = "é".repeat(TEXT_MAX_CHARS + 10);
        let state = playing(Destination::LocalWorld(long)).state.unwrap();
        assert_eq!(state.chars().count(), TEXT_MAX_CHARS);
    }
}
