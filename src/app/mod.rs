use std::{sync::Arc, time::Duration};

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
pub mod logging;
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
    window_geometry: WindowGeometry,
    window_geometry_dirty: bool,
    last_geometry_change: f64,
}

/// How long the window size has to stay unchanged before it is written to the
/// settings file, so that dragging a window edge does not cause a write per
/// frame.
const GEOMETRY_SETTLE_TIME: f64 = 2.0;

/// The window size and maximized state to start with, read before the window
/// exists (see main.rs).
pub fn saved_window_geometry() -> (Option<Vec2>, bool) {
    let window = Settings::load_or_default().window;
    (window.size.map(|[w, h]| vec2(w, h)), window.maximized)
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
            window_geometry: state.settings.window,
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
        CentralPanel::default().show_inside(ui, |ui| {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    self.settings_window.show(
                        &mut self.state,
                        self.selected_combat.as_deref(),
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

                    self.settings_window
                        .show_clear_log_dialog(&self.state.analysis_handler, ui);

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
        // Catches a size change that has not settled yet when the window is
        // closed (see track_window_geometry).
        if self.window_geometry_dirty {
            self.save_window_geometry();
        }
    }
}

impl App {
    /// Remembers the window size and maximized state so that the next launch
    /// can restore them (see `saved_window_geometry`).
    ///
    /// The size comes from the egui viewport rect instead of
    /// `ViewportInfo::inner_rect`, because the latter is `None` on Wayland,
    /// where a client is not told where its window is. That rect is in points,
    /// so it is scaled back by the zoom factor to the logical pixels that
    /// `ViewportBuilder::with_inner_size` expects — otherwise a "ui scale"
    /// other than 1 would shrink or grow the window on every launch.
    fn track_window_geometry(&mut self, ctx: &Context) {
        let now = ctx.input(|i| i.time);
        let maximized = ctx.input(|i| i.viewport().maximized);

        // Only remember the size the window has while not maximized, so that
        // un-maximizing it after a restart gives back a usable window.
        if maximized != Some(true) {
            let size = (ctx.viewport_rect().size() * ctx.zoom_factor()).round();
            self.set_window_geometry(
                now,
                WindowGeometry {
                    size: Some([size.x, size.y]),
                    ..self.window_geometry
                },
            );
        }
        if let Some(maximized) = maximized {
            self.set_window_geometry(
                now,
                WindowGeometry {
                    maximized,
                    ..self.window_geometry
                },
            );
        }

        if self.window_geometry_dirty {
            let settled_in = GEOMETRY_SETTLE_TIME - (now - self.last_geometry_change);
            if settled_in <= 0.0 {
                self.save_window_geometry();
            } else {
                // The size only changes while a window edge is dragged, and no
                // further frame is guaranteed once that stops, so ask for one.
                ctx.request_repaint_after(Duration::from_secs_f64(settled_in));
            }
        }
    }

    fn set_window_geometry(&mut self, now: f64, geometry: WindowGeometry) {
        if self.window_geometry != geometry {
            self.window_geometry = geometry;
            self.window_geometry_dirty = true;
            self.last_geometry_change = now;
        }
    }

    /// Writes the tracked geometry into the settings file. The geometry is held
    /// in a field rather than in `state.settings` because the settings dialog
    /// replaces the whole settings object when it is applied, which would drop
    /// a resize made while the dialog was open.
    fn save_window_geometry(&mut self) {
        self.state.settings.window = self.window_geometry;
        self.state.settings.save();
        self.window_geometry_dirty = false;
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
