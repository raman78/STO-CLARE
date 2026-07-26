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

    // Restore the window size of the previous run. eframe clamps the requested
    // size to the largest monitor on its own, so a window that was last used on
    // a bigger screen still comes up usable here.
    let (size, maximized) = app::saved_window_geometry();
    let native_options = eframe::NativeOptions {
        viewport: ViewportBuilder::default()
            .with_inner_size(size.unwrap_or(vec2(1280.0, 720.0)))
            .with_min_inner_size(vec2(800.0, 600.0))
            .with_maximized(maximized)
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
