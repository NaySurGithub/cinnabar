//! What a player mod package's screens exchange with the host: the events its callbacks
//! receive, the screens it commits, and the declarations a package carries.

use crate::{ModHost, TargetFrame, TextSource, package::Package, runtime::Declared};
use anyhow::Result;
use experience_sdk::mod_manifest::KeyDecl;
use server_experience::{
    screen::{self, GuiSize, Rect, ScreenLayout},
    session_data::{SessionData, Stack},
};
use std::sync::Arc;

/// The modifier keys held with an input.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyModifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// A session source a `data-changed` reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DataSource {
    Items,
    Recipes,
}

impl DataSource {
    /// A mod reads this source whole, which takes the load budget.
    pub fn read_whole(&self) -> bool {
        matches!(self, Self::Items | Self::Recipes)
    }
}

/// The gameplay HUD as laid out while a mod's HUD layer may draw.
#[derive(Clone, Debug, PartialEq)]
pub struct HudLayout {
    /// The HUD root's size in GUI units and the GUI scale.
    pub size: GuiSize,
    /// The laid-out vanilla boss bars, when any shows.
    pub boss_bars: Option<Rect>,
}

/// One host event for a `player-mod` component's callbacks.
#[derive(Clone, Debug, PartialEq)]
pub enum ModEvent {
    ScreenChanged(Option<ScreenLayout>),
    Action {
        id: String,
        index: Option<u32>,
    },
    SecondaryAction {
        id: String,
        index: Option<u32>,
    },
    Scrolled {
        delta: f64,
        x: f64,
        y: f64,
        modifiers: KeyModifiers,
    },
    TextChanged {
        control: String,
        text: String,
    },
    /// `row` is the collection and index of the mod's control under the pointer.
    Key {
        id: String,
        hovered: Option<Stack>,
        row: Option<(String, u32)>,
    },
    /// The host closed the view: Escape, or the container closing.
    ViewClosed,
    /// The sources that changed, each once, in order.
    DataChanged(Vec<DataSource>),
    /// The crosshair target's revision moved.
    TargetChanged,
    /// The HUD layer was shown, hidden (`None`) or laid out anew.
    HudChanged(Option<HudLayout>),
}

impl ModEvent {
    /// One frame's events as delivered: the latest screen and HUD layouts first, one data
    /// change, one target change, then the rest in order, with one text change per edit box
    /// (its latest, where it last occurred).
    pub fn coalesce(events: Vec<Self>) -> Vec<Self> {
        let mut out = Vec::with_capacity(events.len());
        let layouts: [fn(&&Self) -> bool; 2] = [
            |event| matches!(event, Self::ScreenChanged(_)),
            |event| matches!(event, Self::HudChanged(_)),
        ];
        for latest in layouts {
            if let Some(layout) = events.iter().rev().find(latest) {
                out.push(layout.clone());
            }
        }
        let mut sources: Vec<DataSource> = events
            .iter()
            .filter_map(|event| match event {
                Self::DataChanged(sources) => Some(sources.iter().copied()),
                _ => None,
            })
            .flatten()
            .collect();
        sources.sort();
        sources.dedup();
        if events
            .iter()
            .any(|event| matches!(event, Self::DataChanged(_)))
        {
            out.push(Self::DataChanged(sources));
        }
        if events.contains(&Self::TargetChanged) {
            out.push(Self::TargetChanged);
        }
        for (index, event) in events.iter().enumerate() {
            let later_text = |control: &str| {
                events[index + 1..].iter().any(|later| {
                    matches!(later, Self::TextChanged { control: other, .. } if other == control)
                })
            };
            match event {
                Self::ScreenChanged(_)
                | Self::HudChanged(_)
                | Self::DataChanged(_)
                | Self::TargetChanged => {}
                Self::TextChanged { control, .. } if later_text(control) => {}
                event => out.push(event.clone()),
            }
        }
        out
    }
}

/// What a mod draws beside the container screens: its overlay and view templates and the data
/// bound into both.
#[derive(Clone, Debug, Default)]
pub struct ModScreens {
    pub overlay: Option<String>,
    pub view: Option<String>,
    /// The last `focus-text` request, with the data revision it was made at.
    pub focus: Option<(u64, String)>,
    pub data: screen::Modal,
}

/// A loaded package's declarations and screen files.
pub struct LoadedPackage {
    pub id: String,
    pub keys: Vec<KeyDecl>,
    pub files: Arc<screen::Files>,
}

impl ModHost {
    /// Delivers one frame's events, coalesced, each with its own budget. A layout event also
    /// sets what `screen.layout` returns. Stops at the first failure: an undeclared event is
    /// refused, and a trap quarantines the guest.
    pub fn dispatch(&mut self, events: Vec<ModEvent>) -> Result<()> {
        let result = self.dispatch_coalesced(events);
        self.queue_settings();
        result
    }

    fn dispatch_coalesced(&mut self, events: Vec<ModEvent>) -> Result<()> {
        for event in ModEvent::coalesce(events) {
            let mut closed = false;
            if let ModEvent::ScreenChanged(layout) = &event {
                self.layout.clone_from(layout);
                closed = layout.is_none() && self.instance.view_open();
                self.instance.set_layout(layout.clone());
            }
            self.instance.dispatch(&event)?;
            if closed {
                self.instance.dispatch(&ModEvent::ViewClosed)?;
            }
        }
        Ok(())
    }

    /// The session's items and recipes the guest reads from now on; a changed revision
    /// delivers `data-changed`.
    pub fn set_session(&mut self, session: Arc<SessionData>) -> Result<()> {
        let sources: Vec<DataSource> = [
            (
                DataSource::Items,
                session.item_revision != self.session.item_revision,
            ),
            (
                DataSource::Recipes,
                session.recipe_revision != self.session.recipe_revision,
            ),
        ]
        .into_iter()
        .filter_map(|(source, changed)| changed.then_some(source))
        .collect();
        self.session = Arc::clone(&session);
        self.instance.set_session(session);
        if !sources.is_empty() {
            return self.dispatch(vec![ModEvent::DataChanged(sources)]);
        }
        Ok(())
    }

    /// The HUD layer's layout from now on; a change delivers `hud-changed` to a mod granted
    /// `hud`.
    pub fn set_hud_layout(&mut self, layout: Option<HudLayout>) -> Result<()> {
        if self.instance.set_hud_layout(layout.clone()) && self.grants.hud {
            return self.dispatch(vec![ModEvent::HudChanged(layout)]);
        }
        Ok(())
    }

    /// This frame's crosshair target and the seconds since the last frame; a changed look
    /// moves `target.revision` and delivers `target-changed` to a mod granted `target`.
    pub fn set_target(&mut self, target: TargetFrame, frame_seconds: f32) -> Result<()> {
        if self.instance.set_target(target, frame_seconds) && self.grants.target {
            return self.dispatch(vec![ModEvent::TargetChanged]);
        }
        Ok(())
    }

    /// The client's language and HUD font measure, which `text` reads.
    pub fn set_text(&mut self, text: Arc<dyn TextSource>) {
        self.instance.set_text(text);
    }

    /// The committed HUD template and its bound data; empty after a trap.
    pub fn hud(&self) -> &screen::Modal {
        self.instance.hud()
    }

    /// Stops the guest as a trap would, removing everything it presented: the host refused
    /// what it asked to draw.
    pub fn quarantine(&mut self) {
        self.instance.quarantine();
    }

    /// Closes the view as Escape does over it, and tells the guest with `view-closed`.
    pub fn close_view(&mut self) -> Result<()> {
        if self.instance.close_view() {
            self.dispatch(vec![ModEvent::ViewClosed])?;
        }
        Ok(())
    }

    /// The fuel the last `init` or callback consumed: a `player-mod` component's `init` and
    /// `data-changed` get `LOAD_FUEL`, its other events `CALLBACK_FUEL`, `frame` its own.
    pub fn last_fuel_used(&self) -> u64 {
        self.instance.last_fuel()
    }

    /// The committed overlay, view and bound data; empty after a trap.
    pub fn screens(&self) -> &ModScreens {
        self.instance.screens()
    }

    /// The loaded package, when the mod came from one.
    pub fn package(&self) -> Option<&LoadedPackage> {
        self.package.as_ref()
    }

    /// Whether the component exports `player-mod`'s event callbacks.
    pub fn has_events(&self) -> bool {
        self.instance.has_events()
    }
}

pub(crate) fn declared(package: &Package) -> Declared {
    let manifest = &package.manifest;
    Declared {
        templates: manifest.templates.iter().cloned().collect(),
        actions: manifest.actions.iter().cloned().collect(),
        keys: manifest.keys.iter().map(|key| key.id.clone()).collect(),
    }
}

pub(crate) fn loaded(package: Package) -> LoadedPackage {
    LoadedPackage {
        id: package.manifest.id.clone(),
        keys: package.manifest.keys,
        files: package.files,
    }
}
