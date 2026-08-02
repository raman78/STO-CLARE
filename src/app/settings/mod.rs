use std::ffi::OsStr;

pub use app_settings::{Settings, WindowGeometry};
pub use combat_notes::{CombatNotes, MAX_NOTE_CHARS};
use eframe::{Frame, egui::*};

use crate::analyzer::Combat;

use self::{
    analysis::AnalysisTab, debug::DebugTab, general::GeneralTab, upload::UploadTab,
    visuals::VisualsTab,
};

use super::{analysis_handling::AnalysisHandler, state::AppState};

mod analysis;
mod app_settings;
mod combat_notes;
mod debug;
mod general;
mod upload;
mod visuals;

pub struct SettingsWindow {
    is_open: bool,
    modified_settings: Settings,
    selected_tab: SettingsTab,
    general_tab: GeneralTab,
    analysis_tab: AnalysisTab,
    visuals_tab: VisualsTab,
    upload_tab: UploadTab,
    debug_tab: DebugTab,
    /// Title bar + frame margins, measured from the previous frame. The
    /// remembered size describes the window's *content*, so this is what has to
    /// be added to it to know how much room the whole window takes.
    window_chrome: Vec2,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    #[default]
    General,
    Analysis,
    Visuals,
    Debug,
    Upload,
}

impl SettingsWindow {
    pub fn new(ctx: &Context, native_pixels_per_point: Option<f32>) -> Self {
        let mut visuals_tab = VisualsTab::default();
        let settings = Settings::load_or_default();
        visuals_tab.update_visuals(ctx, native_pixels_per_point, &settings);
        Self {
            is_open: false,
            modified_settings: settings.clone(),
            selected_tab: Default::default(),
            general_tab: Default::default(),
            analysis_tab: Default::default(),
            debug_tab: Default::default(),
            upload_tab: Default::default(),
            visuals_tab,
            window_chrome: Vec2::ZERO,
        }
    }

    pub fn show(
        &mut self,
        state: &mut AppState,
        selected_combat: Option<&Combat>,
        combats: &[String],
        ui: &mut Ui,
        frame: &Frame,
    ) {
        if ui.selectable_label(self.is_open, "Settings").clicked() && !self.is_open {
            self.initialize(state);
        }

        self.handle_dropped_file(ui, state);
        if !self.is_open {
            return;
        }
        // Restore the last size. The window is freely resizable, but capped to
        // the viewport so that expanding a collapsed section (which grows the
        // content) cannot push the window off-screen.
        let default_size = state
            .settings
            .general
            .settings_window_size
            .unwrap_or([760.0, 560.0]);
        // `Window::max_size` caps the *content*, while the title bar and frame
        // sit outside it — so the chrome has to come off here too, or the whole
        // window ends up taller than the viewport.
        let max_size = (ui.ctx().content_rect().size() - vec2(16.0, 16.0) - self.window_chrome)
            .at_least(vec2(200.0, 150.0));
        let window_response = Window::new("Settings")
            .collapsible(false)
            .resizable(true)
            .default_size(default_size)
            .min_size([420.0, 300.0])
            .max_size(max_size)
            .constrain(true)
            .show(ui.ctx(), |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.selected_tab, SettingsTab::General, "General");
                    ui.selectable_value(&mut self.selected_tab, SettingsTab::Analysis, "Analysis");
                    ui.selectable_value(&mut self.selected_tab, SettingsTab::Visuals, "Visuals");
                    ui.selectable_value(&mut self.selected_tab, SettingsTab::Upload, "Upload");
                    ui.selectable_value(&mut self.selected_tab, SettingsTab::Debug, "Debug");
                });

                ui.separator();
                // Leave room for the separator and the Ok/Cancel row below, and
                // stop the area auto-sizing to its contents. Without both, the
                // scroll area sizes itself to everything inside it: the buttons
                // are pushed past the bottom edge and the window springs back to
                // full content height whenever it is dragged smaller.
                let bottom_bar = ui.spacing().interact_size.y + ui.spacing().item_spacing.y * 4.0;
                ScrollArea::both()
                    .auto_shrink([false, false])
                    .max_height((ui.available_height() - bottom_bar).at_least(80.0))
                    .show(ui, |ui| match self.selected_tab {
                        SettingsTab::General => self.general_tab.show(
                            &state.analysis_handler,
                            &mut self.modified_settings,
                            combats,
                            ui,
                            frame,
                        ),
                        SettingsTab::Analysis => {
                            self.analysis_tab
                                .show(&mut self.modified_settings, selected_combat, ui)
                        }
                        SettingsTab::Visuals => {
                            self.visuals_tab.show(&mut self.modified_settings, ui)
                        }
                        SettingsTab::Upload => {
                            self.upload_tab.show(&mut self.modified_settings, ui)
                        }
                        SettingsTab::Debug => self.debug_tab.show(&mut self.modified_settings, ui),
                    });

                ui.separator();

                ui.horizontal(|ui| {
                    if ui.button("Ok").clicked() {
                        self.apply_setting_changes(state);
                    }

                    if ui.button("Cancel").clicked() {
                        self.discard_setting_changes(ui, state);
                    }
                });

                // The content size, which is what `default_size` expects back.
                ui.min_rect().size()
            });

        // Remember the current size (persisted with the app settings on apply and
        // on exit). Written to both the working copy and the live settings so it
        // survives regardless of whether the dialog is closed with Ok or Cancel.
        if let Some(window_response) = window_response {
            let Some(content_size) = window_response.inner else {
                return;
            };
            // Measure the chrome for the next frame's cap.
            self.window_chrome = window_response.response.rect.size() - content_size;
            // Store the *content* size. Storing the outer size instead fed the
            // title bar back in as content on the next launch, so the window
            // grew by its own title bar every time the app was restarted.
            let size = [content_size.x, content_size.y];
            if state.settings.general.settings_window_size != Some(size) {
                self.modified_settings.general.settings_window_size = Some(size);
                state.settings.general.settings_window_size = Some(size);
            }
        }
    }

    pub fn show_clear_log_dialog(
        &mut self,
        analysis_handler: &AnalysisHandler,
        combats: &[String],
        ui: &mut Ui,
    ) {
        self.general_tab
            .show_clear_log_dialog(analysis_handler, combats, ui);
    }

    fn handle_dropped_file(&mut self, ui: &mut Ui, state: &mut AppState) {
        ui.ctx().input(|i| {
            let file = i.raw.dropped_files.last().and_then(|f| f.path.as_ref());
            if let Some(file) = file {
                if file.extension() != Some(OsStr::new("log")) {
                    return;
                }
                if !self.is_open {
                    self.initialize(state);
                }
                self.modified_settings.analysis.combatlog_file = file.to_string_lossy().into();
                self.apply_setting_changes(state);
            }
        });
    }

    fn initialize(&mut self, state: &AppState) {
        self.is_open = true;
        self.modified_settings = state.settings.clone();
        self.general_tab.initialize();
    }

    fn apply_setting_changes(&mut self, state: &mut AppState) {
        self.is_open = false;
        // The two halves cost wildly different amounts, so they are gated
        // separately:
        //
        // - `set_settings` replaces the `Analyzer`, which re-parses the whole
        //   combat log from scratch (seconds on a large one). Only the analysis
        //   settings can invalidate it — the analyzer is never given `general`.
        // - `refresh` reuses the existing analyzer and only reads what the log
        //   has grown by, then re-sends the result so the views rebuild.
        //
        // A `general` change still needs the second one, because the tables bake
        // `more_decimals` into their formatted strings when they are built
        // (`ShieldAndHullTextValue::new`) rather than at draw time. It must not
        // trigger the first: moving the overlay writes `overlay_position` into
        // `general` every frame, so applying any setting after nudging the
        // overlay used to re-parse the entire log for nothing.
        let analysis_changed = self.modified_settings.analysis != state.settings.analysis;
        let general_changed = self.modified_settings.general != state.settings.general;

        if analysis_changed || general_changed {
            state.overlay.settings_changed(&self.modified_settings);
        }
        if analysis_changed {
            state
                .analysis_handler
                .set_settings(self.modified_settings.analysis.clone());
        }
        if analysis_changed || general_changed {
            state.analysis_handler.refresh();
        }

        if self.modified_settings.auto_refresh != state.settings.auto_refresh {
            state
                .analysis_handler
                .set_auto_refresh_interval(self.modified_settings.auto_refresh.interval_seconds);
            state
                .analysis_handler
                .enable_auto_refresh(self.modified_settings.auto_refresh.enable);
        }

        state.settings = self.modified_settings.clone();
        self.modified_settings.save();
    }

    fn discard_setting_changes(&mut self, ui: &Ui, state: &AppState) {
        self.is_open = false;
        if self.modified_settings.visuals != state.settings.visuals {
            self.visuals_tab.update_visuals(
                ui.ctx(),
                ui.ctx().native_pixels_per_point(),
                &state.settings,
            );
        }

        self.modified_settings = state.settings.clone();
    }
}
