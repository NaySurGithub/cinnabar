//! Android executes the shared preparation plan in-process instead of spawning shell tools.

use std::{
    fs,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use anyhow::Result;

use super::{
    prepare,
    status::{Phase, Status},
};
use crate::{android::bridge, install_layout::InstallLayout};

// Use the same cancellation type as the desktop executor and publisher.
use super::runner::Cancelled;

pub(crate) fn cancelled() -> anyhow::Error {
    Cancelled.into()
}

pub(crate) fn is_current(layout: &InstallLayout) -> Result<bool> {
    Ok(prepare::selection(layout)?.is_current())
}

/// Called on the Java bootstrap worker before any NativeActivity window exists.
pub(crate) fn bootstrap(cancel: &AtomicBool) -> Result<bool> {
    let layout = InstallLayout::discover()?;
    fs::create_dir_all(layout.log_dir())?;
    let mut report = super::reporter(&layout, |status| {
        let text = match (status.downloaded, status.download_total) {
            (Some(received), Some(total)) => format!(
                "{}: {} / {} MB",
                status.label,
                received / 1_000_000,
                total / 1_000_000
            ),
            _ => status.label.clone(),
        };
        bridge::progress(&text);
    });
    if is_current(&layout)? {
        return Ok(true);
    }
    if !super::consent_recorded(&layout) {
        report(Status::new(
            Phase::AwaitingConsent,
            0,
            0,
            "Waiting for consent",
        ));
        bridge::strings_call("requestConsent", super::TITLE, super::CONSENT_BODY)?;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Ok(false);
            }
            match bridge::consent_state()? {
                1 => break,
                -1 => return Ok(false),
                _ => std::thread::sleep(Duration::from_millis(100)),
            }
        }
        super::record_consent(&layout)?;
    }
    let result = prepare::prepare(&layout, cancel, &mut report);
    match result {
        Ok(()) => {
            report(Status::new(Phase::Done, 0, 0, "Ready"));
            Ok(true)
        }
        Err(error) if error.is::<Cancelled>() => Ok(false),
        Err(error) => {
            report(Status::failed("Setup failed", &format!("{error:#}")));
            Err(error)
        }
    }
}
