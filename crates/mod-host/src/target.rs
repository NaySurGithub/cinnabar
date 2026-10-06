//! What a `player-mod` component reads about the crosshair and the client's text: the frame's
//! target as the app resolved it, and the app's harvest rules and text services behind it.

use crate::runtime::cinnabar::session::target::{HarvestFacts, Look, MiningState};
use std::sync::Arc;

/// Most candidate tools one `target.harvest` call tests.
pub const MAX_HARVEST_CANDIDATES: usize = 64;
/// Most bytes one `text.translate` key or `text.width` text may hold.
pub const MAX_TEXT_BYTES: usize = 1024;

/// Bedrock's harvest rules for the targeted block, which `target.harvest` asks.
pub trait HarvestRules: Send + Sync {
    /// The facts against `candidates`, item identifiers; `effective` lists those that are
    /// correct for the block's drops or mine it faster.
    fn harvest(&self, candidates: &[String]) -> HarvestFacts;
}

/// The client's language and HUD font, which `text` reads.
pub trait TextSource: Send + Sync {
    fn translate(&self, key: &str) -> Option<String>;
    /// Width in GUI units as a HUD label draws `text`.
    fn width(&self, text: &str) -> f32;
}

/// One frame's crosshair target.
#[derive(Clone, Default)]
pub struct TargetFrame {
    /// None outside gameplay.
    pub look: Option<Look>,
    pub mining: Option<MiningState>,
    /// The targeted block's rules; none without a targeted block with destroy facts.
    pub harvest: Option<Arc<dyn HarvestRules>>,
}

impl std::fmt::Debug for TargetFrame {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TargetFrame")
            .field("look", &self.look)
            .field("mining", &self.mining)
            .field("harvest", &self.harvest.is_some())
            .finish()
    }
}
