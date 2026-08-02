//! First-run desktop / Start Menu / Launchpad integrator.
//!
//! Ported from sto-warp's `warp/gui/desktop_install.py`. On a normal launch we
//! register the app with the host OS so it shows up alongside other GUI
//! applications, and expose it explicitly via the `--install-desktop` flag.
//!
//!   - Linux   → `~/.local/share/applications/sto-clare-<id>.desktop` + icon
//!   - Windows → Start Menu `.lnk` shortcut
//!   - macOS   → `~/Applications/STO-CLARE.app` bundle (UNTESTED)
//!
//! Idempotent: re-runs are a no-op once the entry exists (unless `force`).
//! The Linux entry is keyed by an 8-char hash of the executable path, so
//! installs in *different* locations get their own menu entry while updating
//! in place overwrites rather than piling up duplicates. Stale entries left
//! by an old location that points at the *same* binary are swept on launch.
//!
//! Every failure is best-effort and non-fatal — desktop integration must never
//! stop the app from starting.

use std::hash::{Hash, Hasher};
use std::path::PathBuf;

/// Human-facing application name (menu label, .app / .lnk basename).
const APP_NAME: &str = "STO-CLARE";
/// Short id used for the desktop-entry / icon basename and the WM class.
pub const APP_ID: &str = "sto-clare";
// What the app called itself up to 1.8.1. Entries left behind under these names
// are cleaned up when a renamed build registers itself — but only when they are
// dead (see `sweep_stale_entries`), so a 1.8.x that is still installed elsewhere
// keeps its menu entry.
/// Display name of the old Start Menu shortcut (Windows keys its link by name).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const LEGACY_APP_NAME: &str = "STO Combat Log Analyzer";
/// Old desktop-entry / icon basename.
const LEGACY_APP_ID: &str = "sto-cla";
/// Icon shipped with the binary (same asset as the window icon).
const ICON_PNG: &[u8] = include_bytes!("../../icon/icon.png");

/// 8-char stable hash of the current executable's path. Stable across runs of
/// the same binary; differs when the binary lives in a different location.
fn install_id() -> String {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    exe.hash(&mut hasher);
    format!("{:016x}", hasher.finish())[..8].to_string()
}

/// Register the app with the host OS. Returns the path written (or the existing
/// entry). Best-effort: logs and returns `None` on any failure.
pub fn install_desktop_entry(force: bool) -> Option<PathBuf> {
    match install_impl(force) {
        Ok(path) => path,
        Err(e) => {
            log::warn!("desktop installer: {e}");
            None
        }
    }
}

/// Remove the entry created for the current install location. Best-effort.
pub fn uninstall_desktop_entry() {
    if let Err(e) = uninstall_impl() {
        log::warn!("desktop uninstaller: {e}");
    }
}

// ---------------------------------------------------------------- Linux

#[cfg(target_os = "linux")]
fn data_home() -> Option<PathBuf> {
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| dirs::home_dir().map(|h| h.join(".local/share")))
}

#[cfg(target_os = "linux")]
fn install_impl(force: bool) -> std::io::Result<Option<PathBuf>> {
    use std::io::{Error, ErrorKind};

    let exec_path = std::env::current_exe()?;
    let exec = exec_path.to_string_lossy();

    let data_home =
        data_home().ok_or_else(|| Error::new(ErrorKind::NotFound, "no XDG data home"))?;
    let apps_dir = data_home.join("applications");
    let icons_dir = data_home.join("icons");

    let entry_name = format!("{APP_ID}-{}.desktop", install_id());
    let entry_path = apps_dir.join(&entry_name);

    // Tidy up entries left behind after moving/upgrading the binary.
    sweep_stale_entries(&apps_dir, &exec, &entry_name);

    if entry_path.is_file() && !force {
        return Ok(Some(entry_path));
    }

    // Write the icon next to other user icons so the entry has a stable
    // absolute path regardless of where the binary lives.
    std::fs::create_dir_all(&icons_dir)?;
    let icon_path = icons_dir.join(format!("{APP_ID}.png"));
    std::fs::write(&icon_path, ICON_PNG)?;

    // The icon of the old name is ours too. It goes once no entry is left that
    // could still be pointing at it.
    let legacy_icon = icons_dir.join(format!("{LEGACY_APP_ID}.png"));
    if legacy_icon.is_file() && !has_entry_with_prefix(&apps_dir, LEGACY_APP_ID) {
        let _ = std::fs::remove_file(&legacy_icon);
    }

    std::fs::create_dir_all(&apps_dir)?;
    let contents = format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name={APP_NAME}\n\
         Comment=Analyze Star Trek Online combat logs\n\
         Exec=\"{exec}\"\n\
         Icon={icon}\n\
         Terminal=false\n\
         Categories=Game;\n\
         StartupNotify=true\n\
         StartupWMClass={APP_ID}\n",
        icon = icon_path.display(),
    );
    std::fs::write(&entry_path, contents)?;
    log::info!("desktop installer: wrote {}", entry_path.display());

    // Best-effort menu-cache refresh; harmless if the tool is missing.
    let _ = std::process::Command::new("update-desktop-database")
        .arg(&apps_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    Ok(Some(entry_path))
}

/// Remove sibling `sto-clare-*.desktop` / `sto-cla-*.desktop` entries that are
/// stale: they point at the same binary as the current install (a different
/// `install_id` from an old location), or at a binary that is no longer there —
/// which is what the pre-rename entries of *this* install become. An entry
/// targeting a different, existing Exec (a genuinely parallel install) is left
/// alone.
#[cfg(target_os = "linux")]
fn sweep_stale_entries(apps_dir: &std::path::Path, our_exec: &str, our_name: &str) {
    let our_exec = our_exec.trim().trim_matches('"');
    let Ok(entries) = std::fs::read_dir(apps_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == our_name || !is_our_entry_name(&name) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("Exec=") {
                let exec = rest.trim().trim_matches('"');
                if (exec == our_exec || !std::path::Path::new(exec).exists())
                    && std::fs::remove_file(entry.path()).is_ok()
                {
                    log::info!("desktop installer: removed stale entry {name}");
                }
                break;
            }
        }
    }
}

/// Whether a file name is a desktop entry this app wrote, under either name.
#[cfg(target_os = "linux")]
fn is_our_entry_name(name: &str) -> bool {
    name.ends_with(".desktop")
        && [APP_ID, LEGACY_APP_ID]
            .iter()
            .any(|id| name.starts_with(&format!("{id}-")))
}

/// Whether any entry of `id` is still installed.
#[cfg(target_os = "linux")]
fn has_entry_with_prefix(apps_dir: &std::path::Path, id: &str) -> bool {
    let prefix = format!("{id}-");
    std::fs::read_dir(apps_dir).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            name.starts_with(&prefix) && name.ends_with(".desktop")
        })
    })
}

#[cfg(target_os = "linux")]
fn uninstall_impl() -> std::io::Result<()> {
    let Some(data_home) = data_home() else {
        return Ok(());
    };
    let apps_dir = data_home.join("applications");
    let entry_path = apps_dir.join(format!("{APP_ID}-{}.desktop", install_id()));
    if entry_path.is_file() {
        std::fs::remove_file(&entry_path)?;
        log::info!("desktop uninstaller: removed {}", entry_path.display());
    }
    Ok(())
}

// ---------------------------------------------------------------- Windows

#[cfg(target_os = "windows")]
fn start_menu_link() -> std::io::Result<PathBuf> {
    use std::io::{Error, ErrorKind};
    let appdata = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .ok_or_else(|| Error::new(ErrorKind::NotFound, "no APPDATA"))?;
    Ok(appdata
        .join(r"Microsoft\Windows\Start Menu\Programs")
        .join(format!("{APP_NAME}.lnk")))
}

#[cfg(target_os = "windows")]
fn install_impl(force: bool) -> std::io::Result<Option<PathBuf>> {
    use std::io::{Error, ErrorKind};

    let exec_path = std::env::current_exe()?;
    let link_path = start_menu_link()?;

    // A shortcut of the old name that points at a binary which is no longer
    // there is ours and dead — the pre-rename entry of this very install.
    if let Some(dir) = link_path.parent() {
        let legacy = dir.join(format!("{LEGACY_APP_NAME}.lnk"));
        if legacy.is_file()
            && !exec_path
                .with_file_name("STO_CombatLogAnalyzer.exe")
                .exists()
        {
            let _ = std::fs::remove_file(&legacy);
        }
    }

    if link_path.is_file() && !force {
        return Ok(Some(link_path));
    }
    if let Some(dir) = link_path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let shortcut = mslnk::ShellLink::new(&exec_path)
        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
    shortcut
        .create_lnk(&link_path)
        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))?;
    log::info!("desktop installer: wrote {}", link_path.display());
    Ok(Some(link_path))
}

#[cfg(target_os = "windows")]
fn uninstall_impl() -> std::io::Result<()> {
    let link_path = start_menu_link()?;
    if link_path.is_file() {
        std::fs::remove_file(&link_path)?;
        log::info!("desktop uninstaller: removed {}", link_path.display());
    }
    Ok(())
}

// ---------------------------------------------------------------- macOS
// UNTESTED — written by analogy to sto-warp's macos_app_bundle. Verify on real
// hardware: icon should ideally be an .icns, and the bundle may need signing.

#[cfg(target_os = "macos")]
fn app_bundle_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join("Applications").join(format!("{APP_NAME}.app")))
}

#[cfg(target_os = "macos")]
fn install_impl(force: bool) -> std::io::Result<Option<PathBuf>> {
    use std::io::{Error, ErrorKind};
    use std::os::unix::fs::PermissionsExt;

    let exec_path = std::env::current_exe()?;
    let app_dir =
        app_bundle_path().ok_or_else(|| Error::new(ErrorKind::NotFound, "no home dir"))?;
    if app_dir.is_dir() && !force {
        return Ok(Some(app_dir));
    }

    let macos_dir = app_dir.join("Contents/MacOS");
    let resources_dir = app_dir.join("Contents/Resources");
    std::fs::create_dir_all(&macos_dir)?;
    std::fs::create_dir_all(&resources_dir)?;

    // Launcher shim that execs the real binary wherever it lives.
    let launcher = macos_dir.join(APP_ID);
    std::fs::write(
        &launcher,
        format!("#!/bin/sh\nexec \"{}\" \"$@\"\n", exec_path.display()),
    )?;
    std::fs::set_permissions(&launcher, std::fs::Permissions::from_mode(0o755))?;

    std::fs::write(resources_dir.join("icon.png"), ICON_PNG)?;

    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         \t<key>CFBundleName</key><string>{APP_NAME}</string>\n\
         \t<key>CFBundleIdentifier</key><string>com.github.stoclare</string>\n\
         \t<key>CFBundleExecutable</key><string>{APP_ID}</string>\n\
         \t<key>CFBundleIconFile</key><string>icon.png</string>\n\
         \t<key>CFBundlePackageType</key><string>APPL</string>\n\
         </dict>\n</plist>\n"
    );
    std::fs::write(app_dir.join("Contents/Info.plist"), plist)?;
    log::info!("desktop installer: wrote {}", app_dir.display());
    Ok(Some(app_dir))
}

#[cfg(target_os = "macos")]
fn uninstall_impl() -> std::io::Result<()> {
    if let Some(app_dir) = app_bundle_path() {
        if app_dir.is_dir() {
            std::fs::remove_dir_all(&app_dir)?;
            log::info!("desktop uninstaller: removed {}", app_dir.display());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- other

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn install_impl(_force: bool) -> std::io::Result<Option<PathBuf>> {
    Ok(None)
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn uninstall_impl() -> std::io::Result<()> {
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn our_entries_are_recognized_under_both_names() {
        assert!(is_our_entry_name("sto-clare-1a2b3c4d.desktop"));
        assert!(is_our_entry_name("sto-cla-1a2b3c4d.desktop"));
    }

    /// The current name is not a match for the old prefix, and vice versa —
    /// otherwise the sweep would treat live entries as leftovers.
    #[test]
    fn the_two_names_do_not_match_each_other() {
        assert!(!"sto-clare-1a2b3c4d.desktop".starts_with(&format!("{LEGACY_APP_ID}-")));
        assert!(!"sto-cla-1a2b3c4d.desktop".starts_with(&format!("{APP_ID}-")));
    }

    #[test]
    fn other_applications_are_left_alone() {
        assert!(!is_our_entry_name("sto-warp.desktop"));
        assert!(!is_our_entry_name("sto-clare-1a2b3c4d.png"));
    }
}
