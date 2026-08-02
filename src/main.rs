#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{backtrace::Backtrace, io::Cursor};

use app::logging;
use eframe::{
    egui::{IconData, ViewportBuilder},
    epaint::vec2,
};

mod analyzer;
mod app;
mod custom_widgets;
mod helpers;
mod upload;

fn main() {
    std::panic::set_hook(Box::new(|i| {
        log::error!("{}", i);
        let backtrace = Backtrace::capture();
        log::error!("backtrace:");
        log::error!("{}", backtrace);
        println!("{}", i);
        println!("{}", backtrace);
    }));

    if std::env::args().any(|a| a == "--version") {
        println!("STO-CLARE {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    // Settings written under the old name are carried over here, before
    // anything reads them — logging is the first to.
    helpers::paths::migrate_legacy_config();

    logging::initialize();

    // Self-update from GitHub Releases (like `pipx upgrade`). Headless, exits.
    if std::env::args().any(|a| a == "--upgrade") {
        if let Err(e) = app::self_upgrade::run() {
            eprintln!("Upgrade failed: {e}");
            std::process::exit(1);
        }
        return;
    }

    // Desktop integration (Linux .desktop / Windows .lnk / macOS .app). The
    // `--install-desktop` / `--uninstall-desktop` flags run it explicitly and
    // exit; a normal launch registers the entry best-effort.
    if std::env::args().any(|a| a == "--install-desktop") {
        match app::desktop_install::install_desktop_entry(true) {
            Some(path) => println!("Installed desktop entry: {}", path.display()),
            None => println!("Desktop entry not installed (see log)."),
        }
        return;
    }
    if std::env::args().any(|a| a == "--uninstall-desktop") {
        app::desktop_install::uninstall_desktop_entry();
        println!("Removed desktop entry (if present).");
        return;
    }
    app::desktop_install::install_desktop_entry(false);

    // Restore the last window size / maximized state (see app::App::on_exit).
    let (saved_size, maximized) = app::saved_window_geometry();
    #[allow(unused_mut)]
    let mut native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_app_id(app::desktop_install::APP_ID)
            .with_inner_size(saved_size.unwrap_or(vec2(1280.0, 720.0)))
            .with_min_inner_size(vec2(800.0, 600.0))
            .with_maximized(maximized)
            .with_icon(icon_data()),
        ..Default::default()
    };

    // In a Wayland session, create the single wgpu instance/adapter/device/queue
    // up front and share it: eframe's main window renders through it
    // (WgpuSetup::Existing) and so does the layer-shell overlay (handed the same
    // handles via App::new). One Vulkan stack for the whole app, no teardown
    // races when the overlay closes.
    //
    // An X11 session uses the plain viewport overlay and needs none of this, so
    // it is skipped there and eframe sets up its renderer as usual. The window
    // does not exist yet, so the session is determined the same way winit
    // determines it (see winit's platform_impl/linux).
    #[cfg(target_os = "linux")]
    let overlay_instance = if is_wayland_session() {
        let gpu = app::create_shared_gpu();
        let instance = gpu.instance.clone();
        native_options.wgpu_options.wgpu_setup =
            eframe::egui_wgpu::WgpuSetup::Existing(eframe::egui_wgpu::WgpuSetupExisting {
                instance: gpu.instance,
                adapter: gpu.adapter,
                device: gpu.device,
                queue: gpu.queue,
            });
        Some(instance)
    } else {
        None
    };
    #[cfg(not(target_os = "linux"))]
    let overlay_instance: Option<eframe::wgpu::Instance> = None;

    let res = eframe::run_native(
        &format!("STO-CLARE V{}", env!("CARGO_PKG_VERSION")),
        native_options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, overlay_instance)))),
    );

    if let Err(err) = res {
        log::error!("eframe crashed: {}", err);
    }
}

/// Whether this is a Wayland session, decided the same way winit decides which
/// backend to use: a non-empty `WAYLAND_DISPLAY` or `WAYLAND_SOCKET` means
/// Wayland, otherwise X11.
#[cfg(target_os = "linux")]
fn is_wayland_session() -> bool {
    ["WAYLAND_DISPLAY", "WAYLAND_SOCKET"]
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
}

fn icon_data() -> IconData {
    const ICON: &[u8] = include_bytes!("../icon/icon.png");
    let decoder = png::Decoder::new(Cursor::new(ICON));
    let mut reader = decoder.read_info().unwrap();
    let mut data = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut data).unwrap();
    assert_eq!(info.color_type, png::ColorType::Rgba);
    IconData {
        rgba: data,
        width: info.width,
        height: info.height,
    }
}
