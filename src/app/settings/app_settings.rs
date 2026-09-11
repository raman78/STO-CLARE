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
    rules_file_problem: Option<ConfigFileProblem>,
    /// The same, for the settings file itself.
    #[serde(skip)]
    settings_file_problem: Option<ConfigFileProblem>,
    /// The settings file read at start-up still had the rules inside it — an
    /// installation from before they moved to their own file. Describes this
    /// run rather than a preference, so it is not written anywhere.
    #[serde(skip)]
    settings_carried_rules: bool,
}

/// Why a config file could not be read, and whether writing over it is
/// therefore refused.
///
/// Normally it is not: the file has been put aside under a name that says what
/// happened, so there is nothing left to write over and the program can go on
/// saving as a fresh installation would. The refusal is for the one case where
/// it could not even be moved — writing then would finish what the damage
/// started, and the player would have nothing left to repair.
#[derive(Debug, Clone, PartialEq)]
struct ConfigFileProblem {
    /// Said in the Settings window, with the path, so it can be acted on.
    text: String,
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

    /// The settings this run works from.
    ///
    /// **Done once per process.** Four separate parts of the app load the
    /// settings at start-up — the window geometry, the logger, the app state
    /// and the Settings dialog. Each would otherwise repeat everything below,
    /// and the parts of it that move files about can only be right the first
    /// time: a damaged file put aside by the first caller is simply *gone* to
    /// the second, which would then see a fresh installation and say nothing at
    /// all. The Settings dialog loads last, so that is precisely the caller
    /// whose warning would go missing.
    pub fn load_or_default() -> Self {
        static ONCE: OnceLock<Settings> = OnceLock::new();
        ONCE.get_or_init(|| {
            let (mut settings, problem) = Self::read_or_default();
            settings.settings_file_problem = problem;
            settings.rules_file_problem = settings.load_rules();
            settings.take_the_rules_out_of_the_settings();
            settings
        })
        .clone()
        .reported()
    }

    /// Writes whatever went wrong at start-up to the log, once.
    ///
    /// Said from out here rather than while reading, because the logger is
    /// configured *from* the settings and is not up yet at that point — an
    /// error written in there goes nowhere. Written by the first caller that
    /// finds a logger listening, and only that one.
    fn reported(self) -> Self {
        static REPORTED: OnceLock<()> = OnceLock::new();
        let problems = [
            self.settings_file_problem.as_ref().map(|p| p.text.as_str()),
            self.rules_file_problem.as_ref().map(|p| p.text.as_str()),
        ];
        if problems.iter().any(Option::is_some)
            && log::log_enabled!(log::Level::Error)
            && REPORTED.set(()).is_ok()
        {
            for problem in problems.into_iter().flatten() {
                log::error!("{problem}");
            }
        }
        self
    }

    /// The settings as they are on disk, or the defaults — and, when a file is
    /// there but cannot be read, why.
    ///
    /// A settings file that will not parse is **not** the same as no settings
    /// file. Read as "no settings", every default comes back and the next Ok
    /// writes those defaults over it: the log path, the window size, the handle,
    /// the chosen columns and the theme gone, with nothing said at any point.
    /// A fresh installation is the case with no file at all, and that one alone
    /// goes quietly to the defaults.
    ///
    /// So a damaged file is **put aside** rather than written over, and the
    /// program goes on as a fresh installation: its owner may well be able to
    /// read it, or lift values back out of it, and "unreadable" is this
    /// program's opinion rather than something they agreed to. If it cannot
    /// even be moved — a read-only directory, no permission — nothing is
    /// written at all, which is the one case where saving has to stop.
    ///
    /// The pre-1.6 file next to the executable is only ever read, never
    /// written. A damaged one is said and otherwise left exactly where it is.
    fn read_or_default() -> (Self, Option<ConfigFileProblem>) {
        if let Some(path) = Self::file_path()
            && let Some(read) = Self::read_at(&path)
        {
            return match read {
                Ok(settings) => (settings, None),
                Err(why) => (Self::default(), Some(Self::put_aside(&path, why))),
            };
        }
        if let Some(path) = Self::legacy_file_path()
            && let Some(read) = Self::read_at(&path)
        {
            return match read {
                Ok(settings) => (settings, None),
                Err(why) => (
                    Self::default(),
                    Some(ConfigFileProblem {
                        text: format!(
                            "The settings left by a version before 1.6 could not be read, so \
                             STO-CLARE has started with its defaults. That file is not one it \
                             writes to, and it has been left alone.\n\n{}\n\n{why}",
                            path.display()
                        ),
                        blocks_writing: false,
                    }),
                ),
            };
        }
        // No settings anywhere: a fresh installation.
        (Self::default(), None)
    }

    /// One settings file: `None` when there is none there, otherwise what it
    /// held or what the reader objected to.
    ///
    /// Notes on the way past whether the file still had the rules in it. That
    /// cannot be worked out later from the rules in hand, because on a fresh
    /// installation those are the shipped defaults, which came from the binary
    /// and not from anyone's file.
    fn read_at(path: &Path) -> Option<Result<Self, String>> {
        let data = std::fs::read_to_string(path).ok()?;
        Some(
            serde_json::from_str::<Self>(&data)
                .map(|mut settings| {
                    let analysis = &settings.analysis;
                    settings.settings_carried_rules = !(analysis.combat_name_rules.is_empty()
                        && analysis.indirect_source_grouping_revers_rules.is_empty()
                        && analysis.custom_group_rules.is_empty()
                        && analysis.damage_out_exclusion_rules.is_empty());
                    settings
                })
                .map_err(|e| e.to_string()),
        )
    }

    /// Moves a settings file that cannot be read out of the way, and says so in
    /// the words the Settings window shows.
    fn put_aside(path: &Path, why: String) -> ConfigFileProblem {
        match paths::set_aside(path, paths::DAMAGED) {
            Ok(moved) => ConfigFileProblem {
                text: format!(
                    "Your settings file could not be read, so STO-CLARE has started as if it \
                     were newly installed. Nothing has been deleted: the file is now\n\n{}\n\n\
                     so you can look at it or copy values back out of it.\n\n{why}",
                    moved.display()
                ),
                blocks_writing: false,
            },
            Err(e) => ConfigFileProblem {
                text: format!(
                    "Your settings file could not be read, and could not be moved aside \
                     either, so STO-CLARE is leaving it completely alone and will not save \
                     over it. Move or repair it, then start STO-CLARE again.\n\n{}\n\n\
                     {why}\n\nMoving it aside failed with: {e}",
                    path.display()
                ),
                blocks_writing: true,
            },
        }
    }

    /// Why the settings file could not be read at start-up, when it could not.
    /// Held so the Settings window can say it while the settings on screen are
    /// the defaults rather than the player's own.
    pub fn settings_file_problem(&self) -> Option<&str> {
        self.settings_file_problem.as_ref().map(|p| p.text.as_str())
    }

    /// Bring the rules in from their own file, writing one if there is none.
    fn load_rules(&mut self) -> Option<ConfigFileProblem> {
        let path = RuleSets::path()?;
        let (sets, problem) = Self::rules_from(&path, self.rules_the_settings_carried());
        self.analysis.set_rule_sets(sets);
        problem
    }

    /// The rules the settings file was still carrying, or `None` when it was
    /// not carrying any.
    ///
    /// Not the same question as "are there any rules in hand": on a fresh
    /// installation the rules in hand are the shipped defaults, which came from
    /// the binary and not from anybody's file. Only a file written before the
    /// rules moved out has rules in it, and only that case is a migration.
    fn rules_the_settings_carried(&self) -> Option<RuleSets> {
        self.settings_carried_rules
            .then(|| self.analysis.rule_sets())
    }

    /// The body of [`Settings::load_rules`], against a path a test can name.
    ///
    /// Three cases, and the third is the one worth spelling out:
    ///
    /// - **No file.** An installation from before the split, whose rules are in
    ///   `carried` and get written out; or a fresh one, which gets the shipped
    ///   defaults so that it has a file to edit and its fight names read
    ///   properly. Done on the first start rather than at the first Ok, so a
    ///   player who never opens Settings is not left with rules in the old
    ///   place.
    /// - **A file that reads.** Used, and it wins over anything the settings
    ///   were still carrying.
    /// - **A file that does not read.** Put aside under a name that says why,
    ///   and the program goes on with what a fresh installation starts from.
    ///   Never read as an empty file, and never written over: those rules are
    ///   the only copy of an evening's work, their owner can very likely still
    ///   read the file, and **Import…** will take it back whole once it is
    ///   repaired. If it cannot even be moved, nothing is written at all.
    fn rules_from(path: &Path, carried: Option<RuleSets>) -> (RuleSets, Option<ConfigFileProblem>) {
        let as_a_fresh_install = || Self::default().analysis.rule_sets();
        if !path.exists() {
            let sets = carried.unwrap_or_else(as_a_fresh_install);
            match sets.write(path) {
                Ok(()) => log::info!("rules written to {}", path.display()),
                Err(e) => log::warn!("could not write {}: {e}", path.display()),
            }
            return (sets, None);
        }
        let why = match RuleSets::read(path) {
            Ok(sets) => return (sets, None),
            Err(e) => e,
        };
        match paths::set_aside(path, paths::DAMAGED) {
            Ok(moved) => {
                let sets = as_a_fresh_install();
                if let Err(e) = sets.write(path) {
                    log::warn!("could not write {}: {e}", path.display());
                }
                (
                    sets,
                    Some(ConfigFileProblem {
                        text: format!(
                            "Your rules file could not be read, so the rules below are the ones \
                             a new installation starts with. Nothing has been deleted: your file \
                             is now\n\n{}\n\nand if it can be repaired, Import… will bring your \
                             rules back.\n\n{why}",
                            moved.display()
                        ),
                        blocks_writing: false,
                    }),
                )
            }
            Err(e) => (
                carried.unwrap_or_else(as_a_fresh_install),
                Some(ConfigFileProblem {
                    text: format!(
                        "Your rules file could not be read, and could not be moved aside \
                         either, so STO-CLARE is leaving it completely alone and will not save \
                         over it. The rules below are not the ones in that file. Move or repair \
                         it, then start STO-CLARE again.\n\n{}\n\n{why}\n\nMoving it aside \
                         failed with: {e}",
                        path.display()
                    ),
                    blocks_writing: true,
                }),
            ),
        }
    }

    /// Take the rules out of a settings file that still holds them, keeping the
    /// original beside it.
    ///
    /// The rules have their own file now, and one setting living in two places
    /// is a question about which of them is true — asked every start, answered
    /// differently depending on which was written last. So the settings file is
    /// rewritten without them.
    ///
    /// A copy is kept first, and the copy is what makes this safe to do without
    /// asking: the rules are not being taken away, they are being left in two
    /// places instead of one. If the copy cannot be made, the settings file is
    /// left exactly as it is and the rules in it are simply ignored — an
    /// unasked-for rewrite of somebody's config is not worth tidiness.
    fn take_the_rules_out_of_the_settings(&self) {
        let Some(path) = Self::file_path() else {
            return;
        };
        self.take_the_rules_out_of_the_settings_at(&path);
    }

    /// Takes the path for the same reason as [`Self::save_at`].
    fn take_the_rules_out_of_the_settings_at(&self, path: &Path) {
        if !self.settings_carried_rules || !path.is_file() {
            return;
        }
        match paths::keep_a_copy(path, paths::ARCHIVED) {
            Ok(copy) => {
                log::info!(
                    "the rules have their own file now; {} kept as it was at {}",
                    path.display(),
                    copy.display()
                );
                self.save_at(path);
            }
            Err(e) => log::warn!(
                "leaving the rules in {} alone: it could not be copied aside first ({e})",
                path.display()
            ),
        }
    }

    /// Whether the rules file could not be read at start-up, and why. Held so
    /// the Settings window can say it while the rules on screen are not the
    /// ones that were in the file.
    pub fn rules_file_problem(&self) -> Option<&str> {
        self.rules_file_problem.as_ref().map(|p| p.text.as_str())
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
        if self
            .rules_file_problem
            .as_ref()
            .is_some_and(|p| p.blocks_writing)
        {
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

    /// The rules a settings file was carrying, for the migration tests.
    fn carried(name: &str) -> Option<RuleSets> {
        Some(RuleSets {
            custom_group_rules: vec![RulesGroup {
                name: name.to_string(),
                ..Default::default()
            }],
            ..Default::default()
        })
    }

    fn names(sets: &RuleSets) -> Vec<&str> {
        sets.custom_group_rules
            .iter()
            .map(|r| r.name.as_str())
            .collect()
    }

    /// An installation from before the split has its rules inside the settings.
    /// The first start writes them out to their own file, so a player who never
    /// opens Settings is not left with them in the old place.
    #[test]
    fn rules_are_moved_out_of_the_settings_on_the_first_start() {
        let (dir, path) = a_temp_rules_file("cla-rules-migrate");
        let _ = std::fs::remove_file(&path);

        let (sets, problem) = Settings::rules_from(&path, carried("Quad Cannons"));

        assert!(problem.is_none());
        assert_eq!(vec!["Quad Cannons"], names(&sets));
        assert!(path.exists(), "the rules file was not written");

        let written = RuleSets::read(&path).expect("and it reads back");
        assert_eq!(vec!["Quad Cannons"], names(&written));
        assert_eq!(RULES_FILE_VERSION, written.version);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A fresh installation has no rules of anybody's to move, and gets the set
    /// the program ships with — not an empty file. Those include the rules that
    /// give fights their names, so an empty start would leave every fight
    /// unnamed.
    #[test]
    fn a_fresh_installation_starts_from_the_shipped_rules() {
        let (dir, path) = a_temp_rules_file("cla-rules-fresh");
        let _ = std::fs::remove_file(&path);

        let (sets, problem) = Settings::rules_from(&path, None);

        assert!(problem.is_none());
        assert!(
            !sets.combat_name_rules.is_empty(),
            "a new installation has to be able to name a fight"
        );
        assert_eq!(sets, RuleSets::read(&path).expect("and it was written out"));

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

        let (sets, problem) = Settings::rules_from(&path, carried("From the settings"));

        assert!(problem.is_none());
        assert_eq!(vec!["From the file"], names(&sets));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A rules file that cannot be read is never read as an empty one, and
    /// never written over. It is moved aside under a name that says what
    /// happened, the program goes on with what a new installation starts from,
    /// and the message points at where the file went — because Import… takes it
    /// back whole if its owner can repair it.
    #[test]
    fn an_unreadable_rules_file_is_put_aside_rather_than_lost() {
        let (dir, path) = a_temp_rules_file("cla-rules-broken");
        std::fs::write(&path, "this is not = a rules file").unwrap();

        let (sets, problem) = Settings::rules_from(&path, carried("Quad Cannons"));
        let problem = problem.expect("it says what happened");

        let put_aside = dir.join("STO-CLARE_Rules_damaged.toml");
        assert_eq!(
            "this is not = a rules file",
            std::fs::read_to_string(&put_aside).unwrap(),
            "the file itself has to survive, byte for byte"
        );
        assert!(
            problem.text.contains("STO-CLARE_Rules_damaged.toml"),
            "the reader has to be told where it went: {}",
            problem.text
        );
        assert!(
            problem.text.contains("Import"),
            "and how to get the rules back: {}",
            problem.text
        );
        assert!(
            !problem.blocks_writing,
            "with the file out of the way there is nothing left to write over"
        );
        assert!(
            !sets.combat_name_rules.is_empty(),
            "and the program carries on as a new installation would"
        );
        assert_eq!(
            sets,
            RuleSets::read(&path).expect("the rules file has to be usable again"),
            "a start that leaves the damaged file in place would hit the same \
             wall on every later start"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// If the damaged file cannot even be moved — a read-only directory, no
    /// permission — then nothing is written at all. Writing would finish what
    /// the damage started and leave its owner with nothing to repair.
    #[test]
    fn a_rules_file_that_cannot_be_moved_aside_is_not_written_over() {
        let (dir, path) = a_temp_rules_file("cla-rules-stuck");
        std::fs::write(&path, "this is not = a rules file").unwrap();
        let before = std::fs::read_to_string(&path).unwrap();
        // Every name it could be moved to is taken, so the move has to fail.
        for attempt in 1..100 {
            let counted = match attempt {
                1 => String::new(),
                n => format!("_{n}"),
            };
            std::fs::write(dir.join(format!("STO-CLARE_Rules_damaged{counted}.toml")), "").unwrap();
        }

        let (sets, problem) = Settings::rules_from(&path, carried("Quad Cannons"));
        let problem = problem.expect("it says what happened");

        assert!(problem.blocks_writing, "saving has to stop");
        assert_eq!(
            before,
            std::fs::read_to_string(&path).unwrap(),
            "and the file is left exactly as it was"
        );
        assert_eq!(
            vec!["Quad Cannons"],
            names(&sets),
            "what was in hand stays in hand"
        );

        // And Ok must not finish the job. Aimed at the file itself:
        // `save_rules` would write to the real config directory, leaving this
        // one untouched whether the guard held or not.
        let mut settings = settings_with_a_rule("Quad Cannons");
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
    fn an_unreadable_settings_file_is_put_aside_rather_than_lost() {
        let (dir, path) = a_temp_settings_file("cla-settings-broken", "{ not json at all");

        let why = Settings::read_at(&path)
            .expect("the file is there")
            .expect_err("and it does not parse");
        let problem = Settings::put_aside(&path, why);

        let put_aside = dir.join("STO-CLARE_Settings_damaged.json");
        assert_eq!(
            "{ not json at all",
            std::fs::read_to_string(&put_aside).unwrap(),
            "the file itself has to survive, byte for byte"
        );
        assert!(
            problem.text.contains("STO-CLARE_Settings_damaged.json"),
            "the reader has to be told where it went: {}",
            problem.text
        );
        assert!(
            !problem.blocks_writing,
            "with the file out of the way there is nothing left to write over"
        );
        assert!(!path.exists(), "and it is out of the way");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// If the damaged file cannot even be moved, nothing is written at all:
    /// writing would finish what the damage started.
    #[test]
    fn a_settings_file_that_cannot_be_moved_aside_is_not_written_over() {
        let (dir, path) = a_temp_settings_file("cla-settings-stuck", "{ not json at all");
        let before = std::fs::read_to_string(&path).unwrap();
        // Every name it could be moved to is taken, so the move has to fail.
        for attempt in 1..100 {
            let counted = match attempt {
                1 => String::new(),
                n => format!("_{n}"),
            };
            std::fs::write(
                dir.join(format!("STO-CLARE_Settings_damaged{counted}.json")),
                "",
            )
            .unwrap();
        }

        let problem = Settings::put_aside(&path, "broken".to_string());
        assert!(problem.blocks_writing, "saving has to stop");

        let mut settings = Settings::default();
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
    /// about the one file that could not be moved, not a latch that leaves the
    /// player unable to save at all.
    #[test]
    fn a_readable_settings_file_is_still_written() {
        let (dir, path) = a_temp_settings_file("cla-settings-ok", DEFAULT_SETTINGS);

        let settings = Settings::read_at(&path)
            .expect("the file is there")
            .expect("and it parses");
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
        assert!(Settings::read_at(&dir.join(paths::SETTINGS_FILE_NAME)).is_none());
    }

    /// A settings file from before the rules moved out, with one rule in it.
    fn settings_file_carrying_a_rule(name: &str) -> String {
        let mut file: serde_json::Value = serde_json::from_str(DEFAULT_SETTINGS).unwrap();
        file["analysis"]["custom_group_rules"] = serde_json::json!([
            { "name": name, "rules": [], "enabled": true }
        ]);
        serde_json::to_string_pretty(&file).unwrap()
    }

    /// Reading has to note whether the rules came out of somebody's file, which
    /// is not the same question as whether there are any rules in hand: a fresh
    /// installation has the shipped ones, and those came from the binary.
    #[test]
    fn a_settings_file_says_whether_it_still_carries_the_rules() {
        let (dir, path) = a_temp_settings_file("cla-carried-yes", &settings_file_carrying_a_rule("Quad Cannons"));
        let carrying = Settings::read_at(&path).unwrap().unwrap();
        assert!(carrying.settings_carried_rules);
        let _ = std::fs::remove_dir_all(&dir);

        let (dir, path) = a_temp_settings_file("cla-carried-no", DEFAULT_SETTINGS_WITHOUT_RULES);
        let not_carrying = Settings::read_at(&path).unwrap().unwrap();
        assert!(!not_carrying.settings_carried_rules);
        let _ = std::fs::remove_dir_all(&dir);

        assert!(
            !Settings::default().settings_carried_rules,
            "the shipped defaults are not anybody's file"
        );
    }

    /// A settings file with no rules in it at all, which is what every file
    /// written since the split looks like.
    const DEFAULT_SETTINGS_WITHOUT_RULES: &str = r#"{
        "analysis": { "combatlog_file": "", "combat_separation_time_seconds": 60.0 },
        "auto_refresh": { "enable": false, "interval_seconds": 1.0 },
        "visuals": { "ui_scale": 1.0, "theme": "Dark" },
        "debug": { "enable_log": false, "log_level_filter": "Info" }
    }"#;

    /// An installation from before the split, on its first start with a build
    /// that keeps the rules apart. The rules go to their own file, the settings
    /// are kept exactly as they were beside it, and the copy the settings held
    /// is taken out so that one setting does not live in two places.
    #[test]
    fn rules_carried_in_the_settings_are_moved_out_and_the_original_kept() {
        let (dir, path) =
            a_temp_settings_file("cla-migrate-out", &settings_file_carrying_a_rule("Quad Cannons"));
        let before = std::fs::read_to_string(&path).unwrap();
        let rules_path = dir.join(paths::RULES_FILE_NAME);

        let mut settings = Settings::read_at(&path).unwrap().unwrap();
        let (sets, problem) = Settings::rules_from(&rules_path, settings.rules_the_settings_carried());
        settings.analysis.set_rule_sets(sets);
        assert!(problem.is_none());
        settings.take_the_rules_out_of_the_settings_at(&path);

        assert_eq!(
            vec!["Quad Cannons"],
            names(&RuleSets::read(&rules_path).expect("the rules have their own file now"))
        );
        assert_eq!(
            before,
            std::fs::read_to_string(dir.join("STO-CLARE_Settings_archived.json")).unwrap(),
            "the settings as they were have to be kept, untouched"
        );
        let rewritten: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(
            rewritten["analysis"].get("custom_group_rules").is_none(),
            "and taken out of the settings, or they are a second source of the same truth"
        );
        assert_eq!(
            60.0, rewritten["analysis"]["combat_separation_time_seconds"],
            "while everything else in the settings stays"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The worse case: a rules file is already there *and* the settings still
    /// hold a copy — a build that split them ran, then an older one wrote the
    /// settings back. The rules file wins, and the settings are still cleaned
    /// out, but only after their original is kept aside: the copy inside them
    /// may hold something the rules file does not.
    #[test]
    fn a_settings_copy_of_the_rules_is_archived_even_when_the_rules_file_wins() {
        let (dir, path) = a_temp_settings_file(
            "cla-migrate-both",
            &settings_file_carrying_a_rule("From the settings"),
        );
        let before = std::fs::read_to_string(&path).unwrap();
        let rules_path = dir.join(paths::RULES_FILE_NAME);
        RuleSets {
            custom_group_rules: vec![RulesGroup {
                name: "From the file".to_string(),
                ..Default::default()
            }],
            ..Default::default()
        }
        .write(&rules_path)
        .unwrap();

        let mut settings = Settings::read_at(&path).unwrap().unwrap();
        let (sets, _) = Settings::rules_from(&rules_path, settings.rules_the_settings_carried());
        settings.analysis.set_rule_sets(sets);
        settings.take_the_rules_out_of_the_settings_at(&path);

        assert_eq!(
            vec!["From the file"],
            names(&RuleSets::read(&rules_path).unwrap()),
            "the rules file is the one that counts"
        );
        assert!(
            before.contains("From the settings"),
            "and the settings copy is not thrown away"
        );
        assert_eq!(
            before,
            std::fs::read_to_string(dir.join("STO-CLARE_Settings_archived.json")).unwrap()
        );
        let rewritten: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(rewritten["analysis"].get("custom_group_rules").is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A settings file that never carried any rules is not rewritten, and no
    /// copy of it is left lying about. Every start would otherwise leave one.
    #[test]
    fn a_settings_file_without_rules_is_left_exactly_as_it_is() {
        let (dir, path) =
            a_temp_settings_file("cla-migrate-none", DEFAULT_SETTINGS_WITHOUT_RULES);
        let before = std::fs::read_to_string(&path).unwrap();

        let settings = Settings::read_at(&path).unwrap().unwrap();
        settings.take_the_rules_out_of_the_settings_at(&path);

        assert_eq!(before, std::fs::read_to_string(&path).unwrap());
        assert!(
            !dir.join("STO-CLARE_Settings_archived.json").exists(),
            "nothing was taken out, so there is nothing to keep a copy of"
        );

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
