use std::sync::Arc;

use egui::*;
use rfd::FileDialog;

use crate::{
    analyzer::Combat,
    platform::AppWindow,
    upload::{Records, Upload},
};

use self::{
    analysis_handling::AnalysisInfo, main_tabs::*, settings::*, state::AppState, status::*,
    summary_copy::SummaryCopy,
};

mod analysis_handling;
pub mod desktop_install;
#[cfg(target_os = "linux")]
pub mod layer_overlay;
#[cfg(target_os = "linux")]
mod log_consolidation;
pub mod logging;
pub mod self_upgrade;
mod main_tabs;
mod overlay;
mod settings;
mod state;
mod status;
mod summary_copy;

pub struct App {
    settings_window: SettingsWindow,
    combats: Vec<String>,
    selected_combat_index: Option<usize>,
    selected_combat: Option<Arc<Combat>>,
    status_indicator: StatusIndicator,
    main_tabs: MainTabs,
    summary_copy: SummaryCopy,
    upload: Upload,
    records: Records,
    state: AppState,
    // Deferred persistence of the window size: written once resizing settles
    // (see track_window_geometry).
    window_geometry_dirty: bool,
    last_geometry_change: f64,
}

/// Window geometry to restore at startup: last size (points) and whether the
/// window was maximized. Read before the viewport is built (see main.rs).
pub fn saved_window_geometry() -> (Option<Vec2>, bool) {
    let settings = Settings::load_or_default();
    let size = settings.general.window_size.map(|[w, h]| vec2(w, h));
    (size, settings.general.window_maximized)
}

impl App {
    pub fn new(ctx: &egui::Context, pixels_per_point: f32) -> Self {
        ctx.memory_mut(|m| m.options.repaint_on_widget_change = false);
        let state = AppState::new(ctx);
        let settings_window = SettingsWindow::new(ctx, Some(pixels_per_point));
        Self {
            settings_window,
            combats: Default::default(),
            selected_combat_index: None,
            selected_combat: None,
            status_indicator: StatusIndicator::new(),
            main_tabs: MainTabs::empty(),
            summary_copy: Default::default(),
            upload: Default::default(),
            records: Default::default(),
            state,
            window_geometry_dirty: false,
            last_geometry_change: 0.0,
        }
    }
}

impl App {
    pub fn update(&mut self, ctx: &egui::Context, window: &AppWindow) {
        self.handle_analysis_infos();
        // Remember where the overlay was dragged (persisted on exit).
        #[cfg(target_os = "linux")]
        if let Some(position) = self.state.overlay.position() {
            self.state.settings.general.overlay_position = Some(position);
        }
        CentralPanel::default().show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    self.settings_window.show(
                        &mut self.state,
                        self.selected_combat.as_deref(),
                        &self.combats,
                        ui,
                        window,
                    );
                    self.records
                        .show(ui, window, &self.state.settings.upload.oscr_url);
                });

                ui.horizontal_wrapped(|ui| {
                    self.status_indicator
                        .show(self.state.analysis_handler.is_busy(), ui);

                    ComboBox::new("combat list", "Combats")
                        .width(400.0)
                        // Show around 15 combats before the list starts to
                        // scroll (the default only fits a few).
                        .height(360.0)
                        .selected_text(self.main_tabs.identifier.as_str())
                        .show_ui(ui, |ui| {
                            for (i, combat) in self.combats.iter().enumerate().rev() {
                                if ui
                                    .selectable_value(
                                        &mut self.selected_combat_index,
                                        Some(i),
                                        combat.as_str(),
                                    )
                                    .changed()
                                {
                                    if let Some(combat_index) = self.selected_combat_index {
                                        self.state.analysis_handler.get_combat(combat_index);
                                    }
                                }
                            }
                        });

                    if ui.button("Refresh Now ⟲").clicked() {
                        self.state.analysis_handler.refresh();
                    }

                    self.settings_window.show_clear_log_dialog(
                        &self.state.analysis_handler,
                        &self.combats,
                        ui,
                    );

                    if ui
                        .checkbox(
                            &mut self.state.settings.auto_refresh.enable,
                            "Auto Refresh when log changes",
                        )
                        .clicked()
                    {
                        self.state
                            .analysis_handler
                            .enable_auto_refresh(self.state.settings.auto_refresh.enable);
                        self.state.settings.save();
                    }

                    if ui
                        .add_enabled(
                            self.selected_combat.is_some(),
                            Button::new("Save Combat 💾"),
                        )
                        .clicked()
                    {
                        if let Some(file) = FileDialog::new()
                            .set_title("Save Combat")
                            .add_filter("log", &["log"])
                            .set_file_name(
                                &self.selected_combat.as_ref().unwrap().file_identifier(),
                            )
                            .set_parent(window)
                            .save_file()
                        {
                            self.state
                                .analysis_handler
                                .save_combat(self.selected_combat_index.unwrap(), file);
                        }
                    }

                    self.upload.show(
                        ui,
                        self.selected_combat.as_deref(),
                        &self.state.settings.analysis,
                        &self.state.settings.upload.oscr_url,
                    );

                    ui.separator();
                    self.summary_copy.show(self.selected_combat.as_deref(), ui);
                    ui.separator();
                    self.state.overlay.show(ui);
                });

                self.main_tabs.show(&self.state.settings, ui);
            });
        });
    }

    /// Backup flush of the latest settings/geometry on close.
    pub fn save_on_exit(&self) {
        self.state.settings.save();
    }

    /// Remembers the main window's size (in points) and maximized state so the
    /// next launch restores them (see platform::run). Called each frame with the
    /// SDL window size; the settings file is written only once the size has
    /// settled (no change for ~2 s), never while the edge is being dragged.
    pub fn track_window_geometry(&mut self, size: [f32; 2], maximized: bool, now: f64) {
        // Only remember the windowed size, so un-maximizing restores something
        // sane rather than the full-screen size.
        if !maximized && self.state.settings.general.window_size != Some(size) {
            self.state.settings.general.window_size = Some(size);
            self.window_geometry_dirty = true;
            self.last_geometry_change = now;
        }
        if self.state.settings.general.window_maximized != maximized {
            self.state.settings.general.window_maximized = maximized;
            self.window_geometry_dirty = true;
            self.last_geometry_change = now;
        }

        if self.window_geometry_dirty && now - self.last_geometry_change >= 2.0 {
            self.state.settings.save();
            self.window_geometry_dirty = false;
        }
    }

    fn handle_analysis_infos(&mut self) {
        let combatlog_file = &self.state.settings.analysis.combatlog_file;
        for info in self.state.analysis_handler.check_for_info() {
            match info {
                AnalysisInfo::Combat(combat) => {
                    self.main_tabs.update(&self.state.settings, &combat);
                    self.selected_combat = Some(combat);
                }
                AnalysisInfo::Refreshed {
                    latest_combat,
                    combats,
                    file_size,
                } => {
                    self.main_tabs.update(&self.state.settings, &latest_combat);
                    self.combats = combats;
                    self.selected_combat_index = Some(self.combats.len() - 1);
                    self.selected_combat = Some(latest_combat);
                    self.status_indicator.status = Status::Loaded {
                        combatlog_file: combatlog_file.clone(),
                        file_size,
                    };
                }
                AnalysisInfo::RefreshError => {
                    self.status_indicator.status = Status::LoadError {
                        combatlog_file: combatlog_file.clone(),
                    };
                }
            }
        }
    }
}
