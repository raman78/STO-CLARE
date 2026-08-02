//! Where the app keeps its per-user files — and the one-time move from the
//! directory the pre-rename versions used.
//!
//! Every name the app writes into the user's config directory lives here, so a
//! rename is one edit rather than a hunt through the tree. The app was called
//! `STO_CombatLogAnalyzer` up to 1.8.1; settings written by those versions are
//! carried over on the first start of a renamed build.

use std::path::{Path, PathBuf};

/// Config-dir subfolder holding settings, the log and any rule overrides.
pub const APP_CONFIG_DIR: &str = "STO-CLARE";
/// The same folder as written by versions up to 1.8.1.
const LEGACY_APP_CONFIG_DIR: &str = "STO_CombatLogAnalyzer";

pub const SETTINGS_FILE_NAME: &str = "STO-CLARE_Settings.json";
/// Settings file name of versions up to 1.8.1, both in the config dir and in
/// the much older location next to the executable.
pub const LEGACY_SETTINGS_FILE_NAME: &str = "STO_CombatLogAnalyzer_Settings.json";

pub const LOG_FILE_NAME: &str = "STO-CLARE.log";

/// Per-user config directory: `~/.config/STO-CLARE` on Linux,
/// `%APPDATA%\STO-CLARE` on Windows. Using the OS config dir means settings and
/// logs survive when the program lives in a read-only location (e.g. `/usr/bin`,
/// `C:\Program Files`, an AppImage mount).
pub fn config_dir() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join(APP_CONFIG_DIR))
}

fn legacy_config_dir() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join(LEGACY_APP_CONFIG_DIR))
}

/// Carry settings and rule overrides over from the pre-rename config directory.
///
/// Must run before anything reads the settings. Copies rather than moves, so a
/// still-installed 1.8.x keeps working, and never overwrites a file that is
/// already there — which also makes it a no-op on every start after the first.
pub fn migrate_legacy_config() {
    let (Some(from), Some(to)) = (legacy_config_dir(), config_dir()) else {
        return;
    };
    if !from.is_dir() {
        return;
    }
    match migrate_dir(&from, &to) {
        Ok(0) => (),
        Ok(count) => log::info!(
            "carried {count} file(s) over from {} to {}",
            from.display(),
            to.display()
        ),
        Err(e) => log::warn!("could not carry over {}: {e}", from.display()),
    }
}

/// Copies every file of `from` into `to`, under the current name for the
/// settings file. Returns how many files were copied; existing files are left
/// alone and not counted.
fn migrate_dir(from: &Path, to: &Path) -> std::io::Result<usize> {
    let mut copied = 0;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        if !entry.file_type()?.is_file() {
            continue;
        }
        let name = entry.file_name();
        let name = if name == LEGACY_SETTINGS_FILE_NAME {
            SETTINGS_FILE_NAME.into()
        } else {
            name
        };
        let target = to.join(name);
        if target.exists() {
            continue;
        }
        std::fs::create_dir_all(to)?;
        std::fs::copy(entry.path(), &target)?;
        copied += 1;
    }
    Ok(copied)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_settings_file_is_carried_over_under_the_new_name() {
        let root = temp_dir("clare-migrate-rename");
        let from = root.join("old");
        let to = root.join("new");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::write(from.join(LEGACY_SETTINGS_FILE_NAME), "{}").unwrap();

        assert_eq!(1, migrate_dir(&from, &to).unwrap());
        assert!(to.join(SETTINGS_FILE_NAME).is_file());
        assert!(!to.join(LEGACY_SETTINGS_FILE_NAME).exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Rule overrides and anything else keep their own name.
    #[test]
    fn other_files_are_carried_over_unchanged() {
        let root = temp_dir("clare-migrate-other");
        let from = root.join("old");
        let to = root.join("new");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::write(from.join("detection_rules.json"), "[]").unwrap();

        assert_eq!(1, migrate_dir(&from, &to).unwrap());
        assert_eq!("[]", std::fs::read_to_string(to.join("detection_rules.json")).unwrap());

        let _ = std::fs::remove_dir_all(&root);
    }

    /// Settings written since the rename win, so the move cannot undo them —
    /// and re-running it changes nothing.
    #[test]
    fn existing_files_are_never_overwritten() {
        let root = temp_dir("clare-migrate-keep");
        let from = root.join("old");
        let to = root.join("new");
        std::fs::create_dir_all(&from).unwrap();
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(from.join(LEGACY_SETTINGS_FILE_NAME), "old").unwrap();
        std::fs::write(to.join(SETTINGS_FILE_NAME), "new").unwrap();

        assert_eq!(0, migrate_dir(&from, &to).unwrap());
        assert_eq!("new", std::fs::read_to_string(to.join(SETTINGS_FILE_NAME)).unwrap());

        let _ = std::fs::remove_dir_all(&root);
    }
}
