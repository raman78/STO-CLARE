# Distribution & desktop integration

How STO-CLARE is built, released, and registered with the host
OS. Modeled on the sto-warp project, adapted for a Rust/eframe binary (there is
no PyPI/pipx — those are Python-only).

## Channels

| Audience            | Mechanism                                      | Entry point              |
|---------------------|------------------------------------------------|--------------------------|
| End users (Linux)   | Prebuilt tarball from GitHub Releases          | `install.sh`             |
| End users (Windows) | Inno Setup `.exe` from GitHub Releases         | `install.ps1`            |
| Local development   | Symlink to a release build (editable analogue) | `scripts/dev-install.sh` |
| Rust users          | `cargo install --git …`                        | —                        |

## Desktop integration (`src/app/desktop_install.rs`)

The app registers its own menu entry/shortcut on every platform, so there is a
single source of truth:

- **Linux** → `~/.local/share/applications/sto-clare.desktop` + icon in
  `~/.local/share/icons/sto-clare.png`.
- **Windows** → Start Menu `.lnk` (via the `mslnk` crate).
- **macOS** → `~/Applications/STO-CLARE.app` bundle. **Untested.**

**The entry is named after the app id, and so is the window** (`app_id`
`sto-clare`, set on the viewport in `main.rs`). That is not cosmetic: a Wayland
compositor is handed the app id and nothing else, and finds the icon by looking
for `<app id>.desktop` — which is what the xdg-shell spec asks for. Name them
differently and KWin draws the generic "unknown Wayland application" mark in the
title bar and the task switcher, however good the icon compiled into the binary
is. `with_icon` does not help there: it feeds `_NET_WM_ICON` on X11 and the
Windows window class, neither of which exists on Wayland.

One consequence: there is one entry per user, not one per install location.
Installing to a second location overwrites it, and the last install to run owns
the menu entry. Entries from the older per-location scheme
(`sto-clare-<hash>.desktop`, and `sto-cla-<hash>.desktop` from before the
rename) are swept on launch when they are dead — pointing at the binary being
installed for, or at one that is gone.

Triggers:

- Normal launch → `install_desktop_entry(false)` (best-effort, non-fatal).
- `--install-desktop` / `--uninstall-desktop` → explicit, headless, then exit.

The main window's `app_id` is set to `sto-clare` so the runtime WM class matches
the `StartupWMClass` written into the `.desktop` entry.

## Icon

`icon/build.py` draws the icon and is the only file to edit. There is no vector
master: the glass look is gradients, blurs and soft shadows, which are read off
the numbers at the top of that script rather than off a drawing. Its only inputs
are `icon/delta-mask.png` (the silhouette of the mark) and the Ubuntu Bold face
already bundled for the tables. It writes the two files that ship:

| File            | Used by                                                                  |
|-----------------|--------------------------------------------------------------------------|
| `icon/icon.png` | window icon (`include_bytes!` in `main.rs`), desktop entry, macOS bundle |
| `icon/icon.ico` | `build.rs` via `winres` (the exe's own icon) and the Inno installer      |

Both are committed, so a normal build needs neither the script nor Pillow. Run
`python3 icon/build.py` after changing the design; it works at 1024px and scales
down, so edges and blurs stay smooth at every size that ships.

## Local dev ("editable") — `scripts/dev-install.sh`

Rust compiles to a native binary; there is no true editable install. The script
approximates it:

1. `cargo build --release`
2. symlink `~/.local/bin/sto-clare` → `target/release/sto-clare`
3. `--install-desktop` for that build

After a code change, `cargo build --release` alone refreshes what `sto-clare`
(and the menu entry, which resolves to the same real path) runs.

It also writes a second menu entry, **"STO-CLARE (dev build)"**
(`sto-clare-dev.desktop`), which runs `scripts/run-dev.sh`: build the checkout,
then start it. The entry sets `Terminal=true`, so the build is visible while it
runs; on success the app is detached and the terminal closes, on failure the
window stays with the error. The script picks up `~/.cargo/env` when `cargo` is
not on the session's PATH — a menu entry never runs the shell profile that puts
it there. Remove the entry by deleting that file; nothing else refers to it.

## Releases — `.github/workflows/release.yml`

Triggered on `release: published` (tag `vX.Y.Z`) and `workflow_dispatch`.

- **linux** job: apt build deps (winit libs — rfd uses the XDG portal via
  `zbus`, so no GTK needed), `cargo build --release`, package
  `STO-CLARE-<ver>-linux-x86_64.tar.gz`, attach to the release.
- **windows** job: `cargo build --release`, `choco install innosetup`, compile
  `packaging/windows/STO-CLARE.iss` with `/DAppVersion=<ver>`,
  attach `STO-CLARE-<ver>-setup.exe` **and** a bare
  `STO-CLARE-<ver>-windows-x86_64.zip` (used by `--upgrade`).

Asset names must contain the platform tag (`linux-x86_64`, `windows-x86_64`)
and hold the binary at the archive root — that is what `--upgrade` matches on.

## Upgrading — `--upgrade` (`src/app/self_upgrade.rs`)

The analogue of `pipx upgrade`. `sto-clare --upgrade` uses the `self_update` crate
to query the latest GitHub Release, download this platform's asset, and replace
the running executable in place (atomic swap). `sto-clare --version` prints the
current version. Both are headless and exit without opening the GUI.

Requires write access to the installed binary — fine for the `install.sh` /
`dev-install.sh` locations under `~/.local`; a system-wide install would need
elevation. `cargo install` users upgrade with `cargo install --git … --force`
instead.

The Inno installer creates **no** `[Icons]` of its own — it runs the exe with
`--install-desktop` (and `--uninstall-desktop` on removal), reusing the app's
shortcut logic. Its stable `AppId` makes upgrades replace in place — kept
unchanged across the 2.0 rename, with an `[InstallDelete]` entry that removes
the old `STO_CombatLogAnalyzer.exe` from the install folder.

> Not yet exercised end-to-end: the Windows installer and the CI workflow can
> only be verified on a real Windows runner / a tagged release. The macOS `.app`
> path is written by analogy and untested.
