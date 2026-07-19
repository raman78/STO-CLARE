#![allow(non_snake_case)]
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

    logging::initialize();

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

    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_app_id(app::desktop_install::APP_ID)
            .with_inner_size(vec2(1280.0, 720.0))
            .with_min_inner_size(vec2(480.0, 270.0))
            .with_icon(icon_data()),
        ..Default::default()
    };

    let res = eframe::run_native(
        &format!("STO_CombatLogAnalyzer V{}", env!("CARGO_PKG_VERSION")),
        native_options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    );

    if let Err(err) = res {
        log::error!("eframe crashed: {}", err);
    }
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
