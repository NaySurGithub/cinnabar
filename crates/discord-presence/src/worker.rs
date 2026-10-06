//! Background thread that owns the Discord IPC client.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use discord_rich_presence::activity::{Activity, Assets, Timestamps};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};

use crate::link::{Ipc, IpcResult, Link};
use crate::{Card, LOGO_ASSET};

/// How often the worker retries a missing Discord while no new card arrives.
const POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Publishes cards from the game thread without blocking it on Discord.
///
/// Dropping the worker clears the activity and ends its thread.
#[derive(Debug)]
pub struct PresenceWorker {
    cards: mpsc::Sender<Option<Card>>,
}

impl PresenceWorker {
    /// Starts the worker for `application_id`; `logo_text` is the logo's hover text.
    pub fn spawn(application_id: &str, logo_text: String) -> std::io::Result<Self> {
        let (cards, incoming) = mpsc::channel();
        let ipc = DiscordLink {
            client: DiscordIpcClient::new(application_id),
            logo_text,
        };
        thread::Builder::new()
            .name("discord-presence".to_owned())
            .spawn(move || run(Link::new(ipc), &incoming))?;
        Ok(Self { cards })
    }

    /// Shows `card`, or clears the activity when `None`.
    pub fn publish(&self, card: Option<Card>) {
        let _ = self.cards.send(card);
    }
}

fn run(mut link: Link<impl Ipc>, incoming: &mpsc::Receiver<Option<Card>>) {
    let mut wanted = None;
    loop {
        match incoming.recv_timeout(POLL_INTERVAL) {
            Ok(card) => wanted = incoming.try_iter().last().unwrap_or(card),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                link.sync(None, Instant::now());
                return;
            }
        }
        link.sync(wanted.as_ref(), Instant::now());
    }
}

struct DiscordLink {
    client: DiscordIpcClient,
    logo_text: String,
}

impl Ipc for DiscordLink {
    fn connect(&mut self) -> IpcResult {
        Ok(self.client.connect()?)
    }

    fn set(&mut self, card: &Card) -> IpcResult {
        let assets = Assets::new()
            .large_image(LOGO_ASSET)
            .large_text(self.logo_text.as_str());
        let mut activity = Activity::new()
            .details(card.details.as_str())
            .assets(assets);
        if let Some(state) = card.state.as_deref() {
            activity = activity.state(state);
        }
        if let Some(start) = card.started_unix_ms {
            activity = activity.timestamps(Timestamps::new().start(start));
        }
        Ok(self.client.set_activity(activity)?)
    }

    fn clear(&mut self) -> IpcResult {
        Ok(self.client.clear_activity()?)
    }

    fn close(&mut self) -> IpcResult {
        Ok(self.client.close()?)
    }
}
