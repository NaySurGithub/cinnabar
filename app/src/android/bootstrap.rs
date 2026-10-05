//! One bootstrap owner covers shared resources, callbacks and cancellation until completion.

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::{
        Mutex, MutexGuard, TryLockError,
        atomic::{AtomicBool, Ordering},
    },
};

pub(super) struct Coordinator<T> {
    serial: Mutex<()>,
    active: Mutex<Option<T>>,
    cancel: AtomicBool,
    resources_published: AtomicBool,
}

impl<T> Coordinator<T> {
    pub(super) const fn new() -> Self {
        Self {
            serial: Mutex::new(()),
            active: Mutex::new(None),
            cancel: AtomicBool::new(false),
            resources_published: AtomicBool::new(false),
        }
    }

    pub(super) fn begin(&self, owner: T, waiting: impl FnOnce()) -> Attempt<'_, T> {
        let serial = match self.serial.try_lock() {
            Ok(serial) => serial,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => {
                waiting();
                self.serial
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
            }
        };
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.cancel.store(false, Ordering::Relaxed);
        *active = Some(owner);
        Attempt {
            coordinator: self,
            _serial: serial,
        }
    }

    pub(super) fn owner(&self) -> Option<T>
    where
        T: Clone,
    {
        self.active
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    pub(super) fn cancel(&self, matches: impl FnOnce(&T) -> bool) -> bool {
        let active = self
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let matched = active.as_ref().is_some_and(matches);
        if matched {
            self.cancel.store(true, Ordering::Relaxed);
        }
        matched
    }

    pub(super) fn cancellation(&self) -> &AtomicBool {
        &self.cancel
    }
}

pub(super) struct Attempt<'a, T> {
    coordinator: &'a Coordinator<T>,
    _serial: MutexGuard<'a, ()>,
}

impl<T> Attempt<'_, T> {
    /// A successfully published APK kit stays immutable while native startup reads it.
    pub(super) fn publish_resources<E>(
        &self,
        publish: impl FnOnce() -> Result<(), E>,
    ) -> Result<(), E> {
        if self.coordinator.resources_published.load(Ordering::Relaxed) {
            return Ok(());
        }
        publish()?;
        let _active = self
            .coordinator
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !self.coordinator.cancel.load(Ordering::Relaxed) {
            self.coordinator
                .resources_published
                .store(true, Ordering::Relaxed);
        }
        Ok(())
    }

    pub(super) fn fresh_staging(&self, resources: &Path) -> io::Result<PathBuf> {
        let staged = resources.with_extension("staging");
        if staged.exists() {
            fs::remove_dir_all(&staged)?;
        }
        Ok(staged)
    }
}

impl<T> Drop for Attempt<'_, T> {
    fn drop(&mut self) {
        *self
            .coordinator
            .active
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = None;
    }
}

#[cfg(test)]
#[path = "bootstrap/tests.rs"]
mod tests;
