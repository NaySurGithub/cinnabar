//! What a player mod package's screens exchange with the host: the events its 0.2 callbacks
//! receive, the screens it commits, and the declarations a package carries.

use crate::{ModGrants, package::Package, runtime::Declared};
use experience_sdk::mod_manifest::{KeyDecl, ModManifest};
use server_experience::{
    screen::{self, ScreenLayout},
    session_data::Stack,
};
use std::sync::Arc;

/// One host event for a 0.2 mod's callbacks.
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
    DataChanged,
}

impl ModEvent {
    /// One frame's events as delivered: the latest layout first, one data change, then the rest
    /// in order, with one text change per edit box (its latest, where it last occurred).
    pub fn coalesce(events: Vec<Self>) -> Vec<Self> {
        let mut out = Vec::with_capacity(events.len());
        if let Some(layout) = events
            .iter()
            .rev()
            .find(|event| matches!(event, Self::ScreenChanged(_)))
        {
            out.push(layout.clone());
        }
        if events.contains(&Self::DataChanged) {
            out.push(Self::DataChanged);
        }
        for (index, event) in events.iter().enumerate() {
            let later_text = |control: &str| {
                events[index + 1..].iter().any(|later| {
                    matches!(later, Self::TextChanged { control: other, .. } if other == control)
                })
            };
            match event {
                Self::ScreenChanged(_) | Self::DataChanged => {}
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

pub(crate) fn grant(manifest: &ModManifest, extra: ModGrants) -> ModGrants {
    let asked = ModGrants::from_manifest(manifest);
    ModGrants {
        screen: asked.screen,
        items: asked.items,
        recipes: asked.recipes,
        keys: asked.keys,
        ..extra
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
