use std::{path::PathBuf, sync::Arc};

use eframe::egui::*;
use rfd::FileDialog;

use crate::{
    analyzer::{Combat, CombatSummaries, Difficulty, detect_log_owner},
    custom_widgets::toggle::Toggle,
    upload::{Records, Upload},
};

use self::{
    analysis_handling::AnalysisInfo,
    combat_filter::DifficultyFilter,
    combats_list::{CombatsListView, CombatsPanel, ListAction},
    compare::CompareView,
    main_tabs::*,
    settings::*,
    state::AppState,
    status::*,
    summary_copy::SummaryCopy,
};

mod analysis_handling;
pub mod app_icon;
mod combat_filter;
mod combats_list;
mod compare;
pub(crate) mod damage_subset;
mod date_range;
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

pub struct App {
    settings_window: SettingsWindow,
    /// Every combat in the log, oldest first. One entry per fight, carrying
    /// everything the lists show and filter by.
    combats: CombatSummaries,
    /// The list of fights down the side of the window.
    combats_panel: CombatsPanel,
    /// How wide the panel has been dragged. Kept here rather than in the live
    /// settings because the settings dialog replaces the whole settings object
    /// when it is applied, which would undo a drag made while it was open — the
    /// same reason the window geometry is kept here. Written out on exit.
    combats_panel_width: f32,
    /// Whose log this is, worked out from it: the handle in more of its combats
    /// than any other. What the reader named themselves in the settings wins
    /// over it.
    ///
    /// Kept once found, and remembered across launches. A log the fight came
    /// from cannot always say — a single saved fight has every player in it
    /// exactly once, and so does an evening of duo runs — and in that case the
    /// last log that *could* say is a far better answer than none.
    log_owner: Option<String>,
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
    /// The reader's own combats and the list that describes them, as they were
    /// before the ladder run took over. What the comparison picker offers while
    /// a run is on screen.
    own_combats: Vec<(usize, Arc<Combat>)>,
    own_combat_list: CombatSummaries,
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
        // The list follows the log from the moment the window is up, whatever
        // "Auto Refresh" is set to: that setting is about the *view* moving to
        // the newest fight, and a list of what the log holds is no use if it is
        // only right at the moment it was last read by hand.
        state.analysis_handler.enable_list_refresh(true);
        let app = Self {
            settings_window,
            combats: Default::default(),
            combats_panel: CombatsPanel::new(state.settings.general.combats_panel_open),
            combats_panel_width: state.settings.general.combats_panel_width,
            log_owner: state.settings.general.last_detected_handle.clone(),
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
                        self.log_owner.as_deref(),
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

                        // Folds the list of fights in and out, the way a browser
                        // does its sidebar. Kept as a toggle rather than a plain
                        // button so the toolbar says whether the panel is out.
                        if ui
                            .steady_toggle(self.combats_panel.is_open(), "☰ Combats")
                            .hover("Show the list of fights in the log.")
                            .clicked()
                        {
                            self.combats_panel.toggle();
                        }

                        // Reads the log again and puts the newest fight on
                        // screen. The list beside it keeps itself current on its
                        // own, so this button is about the *view*.
                        if ui
                            .button("Analysis of Newest Fight ⟲")
                            .hover("Read the log again and analyze the fight at the end of it.")
                            .clicked()
                        {
                            self.state.analysis_handler.refresh();
                        }

                        // Beside the button it repeats: the two do the same
                        // thing, one when it is pressed and one whenever the log
                        // grows.
                        if ui
                            .checkbox(
                                &mut self.state.settings.auto_refresh.enable,
                                "Always show analysis of Newest Fight",
                            )
                            .hover(
                                "Move to the fight at the end of the log whenever it grows. The \
                                 list of fights keeps itself current either way.",
                            )
                            .clicked()
                        {
                            self.state
                                .analysis_handler
                                .enable_auto_refresh(self.state.settings.auto_refresh.enable);
                            self.state.settings.save();
                        }

                        ui.separator();

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
                }

                if self.compare.is_open() {
                    // While a ladder run is on screen the analyzer holds that
                    // run, not the reader's log — so the picker is offered their
                    // own fights, captured before the switch, with the run
                    // itself pinned at the top of the list.
                    // Cloned rather than borrowed: the picker takes the state
                    // mutably, and these come off the same `self`. The clone is
                    // an `Arc`, so it costs nothing.
                    let ladder = self.ladder_run.is_some();
                    let own = ladder.then(|| self.own_combat_list.clone());
                    let pinned = ladder.then(|| self.ladder_run_label());
                    let picked = self.compare.show(
                        &mut self.state,
                        own.as_deref().unwrap_or(&self.combats),
                        self.log_owner.as_deref(),
                        pinned,
                        ui,
                        frame,
                    );
                    if let Some(picked) = picked {
                        self.compare_ladder_run_with(&picked);
                    }
                } else {
                    // The list of fights goes down the side, under the toolbar,
                    // and the tabs take what is left — a browser's sidebar, not
                    // a column beside the whole window.
                    let view = CombatsListView {
                        combats: &self.combats,
                        notes: &self.state.settings.combat_notes,
                        my_handle: effective_handle(
                            self.state.settings.general.my_handle.as_deref(),
                            self.log_owner.as_deref(),
                        ),
                        shown: self
                            .selected_combat
                            .as_ref()
                            .map(|combat| combat.active_time.start),
                    };
                    let action = self
                        .combats_panel
                        .show(view, &mut self.combats_panel_width, ui);
                    match action {
                        Some(ListAction::Open(index)) => {
                            self.selected_combat_index = Some(index);
                            self.state.analysis_handler.get_combat(index);
                        }
                        // Rewrites the log without the fights that were ticked.
                        // The list comes back through the ordinary channel, so
                        // nothing here has to put it right.
                        Some(ListAction::Keep(keep)) => {
                            log::info!(
                                "clearing the log: keeping {} of {} combats",
                                keep.len(),
                                self.combats.len()
                            );
                            self.state.analysis_handler.keep_combats(keep);
                        }
                        None => (),
                    }

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
        // Same for the panel, and for one more reason: applying the settings
        // replaces the whole settings object with the copy the dialog took when
        // it opened, so a panel folded or dragged in the meantime would be put
        // back the way it was.
        self.state.settings.general.last_detected_handle = self.log_owner.clone();
        self.state.settings.general.combats_panel_open = self.combats_panel.is_open();
        self.state.settings.general.combats_panel_width = self.combats_panel_width;
        self.state.settings.save();
    }
}

/// Whose figures the combats list shows: what the reader named themselves in
/// the settings, else what the log itself says.
///
/// A blank setting counts as "not set" — a field cleared to nothing means the
/// reader wants the log to decide again, not that nobody is to be shown.
fn effective_handle<'a>(configured: Option<&'a str>, detected: Option<&'a str>) -> Option<&'a str> {
    configured
        .map(str::trim)
        .filter(|handle| !handle.is_empty())
        .or(detected)
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
            self.own_combat_list = self.combats.clone();
            log::info!(
                "ladder: captured {} of my combats ({} with positions)",
                self.own_combat_list.len(),
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
        let map = self.combats.first().map(|combat| combat.base_name.clone());
        let difficulty = match self.combats.first().and_then(|combat| combat.difficulty) {
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
            own.iter().any(|combat| {
                map.as_ref().is_none_or(|map| &combat.base_name == map)
                    && difficulty.matches(combat.difficulty)
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
        self.own_combat_list = Default::default();
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

    /// Takes a fresh combats list, and reads off it whose log this is.
    ///
    /// Worked out here rather than per frame: the answer only changes when the
    /// list does, and the panel asks for it on every row of every frame.
    fn set_combats(&mut self, combats: CombatSummaries) {
        self.combats = combats;
        // Only ever replaced by an answer, never cleared by the lack of one:
        // see the field.
        if let Some(owner) = detect_log_owner(&self.combats) {
            self.log_owner = Some(owner);
        }
    }

    /// Puts `selected_combat_index` back on the combat the window is showing,
    /// after a fresh list may have moved it.
    ///
    /// That index is what Save Combat, the upload and a comparison ask the
    /// analyzer by, and the list is live now: a fight the analyzer drops
    /// (nobody dealt any damage in it) shifts every index after it. The start
    /// of a fight is the one thing about it the log fixes, so that is what it
    /// is found by again. `None` when it is no longer in the log at all —
    /// better than an index that now points at somebody else's fight.
    fn follow_shown_combat(&mut self) {
        let Some(shown) = self
            .selected_combat
            .as_ref()
            .map(|combat| combat.active_time.start)
        else {
            return;
        };
        self.selected_combat_index = self.combats.iter().position(|c| c.start == shown);
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
                    file_size,
                } => {
                    self.main_tabs.update(&self.state.settings, &latest_combat);
                    self.set_combats(combats);
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
                AnalysisInfo::CombatsListRefreshed { combats, file_size } => {
                    // Only the combats list is refreshed here — the log growing
                    // under the panel, the "Clear Log File" dialog opening, the
                    // compare view being opened. The combat the main view is
                    // showing is deliberately left where it is.
                    //
                    // The index of the combat on screen can move when this
                    // arrives (a fight without damage in it is dropped from the
                    // list as it goes on), so it is re-found by the one thing
                    // that does not move: when it started.
                    self.set_combats(combats);
                    self.follow_shown_combat();
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
