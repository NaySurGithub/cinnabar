//! Experimental component host. Only the explicit WIT imports carry authority.

pub mod helper;
#[cfg(feature = "execution")]
pub mod package;
#[cfg(feature = "execution")]
mod runtime;
#[cfg(feature = "execution")]
mod screens;
#[cfg(feature = "execution")]
pub mod server;

#[cfg(feature = "execution")]
pub use screens::{LoadedPackage, ModEvent, ModScreens};

#[cfg(feature = "execution")]
pub use experience_sdk::mod_manifest::{KEY_NAMES, KeyDecl, Modifier};
#[cfg(feature = "execution")]
pub use mod_api::{MAX_CAMERA_DELTA_RADIANS, MAX_GAMEPLAY_PLAYERS};
#[cfg(feature = "execution")]
pub use runtime::cinnabar::extension::gameplay::{
    Player as GameplayPlayer, Snapshot as GameplaySnapshot, Vector3 as GameplayVector3,
};

/// Committed local actor rotation; yaw turns left and pitch turns up, in radians.
#[cfg(feature = "execution")]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraDelta {
    pub yaw: f32,
    pub pitch: f32,
}
#[cfg(feature = "execution")]
use {
    anyhow::{Context, Result},
    experience_sdk::mod_manifest::{ModManifest, ModPermission},
    package::{Package, engine, read_component},
    runtime::{Declared, Instance},
    screens::{declared, grant, loaded},
    server_experience::{screen::ScreenLayout, session_data::SessionData},
    sha2::{Digest, Sha256},
    std::{
        path::{Path, PathBuf},
        sync::Arc,
    },
    wasmtime::Engine,
};

/// Maximum bytes accepted before compilation or allocation of a package buffer.
pub const MAX_COMPONENT_BYTES: usize = 4 * 1024 * 1024;
/// Plain-text UI limit, checked before publishing any guest output.
pub const MAX_LABEL_BYTES: usize = 256;
#[cfg(feature = "execution")]
pub(crate) const FRAME_FUEL: u64 = 100_000;
#[cfg(feature = "execution")]
pub(crate) const MEMORY_BYTES: usize = 16 * 1024 * 1024;

/// Explicit per-instance authority; optional capabilities are denied by default.
#[cfg(feature = "execution")]
#[derive(Clone, Copy, Debug, Default)]
pub struct ModGrants {
    /// Allows this instance to replace visual time only.
    pub environment: bool,
    /// Allows current-frame remote player and camera pose reads.
    pub players: bool,
    /// Allows bounded, transactional local camera rotation.
    pub camera: bool,
    /// Allows the overlay and view beside the container screens.
    pub screen: bool,
    /// Allows reading the session's items.
    pub items: bool,
    /// Allows reading the session's recipes.
    pub recipes: bool,
    /// Allows delivering the package's declared keys.
    pub keys: bool,
}

#[cfg(feature = "execution")]
impl ModGrants {
    /// The developer profile: what a package's manifest asks for, except `inventory`, which no
    /// import carries yet.
    pub fn from_manifest(manifest: &ModManifest) -> Self {
        let asks = |permission| manifest.permissions.contains(&permission);
        Self {
            screen: asks(ModPermission::Screen),
            items: asks(ModPermission::Items),
            recipes: asks(ModPermission::Recipes),
            keys: asks(ModPermission::Keys),
            ..Self::default()
        }
    }
}

/// Where a mod came from, which reload reads again.
#[cfg(feature = "execution")]
enum Source {
    Component(PathBuf),
    Package(PathBuf),
}

/// A developer-selected component with transactional reload and trap quarantine.
#[cfg(feature = "execution")]
pub struct ModHost {
    engine: Engine,
    instance: Instance,
    source: Source,
    attempted: [u8; 32],
    grants: ModGrants,
    package: Option<LoadedPackage>,
    layout: Option<ScreenLayout>,
    session: Arc<SessionData>,
}

#[cfg(feature = "execution")]
impl ModHost {
    /// Loads a local component with HUD and demo input, denying optional capabilities.
    pub fn load(path: &Path) -> Result<Self> {
        Self::load_with_grants(path, ModGrants::default())
    }

    /// Loads a component with the developer's explicit per-mod capability grants.
    pub fn load_with_grants(path: &Path, grants: ModGrants) -> Result<Self> {
        let bytes = read_component(path)?;
        let engine = engine()?;
        let instance = Instance::new(&engine, &bytes, grants, Declared::default())?;
        Ok(Self {
            engine,
            instance,
            source: Source::Component(path.to_owned()),
            attempted: Sha256::digest(&bytes).into(),
            grants,
            package: None,
            layout: None,
            session: Arc::default(),
        })
    }

    /// Loads the package at `dir` with `grants` added to what its manifest asks for, as the
    /// developer profile allows.
    pub fn load_package(dir: &Path, extra: ModGrants) -> Result<Self> {
        let package = Package::read(dir)?;
        let grants = grant(&package.manifest, extra);
        let engine = engine()?;
        let instance = Instance::new(&engine, &package.component, grants, declared(&package))?;
        Ok(Self {
            engine,
            instance,
            source: Source::Package(dir.to_owned()),
            attempted: package.digest,
            grants,
            package: Some(loaded(package)),
            layout: None,
            session: Arc::default(),
        })
    }

    /// Runs one bounded callback; a trap revokes its presentation and disables the guest.
    pub fn frame(&mut self, pressed: bool) -> Result<()> {
        self.frame_with_gameplay(pressed, None)
    }

    /// Runs a callback with a validated snapshot belonging only to this frame.
    pub fn frame_with_gameplay(
        &mut self,
        pressed: bool,
        snapshot: Option<GameplaySnapshot>,
    ) -> Result<()> {
        self.instance.frame(pressed, snapshot)
    }

    /// Delivers one frame's events, coalesced, each with its own `CALLBACK_FUEL`. A layout
    /// event also sets what `screen.layout` returns. Stops at the first failure: an undeclared
    /// event is refused, and a trap quarantines the guest.
    pub fn dispatch(&mut self, events: Vec<ModEvent>) -> Result<()> {
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
        let changed = session.item_revision != self.session.item_revision
            || session.recipe_revision != self.session.recipe_revision;
        self.session = Arc::clone(&session);
        self.instance.set_session(session);
        if changed {
            return self.dispatch(vec![ModEvent::DataChanged]);
        }
        Ok(())
    }

    /// Stops the guest as a trap would, removing everything it presented: the host refused
    /// what it asked to draw.
    pub fn quarantine(&mut self) {
        self.instance.quarantine();
    }

    /// Closes the view as Escape does over it, and tells the guest with `view-closed`.
    pub fn close_view(&mut self) -> Result<()> {
        if self.instance.close_view() {
            self.instance.dispatch(&ModEvent::ViewClosed)?;
        }
        Ok(())
    }

    /// The fuel the last `init` or callback consumed: `init` and `data-changed` get
    /// `LOAD_FUEL`, other events `CALLBACK_FUEL`, `frame` its own.
    pub fn last_fuel_used(&self) -> u64 {
        self.instance.last_fuel()
    }

    /// Consumes the last successful frame's rotation once, without entering the guest.
    pub fn take_camera_delta(&mut self) -> Option<CameraDelta> {
        self.instance.take_camera_delta()
    }

    /// Returns only the last successfully committed plain-text label.
    pub fn label(&self) -> Option<&str> {
        self.instance.label()
    }

    /// Returns the committed visual override without entering the guest.
    pub fn time_override(&self) -> Option<u32> {
        self.instance.time_override()
    }

    /// The committed overlay, view and bound data; empty after a trap.
    pub fn screens(&self) -> &ModScreens {
        self.instance.screens()
    }

    /// The loaded package, when the mod came from one.
    pub fn package(&self) -> Option<&LoadedPackage> {
        self.package.as_ref()
    }

    /// Whether the component exports 0.2's event callbacks.
    pub fn has_events(&self) -> bool {
        self.instance.has_events()
    }

    /// Whether this guest can still receive callbacks.
    pub fn is_active(&self) -> bool {
        self.instance.active
    }

    /// Replaces an instance only after changed bytes compile and initialize. The new instance
    /// gets the current layout and session, and `screen-changed` and `data-changed` when it has
    /// events.
    pub fn reload_if_changed(&mut self) -> Result<bool> {
        let (bytes, digest, package) = match &self.source {
            Source::Component(path) => {
                let bytes = read_component(path)?;
                let digest = Sha256::digest(&bytes).into();
                (bytes, digest, None)
            }
            Source::Package(dir) => {
                let package = Package::read(dir)?;
                (Vec::new(), package.digest, Some(package))
            }
        };
        if self.attempted == digest {
            return Ok(false);
        }
        self.attempted = digest;
        let (bytes, declared) = match &package {
            Some(package) => (&package.component, declared(package)),
            None => (&bytes, Declared::default()),
        };
        let mut candidate = Instance::new(&self.engine, bytes, self.grants, declared)
            .context("reload rejected; previous mod retained")?;
        candidate.set_session(Arc::clone(&self.session));
        self.instance = candidate;
        if let Some(package) = package {
            self.package = Some(loaded(package));
        }
        if self.instance.has_events() {
            let layout = ModEvent::ScreenChanged(self.layout.clone());
            self.dispatch(vec![layout, ModEvent::DataChanged])?;
        }
        Ok(true)
    }
}

#[cfg(all(test, feature = "execution"))]
mod tests;
