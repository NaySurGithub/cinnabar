use crate::{
    CameraDelta, FRAME_FUEL, GameplaySnapshot, MAX_LABEL_BYTES, MEMORY_BYTES, ModEvent, ModGrants,
    ModScreens,
};
use anyhow::{Result, bail};
use server_experience::{
    runtime::{CALLBACK_FUEL, LOAD_FUEL},
    screen::ScreenLayout,
    session_data::SessionData,
};
use std::{collections::BTreeSet, sync::Arc};
use wasmtime::{
    Engine, Store, StoreLimits, StoreLimitsBuilder,
    component::{Component, HasSelf, Linker},
};

wasmtime::component::bindgen!({
    path: "../mod-api/wit/0.1", world: "extension", imports: { default: trappable },
});

const MAX_IMPORT_WRITES: u32 = 8;
mod exports;
#[path = "gameplay.rs"]
mod gameplay;
mod v0_2;

/// What a package declares that the host checks guest output and events against.
#[derive(Clone, Debug, Default)]
pub(crate) struct Declared {
    pub(crate) templates: BTreeSet<String>,
    pub(crate) actions: BTreeSet<String>,
    pub(crate) keys: BTreeSet<String>,
}

struct State {
    limits: StoreLimits,
    pressed: bool,
    label: Option<String>,
    pending: Option<String>,
    writes: u32,
    grants: ModGrants,
    time_override: Option<u32>,
    pending_time: Option<Option<u32>>,
    environment_writes: u32,
    snapshot: Option<GameplaySnapshot>,
    gameplay_reads: u32,
    camera_writes: u32,
    pending_camera: Option<CameraDelta>,
    camera_delta: Option<CameraDelta>,
    declared: Declared,
    layout: Option<ScreenLayout>,
    session: Arc<SessionData>,
    screens: ModScreens,
    /// This callback's screen output, applied to a copy of `screens`; committed on return.
    pending_screens: Option<ModScreens>,
    /// Host calls and screen output bytes of the running callback.
    calls: usize,
    output: usize,
}

impl cinnabar::extension::hud::Host for State {
    /// Stages bounded plain text; nothing is published until the guest returns.
    fn set_label(&mut self, text: String) -> Result<Result<(), String>> {
        self.writes += 1;
        if self.writes > MAX_IMPORT_WRITES {
            bail!("HUD import budget exhausted");
        }
        if text.len() > MAX_LABEL_BYTES || text.chars().any(|c| c.is_control() || c == '§') {
            return Ok(Err("label must be short plain text".into()));
        }
        self.pending = Some(text);
        Ok(Ok(()))
    }
}

impl cinnabar::extension::environment::Host for State {
    /// Stages a fixed visual clock only with explicit authority and a valid tick.
    fn set_time_override(&mut self, ticks: Option<u32>) -> Result<Result<(), String>> {
        self.environment_writes += 1;
        if self.environment_writes > MAX_IMPORT_WRITES {
            bail!("environment import budget exhausted");
        }
        if !self.grants.environment {
            return Ok(Err("environment capability denied".into()));
        }
        if ticks.is_some_and(|tick| tick >= mod_api::BEDROCK_DAY_TICKS) {
            return Ok(Err("time override must be within one Bedrock day".into()));
        }
        self.pending_time = Some(ticks);
        Ok(Ok(()))
    }
}

impl cinnabar::extension::input::Host for State {
    /// Reads only the host's one-frame action edge, never raw keyboard state.
    fn demo_pressed(&mut self) -> Result<bool> {
        Ok(self.pressed)
    }
}

pub(super) struct Instance {
    store: Store<State>,
    exports: exports::Exports,
    pub(super) active: bool,
    /// Fuel the last `init` or callback consumed.
    last_fuel: u64,
}

impl Instance {
    /// Initializes a candidate store without changing the published instance. Both 0.1 and 0.2
    /// components link: 0.2's imports include 0.1's unchanged.
    pub(super) fn new(
        engine: &Engine,
        bytes: &[u8],
        grants: ModGrants,
        declared: Declared,
    ) -> Result<Self> {
        let component = Component::new(engine, bytes)?;
        let mut linker = Linker::new(engine);
        Extension::add_to_linker::<_, HasSelf<_>>(&mut linker, |state: &mut State| state)?;
        v0_2::add_to_linker(&mut linker)?;
        let state = State {
            limits: StoreLimitsBuilder::new()
                .memory_size(MEMORY_BYTES)
                .table_elements(4096)
                .instances(16)
                .memories(1)
                .tables(2)
                .trap_on_grow_failure(true)
                .build(),
            pressed: false,
            label: None,
            pending: None,
            writes: 0,
            grants,
            time_override: None,
            pending_time: None,
            environment_writes: 0,
            snapshot: None,
            gameplay_reads: 0,
            camera_writes: 0,
            pending_camera: None,
            camera_delta: None,
            declared,
            layout: None,
            session: Arc::default(),
            screens: ModScreens::default(),
            pending_screens: None,
            calls: 0,
            output: 0,
        };
        let mut store = Store::new(engine, state);
        store.limiter(|state| &mut state.limits);
        store.set_fuel(LOAD_FUEL)?;
        let instance = linker.instantiate(&mut store, &component)?;
        let (init, exports) = exports::Exports::find(&mut store, &instance)?;
        init.call(&mut store, ())?;
        init.post_return(&mut store)?;
        commit(&mut store);
        let last_fuel = LOAD_FUEL - store.get_fuel().unwrap_or(0).min(LOAD_FUEL);
        Ok(Self {
            store,
            exports,
            active: true,
            last_fuel,
        })
    }

    /// Restores the call budget and commits output only on successful return.
    pub(super) fn frame(
        &mut self,
        pressed: bool,
        snapshot: Option<GameplaySnapshot>,
    ) -> Result<()> {
        let state = self.store.data_mut();
        state.snapshot = None;
        state.pending_camera = None;
        state.camera_delta = None;
        if !self.active {
            return Ok(());
        }
        gameplay::validate_snapshot(snapshot.as_ref())?;
        let state = self.store.data_mut();
        state.pressed = pressed;
        state.snapshot = snapshot;
        let result = self.run(FRAME_FUEL, |exports, store| exports.frame(store));
        self.store.data_mut().snapshot = None;
        result
    }

    /// Delivers one event callback with `CALLBACK_FUEL`. A 0.1 component has no event exports,
    /// so it receives nothing.
    pub(super) fn dispatch(&mut self, event: &ModEvent) -> Result<()> {
        if !self.active || !self.exports.has_events() {
            return Ok(());
        }
        self.store.data().check_event(event)?;
        // Loading the session reads all of it across the ABI; every other event is bounded by
        // the per-event budget.
        let fuel = match event {
            ModEvent::DataChanged => LOAD_FUEL,
            _ => CALLBACK_FUEL,
        };
        self.run(fuel, |exports, store| exports.event(store, event))
    }

    /// Runs one callback with `fuel`; a trap discards its output and quarantines the guest,
    /// removing everything it presented.
    fn run(
        &mut self,
        fuel: u64,
        call: impl FnOnce(&exports::Exports, &mut Store<State>) -> Result<()>,
    ) -> Result<()> {
        let state = self.store.data_mut();
        state.writes = 0;
        state.environment_writes = 0;
        state.gameplay_reads = 0;
        state.camera_writes = 0;
        state.calls = 0;
        state.output = 0;
        state.pending_screens = None;
        self.store.set_fuel(fuel)?;
        let called = call(&self.exports, &mut self.store);
        self.last_fuel = fuel - self.store.get_fuel().unwrap_or(0).min(fuel);
        if let Err(error) = called {
            self.quarantine();
            bail!("mod quarantined after a guest trap: {error:#}");
        }
        commit(&mut self.store);
        Ok(())
    }

    /// Disables callbacks and drops everything the guest presented or staged.
    pub(super) fn quarantine(&mut self) {
        self.active = false;
        let state = self.store.data_mut();
        state.pending = None;
        state.label = None;
        state.pending_time = None;
        state.time_override = None;
        state.snapshot = None;
        state.pending_camera = None;
        state.camera_delta = None;
        state.pending_screens = None;
        state.screens = ModScreens::default();
    }

    pub(super) fn take_camera_delta(&mut self) -> Option<CameraDelta> {
        self.store.data_mut().camera_delta.take()
    }

    /// Reads the committed presentation clock without entering the component.
    pub(super) fn time_override(&self) -> Option<u32> {
        self.store.data().time_override
    }

    /// Reads retained UI without entering the component.
    pub(super) fn label(&self) -> Option<&str> {
        self.store.data().label.as_deref()
    }

    pub(super) fn screens(&self) -> &ModScreens {
        &self.store.data().screens
    }

    /// Whether the component exports 0.2's event callbacks.
    pub(super) fn has_events(&self) -> bool {
        self.exports.has_events()
    }

    /// The open container screen's layout that `screen.layout` returns; none also closes the
    /// view, as closing the container returns from it.
    pub(super) fn set_layout(&mut self, layout: Option<ScreenLayout>) {
        let state = self.store.data_mut();
        if layout.is_none() && state.screens.view.is_some() {
            state.screens.view = None;
            state.screens.data.revision += 1;
        }
        state.layout = layout;
    }

    /// Closes the view without the guest, as Escape does; `true` when one was open.
    pub(super) fn close_view(&mut self) -> bool {
        let screens = &mut self.store.data_mut().screens;
        let open = screens.view.take().is_some();
        if open {
            screens.data.revision += 1;
        }
        open
    }

    pub(super) fn view_open(&self) -> bool {
        self.store.data().screens.view.is_some()
    }

    pub(super) fn last_fuel(&self) -> u64 {
        self.last_fuel
    }

    pub(super) fn set_session(&mut self, session: Arc<SessionData>) {
        self.store.data_mut().session = session;
    }
}

/// Publishes retained presentation changes after the entire callback succeeds.
fn commit(store: &mut Store<State>) {
    let state = store.data_mut();
    state.camera_delta = state.pending_camera.take();
    if let Some(ticks) = state.pending_time.take() {
        state.time_override = ticks;
    }
    if let Some(text) = state.pending.take() {
        state.label = (!text.is_empty()).then_some(text);
    }
    if let Some(screens) = state.pending_screens.take() {
        state.screens = screens;
    }
}
