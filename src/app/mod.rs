use std::{path::PathBuf, sync::Arc};

use chrono::NaiveDateTime;

use eframe::egui::*;
use rfd::FileDialog;

use crate::{
    analyzer::{Combat, CombatSummaries, CombatSummary, Difficulty, detect_log_owner},
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
mod job;
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
mod tuning;

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

/// A run fetched from the ladder: the fight itself, the file it came out of
/// (which is what saving it copies), and the row the list draws for it.
struct LadderRun {
    path: PathBuf,
    combat: Arc<Combat>,
    summary: CombatSummary,
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
    /// Runs fetched from the ladder, in the order they were opened.
    ///
    /// Fights like any other, held here rather than read into the analyzer:
    /// they came out of somebody else's log, and looking at one is no reason to
    /// put the reader's own log down. The list shows them at its top, a
    /// comparison takes them beside the reader's own fights, and nothing else
    /// in the program has to know they are special.
    ladder_runs: Vec<LadderRun>,
    /// The fights a comparison is waiting to be built from, in the order they
    /// were ticked, while the ones the analyzer holds are being fetched.
    pending_compare: Vec<NaiveDateTime>,
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
            // Folded away, every time. The window opens on the fight it
            // analyzed and the list is one button from it — restoring whatever
            // the list was doing at the last exit only means opening on a
            // window somebody else's session arranged.
            combats_panel: CombatsPanel::new(false),
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
            ladder_runs: Vec::new(),
            pending_compare: Vec::new(),
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
        // What the analysis thread is doing, read once for this frame: the
        // window below says so, and the list of fights is inert while it runs.
        let job = self.state.analysis_handler.job_progress();
        if job.is_running() {
            // Nothing on this side sends anything while a job runs — the
            // thread only reaches the info channel between instructions — so
            // without this the progress would stand still at whatever it read
            // on the frame the last click drew.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.show_job_progress(job, ui.ctx());
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
                    let open_runs = self.open_run_paths();
                    if let Some(run) = self.records.show(
                        ui,
                        frame,
                        &self.state.settings.upload.oscr_url,
                        &mut self.state.settings.general.ladder_window_position,
                        &open_runs,
                    ) {
                        self.open_ladder_run(run);
                    }
                });

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

                    // Beside the button that shows the list, because that
                    // is where a comparison is put together: this only says
                    // what the ticks in it are for.
                    if ui
                        .steady_toggle(self.compare.is_open(), "Compare Combats")
                        .hover("Tick fights in the list beside this to put them side by side.")
                        .clicked()
                    {
                        self.compare.toggle();
                        // The list can be out of date by now — a fight
                        // finished while something else was on screen. Only
                        // the list is refreshed; the combat being viewed
                        // stays.
                        if self.compare.is_open() {
                            self.state.analysis_handler.refresh_combats_list();
                        }
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

                    // The rest of the row is about the one fight on screen,
                    // which a comparison is not.
                    if self.compare.is_open() {
                        return;
                    }

                    ui.separator();

                    if ui
                        .add_enabled(
                            self.selected_combat.is_some(),
                            Button::new("Save Combat 💾"),
                        )
                        .hover("Write the fight on screen out as a log of its own.")
                        .disabled_hover("Open a fight first — this saves the one on screen.")
                        .clicked()
                        && let Some(file) = FileDialog::new()
                            .set_title("Save Combat")
                            .add_filter("log", &["log"])
                            .set_file_name(self.selected_combat.as_ref().unwrap().file_identifier())
                            .set_parent(frame)
                            .save_file()
                    {
                        self.save_shown_combat(file);
                    }

                    self.upload.show(
                        ui,
                        // A run fetched from the ladder is not the reader's to
                        // send anywhere: it is already on the ladder, and it is
                        // somebody else's fight. The upload offers itself for
                        // fights of their own only.
                        self.selected_combat
                            .as_deref()
                            .filter(|_| !self.showing_ladder_run()),
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

                // The list of fights goes down the side, under the toolbar,
                // and whatever is being read takes what is left — a browser's
                // sidebar, not a column beside the whole window.
                //
                // What the comparison on screen is of, so the list can put its
                // numbers, its colours and its players on the rows it was built
                // from. Empty when there is no comparison.
                let slots = self.compare.slots();
                // The runs fetched from the ladder lead the list. They are
                // fights like any other from here on — ticked into a
                // comparison, opened, read — because the program holds them
                // beside the log rather than instead of it.
                let ladder_runs: Vec<CombatSummary> = self
                    .ladder_runs
                    .iter()
                    .map(|run| run.summary.clone())
                    .collect();
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
                    comparing: self.compare.is_open(),
                    comparison: &slots,
                    ladder_runs: &ladder_runs,
                    locked: job.is_running(),
                };
                let action = self
                    .combats_panel
                    .show(view, &mut self.combats_panel_width, ui);
                match action {
                    Some(ListAction::Open(start)) => self.open_combat(start),
                    // Rewrites the log without the fights that were ticked. The
                    // list comes back through the ordinary channel, so nothing
                    // here has to put it right.
                    Some(ListAction::Keep(keep)) => {
                        log::info!(
                            "clearing the log: keeping {} of {} combats",
                            keep.len(),
                            self.combats.len()
                        );
                        self.state.analysis_handler.keep_combats(keep);
                    }
                    // The comparison is of exactly what is ticked, and it is
                    // built again whenever that changes: numbering the runs
                    // from one each time is what keeps a fight unticked and
                    // another ticked from walking the numbers — and their
                    // colours — up and up.
                    Some(ListAction::Compare(picked)) => self.compare_fights(picked),
                    Some(ListAction::DropLadderRun(start)) => self.drop_ladder_run(start),
                    Some(ListAction::ComparePlayer { start, handle }) => {
                        self.compare.set_player(start, &handle)
                    }
                    None => (),
                }

                if self.compare.is_open() {
                    self.compare.show(&mut self.state, ui, frame);
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
        // Same for the handle and the panel's width, and for one more reason:
        // applying the settings replaces the whole settings object with the
        // copy the dialog took when it opened, so a panel dragged in the
        // meantime would be put back the way it was.
        self.state.settings.general.last_detected_handle = self.log_owner.clone();
        self.state.settings.general.combats_panel_width = self.combats_panel_width;
        self.state.settings.save();
    }
}

/// The level a run was fought at, as the list's picker asks for it.
fn difficulty_filter(run: &CombatSummary) -> DifficultyFilter {
    match run.difficulty {
        Some(Difficulty::Normal) => DifficultyFilter::Normal,
        Some(Difficulty::Advanced) => DifficultyFilter::Advanced,
        Some(Difficulty::Elite) => DifficultyFilter::Elite,
        _ => DifficultyFilter::Any,
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
    /// The runs already in the list, so the ladder window can grey out the
    /// magnifier on one of them.
    fn open_run_paths(&self) -> Vec<PathBuf> {
        self.ladder_runs
            .iter()
            .map(|run| run.path.clone())
            .collect()
    }

    /// Asks for a run fetched from the ladder to be read.
    ///
    /// It is read on the analysis thread and comes back as a fight, which is
    /// then a row of the list like any other. The reader's own log is not
    /// touched: looking at somebody else's fight is no reason to put yours
    /// down, and every kind of trouble this used to have — fights that would
    /// not load, a player that could not be picked, a run opening as the one
    /// before it — came of pretending otherwise.
    fn open_ladder_run(&mut self, run: PathBuf) {
        if self.ladder_runs.iter().any(|open| open.path == run) {
            log::info!("ladder: {} is already in the list", run.display());
            return;
        }
        log::info!("ladder: reading {}", run.display());
        self.state.analysis_handler.read_one_log(run);
        // The run lands in the list, so the list is brought out to receive it.
        // Done at the press rather than when the fight arrives: the point is to
        // answer the press, and reading the log takes a moment during which
        // nothing else on screen would say the magnifier had done anything.
        self.combats_panel.open();
    }

    /// Says what the analysis thread is doing while it clears the log, and
    /// offers to call it off where that is still safe.
    ///
    /// A window rather than a modal: the fight already on screen is worth
    /// reading while the log is rewritten, and nothing about reading it is
    /// dangerous. What must not be touched is the list — a fight is asked for
    /// by its place in it — and that is greyed out on its own
    /// ([`CombatsListView::locked`]).
    ///
    /// It has no close button. There is nothing to close: it goes when the work
    /// does, and a window the reader could dismiss would leave them looking at
    /// a list that is still about to change.
    fn show_job_progress(&self, job: job::JobProgress, ctx: &Context) {
        if !job.is_running() {
            return;
        }
        let stopping = self.state.analysis_handler.cancel_requested();
        Window::new("Clearing the log")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.set_min_width(tuning::JOB_WINDOW_WIDTH);
                match job.fraction() {
                    // Counted: how far through the fights being kept.
                    Some(fraction) => {
                        ui.add(
                            ProgressBar::new(fraction)
                                .text(format!("{} of {}", job.done, job.total)),
                        );
                    }
                    // Nothing to count — the file is being written, or read
                    // back in one go. A bar here would be a made-up number.
                    None => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Working…");
                        });
                    }
                }
                ui.label(job.phase.label());
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    // Offered only while nothing has been written yet. Past
                    // that the log is being replaced and there is no half way
                    // to stop at, so the button says why rather than
                    // disappearing and leaving the reader looking for it.
                    let can_cancel = job.phase.can_cancel() && !stopping;
                    if ui
                        .add_enabled(can_cancel, Button::new("Cancel"))
                        .hover("Stop, and leave the log exactly as it is.")
                        .disabled_hover(match stopping {
                            true => "Stopping.",
                            false => {
                                "The log is already being written; this cannot be stopped \
                                      part way."
                            }
                        })
                        .clicked()
                    {
                        log::info!("clearing the log: cancel asked for");
                        self.state.analysis_handler.cancel_job();
                    }
                    if stopping {
                        ui.label(RichText::new("Stopping…").weak());
                    }
                });
            });
    }

    /// Takes a run out of the list again.
    fn drop_ladder_run(&mut self, start: NaiveDateTime) {
        self.ladder_runs.retain(|run| run.summary.start != start);
        // Whatever it was in is a comparison of fights the list no longer
        // holds.
        self.compare.forget();
    }

    /// Puts a fight on screen, wherever it came from.
    ///
    /// One of the reader's own is asked of the analyzer, which holds it; a run
    /// from the ladder is held here and needs no asking.
    fn open_combat(&mut self, start: NaiveDateTime) {
        if let Some(run) = self
            .ladder_runs
            .iter()
            .find(|run| run.summary.start == start)
        {
            let combat = run.combat.clone();
            self.main_tabs.update(&self.state.settings, &combat);
            self.selected_combat = Some(combat);
            // Nothing in the analyzer's log is what is on screen now, and the
            // things that ask by that index read this as "not one of yours".
            self.selected_combat_index = None;
            return;
        }
        let Some(index) = self.combats.iter().position(|c| c.start == start) else {
            return;
        };
        self.selected_combat_index = Some(index);
        self.state.analysis_handler.get_combat(index);
    }

    /// Whether what is on screen came off the ladder rather than out of the
    /// reader's own log.
    fn showing_ladder_run(&self) -> bool {
        self.selected_combat.as_ref().is_some_and(|combat| {
            self.ladder_runs
                .iter()
                .any(|run| run.summary.start == combat.active_time.start)
        })
    }

    /// Writes the fight on screen out as a log of its own.
    ///
    /// One of the reader's own is cut out of their log by the analyzer, which
    /// knows where in it the fight sits. A run from the ladder is already a log
    /// of exactly one fight — the file it was fetched into — so it is copied.
    fn save_shown_combat(&self, file: PathBuf) {
        let Some(shown) = self.selected_combat.as_ref() else {
            return;
        };
        if let Some(run) = self
            .ladder_runs
            .iter()
            .find(|run| run.summary.start == shown.active_time.start)
        {
            match std::fs::copy(&run.path, &file) {
                Ok(_) => log::info!("saved the run to {}", file.display()),
                Err(error) => log::error!("could not save the run: {error}"),
            }
            return;
        }
        if let Some(index) = self.selected_combat_index {
            self.state.analysis_handler.save_combat(index, file);
        }
    }

    /// Builds a comparison of the ticked fights, in the order they were ticked.
    ///
    /// The runs from the ladder are here already; the reader's own have to be
    /// asked of the analyzer, and that answer arrives later — so what was
    /// ticked is remembered until it does, and the two are put in order then.
    fn compare_fights(&mut self, picked: Vec<NaiveDateTime>) {
        if picked.len() < 2 {
            self.pending_compare.clear();
            self.compare.forget();
            return;
        }
        let mine: Vec<usize> = picked
            .iter()
            .filter_map(|start| self.combats.iter().position(|c| c.start == *start))
            .collect();
        self.pending_compare = picked;
        if mine.is_empty() {
            // Nothing to wait for: every one of them is a run already in hand.
            self.build_comparison(Vec::new());
            return;
        }
        self.state.analysis_handler.get_combats(mine);
    }

    /// Puts the fetched fights and the runs from the ladder into one
    /// comparison, in the order they were ticked.
    fn build_comparison(&mut self, fetched: Vec<(usize, Arc<Combat>)>) {
        let combats: Vec<(usize, Arc<Combat>)> = self
            .pending_compare
            .iter()
            .enumerate()
            .filter_map(|(order, start)| {
                let combat = self
                    .ladder_runs
                    .iter()
                    .find(|run| run.summary.start == *start)
                    .map(|run| run.combat.clone())
                    .or_else(|| {
                        fetched
                            .iter()
                            .find(|(_, combat)| combat.active_time.start == *start)
                            .map(|(_, combat)| combat.clone())
                    })?;
                Some((order, combat))
            })
            .collect();
        if combats.len() < 2 {
            self.compare.forget();
            return;
        }
        self.compare.set_combats(combats, &self.state.settings);
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
                AnalysisInfo::Combats(combats) => self.build_comparison(combats),
                // A run fetched from the ladder, read on its own. It joins the
                // list as a fight like any other; the log the analyzer holds
                // was never touched.
                AnalysisInfo::OneLog { path, combat } => {
                    let Some(combat) = combat else {
                        continue;
                    };
                    let summary = combat.summary();
                    log::info!("ladder: {} is {}", path.display(), summary.identifier);
                    // Where a comparison would nearly always be pointed: the
                    // reader's own runs of the same map and level.
                    self.combats_panel.suggest_filter(
                        Some(summary.base_name.clone()),
                        difficulty_filter(&summary),
                    );
                    self.ladder_runs.retain(|run| run.path != path);
                    self.ladder_runs.push(LadderRun {
                        path,
                        combat,
                        summary,
                    });
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
