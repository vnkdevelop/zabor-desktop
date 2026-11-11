use std::fs;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WipeScope {
    Settings,
    Cache,
    All,
}

#[derive(Debug, PartialEq, Eq)]
pub enum WipeError {
    ProtectedLegacyProfile,
    DrainRequired,
    Io(String),
}

impl std::fmt::Display for WipeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProtectedLegacyProfile => write!(
                f,
                "legacy Electron profile is protected and cannot be wiped"
            ),
            Self::DrainRequired => write!(f, "complete app drain is required before full wipe"),
            Self::Io(msg) => write!(f, "IO error during wipe: {msg}"),
        }
    }
}

impl std::error::Error for WipeError {}

pub fn is_legacy_electron_profile(path: &Path) -> bool {
    let normalized = path.to_string_lossy().to_lowercase();
    if normalized.contains("zabor-desktop-native-beta") {
        return false;
    }
    normalized.ends_with("zabor-desktop")
        || normalized.contains("zabor-desktop\\")
        || normalized.contains("zabor-desktop/")
}

pub fn wipe_app_data(
    app_data_root: &Path,
    scope: WipeScope,
    drain_completed: bool,
) -> Result<(), WipeError> {
    if is_legacy_electron_profile(app_data_root) {
        return Err(WipeError::ProtectedLegacyProfile);
    }

    if scope == WipeScope::All && !drain_completed {
        return Err(WipeError::DrainRequired);
    }

    match scope {
        WipeScope::Settings => {
            let settings_file = app_data_root.join("settings.json");
            if settings_file.exists() {
                fs::remove_file(&settings_file).map_err(|e| WipeError::Io(e.to_string()))?;
            }
        }
        WipeScope::Cache => {
            let cache_dir = app_data_root.join("cache");
            if cache_dir.exists() {
                fs::remove_dir_all(&cache_dir).map_err(|e| WipeError::Io(e.to_string()))?;
            }
        }
        WipeScope::All => {
            if app_data_root.exists() {
                for entry in
                    fs::read_dir(app_data_root).map_err(|e| WipeError::Io(e.to_string()))?
                {
                    let entry = entry.map_err(|e| WipeError::Io(e.to_string()))?;
                    let path = entry.path();
                    if path.is_dir() {
                        let _ = fs::remove_dir_all(&path);
                    } else {
                        let _ = fs::remove_file(&path);
                    }
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_legacy_profile_never_wiped() {
        let legacy_path = Path::new(r"C:\Users\User\AppData\Roaming\zabor-desktop");
        assert!(is_legacy_electron_profile(legacy_path));
        assert_eq!(
            wipe_app_data(legacy_path, WipeScope::All, true),
            Err(WipeError::ProtectedLegacyProfile)
        );

        let legacy_sub = Path::new(r"C:\Users\User\AppData\Roaming\zabor-desktop\Partitions");
        assert!(is_legacy_electron_profile(legacy_sub));
        assert_eq!(
            wipe_app_data(legacy_sub, WipeScope::Settings, true),
            Err(WipeError::ProtectedLegacyProfile)
        );
    }

    #[test]
    fn test_native_beta_allowed() {
        let native_beta = Path::new(r"C:\Users\User\AppData\Roaming\zabor-desktop-native-beta");
        assert!(!is_legacy_electron_profile(native_beta));
    }

    #[test]
    fn test_wipe_requires_drain_for_all() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(
            wipe_app_data(temp.path(), WipeScope::All, false),
            Err(WipeError::DrainRequired)
        );
        assert!(wipe_app_data(temp.path(), WipeScope::All, true).is_ok());
    }

    #[test]
    fn test_wipe_settings_only() {
        let temp = tempfile::tempdir().unwrap();
        let settings = temp.path().join("settings.json");
        let cache = temp.path().join("cache");
        fs::write(&settings, "{}").unwrap();
        fs::create_dir(&cache).unwrap();

        wipe_app_data(temp.path(), WipeScope::Settings, false).unwrap();
        assert!(!settings.exists());
        assert!(cache.exists());
    }
}
