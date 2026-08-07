use std::sync::Arc;

use chrono::NaiveDateTime;
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
pub mod app_icon;
mod combat_filter;

/// How many combats the picker shows before it starts to scroll.
const COMBATS_SHOWN_AT_ONCE: usize = 15;

/// Width of the combats picker, in points. Holds a full identifier — map,
/// environment, level, date and time — followed by a note of the full 50
/// characters. Entries longer than this are cut with an ellipsis rather than
/// wrapped, so a row stays one row tall.
const COMBATS_LIST_WIDTH: f32 = 900.0;
mod compare;
pub mod desktop_install;
mod fonts;
#[cfg(target_os = "linux")]
mod log_consolidation;
pub mod logging;
mod main_tabs;
mod overlay;
pub mod self_upgrade;
mod settings;
mod state;
mod status;
mod summary_copy;
pub mod theme;

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
    /// Each combat's name without the environment and difficulty suffixes, for
    /// the type filters.
    combat_base_names: Vec<String>,
    /// Each combat's environment ("Space" / "Ground" / …), for the same.
    combat_environments: Vec<Option<String>>,
    /// Each combat's start time, aligned with `combats`. It is what a user note
    /// is keyed by, and the only per-combat value the log itself fixes.
    combat_start_times: Vec<NaiveDateTime>,
    /// Narrows the combat picker to one environment, level and/or map.
    combat_filter: combat_filter::CombatFilter,
    /// Bumped whenever the filter changes, and mixed into the picker's id.
    ///
    /// egui keeps a scroll area's measured size under that id, so the list kept
    /// opening at the height it had while filtered however much it then held.
    /// A new id makes it a new scroll area, measured from scratch.
    combat_filter_generation: u64,
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
        // The tables ask for the bold family, which only exists once installed.
        fonts::install(&cc.egui_ctx);
        let state = AppState::new(&cc.egui_ctx);
        let settings_window =
            SettingsWindow::new(&cc.egui_ctx, cc.egui_ctx.native_pixels_per_point());
        let app = Self {
            settings_window,
            combats: Default::default(),
            combat_difficulties: Default::default(),
            combat_base_names: Default::default(),
            combat_environments: Default::default(),
            combat_start_times: Default::default(),
            combat_filter: Default::default(),
            combat_filter_generation: 0,
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
        // Driven here rather than from the toolbar that carries its button: the
        // overlay follows the newest combat on a handler of its own, whatever
        // the main window is showing, and the single-combat toolbar is hidden
        // while Compare Combats is open.
        self.state.overlay.update(ui.ctx());
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
                        // The toolbar below carries "Refresh Now" and is hidden
                        // while comparing, so opening the view is the moment to
                        // pick up combats logged since the list was last read.
                        // Only the list is refreshed; the viewed combat stays.
                        if self.compare.is_open() {
                            self.state.analysis_handler.refresh_combats_list();
                        }
                    }
                });

                // The single-combat toolbar; hidden entirely while comparing.
                if !self.compare.is_open() {
                    ui.horizontal_wrapped(|ui| {
                        self.status_indicator
                            .show(self.state.analysis_handler.is_busy(), ui);

                        // One row of the popup, used both for its height cap
                        // and for the room reserved inside it, so the two agree
                        // whatever the UI scale is.
                        let row = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
                        // The combat on show, with its note where there is one,
                        // so the closed box says the same as the list entry it
                        // came from.
                        let selected = {
                            let note = self
                                .selected_combat
                                .as_deref()
                                .map(|combat| {
                                    self.state
                                        .settings
                                        .combat_notes
                                        .get(&CombatNotes::key(combat))
                                })
                                .unwrap_or("");
                            if note.is_empty() {
                                self.main_tabs.identifier.clone()
                            } else {
                                format!("{} — {}", self.main_tabs.identifier, note)
                            }
                        };
                        ComboBox::new(("combat list", self.combat_filter_generation), "Combats")
                            // Room for a full identifier — map, environment,
                            // level, date and time — plus a note of the full 50
                            // characters, on one line.
                            .width(COMBATS_LIST_WIDTH)
                            .height(row * COMBATS_SHOWN_AT_ONCE as f32)
                            .selected_text(selected)
                            .show_ui(ui, |ui| {
                                // Reserve the room here. The popup reports only
                                // about three rows of available height, and
                                // egui's scroll area sizes its viewport to that,
                                // so without a minimum the list opens three tall
                                // however many combats it holds. Clamping the
                                // minimum to the reported height puts the same
                                // three rows straight back.
                                let visible = (0..self.combats.len())
                                    .filter(|&i| self.combat_matches_filter(i))
                                    .count();
                                ui.set_min_height(row * visible.min(COMBATS_SHOWN_AT_ONCE) as f32);
                                // The scroll area takes its viewport from the
                                // content size it measured last frame. After a
                                // filter is cleared the first frame still has
                                // the narrowed list's size, and nothing else
                                // asks for another one — so the list would stay
                                // as short as it was while filtered.
                                ui.ctx().request_repaint();
                                // An entry that is too long is cut with an
                                // ellipsis rather than wrapped onto a second
                                // line: a wrapped row is twice as tall, and the
                                // height reserved above counts single rows.
                                ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
                                for (i, combat) in self.combats.iter().enumerate().rev() {
                                    if !self.combat_matches_filter(i) {
                                        continue;
                                    }
                                    // The user's own note, where there is one,
                                    // is what tells two runs of the same map
                                    // apart at a glance.
                                    let note = self
                                        .combat_start_times
                                        .get(i)
                                        .map(|&start| {
                                            self.state
                                                .settings
                                                .combat_notes
                                                .get(&CombatNotes::key_at(start))
                                        })
                                        .unwrap_or("");
                                    let entry = if note.is_empty() {
                                        combat.clone()
                                    } else {
                                        format!("{combat} — {note}")
                                    };
                                    if ui
                                        .selectable_value(
                                            &mut self.selected_combat_index,
                                            Some(i),
                                            entry,
                                        )
                                        .changed()
                                        && let Some(combat_index) = self.selected_combat_index
                                    {
                                        self.state.analysis_handler.get_combat(combat_index);
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
                            && let Some(file) = FileDialog::new()
                                .set_title("Save Combat")
                                .add_filter("log", &["log"])
                                .set_file_name(
                                    self.selected_combat.as_ref().unwrap().file_identifier(),
                                )
                                .set_parent(frame)
                                .save_file()
                        {
                            self.state
                                .analysis_handler
                                .save_combat(self.selected_combat_index.unwrap(), file);
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
                        self.state.overlay.show_button(ui);
                    });

                    // Own row: a "Clear filter" button appearing next to the
                    // pickers would otherwise shove the toolbar buttons sideways.
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Show only:");
                        let before = self.combat_filter.clone();
                        // Each menu offers only what the other two leave
                        // reachable, so no combination can empty the list.
                        let entries: Vec<combat_filter::CombatEntry> = (0..self.combats.len())
                            .map(|i| combat_filter::CombatEntry {
                                environment: self
                                    .combat_environments
                                    .get(i)
                                    .and_then(|e| e.as_deref()),
                                difficulty: self.combat_difficulties.get(i).copied().flatten(),
                                base_name: self
                                    .combat_base_names
                                    .get(i)
                                    .map(String::as_str)
                                    .unwrap_or(""),
                            })
                            .collect();
                        self.combat_filter.show("combats", &entries, ui);
                        if self.combat_filter.is_active() && ui.button("Clear filter").clicked() {
                            self.combat_filter.clear();
                        }
                        if self.combat_filter != before {
                            self.combat_filter_generation =
                                self.combat_filter_generation.wrapping_add(1);
                            self.follow_filter_change();
                        }
                    });
                }

                if self.compare.is_open() {
                    self.compare.show(
                        &mut self.state,
                        &self.combats,
                        &self.combat_difficulties,
                        &self.combat_base_names,
                        &self.combat_environments,
                        &self.combat_start_times,
                        ui,
                    );
                } else {
                    self.main_tabs.show(&mut self.state.settings, ui);
                }
            });
        });
    }

    /// Fully transparent, because eframe hands this one colour to *every*
    /// window it paints — the main one and the overlay alike. Anything solid
    /// here is painted underneath the overlay's own surface and cancels its
    /// opacity setting out (see `overlay::surface_fill`).
    ///
    /// The main window loses nothing by it: its surface is opaque, so the alpha
    /// is ignored there, and its central panel covers every pixel with the
    /// theme's own colour before the frame is shown.
    fn clear_color(&self, _visuals: &eframe::egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn on_exit(&mut self) {
        // Persists the overlay position picked up in `ui`, plus a size change
        // that has not settled yet (see track_window_geometry).
        self.state.settings.window = self.window_geometry;
        // Written here rather than as the overlay is toggled: `general` is
        // compared when the settings dialog is applied, and a difference there
        // re-analyzes the log.
        self.state.settings.general.overlay_shown = self.state.overlay.is_shown();
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

    /// Whether the combat at `index` passes the combat picker's filter.
    fn combat_matches_filter(&self, index: usize) -> bool {
        self.combat_filter.matches(
            self.combat_environments
                .get(index)
                .and_then(|e| e.as_deref()),
            self.combat_difficulties.get(index).copied().flatten(),
            self.combat_base_names
                .get(index)
                .map(String::as_str)
                .unwrap_or(""),
        )
    }

    /// After the filter changed, move to the newest combat that still passes it.
    ///
    /// Without this the window keeps showing a combat the list no longer offers,
    /// which reads as the filter having done nothing. A selection that still
    /// passes is left alone, so narrowing around the combat being looked at does
    /// not jump away from it.
    fn follow_filter_change(&mut self) {
        if self
            .selected_combat_index
            .is_some_and(|index| self.combat_matches_filter(index))
        {
            return;
        }
        let Some(index) = (0..self.combats.len())
            .rev()
            .find(|&index| self.combat_matches_filter(index))
        else {
            return;
        };
        self.selected_combat_index = Some(index);
        self.state.analysis_handler.get_combat(index);
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
                    base_names,
                    environments,
                    start_times,
                    file_size,
                } => {
                    self.main_tabs.update(&self.state.settings, &latest_combat);
                    self.combats = combats;
                    self.combat_difficulties = difficulties;
                    self.combat_base_names = base_names;
                    self.combat_environments = environments;
                    self.combat_start_times = start_times;
                    self.selected_combat_index = Some(self.combats.len() - 1);
                    self.selected_combat = Some(latest_combat);
                    self.status_indicator.status = Status::Loaded {
                        combatlog_file: combatlog_file.clone(),
                        file_size,
                    };
                }
                AnalysisInfo::CombatsListRefreshed {
                    combats,
                    difficulties,
                    base_names,
                    environments,
                    start_times,
                    file_size,
                } => {
                    // Only the combats list is refreshed here (the "Clear Log
                    // File" dialog opening, or the compare view being opened);
                    // the currently-viewed combat in the main view is
                    // deliberately left untouched. The three metadata lists are
                    // indexed alongside `combats` and must move with it, or the
                    // filters read the wrong entry for every new combat.
                    self.combats = combats;
                    self.combat_difficulties = difficulties;
                    self.combat_base_names = base_names;
                    self.combat_environments = environments;
                    self.combat_start_times = start_times;
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
