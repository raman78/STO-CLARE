use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

use serde::{Deserialize, Serialize};

use crate::{
    analyzer::settings::{AnalysisSettings, RuleSets},
    app::{
        compare::CompareSettings,
        settings::{ColumnVisibility, CombatNotes},
    },
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
    /// Which columns the main window's tables show. Its own section for the
    /// same reason as the notes: hiding a column is no reason to re-read the
    /// log.
    #[serde(default)]
    pub columns: ColumnVisibility,
    /// Why the rules file could not be read at start-up, when it could not.
    ///
    /// Not part of the settings on disk — it describes this run, not a
    /// preference — but it travels with them because everything that has to act
    /// on it already holds a `Settings`: the dialog that says so, and `save`,
    /// which refuses to write over a file it could not read.
    #[serde(skip)]
    rules_file_problem: Option<String>,
    /// The same, for the settings file itself. See [`SettingsFileProblem`].
    #[serde(skip)]
    settings_file_problem: Option<SettingsFileProblem>,
}

/// Why the settings file on disk could not be read, and whether writing over it
/// is therefore refused.
///
/// Refusing matters more here than anywhere else: the settings in memory are
/// then the defaults, and saving them would finish what the broken file
/// started. The player gets the chance to fix or move the file instead.
#[derive(Debug, Clone, PartialEq)]
struct SettingsFileProblem {
    /// Said in the Settings window, with the path, so it can be acted on.
    text: String,
    /// True when the unreadable file is the one `save` writes to. The pre-1.6
    /// file next to the executable is only ever read, so a broken one is worth
    /// saying but is no reason to stop saving.
    blocks_writing: bool,
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
    /// A combat log to come back to, remembered so switching away from it and
    /// back is two clicks instead of a trip through the file dialog. Kept here
    /// rather than beside `combatlog_file` in the analysis settings on purpose:
    /// a difference there replaces the analyzer and re-reads the whole log, and
    /// noting down a path is no reason to re-read 300 MB.
    #[serde(default)]
    pub default_combatlog_file: Option<String>,
    /// Where the Ladder window was left, so it comes back there. Unset until it
    /// has been moved, and it then opens in the middle of the main window.
    #[serde(default)]
    pub ladder_window_position: Option<[f32; 2]>,
    /// Whose log this is: the account handle, `@` and all, whose figures the
    /// combats list shows. Unset means "work it out from the log", which is
    /// what it does on every start — this is only written when the reader says
    /// otherwise (a shared machine, a second account, a log that is not theirs).
    #[serde(default)]
    pub my_handle: Option<String>,
    /// The handle the log last said it belonged to, remembered so a log that
    /// cannot say — one saved fight, a run fetched from the ladder, an evening
    /// of nothing but duo runs — still shows the reader their own figures.
    /// Written by the program, not by the reader; [`Self::my_handle`] is the
    /// one they set.
    #[serde(default)]
    pub last_detected_handle: Option<String>,
    /// How wide the reader dragged the combats panel. Zero (or missing) opens
    /// it at its own default width.
    #[serde(default)]
    pub combats_panel_width: f32,
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
    /// Draw chart series in the theme's colour-blind set instead of its
    /// ordinary one. Off in a settings file written before this existed, which
    /// is what every reader who does not need it wants anyway.
    #[serde(default)]
    pub color_blind_series: bool,
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
        let (mut settings, problem) = Self::read_or_default();
        if let Some(problem) = &problem {
            // Written from here for the same reason as the rules file's: the
            // logger is configured from the settings, so nothing said while
            // reading them goes anywhere. Said by the first of the four
            // start-up callers that finds a logger listening, and only that one.
            static REPORTED: OnceLock<()> = OnceLock::new();
            if log::log_enabled!(log::Level::Error) && REPORTED.set(()).is_ok() {
                log::error!("the settings could not be read: {}", problem.text);
            }
        }
        settings.settings_file_problem = problem;
        settings.rules_file_problem = settings.load_rules();
        settings
    }

    /// The settings as they are on disk, or the defaults — and, when the file
    /// is there but cannot be read, why.
    ///
    /// A settings file that will not parse is **not** the same as no settings
    /// file. Reading it as "no settings" puts back every default, and the next
    /// Ok writes those defaults over it: the log path, the window size, the
    /// handle, the chosen columns and the theme are gone, with nothing said at
    /// any point. A fresh installation is the case with no file at all, and
    /// that one alone goes quietly to the defaults.
    ///
    /// The pre-1.6 file next to the executable is only ever read, never
    /// written, so a broken one is worth saying but is no reason to stop the
    /// program saving.
    fn read_or_default() -> (Self, Option<SettingsFileProblem>) {
        Self::file_path()
            .and_then(|p| Self::read_at(&p, true))
            .or_else(|| Self::legacy_file_path().and_then(|p| Self::read_at(&p, false)))
            // No settings anywhere: a fresh installation.
            .unwrap_or_else(|| (Self::default(), None))
    }

    /// One settings file, or `None` when there is none there to read.
    ///
    /// `blocks_writing` says whether this is the file `save` would write to, so
    /// that a broken one stops the write.
    fn read_at(path: &Path, blocks_writing: bool) -> Option<(Self, Option<SettingsFileProblem>)> {
        let data = std::fs::read_to_string(path).ok()?;
        Some(match serde_json::from_str(&data) {
            Ok(settings) => (settings, None),
            // Not logged here: the logger is configured from the settings and
            // is not up yet. The caller writes it, as with the rules file.
            Err(e) => (
                Self::default(),
                Some(SettingsFileProblem {
                    text: format!("{}\n\n{e}", path.display()),
                    blocks_writing,
                }),
            ),
        })
    }

    /// Why the settings file could not be read at start-up, when it could not.
    /// Held so the Settings window can say it while the settings on screen are
    /// the defaults rather than the player's own.
    pub fn settings_file_problem(&self) -> Option<&str> {
        self.settings_file_problem.as_ref().map(|p| p.text.as_str())
    }

    /// Bring the rules in from their own file, or leave in place the ones that
    /// came out of the settings.
    ///
    /// No rules file means one of two things and they are handled the same way:
    /// this installation predates the split, in which case the rules just read
    /// out of the settings are the ones to keep and to write out; or this is a
    /// fresh installation, in which case those are the shipped defaults and
    /// writing them out gives the player a file to edit. Either way the move
    /// happens on the first start rather than waiting for the first Ok, so a
    /// player who never opens Settings is not left with rules in the old place.
    ///
    /// A rules file that exists but cannot be read is **not** treated as an
    /// empty one — that would throw away every rule its owner wrote and look
    /// exactly like a fresh install. The rules from the settings stay in place,
    /// the file is left untouched, and the reason is returned for the UI to
    /// show and for the save path to refuse to overwrite it.
    /// Done **once per process.** Four separate parts of the app load the
    /// settings at start-up — the window geometry, the logger, the app state
    /// and the Settings dialog — and each of them would otherwise re-read the
    /// file, re-run the move, and, when the file is unreadable, log the same
    /// error four times over.
    fn load_rules(&mut self) -> Option<String> {
        static ONCE: OnceLock<Result<RuleSets, String>> = OnceLock::new();

        let path = RuleSets::path()?;
        let from_the_settings = self.analysis.rule_sets();
        let outcome = ONCE.get_or_init(move || {
            let mut analysis = AnalysisSettings::default();
            analysis.set_rule_sets(from_the_settings);
            match Self::load_rules_at(&mut analysis, &path) {
                Some(problem) => Err(problem),
                None => Ok(analysis.rule_sets()),
            }
        });

        match outcome {
            Ok(sets) => {
                self.analysis.set_rule_sets(sets.clone());
                None
            }
            Err(problem) => {
                // Logged from out here rather than from inside the once-only
                // block, because the logger is configured *from* the settings
                // and so is not up yet the first time they are loaded — an
                // error written in there goes nowhere. Written by the first
                // caller that finds a logger listening, and only that one.
                static REPORTED: OnceLock<()> = OnceLock::new();
                if log::log_enabled!(log::Level::Error) && REPORTED.set(()).is_ok() {
                    log::error!("{problem}");
                }
                Some(problem.clone())
            }
        }
    }

    /// The body of [`Settings::load_rules`], against a path a test can name.
    fn load_rules_at(analysis: &mut AnalysisSettings, path: &std::path::Path) -> Option<String> {
        if !path.exists() {
            if let Err(e) = analysis.rule_sets().write(path) {
                log::warn!("could not write {}: {e}", path.display());
            } else {
                log::info!("rules moved to {}", path.display());
            }
            return None;
        }
        match RuleSets::read(path) {
            Ok(sets) => {
                analysis.set_rule_sets(sets);
                None
            }
            // Not logged here: the caller writes it, because the logger is
            // configured from the settings and is not up yet the first time
            // they are loaded.
            Err(e) => Some(format!("{}\n\n{e}", path.display())),
        }
    }

    /// Whether the rules file was unreadable at start-up, and why.
    ///
    /// Held so the Settings window can say it, and so `save` will not write
    /// over a file it could not read: the rules in memory are the ones from
    /// before the split, and overwriting with those would finish the job the
    /// unreadable file started.
    pub fn rules_file_problem(&self) -> Option<&str> {
        self.rules_file_problem.as_deref()
    }

    fn save_rules(&self) {
        let Some(path) = RuleSets::path() else {
            return;
        };
        self.save_rules_at(&path);
    }

    /// Takes the path so that the refusal below can be tested against a file
    /// somewhere harmless. Asserting that the real config directory was left
    /// alone would pass either way — a guard that let the write through would
    /// put it there, not in the file the test is watching.
    fn save_rules_at(&self, path: &Path) {
        if self.rules_file_problem.is_some() {
            log::warn!("not writing the rules file: it could not be read at start-up");
            return;
        }
        if let Err(e) = self.analysis.rule_sets().write(path) {
            log::error!("could not write {}: {e}", path.display());
        }
    }

    pub fn save(&self) {
        self.save_rules();
        let Some(path) = Self::file_path() else {
            return;
        };
        self.save_at(&path);
    }

    /// Takes the path for the same reason as [`Self::save_rules_at`].
    fn save_at(&self, path: &Path) {
        // The settings in hand are the defaults, because the file could not be
        // read. Writing them would destroy what is still in it.
        if self
            .settings_file_problem
            .as_ref()
            .is_some_and(|p| p.blocks_writing)
        {
            log::warn!("not writing the settings file: it could not be read at start-up");
            return;
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let data = match serde_json::to_string_pretty(self) {
            Ok(d) => d,
            Err(_) => {
                return;
            }
        };

        let _ = std::fs::write(path, data);
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
            default_combatlog_file: None,
            ladder_window_position: None,
            my_handle: None,
            last_detected_handle: None,
            combats_panel_width: 0.0,
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
            color_blind_series: false,
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
    use crate::analyzer::settings::{RULES_FILE_VERSION, RulesFileError, RulesGroup};

    /// Settings written before there was a file to come back to must keep
    /// loading, with nothing remembered — not with an empty path, which would
    /// read as "come back to nowhere".
    #[test]
    fn settings_file_without_a_remembered_combatlog_still_loads() {
        let settings: Settings = serde_json::from_str(DEFAULT_SETTINGS).unwrap();
        assert_eq!(None, settings.general.default_combatlog_file);
    }

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
            settings
                .analysis
                .indirect_source_grouping_revers_rules
                .len()
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

    /// A hidden column stays hidden across a restart, and a settings file
    /// written before the picker existed still loads — the section defaults to
    /// "nothing hidden" rather than failing to parse.
    #[test]
    fn hidden_columns_survive_a_save_and_load() {
        let mut settings = Settings::default();
        settings
            .columns
            .set_shown(crate::app::settings::TableKind::Damage, "Flanking %", false);

        let json = serde_json::to_string(&settings).unwrap();
        let loaded: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(settings.columns, loaded.columns);
        assert!(
            !loaded
                .columns
                .is_shown(crate::app::settings::TableKind::Damage, "Flanking %")
        );

        // A file written before the picker existed has no such section at all.
        let mut older: serde_json::Value = serde_json::from_str(&json).unwrap();
        older.as_object_mut().unwrap().remove("columns");
        let older: Settings = serde_json::from_value(older).unwrap();
        assert_eq!(ColumnVisibility::default(), older.columns);
    }

    /// A scratch directory, and the rules-file path inside it.
    fn a_temp_rules_file(name: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(paths::RULES_FILE_NAME);
        (dir, path)
    }

    fn settings_with_a_rule(name: &str) -> Settings {
        let mut settings = Settings::default();
        settings.analysis.custom_group_rules = vec![RulesGroup {
            name: name.to_string(),
            ..Default::default()
        }];
        settings
    }

    /// An installation from before the split has its rules inside the settings.
    /// The first start writes them out to their own file, so a player who never
    /// opens Settings is not left with them in the old place.
    #[test]
    fn rules_are_moved_out_of_the_settings_on_the_first_start() {
        let (dir, path) = a_temp_rules_file("cla-rules-migrate");
        let mut settings = settings_with_a_rule("Quad Cannons");

        assert_eq!(None, Settings::load_rules_at(&mut settings.analysis, &path));
        assert!(path.exists(), "the rules file was not written");

        let written = RuleSets::read(&path).expect("and it reads back");
        assert_eq!(
            vec!["Quad Cannons"],
            written
                .custom_group_rules
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(RULES_FILE_VERSION, written.version);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The rules file, once there, is what the program uses — not the copy the
    /// settings may still be carrying from before the move.
    #[test]
    fn the_rules_file_wins_over_the_settings_copy() {
        let (dir, path) = a_temp_rules_file("cla-rules-file-wins");
        RuleSets {
            custom_group_rules: vec![RulesGroup {
                name: "From the file".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        }
        .write(&path)
        .unwrap();

        let mut settings = settings_with_a_rule("From the settings");
        assert_eq!(None, Settings::load_rules_at(&mut settings.analysis, &path));
        assert_eq!(
            vec!["From the file"],
            settings
                .analysis
                .custom_group_rules
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>()
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A rules file that cannot be read is never treated as an empty one: that
    /// would throw away every rule its owner wrote while looking exactly like a
    /// fresh installation. What was in hand stays, the file is left alone, and
    /// the reason is reported.
    #[test]
    fn an_unreadable_rules_file_does_not_wipe_the_rules() {
        let (dir, path) = a_temp_rules_file("cla-rules-broken");
        std::fs::write(&path, "this is not = a rules file").unwrap();
        let before = std::fs::read_to_string(&path).unwrap();

        let mut settings = settings_with_a_rule("Quad Cannons");
        let problem =
            Settings::load_rules_at(&mut settings.analysis, &path).expect("it says what is wrong");

        assert!(
            problem.contains("does not look like a rules file"),
            "{problem}"
        );
        assert_eq!(
            vec!["Quad Cannons"],
            settings
                .analysis
                .custom_group_rules
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>(),
            "the rules in hand must survive"
        );
        assert_eq!(
            before,
            std::fs::read_to_string(&path).unwrap(),
            "and the file is untouched"
        );

        // And saving must not finish the job the broken file started. Aimed at
        // the file itself: `save_rules` would write to the real config
        // directory, leaving this one untouched whether the guard held or not.
        settings.rules_file_problem = Some(problem);
        settings.save_rules_at(&path);
        assert_eq!(before, std::fs::read_to_string(&path).unwrap());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A file from a newer build is refused rather than half-read, so rules it
    /// holds in a shape this build does not know are not quietly dropped.
    #[test]
    fn a_rules_file_from_a_newer_build_is_refused() {
        let (dir, path) = a_temp_rules_file("cla-rules-newer");
        std::fs::write(&path, format!("version = {}\n", RULES_FILE_VERSION + 1)).unwrap();

        match RuleSets::read(&path) {
            Err(RulesFileError::FromANewerVersion { found, understood }) => {
                assert_eq!(RULES_FILE_VERSION + 1, found);
                assert_eq!(RULES_FILE_VERSION, understood);
            }
            other => panic!("expected a refusal, got {other:?}"),
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The settings file no longer carries the rules, so the old copy goes on
    /// the first save instead of lingering as a second source of the same
    /// truth — while a settings file that still has them is still read.
    #[test]
    fn the_settings_file_stops_carrying_the_rules_but_still_reads_them() {
        let settings = settings_with_a_rule("Quad Cannons");
        let written = serde_json::to_value(&settings).unwrap();
        assert!(
            written["analysis"].get("custom_group_rules").is_none(),
            "the rules must not be written into the settings any more"
        );

        let older = serde_json::json!({
            "analysis": {
                "combatlog_file": "",
                "combat_separation_time_seconds": 60.0,
                "indirect_source_grouping_revers_rules": [],
                "custom_group_rules": [{ "name": "From an old settings file", "rules": [], "enabled": true }],
                "combat_name_rules": []
            },
            "auto_refresh": written["auto_refresh"],
            "visuals": written["visuals"],
            "debug": written["debug"],
        });
        let older: Settings = serde_json::from_value(older).unwrap();
        assert_eq!(
            vec!["From an old settings file"],
            older
                .analysis
                .custom_group_rules
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>(),
            "an existing installation's rules still have to be found"
        );
    }

    /// A scratch directory with a settings file of the given contents in it.
    fn a_temp_settings_file(name: &str, contents: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(paths::SETTINGS_FILE_NAME);
        std::fs::write(&path, contents).unwrap();
        (dir, path)
    }

    /// A settings file that will not parse is not the same as no settings file.
    /// Read as "none", every default comes back and the next Ok writes those
    /// defaults over it — the log path, the handle, the window size and the
    /// theme gone, with nothing said at any point.
    #[test]
    fn an_unreadable_settings_file_is_not_read_as_a_fresh_install() {
        let (dir, path) = a_temp_settings_file("cla-settings-broken", "{ not json at all");
        let before = std::fs::read_to_string(&path).unwrap();

        let (settings, problem) = Settings::read_at(&path, true).expect("the file is there");
        let problem = problem.expect("it has to say what is wrong");

        assert!(problem.text.contains("cla-settings-broken"), "{problem:?}");
        assert!(
            problem.blocks_writing,
            "the file it could not read is the one it writes to"
        );
        assert_eq!(Settings::default().general, settings.general);

        // And Ok must not finish what the broken file started.
        let mut settings = settings;
        settings.settings_file_problem = Some(problem);
        settings.save_at(&path);
        assert_eq!(
            before,
            std::fs::read_to_string(&path).unwrap(),
            "the file has to be left as it was"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Once the file is out of the way, saving works again — the refusal is
    /// about this one file, not a latch that leaves the player unable to save.
    #[test]
    fn a_readable_settings_file_is_still_written() {
        let (dir, path) = a_temp_settings_file("cla-settings-ok", DEFAULT_SETTINGS);

        let (settings, problem) = Settings::read_at(&path, true).expect("the file is there");
        assert!(problem.is_none(), "a good file has nothing to report");
        settings.save_at(&path);

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("\"analysis\""), "{written}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// No settings file anywhere is a fresh installation, and that one goes
    /// quietly to the defaults.
    #[test]
    fn a_missing_settings_file_says_nothing() {
        let dir = std::env::temp_dir().join("cla-settings-absent");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(Settings::read_at(&dir.join(paths::SETTINGS_FILE_NAME), true).is_none());
    }

    /// The pre-1.6 file next to the executable is only ever read, so a broken
    /// one is worth saying but must not stop the program writing its own.
    #[test]
    fn a_broken_file_from_before_1_6_does_not_stop_saving() {
        let (dir, path) = a_temp_settings_file("cla-settings-legacy", "not json");

        let (_, problem) = Settings::read_at(&path, false).expect("the file is there");
        assert!(!problem.expect("still said").blocks_writing);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The sections of the settings file, as written today.
    ///
    /// A section the reader does not know is ignored, so a *renamed* one is a
    /// preference every existing installation loses without a word — read as
    /// "the player never set that". The names are pinned so that renaming or
    /// dropping one is a decision about the files already on disk rather than a
    /// side effect of tidying a struct. Adding a section is safe; add it here
    /// too.
    #[test]
    fn the_sections_of_the_settings_file_are_pinned() {
        let written = serde_json::to_value(Settings::default()).unwrap();
        let mut sections: Vec<_> = written
            .as_object()
            .expect("the settings are written as an object")
            .keys()
            .map(String::as_str)
            .collect();
        sections.sort_unstable();

        assert_eq!(
            vec![
                "analysis",
                "auto_refresh",
                "columns",
                "combat_notes",
                "compare",
                "debug",
                "general",
                "upload",
                "visuals",
                "window",
            ],
            sections,
            "the settings file changed shape — a renamed or dropped section is \
             a setting every existing installation silently loses"
        );
    }

    /// Which sections a settings file cannot be read without.
    ///
    /// A section without `#[serde(default)]` is required, and a file lacking it
    /// does not half-read — the whole file is refused, and the player starts at
    /// the defaults with their own file left alone. That is the right outcome,
    /// but it is a hard break, so adding a *new* required section would refuse
    /// every settings file already written. This pins which ones are required,
    /// so that stays a decision: a new section gets `#[serde(default)]`, and if
    /// it genuinely cannot, the old files need a way through first.
    #[test]
    fn only_the_sections_that_always_existed_are_required() {
        let full: serde_json::Value = serde_json::from_str(DEFAULT_SETTINGS).unwrap();
        let mut required: Vec<&str> = full
            .as_object()
            .unwrap()
            .keys()
            .filter(|key| {
                let mut without = full.clone();
                without.as_object_mut().unwrap().remove(*key);
                serde_json::from_value::<Settings>(without).is_err()
            })
            .map(String::as_str)
            .collect();
        required.sort_unstable();

        assert_eq!(
            vec!["analysis", "auto_refresh", "debug", "visuals"],
            required,
            "a newly required section refuses every settings file already written"
        );
    }

    /// The defaults shipped in the binary are a settings file the build can
    /// read. They are what a fresh installation gets and what every test here
    /// starts from, so a typo in them would surface as a panic at start-up.
    #[test]
    fn the_shipped_default_settings_still_read() {
        let settings: Settings =
            serde_json::from_str(DEFAULT_SETTINGS).expect("the shipped defaults must parse");
        assert!(settings.settings_file_problem.is_none());
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
