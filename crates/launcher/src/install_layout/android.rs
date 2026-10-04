//! Android paths come from the activity, never from desktop executable/home discovery.

use std::{path::PathBuf, sync::OnceLock};

use serde::Deserialize;

use super::{InstallLayout, LayoutError, physics_registry_relative};

#[derive(Deserialize)]
pub(super) struct HelperNames {
    pub(super) core_library: String,
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    pub(super) local_server_library: String,
}

pub(super) fn helper_names() -> &'static HelperNames {
    static NAMES: OnceLock<HelperNames> = OnceLock::new();
    NAMES.get_or_init(|| {
        serde_json::from_str(include_str!("../../../../packaging/android/runtime.json"))
            .expect("tracked Android runtime manifest")
    })
}

#[cfg(target_os = "android")]
static LAYOUT: OnceLock<InstallLayout> = OnceLock::new();

/// Installs the activity's private paths before client startup, without changing process env.
#[cfg(target_os = "android")]
pub fn configure_android(
    files_dir: PathBuf,
    native_library_dir: PathBuf,
    resource_root: PathBuf,
) -> Result<(), LayoutError> {
    let layout = resolve_paths(files_dir, native_library_dir, resource_root)?;
    if LAYOUT.get().is_some_and(|current| current == &layout) {
        return Ok(());
    }
    LAYOUT
        .set(layout)
        .map_err(|_| LayoutError::AndroidAlreadyConfigured)
}

#[cfg(target_os = "android")]
pub(super) fn discover() -> Result<InstallLayout, LayoutError> {
    LAYOUT
        .get()
        .cloned()
        .map(InstallLayout::with_prepared_assets)
        .ok_or(LayoutError::AndroidNotConfigured)
}

fn resolve_paths(
    files_dir: PathBuf,
    native_library_dir: PathBuf,
    resource_root: PathBuf,
) -> Result<InstallLayout, LayoutError> {
    for (variable, path) in [
        ("files_dir", &files_dir),
        ("native_library_dir", &native_library_dir),
        ("resource_root", &resource_root),
    ] {
        if !path.is_absolute() {
            return Err(LayoutError::InvalidUserRoot {
                variable,
                platform: "Android",
            });
        }
    }
    let runtime_root = files_dir.join("run");
    Ok(InstallLayout {
        compiled_assets: resource_root.join("assets"),
        physics_registry: resource_root.join(physics_registry_relative()),
        core_executable: native_library_dir.join(&helper_names().core_library),
        user_config_root: files_dir.join("config"),
        user_data_root: files_dir.join("data"),
        transient_runtime_root: runtime_root.clone(),
        runtime_root,
        resource_root,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn android_separates_immutable_helpers_from_private_data_and_prepared_carriers() {
        let files = std::env::temp_dir().join("cinnabar-android-files");
        let native = std::env::temp_dir().join("cinnabar-android-native");
        let resources = files.join("resources");
        let layout = resolve_paths(files.clone(), native.clone(), resources.clone()).unwrap();
        assert_eq!(layout.core_executable.parent(), Some(native.as_path()));
        assert_eq!(layout.user_config_root, files.join("config"));
        assert_eq!(layout.user_data_root, files.join("data"));
        assert_eq!(layout.runtime_root, files.join("run"));
        assert_eq!(
            layout.physics_registry.parent(),
            Some(resources.join("assets").as_path())
        );
        assert!(layout.is_installed());
        let prepared = layout.with_prepared_assets();
        assert!(
            prepared
                .compiled_assets
                .starts_with(&prepared.user_data_root)
        );
        assert!(!prepared.compiled_assets.starts_with(native));
    }

    #[test]
    fn android_requires_absolute_activity_paths_without_a_desktop_home_fallback() {
        let absolute = std::env::temp_dir().join("cinnabar-android-absolute");
        for relative_index in 0..3 {
            let mut paths = [absolute.clone(), absolute.clone(), absolute.clone()];
            paths[relative_index] = PathBuf::from("relative");
            assert!(matches!(
                resolve_paths(paths[0].clone(), paths[1].clone(), paths[2].clone()),
                Err(LayoutError::InvalidUserRoot {
                    platform: "Android",
                    ..
                })
            ));
        }
    }
}
