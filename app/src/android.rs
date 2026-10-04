//! Native Android host and private storage; desktop entry points are unchanged.

pub(crate) mod archive;
pub(crate) mod bridge;

use std::{path::PathBuf, sync::OnceLock};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Paths {
    pub files_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub native_library_dir: PathBuf,
    pub resources_dir: PathBuf,
}

#[derive(Deserialize)]
pub(crate) struct Runtime {
    pub application_id: &'static str,
    pub resource_archive: &'static str,
    pub compiler_identity_asset: &'static str,
    pub archive_limits: archive::Limits,
}

pub(crate) fn runtime() -> &'static Runtime {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        serde_json::from_str(include_str!("../../packaging/android/runtime.json"))
            .expect("the Android runtime manifest is checked by packaging")
    })
}

/// Starts the real client only after the bootstrap Activity has prepared its required carriers.
pub fn run() {
    if let Err(error) = run_client() {
        eprintln!("Android client startup failed: {error:#}");
        if bridge::show_failure(&format!("{error:#}")).is_ok()
            && let Some(app) = bevy::android::ANDROID_APP.get()
        {
            // Keep native lifecycle callbacks flowing until the user closes the error dialog.
            let mut destroyed = false;
            while !destroyed {
                app.poll_events(None, |event| {
                    destroyed |= matches!(
                        event,
                        bevy::android::android_activity::PollEvent::Main(
                            bevy::android::android_activity::MainEvent::Destroy
                        )
                    );
                });
            }
        }
    }
}

fn run_client() -> Result<()> {
    let paths = paths()?;
    launcher::install_layout::configure_android(
        paths.files_dir,
        paths.native_library_dir,
        paths.resources_dir,
    )?;
    if crate::lifecycle::before_run(false)? {
        crate::run(crate::args::ClientArgs {
            force_vsync: true,
            ..Default::default()
        })?;
    }
    Ok(())
}

pub(crate) fn paths() -> Result<Paths> {
    bridge::paths().context("resolve Android private storage")
}

/// Pre-window validation called by the ordinary client lifecycle.
pub(crate) fn prepare() -> Result<bool> {
    let layout = crate::install_layout::InstallLayout::discover()?;
    if crate::first_run::android::is_current(&layout)? {
        Ok(true)
    } else {
        bail!("Android resource preparation did not finish; close the app and launch it again")
    }
}

pub(crate) fn open_url(url: &str) -> Result<()> {
    bridge::string_call("openExternalUrl", url)
}

pub(crate) fn jni_call<T>(
    call: impl FnOnce(&mut jni::JNIEnv<'_>, &jni::objects::JObject<'_>) -> Result<T>,
) -> Result<T> {
    bridge::jni_call(call)
}
