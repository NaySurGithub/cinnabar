//! Bounded archive extraction into a fresh private directory; no links or traversal paths.

use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{self, Read},
    path::{Component, Path},
    sync::atomic::{AtomicBool, Ordering},
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// Experimental Android limits cover the pinned pack's measured 22,859 entries and 319 MB
/// expanded payload. They are tighter than desktop's extraction budget for mobile storage.
#[derive(Deserialize)]
pub(crate) struct Limits {
    entries: usize,
    file_bytes: u64,
    expanded_bytes: u64,
    archive_bytes: u64,
}

impl Limits {
    pub(crate) fn file_bytes(&self) -> u64 {
        self.file_bytes
    }
}

pub(crate) fn extract(archive: &Path, destination: &Path, cancel: &AtomicBool) -> Result<()> {
    let limits = &super::runtime().archive_limits;
    if fs::metadata(archive)?.len() > limits.archive_bytes {
        bail!(
            "archive exceeds the Android download size limit: {}",
            archive.display()
        );
    }
    let mut zip = zip::ZipArchive::new(File::open(archive)?)
        .with_context(|| format!("read archive {}", archive.display()))?;
    if zip.len() > limits.entries {
        bail!("archive has too many entries");
    }
    let mut seen = BTreeSet::new();
    let mut declared = 0_u64;
    // Inspect every entry before creating any payload file.
    for index in 0..zip.len() {
        let entry = zip.by_index(index)?;
        let name = entry.name();
        if name.contains('\\') || name.contains(':') || name.contains('\0') {
            bail!("unsafe archive name {name:?}");
        }
        let path = entry
            .enclosed_name()
            .context("archive path escapes the destination")?;
        if path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        {
            bail!("unsafe archive path {name:?}");
        }
        if !seen.insert(path.clone()) {
            bail!("duplicate archive path {name:?}");
        }
        if let Some(mode) = entry.unix_mode() {
            let kind = mode & 0o170000;
            if !matches!(kind, 0 | 0o100000 | 0o040000) {
                bail!("archive contains a link or special file {name:?}");
            }
        }
        if entry.size() > limits.file_bytes {
            bail!("archive entry exceeds the Android file size limit: {name}");
        }
        declared = declared
            .checked_add(entry.size())
            .context("archive size overflow")?;
        if declared > limits.expanded_bytes {
            bail!("archive exceeds the Android expanded size limit");
        }
    }
    fs::create_dir_all(destination)?;
    let mut written = 0_u64;
    for index in 0..zip.len() {
        if cancel.load(Ordering::Relaxed) {
            return Err(crate::first_run::android::cancelled());
        }
        let mut entry = zip.by_index(index)?;
        let target = destination.join(entry.enclosed_name().context("unsafe archive path")?);
        if entry.is_dir() {
            fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = File::options()
            .write(true)
            .create_new(true)
            .open(&target)
            .with_context(|| format!("create {}", target.display()))?;
        let expected = entry.size();
        let bytes = io::copy(&mut entry.by_ref().take(limits.file_bytes + 1), &mut output)?;
        written = written
            .checked_add(bytes)
            .context("archive size overflow")?;
        if bytes != expected || bytes > limits.file_bytes || written > limits.expanded_bytes {
            bail!(
                "archive expanded past its declared limits: {}",
                target.display()
            );
        }
        output.sync_all()?;
    }
    Ok(())
}
