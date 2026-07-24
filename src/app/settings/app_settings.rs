use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{analyzer::settings::AnalysisSettings, app::compare::CompareSettings};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub analysis: AnalysisSettings,
    #[serde(default)]
    pub general: General,
    pub auto_refresh: AutoRefresh,
    pub visuals: Visuals,
    pub debug: DebugSettings,
    #[serde(default)]
    pub upload: UploadSettings,
    #[serde(default)]
    pub compare: CompareSettings,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
pub struct General {
    pub more_decimals: bool,
    // Last main-window size (points, while not maximized) and maximized state,
    // restored on the next launch. See App::ui / App::on_exit and main.rs.
    #[serde(default)]
    pub window_size: Option<[f32; 2]>,
    #[serde(default)]
    pub window_maximized: bool,
    // Last size of the Settings dialog (points), restored on the next open.
    #[serde(default)]
    pub settings_window_size: Option<[f32; 2]>,
    // Last overlay position as the (top, left) layer-shell anchor margin
    // (Linux). Restored when the overlay is next shown. See app::overlay.
    #[serde(default)]
    pub overlay_position: Option<[i32; 2]>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AutoRefresh {
    pub enable: bool,
    pub interval_seconds: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Visuals {
    pub ui_scale: f64,
    pub theme: Theme,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Default)]
pub enum Theme {
    Dark,
    #[default]
    LightDark,
    Light,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct DebugSettings {
    pub enable_log: bool,
    pub log_level_filter: log::LevelFilter,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UploadSettings {
    pub oscr_url: String,
}

static DEFAULT_SETTINGS: &str = include_str!("STO_CombatLogAnalyzer_Settings.json");

const SETTINGS_FILE_NAME: &str = "STO_CombatLogAnalyzer_Settings.json";

impl Settings {
    /// Per-user config directory for this app: `~/.config/STO_CombatLogAnalyzer`
    /// on Linux, `%APPDATA%\STO_CombatLogAnalyzer` on Windows. Using the OS
    /// config dir means settings and logs survive when the program lives in a
    /// read-only location (e.g. /usr/bin, C:\Program Files, an AppImage mount).
    pub fn config_dir() -> Option<PathBuf> {
        let mut path = dirs::config_dir()?;
        path.push("STO_CombatLogAnalyzer");
        Some(path)
    }

    fn file_path() -> Option<PathBuf> {
        Some(Self::config_dir()?.join(SETTINGS_FILE_NAME))
    }

    /// Location used by older versions: next to the executable. Read as a
    /// fallback so existing settings are not lost on upgrade; never written to.
    fn legacy_file_path() -> Option<PathBuf> {
        let mut path = std::env::current_exe().ok()?;
        path.pop();
        path.push(SETTINGS_FILE_NAME);
        Some(path)
    }

    pub fn load_or_default() -> Self {
        Self::file_path()
            .and_then(|f| std::fs::read_to_string(&f).ok())
            .or_else(|| Self::legacy_file_path().and_then(|f| std::fs::read_to_string(&f).ok()))
            .and_then(|d| serde_json::from_str(&d).ok())
            .unwrap_or_else(|| Self::default())
    }

    pub fn save(&self) {
        let file_path = match Self::file_path() {
            Some(p) => p,
            None => {
                return;
            }
        };
        if let Some(dir) = file_path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let data = match serde_json::to_string_pretty(self) {
            Ok(d) => d,
            Err(_) => {
                return;
            }
        };

        let _ = std::fs::write(&file_path, data);
    }
}

impl Default for Settings {
    fn default() -> Self {
        serde_json::from_str(DEFAULT_SETTINGS).unwrap()
    }
}

impl Default for AutoRefresh {
    fn default() -> Self {
        Self {
            enable: false,
            interval_seconds: 1.0,
        }
    }
}

impl Theme {
    pub const fn display(&self) -> &'static str {
        match self {
            Theme::Dark => "Dark",
            Theme::LightDark => "Light Dark",
            Theme::Light => "Light",
        }
    }
}

impl Default for Visuals {
    fn default() -> Self {
        Self {
            ui_scale: 1.0,
            theme: Default::default(),
        }
    }
}

impl Default for DebugSettings {
    fn default() -> Self {
        Self {
            enable_log: false,
            log_level_filter: log::LevelFilter::Info,
        }
    }
}

impl Default for UploadSettings {
    fn default() -> Self {
        Settings::default().upload.clone()
    }
}
