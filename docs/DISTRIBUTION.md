# Distribution & desktop integration

How STO Combat Log Analyzer is built, released, and registered with the host
OS. Modeled on the sto-warp project, adapted for a Rust/eframe binary (there is
no PyPI/pipx — those are Python-only).

## Channels

| Audience | Mechanism | Entry point |
|---|---|---|
| End users (Linux) | Prebuilt tarball from GitHub Releases | `install.sh` |
| End users (Windows) | Inno Setup `.exe` from GitHub Releases | `install.ps1` |
| Local development | Symlink to a release build (editable analogue) | `scripts/dev-install.sh` |
| Rust users | `cargo install --git …` | — |

## Desktop integration (`src/app/desktop_install.rs`)

The app registers its own menu entry/shortcut on every platform, so there is a
single source of truth:

- **Linux** → `~/.local/share/applications/sto-cla-<id>.desktop` + icon in
  `~/.local/share/icons/sto-cla.png`.
- **Windows** → Start Menu `.lnk` (via the `mslnk` crate).
- **macOS** → `~/Applications/STO Combat Log Analyzer.app` bundle. **Untested.**

`<id>` is an 8-char hash of `std::env::current_exe()`. Consequences:

- Updating a binary **in place** keeps the same id → the entry is overwritten,
  never duplicated.
- Installing to a **different location** yields a different id → its own entry
  (as requested — duplicates only across distinct install locations).
- On Linux, sibling `sto-cla-*.desktop` files pointing at the *same* binary are
  swept on launch (safety net if the hash scheme ever changes).

Triggers:

- Normal launch → `install_desktop_entry(false)` (best-effort, non-fatal).
- `--install-desktop` / `--uninstall-desktop` → explicit, headless, then exit.

The main window's `app_id` is set to `sto-cla` so the runtime WM class matches
the `StartupWMClass` written into the `.desktop` entry.

## Local dev ("editable") — `scripts/dev-install.sh`

Rust compiles to a native binary; there is no true editable install. The script
approximates it:

1. `cargo build --release`
2. symlink `~/.local/bin/sto-cla` → `target/release/STO_CombatLogAnalyzer`
3. `--install-desktop` for that build

After a code change, `cargo build --release` alone refreshes what `sto-cla`
(and the menu entry, which resolves to the same real path) runs.

## Releases — `.github/workflows/release.yml`

Triggered on `release: published` (tag `vX.Y.Z`) and `workflow_dispatch`.

- **linux** job: apt build deps (winit libs — rfd uses the XDG portal via
  `zbus`, so no GTK needed), `cargo build --release`, package
  `STO_CombatLogAnalyzer-<ver>-linux-x86_64.tar.gz`, attach to the release.
- **windows** job: `cargo build --release`, `choco install innosetup`, compile
  `packaging/windows/STO_CombatLogAnalyzer.iss` with `/DAppVersion=<ver>`,
  attach `STO_CombatLogAnalyzer-<ver>-setup.exe`.

The Inno installer creates **no** `[Icons]` of its own — it runs the exe with
`--install-desktop` (and `--uninstall-desktop` on removal), reusing the app's
shortcut logic. Its stable `AppId` makes upgrades replace in place.

> Not yet exercised end-to-end: the Windows installer and the CI workflow can
> only be verified on a real Windows runner / a tagged release. The macOS `.app`
> path is written by analogy and untested.
