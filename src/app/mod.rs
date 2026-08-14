use std::{path::PathBuf, sync::Arc};

use chrono::NaiveDateTime;
use eframe::egui::*;
use rfd::FileDialog;

use crate::{
    analyzer::{Combat, Difficulty},
    custom_widgets::toggle::Toggle,
    upload::{Records, Upload},
};

use self::{
    analysis_handling::AnalysisInfo, combat_filter::DifficultyFilter, compare::CompareView,
    main_tabs::*, settings::*, state::AppState, status::*, summary_copy::SummaryCopy,
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
mod damage_subset;
pub mod desktop_install;
mod export;
mod fonts;
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
use crate::custom_widgets::tooltip::CloseTooltip;
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

/// The lists that describe a set of combats to the comparison picker, kept
/// together because they are indexed alongside each other and a mismatch reads
/// the wrong entry for every combat after it.
#[derive(Default, Clone)]
struct OwnCombatList {
    combats: Vec<String>,
    difficulties: Vec<Option<Difficulty>>,
    base_names: Vec<String>,
    environments: Vec<Option<String>>,
    start_times: Vec<NaiveDateTime>,
    solos: Vec<bool>,
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
    /// Whether each combat was fought alone, aligned with `combats` (the fourth
    /// axis of the picker's filter).
    combat_solos: Vec<bool>,
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
    /// Set while the main window is showing a run fetched from the ladder
    /// instead of the reader's own log: the log to go back to. The settings are
    /// left alone throughout — this is a look at somebody else's fight, not a
    /// change of which log is theirs.
    ladder_run: Option<String>,
    /// The fetched run being shown, by its own path. Kept apart from the
    /// settings on purpose: those still name the reader's log, because looking
    /// at somebody else's fight does not change which log is theirs — so the
    /// settings are exactly the wrong place to ask what is on screen.
    ladder_run_file: Option<PathBuf>,
    /// What the run turned out to be, read off it when it was read. See
    /// [`App::ladder_run_label`].
    ladder_run_name: Option<String>,
    /// Set while the composed comparison log is being read: its combats have to
    /// be asked for once it is, which is what actually builds the comparison.
    build_comparison_when_read: bool,
    /// Set when a run has just been put on screen and its analysis has not come
    /// back yet. The map and level to start the comparison pickers at can only
    /// be read off the run once it has been read, and that arrives later.
    suggest_filter_from_run: bool,
    /// A run fetched from the ladder, waiting to be shown. It is held here while
    /// the reader's own combats are asked for: those carry where each fight sits
    /// in their log, which is what a comparison has to be cut from, and once the
    /// analyzer has moved to the run there is no asking for them.
    pending_ladder_run: Option<PathBuf>,
    /// The reader's own combats and the lists that describe them, as they were
    /// before the ladder run took over. What the comparison picker offers while
    /// a run is on screen.
    own_combats: Vec<(usize, Arc<Combat>)>,
    own_combat_list: OwnCombatList,
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
            combat_solos: Default::default(),
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
            ladder_run: None,
            ladder_run_file: None,
            ladder_run_name: None,
            suggest_filter_from_run: false,
            build_comparison_when_read: false,
            pending_ladder_run: None,
            own_combats: Default::default(),
            own_combat_list: Default::default(),
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
                    if let Some(run) = self.records.show(
                        ui,
                        frame,
                        &self.state.settings.upload.oscr_url,
                        &mut self.state.settings.general.ladder_window_position,
                    ) {
                        self.show_ladder_run(run);
                    }

                    // Says whose fight is on screen, and offers the way back.
                    // Without it the window would look like the reader's own log
                    // had been replaced, which is exactly what has not happened.
                    if self.ladder_run.is_some() {
                        ui.label(
                            RichText::new("⚑ a run from the ladder").color(theme::palette().busy),
                        );
                        if ui
                            .button("Back to my log")
                            .hover("Reads your own combat log again.")
                            .clicked()
                        {
                            self.leave_ladder_run();
                        }
                    }

                    // Compare toggle (ON/OFF) as the last item on the top bar so it
                    // stays put regardless of mode. Rendered as a frameless toggle to
                    // match the Settings and Records buttons (highlighted while active).
                    if ui
                        .steady_toggle(self.compare.is_open(), "Compare Combats")
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
                        self.summary_copy.show(
                            self.selected_combat.as_deref(),
                            &self.state.settings.combat_notes,
                            ui,
                        );
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
                                solo: self.combat_solos.get(i).copied().unwrap_or(false),
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
                    // While a ladder run is on screen the analyzer holds that
                    // run, not the reader's log — so the picker is offered their
                    // own fights, captured before the switch, with the run
                    // itself pinned at the top of the list.
                    // Cloned rather than borrowed: the picker takes the state
                    // mutably, and these come off the same `self`.
                    let ladder = self.ladder_run.is_some();
                    let own = ladder.then(|| self.own_combat_list.clone());
                    let pinned = ladder.then(|| self.ladder_run_label());
                    let picked = self.compare.show(
                        &mut self.state,
                        own.as_ref()
                            .map(|own| own.combats.as_slice())
                            .unwrap_or(&self.combats),
                        own.as_ref()
                            .map(|own| own.difficulties.as_slice())
                            .unwrap_or(&self.combat_difficulties),
                        own.as_ref()
                            .map(|own| own.base_names.as_slice())
                            .unwrap_or(&self.combat_base_names),
                        own.as_ref()
                            .map(|own| own.environments.as_slice())
                            .unwrap_or(&self.combat_environments),
                        own.as_ref()
                            .map(|own| own.start_times.as_slice())
                            .unwrap_or(&self.combat_start_times),
                        own.as_ref()
                            .map(|own| own.solos.as_slice())
                            .unwrap_or(&self.combat_solos),
                        pinned,
                        ui,
                        frame,
                    );
                    if let Some(picked) = picked {
                        self.compare_ladder_run_with(&picked);
                    }
                } else {
                    self.main_tabs.show(
                        &mut self.state.settings,
                        self.selected_combat.as_deref(),
                        frame,
                        ui,
                    );
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
    /// Asks for the reader's own combats, then shows the run once they are in
    /// hand.
    ///
    /// The order matters and cannot be swapped: a combat carries where it sits
    /// in the log it came from, and a comparison is cut from exactly that. Once
    /// the analyzer has moved to the fetched run, the reader's own fights are no
    /// longer anywhere to be asked for. The answer arrives through the ordinary
    /// channel, so the move itself happens in `handle_analysis_infos`.
    fn show_ladder_run(&mut self, run: PathBuf) {
        if self.ladder_run_file.as_ref() == Some(&run) {
            return;
        }
        // Already reading a run: what the analyzer holds is that run, not the
        // reader's log, so there is nothing here worth capturing — and asking
        // would replace their fights with the single one on screen. The capture
        // from the first run is still the right one.
        if self.ladder_run.is_some() {
            self.enter_ladder_run(run);
            return;
        }
        self.pending_ladder_run = Some(run);
        self.state
            .analysis_handler
            .get_combats((0..self.combats.len()).collect());
    }

    /// Moves the analysis onto the run now that the reader's own fights are
    /// captured.
    ///
    /// The settings are not touched. `set_settings` takes what it is given and
    /// replaces the analyzer; saving is a separate step that happens when the
    /// settings dialog is applied. So the reader's own log stays their log, and
    /// looking at somebody else's fight does not quietly become a change of
    /// which log the program reads.
    fn enter_ladder_run(&mut self, run: PathBuf) {
        let own_log = self.state.settings.analysis.combatlog_file.clone();
        // Only on the way in from the reader's own log. A second run opened
        // while the first is on screen would otherwise capture that first run.
        if self.ladder_run.is_none() {
            self.own_combat_list = OwnCombatList {
                combats: self.combats.clone(),
                difficulties: self.combat_difficulties.clone(),
                base_names: self.combat_base_names.clone(),
                environments: self.combat_environments.clone(),
                start_times: self.combat_start_times.clone(),
                solos: self.combat_solos.clone(),
            };
            log::info!(
                "ladder: captured {} of my combats ({} with positions)",
                self.own_combat_list.combats.len(),
                self.own_combats.len()
            );
        }
        log::info!("ladder: showing {}", run.display());
        self.suggest_filter_from_run = true;
        self.ladder_run.get_or_insert(own_log);
        let mut analysis = self.state.settings.analysis.clone();
        analysis.combatlog_file = run.display().to_string();
        self.ladder_run_file = Some(run);
        // Merging rotating logs is about the reader's own game folder; a fetched
        // run is a single fight in a scratch directory and has nothing to merge.
        analysis.consolidate_combatlog = false;
        self.state.analysis_handler.set_settings(analysis);
        // `set_settings` only puts a new analyzer in place; nothing is read
        // until it is asked to. The settings dialog does both, which is why
        // doing only the first left the window saying it was showing a ladder
        // run while still holding the previous log's figures.
        self.state.analysis_handler.refresh();
    }

    /// Writes the run and the fights picked from the reader's own log into one
    /// log, and reads that.
    ///
    /// Neither the reader's log nor the fetched run is touched — this is a third
    /// file, made for the question and thrown away with the rest of the scratch
    /// directory.
    fn compare_ladder_run_with(&mut self, picked: &[usize]) {
        let (Some(run), Some(own_log)) = (self.ladder_run_path(), self.ladder_run.clone()) else {
            return;
        };
        let Ok(mut composed) = std::fs::read(&run) else {
            return;
        };
        let own_log = std::path::Path::new(&own_log);
        for index in picked {
            let Some(combat) = self
                .own_combats
                .iter()
                .find(|(i, _)| i == index)
                .and_then(|(_, combat)| combat.read_log_combat_data(own_log))
            else {
                continue;
            };
            composed = crate::helpers::compose_comparison_log(&composed, &combat);
        }
        let path = crate::helpers::paths::comparison_log();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(error) = std::fs::write(&path, &composed) {
            log::error!("ladder: could not write the comparison log: {error}");
            return;
        }
        log::info!(
            "ladder: composed {} bytes from the run and {} of my fights into {}",
            composed.len(),
            picked.len(),
            path.display()
        );
        let mut analysis = self.state.settings.analysis.clone();
        analysis.combatlog_file = path.display().to_string();
        analysis.consolidate_combatlog = false;
        self.state.analysis_handler.set_settings(analysis);
        self.state.analysis_handler.refresh();
        // Reading it is only half of it: the comparison is built from the
        // combats, and those have to be asked for once there are any. Without
        // this the log was composed and read and nothing appeared, which looked
        // exactly like a button that does nothing.
        self.build_comparison_when_read = true;
    }

    /// What the pinned entry says it is: the fight the run holds, taken when the
    /// run itself was read.
    ///
    /// Not from whatever combat happens to be open. Once the two fights are
    /// composed into one log, the newest of them is the one that comes up — so
    /// the pinned entry started naming the reader's own fight instead of the run
    /// it stands for.
    fn ladder_run_label(&self) -> String {
        self.ladder_run_name
            .clone()
            .unwrap_or_else(|| "the run from the ladder".to_owned())
    }

    /// Points the comparison pickers at the same map and level as the run just
    /// opened. Done when its own analysis arrives, which is the first moment
    /// there is anything to read them off.
    fn suggest_compare_filter_from_run(&mut self) {
        let map = self.combat_base_names.first().cloned();
        let difficulty = match self.combat_difficulties.first().copied().flatten() {
            Some(Difficulty::Normal) => DifficultyFilter::Normal,
            Some(Difficulty::Advanced) => DifficultyFilter::Advanced,
            Some(Difficulty::Elite) => DifficultyFilter::Elite,
            _ => DifficultyFilter::Any,
        };
        // Suggested only where it leaves something. A run of a map the reader
        // has never played — which is most of the ladder, most of the time —
        // would otherwise open the picker on an empty list, and an empty list
        // reads as a broken window rather than as an answer. Falling back to the
        // level alone at least keeps their own fights of that difficulty in
        // view; falling back to nothing shows them all.
        let own = &self.own_combat_list;
        let leaves_something = |map: &Option<String>, difficulty: DifficultyFilter| {
            (0..own.combats.len()).any(|i| {
                map.as_ref()
                    .is_none_or(|map| own.base_names.get(i) == Some(map))
                    && difficulty.matches(own.difficulties.get(i).copied().flatten())
            })
        };
        let (map, difficulty) = if leaves_something(&map, difficulty) {
            (map, difficulty)
        } else if leaves_something(&None, difficulty) {
            log::info!("ladder: no fight of my own on that map, keeping the level only");
            (None, difficulty)
        } else {
            log::info!("ladder: nothing of my own matches that run, leaving the pickers open");
            (None, DifficultyFilter::Any)
        };
        self.compare.suggest_filter(map, difficulty);
    }

    /// The run on screen, when one is.
    fn ladder_run_path(&self) -> Option<PathBuf> {
        self.ladder_run_file.clone()
    }

    /// Puts the reader's own log back.
    fn leave_ladder_run(&mut self) {
        self.ladder_run_file = None;
        self.ladder_run_name = None;
        self.suggest_filter_from_run = false;
        self.own_combats.clear();
        self.own_combat_list = OwnCombatList::default();
        if self.ladder_run.take().is_some() {
            self.state
                .analysis_handler
                .set_settings(self.state.settings.analysis.clone());
            self.state.analysis_handler.refresh();
        }
    }

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
            self.combat_solos.get(index).copied().unwrap_or(false),
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
        let combatlog_file = self.state.settings.analysis.combatlog_file.clone();
        // Collected before the loop: entering a ladder run replaces the
        // analyzer, and the iterator borrows the handler it would be replacing.
        let infos: Vec<_> = self.state.analysis_handler.check_for_info().collect();
        for info in infos {
            match info {
                AnalysisInfo::Combat(combat) => {
                    self.main_tabs.update(&self.state.settings, &combat);
                    self.selected_combat = Some(combat);
                }
                AnalysisInfo::Combats(combats) => {
                    // Either the answer to "show me this ladder run", which had
                    // to capture the reader's own fights before the analyzer
                    // moved off their log, or the ordinary one the comparison
                    // asked for.
                    match self.pending_ladder_run.take() {
                        Some(run) => {
                            self.own_combats = combats;
                            self.enter_ladder_run(run);
                        }
                        None => self.compare.set_combats(combats, &self.state.settings),
                    }
                }
                AnalysisInfo::Refreshed {
                    latest_combat,
                    combats,
                    difficulties,
                    base_names,
                    environments,
                    start_times,
                    solos,
                    file_size,
                } => {
                    self.main_tabs.update(&self.state.settings, &latest_combat);
                    self.combats = combats;
                    self.combat_difficulties = difficulties;
                    self.combat_base_names = base_names;
                    self.combat_environments = environments;
                    self.combat_start_times = start_times;
                    self.combat_solos = solos;
                    self.selected_combat_index = Some(self.combats.len() - 1);
                    self.selected_combat = Some(latest_combat);
                    self.status_indicator.status = Status::Loaded {
                        combatlog_file: combatlog_file.clone(),
                        file_size,
                    };
                    // The run has been read: point the comparison pickers at the
                    // same map and level, which is what it will nearly always be
                    // compared against.
                    // Only for a run just opened, and only once. The composed
                    // comparison log arrives this way too, and reading the
                    // filters off that would narrow them again under a reader
                    // who had just widened them.
                    if std::mem::take(&mut self.suggest_filter_from_run) {
                        self.ladder_run_name =
                            self.selected_combat.as_ref().map(|combat| combat.name());
                        self.suggest_compare_filter_from_run();
                    }
                    if std::mem::take(&mut self.build_comparison_when_read) {
                        log::info!(
                            "ladder: comparison log holds {} combats, building",
                            self.combats.len()
                        );
                        self.state
                            .analysis_handler
                            .get_combats((0..self.combats.len()).collect());
                    }
                }
                AnalysisInfo::CombatsListRefreshed {
                    combats,
                    difficulties,
                    base_names,
                    environments,
                    start_times,
                    solos,
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
                    self.combat_solos = solos;
                    self.status_indicator.status = Status::Loaded {
                        combatlog_file: combatlog_file.clone(),
                        file_size,
                    };
                }
                AnalysisInfo::RefreshError => {
                    log::error!("ladder: the analyzer could not read what it was given");
                    self.status_indicator.status = Status::LoadError {
                        combatlog_file: combatlog_file.clone(),
                    };
                }
            }
        }
    }
}
