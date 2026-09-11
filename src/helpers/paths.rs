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

/// The Analysis tab's four rule sets, kept apart from the settings so the file
/// can be copied to another machine or handed to someone else on its own.
/// Written from 2.8 on; before that the rules lived inside the settings file
/// and are carried over on the first start — see `AnalysisSettings::load_rules`.
pub const RULES_FILE_NAME: &str = "STO-CLARE_Rules.toml";

/// Added to a file's name when the program could not read it and has started
/// without it.
pub const DAMAGED: &str = "damaged";
/// Added to a file's name when the program has taken something out of it and
/// kept the original as it was — the settings file, once the rules it used to
/// hold moved to their own.
pub const ARCHIVED: &str = "archived";

/// Moves a file the program cannot use out of the way, and says where it went.
///
/// Moved rather than deleted, and never overwritten: a config file is the only
/// copy of choices its owner made by hand, and "it was unreadable" is the
/// program's opinion, not a fact the owner has agreed to. They may well be able
/// to read it, or lift values out of it. A second mishap therefore cannot erase
/// the evidence of the first either — the next free number is used.
pub fn set_aside(path: &Path, why: &str) -> std::io::Result<PathBuf> {
    let target = free_name(path, why)?;
    std::fs::rename(path, &target)?;
    Ok(target)
}

/// The same, but leaves the original in place. For a file that is still good
/// and is about to be rewritten with less in it.
pub fn keep_a_copy(path: &Path, why: &str) -> std::io::Result<PathBuf> {
    let target = free_name(path, why)?;
    std::fs::copy(path, &target)?;
    Ok(target)
}

/// `STO-CLARE_Settings.json` → `STO-CLARE_Settings_damaged.json`, or
/// `..._damaged_2.json` if that is taken, and so on.
///
/// The extension is kept so the file still opens in whatever the owner reads
/// JSON or TOML with; the reason goes before it, where it can be read.
fn free_name(path: &Path, why: &str) -> std::io::Result<PathBuf> {
    let taken = || {
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            format!("no free name left beside {}", path.display()),
        )
    };
    let missing = || std::io::Error::other(format!("{} has no name to work from", path.display()));
    let (Some(dir), Some(stem)) = (path.parent(), path.file_stem().and_then(|s| s.to_str())) else {
        return Err(missing());
    };
    let extension = path.extension().and_then(|e| e.to_str());
    for attempt in 1..100 {
        let counted = match attempt {
            1 => String::new(),
            n => format!("_{n}"),
        };
        let name = match extension {
            Some(extension) => format!("{stem}_{why}{counted}.{extension}"),
            None => format!("{stem}_{why}{counted}"),
        };
        let candidate = dir.join(name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(taken())
}

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

/// Where a run fetched from the ladder is put so it can be looked at.
///
/// A scratch directory rather than the config one: it is data the program can
/// fetch again at any time, the system clears it for us, and the config folder
/// is for settings. Named after the log it came from, so opening the same run
/// twice does not fetch it twice.
pub fn ladder_run(combatlog_id: i32) -> PathBuf {
    std::env::temp_dir()
        .join(APP_CONFIG_DIR)
        .join(format!("ladder-{combatlog_id}.log"))
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
        assert_eq!(
            "[]",
            std::fs::read_to_string(to.join("detection_rules.json")).unwrap()
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A file put aside keeps its extension, so it still opens in whatever
    /// reads that format, and says in its name why it was put there.
    #[test]
    fn a_file_put_aside_says_why_and_stays_openable() {
        let dir = temp_dir("clare-set-aside");
        let path = dir.join(SETTINGS_FILE_NAME);
        std::fs::write(&path, "broken").unwrap();

        let moved = set_aside(&path, DAMAGED).unwrap();

        assert_eq!(
            Some("STO-CLARE_Settings_damaged.json"),
            moved.file_name().and_then(|n| n.to_str())
        );
        assert_eq!("broken", std::fs::read_to_string(&moved).unwrap());
        assert!(!path.exists(), "and it is out of the way");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A second mishap must not erase the evidence of the first. `fs::rename`
    /// would overwrite silently on Linux, which is the whole reason a free name
    /// is looked for rather than assumed.
    #[test]
    fn putting_a_second_file_aside_keeps_the_first() {
        let dir = temp_dir("clare-set-aside-twice");
        let path = dir.join(SETTINGS_FILE_NAME);

        std::fs::write(&path, "first").unwrap();
        let first = set_aside(&path, DAMAGED).unwrap();
        std::fs::write(&path, "second").unwrap();
        let second = set_aside(&path, DAMAGED).unwrap();

        assert_ne!(first, second);
        assert_eq!("first", std::fs::read_to_string(&first).unwrap());
        assert_eq!("second", std::fs::read_to_string(&second).unwrap());
        assert_eq!(
            Some("STO-CLARE_Settings_damaged_2.json"),
            second.file_name().and_then(|n| n.to_str())
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A copy leaves the original where it is: it is still the file the program
    /// is about to rewrite, and losing it between the two steps would be worse
    /// than what the copy is there to prevent.
    #[test]
    fn a_copy_kept_aside_leaves_the_original() {
        let dir = temp_dir("clare-keep-copy");
        let path = dir.join(SETTINGS_FILE_NAME);
        std::fs::write(&path, "with rules in it").unwrap();

        let copy = keep_a_copy(&path, ARCHIVED).unwrap();

        assert_eq!(
            Some("STO-CLARE_Settings_archived.json"),
            copy.file_name().and_then(|n| n.to_str())
        );
        assert_eq!("with rules in it", std::fs::read_to_string(&copy).unwrap());
        assert_eq!("with rules in it", std::fs::read_to_string(&path).unwrap());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The rules file is TOML, and the name has to come out as TOML too.
    #[test]
    fn the_extension_survives() {
        let dir = temp_dir("clare-set-aside-toml");
        let path = dir.join(RULES_FILE_NAME);
        std::fs::write(&path, "version = 1").unwrap();

        let moved = set_aside(&path, DAMAGED).unwrap();

        assert_eq!(
            Some("STO-CLARE_Rules_damaged.toml"),
            moved.file_name().and_then(|n| n.to_str())
        );

        let _ = std::fs::remove_dir_all(&dir);
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
        assert_eq!(
            "new",
            std::fs::read_to_string(to.join(SETTINGS_FILE_NAME)).unwrap()
        );

        let _ = std::fs::remove_dir_all(&root);
    }
}
