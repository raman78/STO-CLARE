use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{
    analyzer::settings::AnalysisSettings,
    app::{compare::CompareSettings, settings::CombatNotes},
    helpers::paths,
};

// How each theme looks lives in `crate::app::theme`; the settings only store
// which one is picked, so a theme is added in one file.
pub use crate::app::theme::Theme;

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
    #[serde(default)]
    pub window: WindowGeometry,
    /// The user's own short note per combat. Its own section rather than part
    /// of `analysis`, so writing one does not count as an analysis change and
    /// re-read the whole log.
    #[serde(default)]
    pub combat_notes: CombatNotes,
}

/// Size and maximized state of the main window, remembered between runs.
///
/// Kept out of [`General`] on purpose: the settings dialog compares that
/// section to decide whether the log has to be analyzed again, and resizing a
/// window is no reason to redo the analysis.
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct WindowGeometry {
    /// Inner size in logical pixels, as last seen while not maximized.
    pub size: Option<[f32; 2]>,
    pub maximized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct General {
    #[serde(default)]
    pub more_decimals: bool,
    /// Show the Hull and Shield halves of a metric as their own columns instead
    /// of only in the hover tooltip. Defaults to on, including for settings
    /// files written before the option existed.
    #[serde(default = "default_true")]
    pub split_shield_hull_columns: bool,
    // Last size of the Settings dialog (points), restored on the next open.
    #[serde(default)]
    pub settings_window_size: Option<[f32; 2]>,
    // Last overlay position as the (top, left) layer-shell anchor margin
    // (Linux). Restored when the overlay is next shown. See app::overlay.
    #[serde(default)]
    pub overlay_position: Option<[i32; 2]>,
    // Whether the overlay was open when the app was last closed, so the next
    // launch comes back up the same way. Written only in `App::on_exit`: this
    // section is compared when the settings dialog is applied, and a difference
    // there triggers a re-analysis of the log, which toggling an overlay is no
    // reason for.
    #[serde(default)]
    pub overlay_shown: bool,
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
    /// How solid the overlay is, 0.2 to 1.0. Only the overlay is affected — the
    /// main window is a window like any other and stays opaque. Settings files
    /// written before this existed come up at the value the overlay has always
    /// had.
    #[serde(default = "default_overlay_opacity")]
    pub overlay_opacity: f64,
}

fn default_overlay_opacity() -> f64 {
    0.85
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

static DEFAULT_SETTINGS: &str = include_str!("STO-CLARE_Settings.json");

impl Settings {
    /// Per-user config directory — see [`crate::helpers::paths`], which owns
    /// every name the app writes there.
    pub fn config_dir() -> Option<PathBuf> {
        paths::config_dir()
    }

    fn file_path() -> Option<PathBuf> {
        Some(Self::config_dir()?.join(paths::SETTINGS_FILE_NAME))
    }

    /// Location used by older versions: next to the executable, under the name
    /// they wrote. Read as a fallback so existing settings are not lost on
    /// upgrade; never written to.
    fn legacy_file_path() -> Option<PathBuf> {
        let mut path = std::env::current_exe().ok()?;
        path.pop();
        path.push(paths::LEGACY_SETTINGS_FILE_NAME);
        Some(path)
    }

    pub fn load_or_default() -> Self {
        Self::file_path()
            .and_then(|f| std::fs::read_to_string(&f).ok())
            .or_else(|| Self::legacy_file_path().and_then(|f| std::fs::read_to_string(&f).ok()))
            .and_then(|d| serde_json::from_str(&d).ok())
            .unwrap_or_default()
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

fn default_true() -> bool {
    true
}

impl Default for General {
    fn default() -> Self {
        Self {
            more_decimals: false,
            split_shield_hull_columns: true,
            settings_window_size: None,
            overlay_position: None,
            overlay_shown: false,
        }
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

impl Default for Visuals {
    fn default() -> Self {
        Self {
            ui_scale: 1.0,
            theme: Default::default(),
            overlay_opacity: default_overlay_opacity(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_file_without_window_section_still_loads() {
        // Settings files written by older versions have no window section.
        let settings: Settings = serde_json::from_str(DEFAULT_SETTINGS).unwrap();
        assert_eq!(WindowGeometry::default(), settings.window);
    }

    /// Existing settings files have no `overlay_shown`; they must keep loading,
    /// with the overlay staying closed as before.
    /// A file written before combat notes existed — or by the stock program,
    /// which has no such section — must load with no notes rather than fail.
    #[test]
    fn settings_file_without_notes_still_loads() {
        let settings: Settings = serde_json::from_str(DEFAULT_SETTINGS).unwrap();
        assert_eq!(CombatNotes::default(), settings.combat_notes);
    }

    #[test]
    fn settings_file_without_overlay_shown_still_loads() {
        let json = r#"{"more_decimals": false, "overlay_position": [19, 1920]}"#;
        let general: General = serde_json::from_str(json).unwrap();
        assert!(!general.overlay_shown);
        assert_eq!(Some([19, 1920]), general.overlay_position);
    }

    #[test]
    fn overlay_shown_survives_a_save_and_load() {
        let mut settings = Settings::default();
        settings.general.overlay_shown = true;

        let json = serde_json::to_string(&settings).unwrap();
        let loaded: Settings = serde_json::from_str(&json).unwrap();

        assert!(loaded.general.overlay_shown);
    }

    /// A settings file written by the stock STO_CombatLogAnalyzer, whose file
    /// this program is documented as being able to take over. It has none of
    /// the sections added since the fork, and its own sections have to arrive
    /// intact — the rule lists above all, which are what a user spent time on.
    const UPSTREAM_SETTINGS: &str = r#"{
        "analysis": {
            "combatlog_file": "/games/Star Trek Online/Live/logs/GameClient/combatlog.log",
            "combat_separation_time_seconds": 45.0,
            "indirect_source_grouping_revers_rules": [
                {"aspect": "DamageOrHealName", "expression": "Spore-Infused Anomalies",
                 "method": "Equals", "enabled": false}
            ],
            "custom_group_rules": [
                {"name": "Dark Matter Quantum Torpedo Launcher",
                 "rules": [
                    {"aspect": "DamageOrHealName", "expression": "Dark Matter Laced Quantum Torpedo",
                     "method": "StartsWith", "enabled": true}
                 ],
                 "enabled": true}
            ],
            "combat_name_rules": [
                {"name_rule": {"name": "Infected Conduit",
                    "rules": [
                        {"aspect": "SourceOrTargetUniqueName",
                         "expression": "Space_Borg_Dreadnought_Raidisode_Sibrian_Final_Boss",
                         "method": "Equals", "enabled": true}
                    ],
                    "enabled": true},
                 "additional_info_rules": [
                    {"name": "Elite",
                     "rules": [
                        {"aspect": "SourceOrTargetUniqueName", "expression": "Elite_Initial",
                         "method": "EndsWith", "enabled": true}
                     ],
                     "enabled": true}
                 ]}
            ]
        },
        "auto_refresh": {"enable": false, "interval_seconds": 1.0},
        "visuals": {"ui_scale": 1.0, "theme": "LightDark"},
        "debug": {"enable_log": false, "log_level_filter": "INFO"},
        "upload": {"oscr_url": "https://oscr.stobuilds.com/"}
    }"#;

    #[test]
    fn a_stock_analyzer_settings_file_loads_with_its_rules_intact() {
        let settings: Settings = serde_json::from_str(UPSTREAM_SETTINGS)
            .expect("a stock STO_CombatLogAnalyzer settings file has to load");

        assert_eq!(
            "/games/Star Trek Online/Live/logs/GameClient/combatlog.log",
            settings.analysis.combatlog_file
        );
        assert_eq!(45.0, settings.analysis.combat_separation_time_seconds);
        assert_eq!(1, settings.analysis.custom_group_rules.len());
        assert_eq!(
            "Dark Matter Quantum Torpedo Launcher",
            settings.analysis.custom_group_rules[0].name
        );
        assert_eq!(1, settings.analysis.combat_name_rules.len());
        assert_eq!(
            "Infected Conduit",
            settings.analysis.combat_name_rules[0].name_rule.name
        );
        assert_eq!(
            1,
            settings.analysis.indirect_source_grouping_revers_rules.len()
        );
        assert_eq!(Theme::LightDark, settings.visuals.theme);
        assert_eq!("https://oscr.stobuilds.com/", settings.upload.oscr_url);
    }

    /// The sections that did not exist upstream have to come up at their
    /// defaults rather than stopping the file from loading at all.
    #[test]
    fn a_stock_settings_file_gets_defaults_for_what_it_does_not_have() {
        let settings: Settings = serde_json::from_str(UPSTREAM_SETTINGS).unwrap();

        assert_eq!(WindowGeometry::default(), settings.window);
        assert_eq!(CombatNotes::default(), settings.combat_notes);
        assert!(!settings.general.overlay_shown);
        assert_eq!(
            default_overlay_opacity(),
            settings.visuals.overlay_opacity,
            "a file with no opacity in it keeps the overlay as it always looked"
        );
        assert!(
            settings.analysis.consolidate_combatlog,
            "log merging defaults to on for a file that predates the option"
        );
    }

    #[test]
    fn window_geometry_survives_a_save_and_load() {
        let settings = Settings {
            window: WindowGeometry {
                size: Some([1024.0, 768.0]),
                maximized: true,
            },
            ..Default::default()
        };

        let json = serde_json::to_string(&settings).unwrap();
        let loaded: Settings = serde_json::from_str(&json).unwrap();

        assert_eq!(settings.window, loaded.window);
    }
}
