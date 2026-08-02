//! Self-update from GitHub Releases — the analogue of `pipx upgrade`.
//!
//! `--upgrade` downloads the latest release asset for the current platform and
//! replaces the running executable in place (handled atomically by the
//! `self_update` crate). Asset names must contain the platform tag used below
//! (see `.github/workflows/release.yml`): `linux-x86_64`, `windows-x86_64`.

/// Platform tag embedded in our release asset names, or `None` if unsupported.
fn platform_target() -> Option<&'static str> {
    if cfg!(target_os = "linux") {
        Some("linux-x86_64")
    } else if cfg!(target_os = "windows") {
        Some("windows-x86_64")
    } else if cfg!(target_os = "macos") {
        Some("macos-x86_64")
    } else {
        None
    }
}

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let target = platform_target().ok_or("self-update is not supported on this platform")?;
    let bin_name = if cfg!(windows) {
        "sto-clare.exe"
    } else {
        "sto-clare"
    };

    let status = self_update::backends::github::Update::configure()
        .repo_owner("raman78")
        .repo_name("STO-CLARE")
        .bin_name(bin_name)
        .target(target)
        .show_download_progress(true)
        .current_version(env!("CARGO_PKG_VERSION"))
        .build()?
        .update()?;

    if status.updated() {
        println!(
            "Updated to {}. Restart the app to use the new version.",
            status.version()
        );
    } else {
        println!("Already up to date ({}).", status.version());
    }
    Ok(())
}
