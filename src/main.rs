#![allow(non_snake_case)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::backtrace::Backtrace;

use app::logging;

mod analyzer;
mod app;
mod custom_widgets;
mod helpers;
mod upload;
mod platform;

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
        println!("STO_CombatLogAnalyzer {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    logging::initialize();

    if std::env::args().any(|a| a == "--upgrade") {
        if let Err(e) = app::self_upgrade::run() {
            eprintln!("Upgrade failed: {e}");
            std::process::exit(1);
        }
        return;
    }

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

    if let Err(e) = platform::run() {
        log::error!("app crashed: {e}");
        eprintln!("app crashed: {e}");
    }
}
