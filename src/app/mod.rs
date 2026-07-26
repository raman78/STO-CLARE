use std::sync::Arc;

use eframe::egui::*;
use rfd::FileDialog;

use crate::{
    analyzer::{Combat, Difficulty},
    upload::{Records, Upload},
};

use self::{
    analysis_handling::AnalysisInfo, compare::CompareView, main_tabs::*, settings::*,
    state::AppState, status::*, summary_copy::SummaryCopy,
};

mod analysis_handling;
mod compare;
pub mod desktop_install;
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

// The layer-shell overlay backend lives under `overlay::layer_shell`; re-export
// the startup helper so main.rs can build the shared wgpu stack (see main.rs).
#[cfg(target_os = "linux")]
pub use overlay::layer_shell::create_shared_gpu;

/// Whether the app came up on Wayland, which is where the overlay needs the
/// layer-shell backend. Asks the window system handle eframe was given, so it
/// reports the backend winit actually chose.
#[cfg(target_os = "linux")]
fn is_wayland(cc: &eframe::CreationContext) -> bool {
    use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
    matches!(
        cc.display_handle().map(|handle| handle.as_raw()),
        Ok(RawDisplayHandle::Wayland(_))
    )
}

pub struct App {
    settings_window: SettingsWindow,
    combats: Vec<String>,
    /// Detected difficulty per combat, aligned with `combats` (compare filter).
    combat_difficulties: Vec<Option<Difficulty>>,
    selected_combat_index: Option<usize>,
    selected_combat: Option<Arc<Combat>>,
    status_indicator: StatusIndicator,
    main_tabs: MainTabs,
    compare: CompareView,
    summary_copy: SummaryCopy,
    upload: Upload,
    records: Records,
    state: AppState,
    // Deferred persistence of the window size: written once resizing settles
    // (see track_window_geometry).
    window_geometry: WindowGeometry,
    window_geometry_dirty: bool,
    last_geometry_change: f64,
}

/// How long the window size has to stay unchanged before it is written to the
/// settings file, so that dragging a window edge does not cause a write per
/// frame.
const GEOMETRY_SETTLE_TIME: f64 = 2.0;

/// How long after the last size change the window still counts as being
/// dragged, and is therefore redrawn every frame.
const ACTIVE_RESIZE_TIME: f64 = 0.5;

/// Window geometry to restore at startup: last size and whether the window was
/// maximized. Read before the viewport is built (see main.rs).
pub fn saved_window_geometry() -> (Option<Vec2>, bool) {
    let window = Settings::load_or_default().window;
    (window.size.map(|[w, h]| vec2(w, h)), window.maximized)
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext,
        overlay_instance: Option<eframe::wgpu::Instance>,
    ) -> Self {
        cc.egui_ctx
            .memory_mut(|m| m.options.repaint_on_widget_change = false);
        let state = AppState::new(&cc.egui_ctx);
        let settings_window =
            SettingsWindow::new(&cc.egui_ctx, cc.egui_ctx.native_pixels_per_point());
        let app = Self {
            settings_window,
            combats: Default::default(),
            combat_difficulties: Default::default(),
            selected_combat_index: None,
            selected_combat: None,
            status_indicator: StatusIndicator::new(),
            main_tabs: MainTabs::empty(),
            compare: Default::default(),
            summary_copy: Default::default(),
            upload: Default::default(),
            records: Default::default(),
            window_geometry: state.settings.window,
            state,
            window_geometry_dirty: false,
            last_geometry_change: 0.0,
        };

        // In a Wayland session, hand the layer-shell overlay the shared wgpu
        // handles: the instance we created up front (passed in) plus eframe's
        // adapter/device/queue — which, thanks to WgpuSetup::Existing, are the
        // very ones we handed eframe. So both render through one device.
        //
        // In an X11 session there is no layer-shell to talk to, so the handles
        // stay unset and the overlay falls back to the plain always-on-top
        // viewport, which works there. `overlay_instance` is already `None` in
        // that case (see main.rs); asking the window handle as well means the
        // backend winit actually picked decides, not a guess.
        #[cfg(target_os = "linux")]
        if is_wayland(cc)
            && let (Some(instance), Some(render_state)) =
                (overlay_instance, cc.wgpu_render_state.as_ref())
        {
            log::info!("overlay backend: layer-shell (Wayland session)");
            app.state.overlay.set_gpu(overlay::layer_shell::OverlayGpu {
                instance,
                adapter: render_state.adapter.clone(),
                device: render_state.device.clone(),
                queue: render_state.queue.clone(),
            });
        } else {
            log::info!("overlay backend: always-on-top window");
        }
        #[cfg(not(target_os = "linux"))]
        let _ = overlay_instance;

        app
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

                    // Compare toggle (ON/OFF) as the last item on the top bar so it
                    // stays put regardless of mode. Rendered as a frameless toggle to
                    // match the Settings and Records buttons (highlighted while active).
                    if ui
                        .selectable_label(self.compare.is_open(), "Compare Combats 🆚")
                        .clicked()
                    {
                        self.compare.toggle();
                    }
                });

                // The single-combat toolbar; hidden entirely while comparing.
                if !self.compare.is_open() {
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
                }

                if self.compare.is_open() {
                    self.compare.show(
                        &mut self.state,
                        &self.combats,
                        &self.combat_difficulties,
                        ui,
                    );
                } else {
                    self.main_tabs.show(&self.state.settings, ui);
                }
            });
        });
    }

    fn clear_color(&self, _visuals: &eframe::egui::Visuals) -> [f32; 4] {
        _visuals.window_fill().to_normalized_gamma_f32()
    }

    fn on_exit(&mut self) {
        // Persists the overlay position picked up in `ui`, plus a size change
        // that has not settled yet (see track_window_geometry).
        self.state.settings.window = self.window_geometry;
        self.state.settings.save();
    }
}

impl App {
    /// Remembers the main window's size and maximized state so the next launch
    /// restores them (see main.rs).
    ///
    /// The size comes from the egui viewport rect instead of
    /// `ViewportInfo::inner_rect`, because the latter is `None` on Wayland,
    /// where a client is not told where its window is. That rect is in points,
    /// so it is scaled back by the zoom factor to the logical pixels that
    /// `ViewportBuilder::with_inner_size` expects — otherwise a "ui scale"
    /// other than 1 would shrink or grow the window on every launch.
    ///
    /// The settings file is written only once the size has settled, never while
    /// the edge is being dragged, so resizing stays smooth.
    fn track_window_geometry(&mut self, ctx: &eframe::egui::Context) {
        let now = ctx.input(|i| i.time);
        let maximized = ctx.input(|i| i.viewport().maximized);

        // Only remember the windowed size, so un-maximizing restores something
        // sane rather than the full-screen size.
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
            let idle = now - self.last_geometry_change;
            if idle >= GEOMETRY_SETTLE_TIME {
                self.save_window_geometry();
            } else if idle < ACTIVE_RESIZE_TIME {
                // The edge is still being dragged. Redraw every frame so the
                // contents follow the window instead of trailing behind it.
                ctx.request_repaint();
            } else {
                // Dragging has stopped and no further frame is guaranteed, so
                // ask for the one that writes the settled size.
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                    GEOMETRY_SETTLE_TIME - idle,
                ));
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
                AnalysisInfo::Combats(combats) => {
                    self.compare.set_combats(combats, &self.state.settings);
                }
                AnalysisInfo::Refreshed {
                    latest_combat,
                    combats,
                    difficulties,
                    file_size,
                } => {
                    self.main_tabs.update(&self.state.settings, &latest_combat);
                    self.combats = combats;
                    self.combat_difficulties = difficulties;
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
