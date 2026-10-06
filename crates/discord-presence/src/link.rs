//! Connection state machine between the wanted card and Discord's IPC.

use std::time::{Duration, Instant};

use crate::Card;

/// Delay before reconnecting after Discord was missing, so an absent pipe isn't probed every poll.
pub(crate) const RECONNECT_BACKOFF: Duration = Duration::from_secs(30);

pub(crate) type IpcResult = Result<(), Box<dyn std::error::Error>>;

/// The Discord IPC operations the link drives.
pub(crate) trait Ipc {
    fn connect(&mut self) -> IpcResult;
    fn set(&mut self, card: &Card) -> IpcResult;
    fn clear(&mut self) -> IpcResult;
    fn close(&mut self) -> IpcResult;
}

pub(crate) struct Link<I> {
    ipc: I,
    connected: bool,
    /// The card Discord shows, so unchanged cards aren't rewritten.
    shown: Option<Card>,
    retry_at: Option<Instant>,
}

impl<I: Ipc> Link<I> {
    pub(crate) fn new(ipc: I) -> Self {
        Self {
            ipc,
            connected: false,
            shown: None,
            retry_at: None,
        }
    }

    /// Makes Discord show `wanted`, or nothing when `None`.
    pub(crate) fn sync(&mut self, wanted: Option<&Card>, now: Instant) {
        let Some(card) = wanted else {
            self.disconnect();
            return;
        };
        if !self.connected {
            if self.retry_at.is_some_and(|at| now < at) {
                return;
            }
            if let Err(error) = self.ipc.connect() {
                tracing::trace!(%error, "discord ipc unavailable; will retry");
                self.retry_at = Some(now + RECONNECT_BACKOFF);
                return;
            }
            tracing::debug!("connected to discord rich presence");
            self.connected = true;
            self.retry_at = None;
        }
        if self.shown.as_ref() == Some(card) {
            return;
        }
        match self.ipc.set(card) {
            Ok(()) => self.shown = Some(card.clone()),
            // Discord closed mid-session; reconnect on the next sync.
            Err(error) => {
                tracing::debug!(%error, "discord set_activity failed; reconnecting");
                let _ = self.ipc.close();
                self.connected = false;
                self.shown = None;
            }
        }
    }

    fn disconnect(&mut self) {
        if self.connected {
            let _ = self.ipc.clear();
            let _ = self.ipc.close();
        }
        self.connected = false;
        self.shown = None;
        self.retry_at = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Card, Status};

    #[derive(Default)]
    struct FakeIpc {
        discord_running: bool,
        fail_next_set: bool,
        calls: Vec<&'static str>,
    }

    impl Ipc for FakeIpc {
        fn connect(&mut self) -> IpcResult {
            self.calls.push("connect");
            if self.discord_running {
                Ok(())
            } else {
                Err("no pipe".into())
            }
        }
        fn set(&mut self, _: &Card) -> IpcResult {
            self.calls.push("set");
            if std::mem::take(&mut self.fail_next_set) {
                Err("broken pipe".into())
            } else {
                Ok(())
            }
        }
        fn clear(&mut self) -> IpcResult {
            self.calls.push("clear");
            Ok(())
        }
        fn close(&mut self) -> IpcResult {
            self.calls.push("close");
            Ok(())
        }
    }

    fn link(discord_running: bool) -> Link<FakeIpc> {
        Link::new(FakeIpc {
            discord_running,
            ..FakeIpc::default()
        })
    }

    fn take_calls(link: &mut Link<FakeIpc>) -> Vec<&'static str> {
        std::mem::take(&mut link.ipc.calls)
    }

    #[test]
    fn unchanged_card_is_written_once() {
        let mut link = link(true);
        let card = Card::for_status(&Status::Menus);
        let now = Instant::now();
        link.sync(Some(&card), now);
        link.sync(Some(&card), now);
        assert_eq!(take_calls(&mut link), ["connect", "set"]);
    }

    #[test]
    fn missing_discord_backs_off_before_reconnecting() {
        let mut link = link(false);
        let card = Card::for_status(&Status::Menus);
        let start = Instant::now();
        link.sync(Some(&card), start);
        link.sync(Some(&card), start + RECONNECT_BACKOFF / 2);
        assert_eq!(take_calls(&mut link), ["connect"]);
        link.ipc.discord_running = true;
        link.sync(Some(&card), start + RECONNECT_BACKOFF);
        assert_eq!(take_calls(&mut link), ["connect", "set"]);
    }

    #[test]
    fn failed_write_reconnects_and_republishes() {
        let mut link = link(true);
        let card = Card::for_status(&Status::Menus);
        let now = Instant::now();
        link.ipc.fail_next_set = true;
        link.sync(Some(&card), now);
        link.sync(Some(&card), now);
        assert_eq!(
            take_calls(&mut link),
            ["connect", "set", "close", "connect", "set"]
        );
    }

    #[test]
    fn disabling_clears_and_reenabling_connects_without_backoff() {
        let mut link = link(false);
        let card = Card::for_status(&Status::Menus);
        let now = Instant::now();
        link.sync(Some(&card), now);
        link.ipc.discord_running = true;
        link.sync(None, now);
        link.sync(Some(&card), now);
        link.sync(None, now);
        assert_eq!(
            take_calls(&mut link),
            ["connect", "connect", "set", "clear", "close"]
        );
    }
}
