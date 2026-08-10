use std::{fs::OpenOptions, path::PathBuf, sync::RwLock};

use log::{LevelFilter, Log, Metadata, Record};
use simplelog::{CombinedLogger, Config, SharedLogger, SimpleLogger, WriteLogger};

use super::settings::{DebugSettings, Settings};
use crate::helpers::paths;

/// The one logger the process ever installs.
///
/// `log::set_logger` takes effect once and for good, so the switch in Debug
/// settings cannot put a logger in place or take it away — it can only change
/// what this one forwards to. That is what lets **Enable Log** start logging the
/// moment the settings window is closed with OK, instead of at the next start,
/// which is when the log is least useful: whatever was being investigated is
/// gone by then.
static ROUTER: Router = Router {
    inner: RwLock::new(None),
};

struct Router {
    /// The real logger while logging is on. `None` closes the log file.
    inner: RwLock<Option<Box<dyn Log>>>,
}

impl Log for Router {
    fn enabled(&self, metadata: &Metadata) -> bool {
        match self.inner.read() {
            Ok(inner) => inner
                .as_ref()
                .is_some_and(|logger| logger.enabled(metadata)),
            Err(_) => false,
        }
    }

    fn log(&self, record: &Record) {
        if let Ok(inner) = self.inner.read()
            && let Some(logger) = inner.as_ref()
        {
            logger.log(record);
        }
    }

    fn flush(&self) {
        if let Ok(inner) = self.inner.read()
            && let Some(logger) = inner.as_ref()
        {
            logger.flush();
        }
    }
}

/// Installs the router and points it at whatever the saved Debug settings ask
/// for. Called once, before anything logs.
pub fn initialize() {
    if log::set_logger(&ROUTER).is_err() {
        return;
    }
    apply(&Settings::load_or_default().debug);
}

/// The level the console mirror is kept at, whatever the file is set to.
const STDERR_LEVEL: LevelFilter = LevelFilter::Info;

/// Points the logger at what the Debug settings ask for. Called again every time
/// they are applied, so the switch and the level take effect immediately.
///
/// Switched off, the log file is closed and the maximum level goes to `Off`.
pub fn apply(debug: &DebugSettings) {
    log::set_max_level(max_level(debug));

    let logger = debug.enable_log.then(|| {
        let mut loggers: Vec<Box<dyn SharedLogger>> =
            vec![SimpleLogger::new(STDERR_LEVEL, Config::default())];
        if let Some(file) =
            file_path().and_then(|p| OpenOptions::new().create(true).append(true).open(&p).ok())
        {
            loggers.push(WriteLogger::new(
                debug.log_level_filter,
                Config::default(),
                file,
            ));
        }
        CombinedLogger::new(loggers) as Box<dyn Log>
    });

    if let Ok(mut inner) = ROUTER.inner.write() {
        *inner = logger;
    }
}

/// The most either destination wants: the file at the chosen level, the console
/// mirror always at `STDERR_LEVEL`, and nothing at all when logging is off. This
/// is what the `log` macros test before they format anything, so a switched-off
/// logger costs one relaxed atomic load per call site.
fn max_level(debug: &DebugSettings) -> LevelFilter {
    if debug.enable_log {
        debug.log_level_filter.max(STDERR_LEVEL)
    } else {
        LevelFilter::Off
    }
}

fn file_path() -> Option<PathBuf> {
    let dir = Settings::config_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join(paths::LOG_FILE_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn debug_settings(enable_log: bool, log_level_filter: LevelFilter) -> DebugSettings {
        DebugSettings {
            enable_log,
            log_level_filter,
        }
    }

    #[test]
    fn logging_switched_off_lets_every_call_site_skip_its_work() {
        assert_eq!(
            max_level(&debug_settings(false, LevelFilter::Trace)),
            LevelFilter::Off
        );
    }

    #[test]
    fn a_chatty_file_level_lifts_the_maximum() {
        assert_eq!(
            max_level(&debug_settings(true, LevelFilter::Trace)),
            LevelFilter::Trace
        );
    }

    #[test]
    fn a_file_quieter_than_the_console_still_lets_info_through() {
        // The file is asked for warnings only, but the console mirror is always
        // at Info — so Info must not be dropped before either of them sees it.
        assert_eq!(
            max_level(&debug_settings(true, LevelFilter::Warn)),
            LevelFilter::Info
        );
    }

    /// Switching logging off has to close the log file, not just go quiet: the
    /// point of the switch is that the file stops being held open.
    #[test]
    fn switching_off_drops_the_logger() {
        apply(&debug_settings(false, LevelFilter::Info));
        assert_eq!(log::max_level(), LevelFilter::Off);
        assert!(ROUTER.inner.read().unwrap().is_none());
    }
}
