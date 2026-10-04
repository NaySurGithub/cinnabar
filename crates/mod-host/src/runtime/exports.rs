//! The guest's callbacks, found and type-checked once. A 0.1 component exports `init` and
//! `frame`; a 0.2 component also exports every event callback.

use super::{State, v0_2};
use crate::ModEvent;
use anyhow::{Result, bail};
use wasmtime::{
    Store,
    component::{Func, Instance, TypedFunc},
};

type Index = Option<u32>;
/// A collection name and the index of a row in it.
type Row = Option<(String, u32)>;

struct Events {
    screen_changed: TypedFunc<(Option<v0_2::Layout>,), ()>,
    action: TypedFunc<(String, Index), ()>,
    secondary: TypedFunc<(String, Index), ()>,
    scrolled: TypedFunc<(f64, f64, f64), ()>,
    text: TypedFunc<(String, String), ()>,
    key: TypedFunc<(String, Option<v0_2::GuestStack>, Row), ()>,
    data_changed: TypedFunc<(), ()>,
    view_closed: TypedFunc<(), ()>,
}

pub(super) struct Exports {
    frame: TypedFunc<(), ()>,
    events: Option<Events>,
}

/// The 0.2 event exports, all or none of which a component has.
const EVENTS: [&str; 8] = [
    "screen-changed",
    "action",
    "secondary-action",
    "scrolled",
    "text-changed",
    "key",
    "data-changed",
    "view-closed",
];

impl Exports {
    pub(super) fn find(
        store: &mut Store<State>,
        instance: &Instance,
    ) -> Result<(TypedFunc<(), ()>, Self)> {
        let mut find = |name: &str| instance.get_func(&mut *store, name);
        let (Some(init), Some(frame)) = (find("init"), find("frame")) else {
            bail!("component lacks init or frame");
        };
        let found: Vec<Option<Func>> = EVENTS.iter().map(|name| find(name)).collect();
        let events = match found.iter().filter(|func| func.is_some()).count() {
            0 => None,
            count if count == EVENTS.len() => {
                let func = |index: usize| found[index].expect("every event export found");
                Some(Events {
                    screen_changed: func(0).typed(&*store)?,
                    action: func(1).typed(&*store)?,
                    secondary: func(2).typed(&*store)?,
                    scrolled: func(3).typed(&*store)?,
                    text: func(4).typed(&*store)?,
                    key: func(5).typed(&*store)?,
                    data_changed: func(6).typed(&*store)?,
                    view_closed: func(7).typed(&*store)?,
                })
            }
            _ => bail!("component exports only some of the 0.2 event callbacks"),
        };
        Ok((
            init.typed(&*store)?,
            Self {
                frame: frame.typed(&*store)?,
                events,
            },
        ))
    }

    pub(super) fn has_events(&self) -> bool {
        self.events.is_some()
    }

    pub(super) fn frame(&self, store: &mut Store<State>) -> Result<()> {
        self.frame.call(&mut *store, ())?;
        self.frame.post_return(store)
    }

    pub(super) fn event(&self, store: &mut Store<State>, event: &ModEvent) -> Result<()> {
        let Some(events) = &self.events else {
            return Ok(());
        };
        fn call<P: wasmtime::component::ComponentNamedList + wasmtime::component::Lower>(
            func: &TypedFunc<P, ()>,
            store: &mut Store<State>,
            params: P,
        ) -> Result<()> {
            func.call(&mut *store, params)?;
            func.post_return(store)
        }
        match event {
            ModEvent::ScreenChanged(layout) => call(
                &events.screen_changed,
                store,
                (layout.as_ref().map(v0_2::layout),),
            ),
            ModEvent::Action { id, index } => call(&events.action, store, (id.clone(), *index)),
            ModEvent::SecondaryAction { id, index } => {
                call(&events.secondary, store, (id.clone(), *index))
            }
            ModEvent::Scrolled { delta, x, y } => call(&events.scrolled, store, (*delta, *x, *y)),
            ModEvent::TextChanged { control, text } => {
                call(&events.text, store, (control.clone(), text.clone()))
            }
            ModEvent::Key { id, hovered, row } => call(
                &events.key,
                store,
                (id.clone(), hovered.as_ref().map(v0_2::stack), row.clone()),
            ),
            ModEvent::DataChanged => call(&events.data_changed, store, ()),
            ModEvent::ViewClosed => call(&events.view_closed, store, ()),
        }
    }
}
