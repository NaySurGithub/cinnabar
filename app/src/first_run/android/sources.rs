//! The pinned sample pack uses the existing manifest and private workspace paths.

use std::{
    fs,
    path::{Component, Path},
    sync::atomic::AtomicBool,
};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::super::{download, plan};

#[derive(Deserialize)]
struct PackSource {
    archive: String,
    cache_dir: String,
    artifact_policy: String,
}

pub(super) fn unpack_pack(workspace: &Path, cancel: &AtomicBool) -> Result<()> {
    let source: PackSource =
        serde_json::from_slice(&fs::read(workspace.join(plan::VANILLA_MANIFEST))?)?;
    if source.artifact_policy != "local-only" {
        bail!("the sample pack must remain local-only");
    }
    safe_relative(&source.archive)?;
    if Path::new(&source.archive).components().count() != 1 {
        bail!("invalid archive basename");
    }
    safe_relative(&source.cache_dir)?;
    if !source.cache_dir.starts_with(".local/assets/") {
        bail!("invalid sample pack cache path");
    }
    let target = workspace.join(&source.cache_dir);
    let staged = target.with_extension("staging");
    if staged.exists() {
        fs::remove_dir_all(&staged)?;
    }
    crate::android::archive::extract(
        &download::archive_path(workspace, &source.archive),
        &staged,
        cancel,
    )?;
    let normalized = if staged.join("resource_pack/blocks.json").is_file() {
        staged.clone()
    } else {
        let roots: Vec<_> = fs::read_dir(&staged)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.path().join("resource_pack/blocks.json").is_file())
            .collect();
        if roots.len() != 1 {
            bail!("sample pack archive has no unique resource_pack/blocks.json");
        }
        roots[0].path()
    };
    if target.exists() {
        fs::remove_dir_all(&target)?;
    }
    fs::rename(&normalized, &target)
        .with_context(|| format!("publish sample pack {}", target.display()))?;
    if staged.exists() {
        fs::remove_dir_all(&staged)?;
    }
    Ok(())
}

fn safe_relative(name: &str) -> Result<()> {
    if name.is_empty()
        || name.contains('\\')
        || name.contains(':')
        || Path::new(name)
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        bail!("unsafe source path {name:?}");
    }
    Ok(())
}
