use std::sync::Arc;

use eframe::egui::*;
use rfd::FileDialog;

use crate::{
    analyzer::Combat,
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
    pub fn new(cc: &eframe::CreationContext) -> Self {
        cc.egui_ctx
            .memory_mut(|m| m.options.repaint_on_widget_change = false);
        let state = AppState::new(&cc.egui_ctx);
        let settings_window =
            SettingsWindow::new(&cc.egui_ctx, cc.egui_ctx.native_pixels_per_point());
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

impl eframe::App for App {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, frame: &mut eframe::Frame) {
        self.handle_analysis_infos();
        self.track_window_geometry(ui.ctx());
        // Remember where the overlay was dragged (persisted on exit).
        #[cfg(target_os = "linux")]
        if let Some(position) = self.state.overlay.position() {
            self.state.settings.general.overlay_position = Some(position);
        }
        CentralPanel::default().show_inside(ui, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    self.settings_window.show(
                        &mut self.state,
                        self.selected_combat.as_deref(),
                        &self.combats,
                        ui,
                        frame,
                    );
                    self.records
                        .show(ui, frame, &self.state.settings.upload.oscr_url);
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
                            .set_parent(frame)
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

    fn clear_color(&self, _visuals: &eframe::egui::Visuals) -> [f32; 4] {
        _visuals.window_fill().to_normalized_gamma_f32()
    }

    fn on_exit(&mut self) {
        // Backup flush of the latest geometry on close (see track_window_geometry).
        self.state.settings.save();
    }
}

impl App {
    /// Remembers the main window's size and maximized state so the next launch
    /// restores them (see main.rs). The size comes from the egui viewport rect
    /// because on Wayland the OS-reported `inner_rect` is `None`. The settings
    /// file is written only once the size has settled (no change for a moment),
    /// never while the edge is being dragged, so resizing stays smooth.
    fn track_window_geometry(&mut self, ctx: &eframe::egui::Context) {
        let now = ctx.input(|i| i.time);
        let maximized = ctx.input(|i| i.viewport().maximized);
        let size = ctx.viewport_rect().size();

        // Only remember the windowed size, so un-maximizing restores something
        // sane rather than the full-screen size.
        if maximized != Some(true) {
            let size = [size.x, size.y];
            if self.state.settings.general.window_size != Some(size) {
                self.state.settings.general.window_size = Some(size);
                self.window_geometry_dirty = true;
                self.last_geometry_change = now;
            }
        }
        if let Some(maximized) = maximized {
            if self.state.settings.general.window_maximized != maximized {
                self.state.settings.general.window_maximized = maximized;
                self.window_geometry_dirty = true;
                self.last_geometry_change = now;
            }
        }

        if self.window_geometry_dirty {
            let idle = now - self.last_geometry_change;
            if idle >= 2.0 {
                // Settled for 2 s: write once, off the resize hot path.
                self.state.settings.save();
                self.window_geometry_dirty = false;
            } else if idle < 0.5 {
                // Actively resizing: keep redrawing every frame so the content
                // tracks the window instead of lagging behind the drag.
                ctx.request_repaint();
            } else {
                // Idle but not yet settled: check again to flush the size.
                ctx.request_repaint_after(std::time::Duration::from_millis(300));
            }
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
                AnalysisInfo::CombatsListRefreshed { combats, file_size } => {
                    // Only the combats list is refreshed here (e.g. the "Clear
                    // Log File" dialog opening); the currently-viewed combat in
                    // the main view is deliberately left untouched.
                    self.combats = combats;
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
