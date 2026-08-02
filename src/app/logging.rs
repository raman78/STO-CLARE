use std::{fs::OpenOptions, path::PathBuf};

use simplelog::{CombinedLogger, Config, SharedLogger, SimpleLogger, WriteLogger};

use super::settings::Settings;
use crate::helpers::paths;

pub fn initialize() {
    let settings = Settings::load_or_default();

    if !settings.debug.enable_log {
        return;
    }

    let mut loggers: Vec<Box<dyn SharedLogger>> =
        vec![SimpleLogger::new(log::LevelFilter::Info, Config::default())];

    if let Some(file) =
        file_path().and_then(|p| OpenOptions::new().create(true).append(true).open(&p).ok())
    {
        loggers.push(WriteLogger::new(
            settings.debug.log_level_filter,
            Config::default(),
            file,
        ));
    }

    CombinedLogger::init(loggers).unwrap();
}

fn file_path() -> Option<PathBuf> {
    let dir = Settings::config_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(paths::LOG_FILE_NAME))
}
