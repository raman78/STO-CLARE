use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::{Instant, SystemTime},
};

use chrono::{Duration, NaiveDateTime};
use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};
use eframe::egui::{Context, ViewportId};
use log::info;
use notify::{RecommendedWatcher, Watcher, recommended_watcher};

use crate::{
    analyzer::{Analyzer, Combat, Difficulty, settings::AnalysisSettings},
    unwrap_or_return,
};

use super::log_consolidation::Consolidator;

pub struct AnalysisHandler {
    tx: Sender<Instruction>,
    rx: Receiver<AnalysisInfo>,
    is_busy: Arc<AtomicBool>,
    id: u32,
    id_counter: Arc<AtomicU32>,
}

struct AnalysisContext {
    instruction_rx: Receiver<Instruction>,
    instruction_tx: Sender<Instruction>,
    handlers: Vec<HandlerContext>,
    analyzer: Option<Analyzer>,
    ctx: Context,
    is_busy: Arc<AtomicBool>,
    auto_refresh_interval: Duration,
    auto_refresh: Option<AutoRefreshContext>,
    // Size of the log at the last refresh we notified about; used to skip
    // no-op refreshes so auto refresh doesn't rebuild the view when nothing
    // changed. Reset whenever the analyzer is (re)built.
    last_file_size: Option<u64>,
    // Merges STO's rotating logs into one file the analyzer reads (see
    // set_analyzer); None when disabled or nothing to merge.
    consolidator: Option<Consolidator>,
}

#[derive(Debug)]
struct HandlerContext {
    tx: Sender<AnalysisInfo>,
    auto_refresh: bool,
    id: u32,
    viewport: ViewportId,
}

struct AutoRefreshContext {
    _watcher: RecommendedWatcher,
    state: AutoRefreshState,
    interval: Duration,
    last_refresh: SystemTime,
}

enum AutoRefreshState {
    Idle,
    /// The log changed less than one interval ago, so the refresh waits until
    /// this moment. The run loop waits for it on the instruction channel, which
    /// is also what cancels it: anything that refreshes in the meantime puts the
    /// state back to `Idle` and the deadline is simply forgotten.
    RefreshDue(Instant),
}

enum Instruction {
    Refresh(bool),
    // Re-read the log but only send back the combats list to the requesting
    // handler, so the main view's currently-open combat is left untouched.
    RefreshCombatsList(u32),
    AutoRefresh,
    GetCombat(usize, u32),
    GetCombats(Vec<usize>, u32),
    KeepCombats(Vec<usize>),
    SaveCombat(usize, PathBuf),
    EnableAutoRefresh(bool, u32),
    SetAutoRefreshInterval(f64),
    AddHandler(HandlerContext),
    RemoveHandler(u32),
    SetSettings(Arc<AnalysisSettings>),
}

#[derive(Clone)]
pub enum AnalysisInfo {
    Combat(Arc<Combat>),
    // Several combats fetched together for the compare view, each with its
    // index in the combats list so the caller can tell them apart.
    Combats(Vec<(usize, Arc<Combat>)>),
    Refreshed {
        latest_combat: Arc<Combat>,
        combats: Vec<String>,
        /// Detected difficulty per combat, aligned with `combats`, for the
        /// compare view's difficulty filter.
        difficulties: Vec<Option<Difficulty>>,
        /// Each combat's name without the environment and difficulty suffixes,
        /// aligned with `combats`. The views group and filter by it instead of
        /// picking it back out of the formatted identifier.
        base_names: Vec<String>,
        /// Each combat's environment ("Space" / "Ground" / …) where the map was
        /// recognized, aligned with `combats`.
        environments: Vec<Option<String>>,
        /// Each combat's start time, aligned with `combats`. The views key the
        /// user's own note by it.
        start_times: Vec<NaiveDateTime>,
        file_size: Option<u64>,
    },
    // Like `Refreshed`, but without a combat to switch the main view to. Used
    // by the "Clear Log File" dialog and by the compare view, so both refresh
    // the list without moving off the combat being viewed. Carries the same
    // per-combat metadata as `Refreshed`, because the filters read it by index
    // and a list that grew past it would leave the new combats unlabelled.
    CombatsListRefreshed {
        combats: Vec<String>,
        difficulties: Vec<Option<Difficulty>>,
        base_names: Vec<String>,
        environments: Vec<Option<String>>,
        start_times: Vec<NaiveDateTime>,
        file_size: Option<u64>,
    },
    RefreshError,
}

impl AnalysisHandler {
    pub fn new(
        settings: AnalysisSettings,
        ctx: Context,
        auto_refresh_interval_seconds: f64,
        enable_auto_refresh: bool,
    ) -> Self {
        let (instruction_tx, instruction_rx) = unbounded();
        let (info_tx, info_rx) = unbounded();
        let is_busy = Arc::new(AtomicBool::new(false));
        let handler_ctx = HandlerContext {
            auto_refresh: enable_auto_refresh,
            id: 0,
            tx: info_tx,
            viewport: ViewportId::ROOT,
        };

        let mut analysis_context = AnalysisContext::new(
            instruction_rx,
            handler_ctx,
            instruction_tx.clone(),
            settings,
            ctx,
            is_busy.clone(),
            auto_refresh_interval_seconds,
        );
        std::thread::spawn(move || {
            analysis_context.run();
        });
        Self {
            tx: instruction_tx,
            rx: info_rx,
            is_busy,
            id: 0,
            id_counter: AtomicU32::new(1).into(),
        }
    }

    pub fn is_busy(&self) -> bool {
        self.is_busy.load(Ordering::Relaxed)
    }

    pub fn check_for_info(&self) -> impl Iterator<Item = AnalysisInfo> + '_ {
        self.rx.try_iter()
    }

    pub fn refresh(&self) {
        self.tx.send(Instruction::Refresh(false)).unwrap();
    }

    /// Re-reads the log and sends back only the refreshed combats list to this
    /// handler, leaving the currently-viewed combat in place.
    pub fn refresh_combats_list(&self) {
        self.tx
            .send(Instruction::RefreshCombatsList(self.id))
            .unwrap();
    }

    pub fn get_combat(&self, combat_index: usize) {
        self.tx
            .send(Instruction::GetCombat(combat_index, self.id))
            .unwrap();
    }

    /// Fetch several combats at once (for the compare view). They come back as a
    /// single `AnalysisInfo::Combats` on this handler.
    pub fn get_combats(&self, combat_indices: Vec<usize>) {
        self.tx
            .send(Instruction::GetCombats(combat_indices, self.id))
            .unwrap();
    }

    /// Rewrites the log to keep only the combats at `keep` (dropping the rest).
    pub fn keep_combats(&self, keep: Vec<usize>) {
        self.tx.send(Instruction::KeepCombats(keep)).unwrap();
    }

    pub fn save_combat(&self, combat_index: usize, file: PathBuf) {
        self.tx
            .send(Instruction::SaveCombat(combat_index, file))
            .unwrap();
    }

    pub fn set_settings(&self, settings: AnalysisSettings) {
        self.tx
            .send(Instruction::SetSettings(settings.into()))
            .unwrap();
    }

    pub fn enable_auto_refresh(&self, enable: bool) {
        self.tx
            .send(Instruction::EnableAutoRefresh(enable, self.id))
            .unwrap();
    }

    pub fn set_auto_refresh_interval(&self, refresh_interval: f64) {
        self.tx
            .send(Instruction::SetAutoRefreshInterval(refresh_interval))
            .unwrap();
    }

    pub fn get_handler(&self, auto_refresh: bool, viewport: ViewportId) -> Self {
        let (tx, rx) = unbounded();
        let id = self.id_counter.fetch_add(1, Ordering::Relaxed);
        let ctx = HandlerContext {
            auto_refresh,
            id,
            tx,
            viewport,
        };
        self.tx.send(Instruction::AddHandler(ctx)).unwrap();
        Self {
            tx: self.tx.clone(),
            rx,
            is_busy: self.is_busy.clone(),
            id,
            id_counter: self.id_counter.clone(),
        }
    }
}

impl Drop for AnalysisHandler {
    fn drop(&mut self) {
        let _ = self.tx.send(Instruction::RemoveHandler(self.id));
    }
}

impl AnalysisContext {
    fn new(
        instruction_rx: Receiver<Instruction>,
        handler_ctx: HandlerContext,
        instruction_tx: Sender<Instruction>,
        settings: AnalysisSettings,
        ctx: Context,
        is_busy: Arc<AtomicBool>,
        auto_refresh_interval_seconds: f64,
    ) -> Self {
        let mut _self = Self {
            instruction_rx,
            instruction_tx,
            handlers: vec![handler_ctx],
            analyzer: None,
            ctx,
            is_busy,
            auto_refresh_interval: AutoRefreshContext::interval(auto_refresh_interval_seconds),
            auto_refresh: None,
            last_file_size: None,
            consolidator: None,
        };
        _self.set_analyzer(settings);
        _self.update_auto_refresh();
        // Load the combats list on startup without waiting for the user to hit
        // Refresh. It is queued so it runs on the analysis thread (the window
        // stays responsive); by the time it computes the list, the initial
        // consolidation pass (in set_analyzer, plus the one at the start of
        // try_refresh) has finished, so the merged combatlog.log is only shown
        // once consolidation is done.
        if _self.analyzer.is_some() {
            let _ = _self.instruction_tx.send(Instruction::Refresh(false));
        }
        _self
    }

    /// Builds the analyzer for `settings`, always reading the configured file
    /// directly (so opening a specific log shows exactly that file). On Linux
    /// with consolidation enabled, a background merge keeps a single
    /// combatlog.log up to date so the user can point at it for the live
    /// overlay / all-combats view; the file being read is left untouched.
    fn set_analyzer(&mut self, settings: AnalysisSettings) {
        {
            let dir = std::path::Path::new(&settings.combatlog_file)
                .parent()
                .filter(|dir| dir.is_dir());
            if let (true, Some(dir)) = (settings.consolidate_combatlog, dir) {
                let mut consolidator = Consolidator::new(dir);
                consolidator.tick(Some(std::path::Path::new(&settings.combatlog_file)));
                self.consolidator = Some(consolidator);
            } else {
                self.consolidator = None;
            }
        }
        self.analyzer = Analyzer::new(settings);
        // Force the next refresh to notify: the file (and its size) just changed.
        self.last_file_size = None;
    }

    fn run(&mut self) {
        loop {
            let instruction = match self.refresh_due_at() {
                // A refresh is waiting for its interval to pass: take whichever
                // comes first, an instruction or that moment.
                Some(deadline) => match self.instruction_rx.recv_deadline(deadline) {
                    Ok(i) => i,
                    Err(RecvTimeoutError::Timeout) => {
                        self.refresh(true);
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                },
                None => match self.instruction_rx.recv() {
                    Ok(i) => i,
                    Err(_) => return,
                },
            };

            match instruction {
                Instruction::Refresh(auto_refresh) => self.refresh(auto_refresh),
                Instruction::RefreshCombatsList(handler) => self.refresh_combats_list(handler),
                Instruction::AutoRefresh => self.auto_refresh(),
                Instruction::GetCombat(combat_index, handler) => {
                    self.get_combat(combat_index, handler);
                }
                Instruction::GetCombats(combat_indices, handler) => {
                    self.get_combats(combat_indices, handler);
                }
                Instruction::KeepCombats(keep) => self.keep_combats(keep),
                Instruction::SaveCombat(combat_index, file) => self.save_combat(combat_index, file),
                Instruction::EnableAutoRefresh(enable, handler) => {
                    self.handler_mut(handler, |h| h.auto_refresh = enable);
                    self.update_auto_refresh();
                    if enable {
                        // Show the latest combat right away on the just-enabled
                        // handler (e.g. the overlay), instead of only after the
                        // next refresh.
                        if let info @ AnalysisInfo::Refreshed { .. } = self.latest_info() {
                            self.send_info(info, handler);
                        }
                    }
                }
                Instruction::SetAutoRefreshInterval(refresh_interval) => {
                    self.set_auto_refresh_interval(refresh_interval)
                }
                Instruction::AddHandler(tx) => {
                    self.handlers.push(tx);
                    self.update_auto_refresh();
                }
                Instruction::RemoveHandler(id) => {
                    if let Some(index) = self.handlers.iter().position(|t| t.id == id) {
                        self.handlers.remove(index);
                        if self.handlers.is_empty() {
                            return;
                        }
                        self.update_auto_refresh();
                    }
                }
                Instruction::SetSettings(settings) => {
                    self.set_analyzer(Arc::into_inner(settings).unwrap());
                    self.update_auto_refresh();
                }
            }

            Self::set_is_busy(&self.is_busy, false);
        }
    }

    fn refresh(&mut self, only_when_auto_refresh: bool) {
        Self::set_is_busy(&self.is_busy, true);
        // An automatic (live) refresh may skip no-op events so it does not
        // collapse an expanded breakdown; a manual refresh (Refresh Now /
        // reload) always rebuilds the list so every combat shows up.
        if let Some(info) = self.try_refresh(only_when_auto_refresh) {
            if only_when_auto_refresh {
                for handler in self.handlers.iter().filter(|h| h.auto_refresh) {
                    handler.send(info.clone(), &self.ctx);
                }
            } else {
                self.send_info_all(info);
            }
        }
        if let Some(ctx) = &mut self.auto_refresh {
            ctx.state = AutoRefreshState::Idle;
            ctx.last_refresh = SystemTime::now();
        }
    }

    /// Runs a consolidation pass and re-parses the log. When
    /// `skip_when_unchanged` is set (automatic refreshes), returns `None` if the
    /// log has not grown since the last notified refresh, so the view is not
    /// rebuilt (which would collapse expanded damage trees) for nothing. A
    /// manual refresh passes `false` so it always rebuilds the combats list.
    /// Re-reads the log (like a manual refresh) but sends only the combats list
    /// back to the requesting handler, so the main view keeps showing the combat
    /// it was on. Reuses `try_refresh` for the actual re-read.
    fn refresh_combats_list(&mut self, handler: u32) {
        Self::set_is_busy(&self.is_busy, true);
        if let Some(info) = self.try_refresh(false) {
            let info = match info {
                AnalysisInfo::Refreshed {
                    combats,
                    difficulties,
                    base_names,
                    environments,
                    start_times,
                    file_size,
                    ..
                } => AnalysisInfo::CombatsListRefreshed {
                    combats,
                    difficulties,
                    base_names,
                    environments,
                    start_times,
                    file_size,
                },
                other => other,
            };
            self.send_info(info, handler);
        }
    }

    fn try_refresh(&mut self, skip_when_unchanged: bool) -> Option<AnalysisInfo> {
        // Keep combatlog.log current in the background, but never touch the file
        // currently being read (the user may have opened a specific old log).
        {
            let protected = self
                .analyzer
                .as_ref()
                .map(|a| a.settings().combatlog_file().to_path_buf());
            if let Some(consolidator) = self.consolidator.as_mut() {
                consolidator.tick(protected.as_deref());
            }
        }
        match self.analyzer.as_mut() {
            Some(a) => a.update(),
            None => {
                self.last_file_size = None;
                return Some(AnalysisInfo::RefreshError);
            }
        }
        let size = self
            .analyzer
            .as_ref()
            .and_then(|a| std::fs::metadata(&a.settings().combatlog_file).ok())
            .map(|m| m.len());
        // Nothing new since the last notified refresh: for an automatic refresh,
        // skip so the view is not rebuilt (which would collapse any expanded tree
        // the user opened). A manual refresh always rebuilds.
        if skip_when_unchanged && size.is_some() && size == self.last_file_size {
            return None;
        }
        self.last_file_size = size;
        Some(self.latest_info())
    }

    /// Builds a `Refreshed` from the analyzer's current results without
    /// re-reading the log; `RefreshError` if nothing is loaded yet.
    fn latest_info(&self) -> AnalysisInfo {
        let analyzer = match self.analyzer.as_ref() {
            Some(a) => a,
            None => return AnalysisInfo::RefreshError,
        };
        let latest_combat = match analyzer.result().last() {
            Some(c) => c.clone(),
            None => return AnalysisInfo::RefreshError,
        };
        AnalysisInfo::Refreshed {
            latest_combat: latest_combat.into(),
            combats: analyzer.result().iter().map(|c| c.identifier()).collect(),
            difficulties: analyzer
                .result()
                .iter()
                .map(|c| c.detected_difficulty)
                .collect(),
            base_names: analyzer.result().iter().map(|c| c.base_name()).collect(),
            environments: analyzer
                .result()
                .iter()
                .map(|c| c.detected_combat_type.clone())
                .collect(),
            start_times: analyzer
                .result()
                .iter()
                .map(|c| c.active_time.start)
                .collect(),
            file_size: std::fs::metadata(&analyzer.settings().combatlog_file)
                .ok()
                .map(|m| m.len()),
        }
    }

    fn auto_refresh(&mut self) {
        if let Some(ctx) = &mut self.auto_refresh {
            if let AutoRefreshState::RefreshDue(_) = ctx.state {
                return;
            }

            let delta_time = match ctx.last_refresh.elapsed().map(Duration::from_std) {
                Ok(Ok(t)) => t,
                err => {
                    info!("failed to retrieve elapsed time {:?}", err);
                    return;
                }
            };

            if delta_time >= ctx.interval {
                ctx.state = AutoRefreshState::Idle;
                self.refresh(true);
                return;
            }

            let delay = (ctx.interval - delta_time).to_std().unwrap_or_default();
            ctx.state = AutoRefreshState::RefreshDue(Instant::now() + delay);
        }
    }

    fn get_combat(&self, combat_index: usize, handler: u32) {
        let analyzer = match &self.analyzer {
            Some(a) => a,
            None => return,
        };

        let combat = match analyzer.result().get(combat_index) {
            Some(c) => c.clone(),
            None => return,
        };

        self.send_info(AnalysisInfo::Combat(combat.into()), handler);
    }

    fn get_combats(&self, combat_indices: Vec<usize>, handler: u32) {
        let analyzer = match &self.analyzer {
            Some(a) => a,
            None => return,
        };

        let combats: Vec<(usize, Arc<Combat>)> = combat_indices
            .into_iter()
            .filter_map(|i| analyzer.result().get(i).map(|c| (i, Arc::new(c.clone()))))
            .collect();

        if combats.is_empty() {
            return;
        }

        self.send_info(AnalysisInfo::Combats(combats), handler);
    }

    /// Rewrites the log so it keeps only the combats at `keep` (their byte
    /// ranges), dropping the rest. On Linux the background consolidator is left
    /// intact: its persisted offset means the deleted combats are not merged
    /// back in from the still-growing active log, and the active file (which STO
    /// keeps open) is never touched.
    fn keep_combats(&mut self, mut keep: Vec<usize>) {
        let analyzer = match &self.analyzer {
            Some(a) => a,
            None => return,
        };
        let settings = analyzer.settings().clone();

        keep.sort_unstable();
        keep.dedup();
        let mut data = Vec::new();
        for &index in &keep {
            match analyzer
                .result()
                .get(index)
                .and_then(|combat| combat.read_log_combat_data(settings.combatlog_file()))
            {
                Some(bytes) => data.extend_from_slice(&bytes),
                None => {
                    // Abort rather than risk dropping a combat the user wanted
                    // to keep; leave the log untouched.
                    log::error!(
                        "aborting combat deletion: could not read combat {index} from the log"
                    );
                    return;
                }
            }
        }

        self.analyzer = None;
        if let Err(e) = rewrite_file(settings.combatlog_file(), &data) {
            log::error!("failed to rewrite combat log while deleting combats: {e}");
        }
        self.set_analyzer(settings);
        // The rewrite replaces the file (new inode), so the auto-refresh watcher
        // must be re-created or it would keep watching the old, deleted file.
        self.update_auto_refresh();
        self.refresh(false);
    }

    fn save_combat(&self, combat_index: usize, file: PathBuf) {
        let analyzer = unwrap_or_return!(&self.analyzer);
        let combat = unwrap_or_return!(analyzer.result().get(combat_index));
        Self::set_is_busy(&self.is_busy, true);
        let combat_data = match combat.read_log_combat_data(analyzer.settings().combatlog_file()) {
            Some(d) => d,
            None => {
                Self::set_is_busy(&self.is_busy, false);
                return;
            }
        };
        let _ = std::fs::write(file, combat_data.as_slice());
        Self::set_is_busy(&self.is_busy, false);
    }

    fn send_info(&self, info: AnalysisInfo, handler: u32) {
        self.handler(handler, |handler| handler.send(info, &self.ctx));
    }

    fn send_info_all(&self, info: AnalysisInfo) {
        for handler in self.handlers.iter() {
            handler.send(info.clone(), &self.ctx);
        }
    }

    fn handler(&self, handler: u32, action: impl FnOnce(&HandlerContext)) {
        if let Some(handler) = self.handlers.iter().find(|h| h.id == handler) {
            action(handler);
        }
    }

    fn handler_mut(&mut self, handler: u32, action: impl FnOnce(&mut HandlerContext)) {
        if let Some(handler) = self.handlers.iter_mut().find(|h| h.id == handler) {
            action(handler);
        }
    }

    fn set_is_busy(is_busy: &AtomicBool, value: bool) {
        is_busy.store(value, Ordering::Relaxed);
    }

    fn set_auto_refresh_interval(&mut self, refresh_interval: f64) {
        self.auto_refresh_interval = AutoRefreshContext::interval(refresh_interval);
        self.update_auto_refresh();
    }

    fn update_auto_refresh(&mut self) {
        let file = match &self.analyzer {
            Some(analyzer) => PathBuf::from(&analyzer.settings().combatlog_file),
            None => return,
        };
        if !self.auto_refresh_enabled() {
            self.auto_refresh = None;
            return;
        }
        // With consolidation running, the game writes to a rotating combatlog_*
        // file rather than the combatlog.log we read, so those writes would
        // never change the watched file. Watch the whole folder instead, so a
        // game write still triggers a refresh (which consolidates + reparses).
        let watch_dir = self.consolidator.is_some();
        self.auto_refresh = AutoRefreshContext::new(
            self.instruction_tx.clone(),
            self.auto_refresh_interval,
            &file,
            watch_dir,
        );
    }

    /// When the next automatic refresh is due, if one is waiting.
    fn refresh_due_at(&self) -> Option<Instant> {
        match self.auto_refresh.as_ref()?.state {
            AutoRefreshState::Idle => None,
            AutoRefreshState::RefreshDue(deadline) => Some(deadline),
        }
    }

    fn auto_refresh_enabled(&self) -> bool {
        self.handlers.iter().any(|h| h.auto_refresh)
    }
}

impl AutoRefreshContext {
    fn new(
        tx: Sender<Instruction>,
        interval: Duration,
        file: &Path,
        watch_dir: bool,
    ) -> Option<Self> {
        let tx_watcher = tx;
        // When watching the whole folder, the game's logs dir holds unrelated
        // files too, so only react to combat log files. When watching a single
        // file every event on it is already relevant.
        let mut watcher = recommended_watcher(move |res: notify::Result<notify::Event>| {
            let relevant = match res {
                Ok(event) => !watch_dir || event.paths.iter().any(|p| is_combatlog_path(p)),
                Err(_) => false,
            };
            if relevant {
                let _ = tx_watcher.send(Instruction::AutoRefresh);
            }
        })
        .ok()?;

        let target = if watch_dir {
            file.parent().unwrap_or_else(|| Path::new("."))
        } else {
            file
        };
        watcher
            .watch(target, notify::RecursiveMode::NonRecursive)
            .ok()?;

        Some(Self {
            state: AutoRefreshState::Idle,
            interval,
            _watcher: watcher,
            last_refresh: SystemTime::now(),
        })
    }

    fn interval(interval_seconds: f64) -> Duration {
        Duration::milliseconds((interval_seconds * 1.0e3) as _)
    }
}

impl HandlerContext {
    fn send(&self, info: AnalysisInfo, ctx: &Context) {
        if self.tx.send(info).is_ok() {
            ctx.request_repaint_of(self.viewport)
        }
    }
}

/// Whether `path` is a combat log (the consolidated `combatlog.log` or a
/// rotating `combatlog_<timestamp>.log`), used to filter folder watch events.
fn is_combatlog_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(|n| n.to_lowercase())
        .is_some_and(|n| n.starts_with("combatlog") && n.ends_with(".log"))
}

/// Atomically replaces `path`'s contents with `data` (write to a sibling temp
/// file, flush and sync, then rename over the target) so a crash mid-write
/// cannot leave a truncated log behind.
fn rewrite_file(path: &Path, data: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let tmp = dir.join(".cla-log-rewrite.tmp");
    {
        let mut file = File::create(&tmp)?;
        file.write_all(data)?;
        file.flush()?;
        file.sync_data()?;
    }
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The app should populate the combats list on startup on its own (no manual
    /// Refresh). Creates a handler exactly like the app does and waits for the
    /// automatic first refresh to deliver the combats. Point
    /// `CLA_TEST_COMBATLOG` at a real combatlog.log to run it.
    #[test]
    #[ignore = "reads a real STO log"]
    fn loads_combats_on_startup() {
        // Work on a copy in a scratch dir: pointing the handler at the real log
        // folder would let its consolidator touch the real files.
        let Some(src) = std::env::var_os("CLA_TEST_COMBATLOG") else {
            println!("set CLA_TEST_COMBATLOG to a combatlog.log to run this test");
            return;
        };
        let dir = std::env::temp_dir().join(format!("cla-startup-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let work = dir.join("combatlog.log");
        std::fs::copy(src, &work).unwrap();

        let settings = AnalysisSettings {
            combatlog_file: work.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let handler = AnalysisHandler::new(settings, Context::default(), 1.0, false);

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            if let Some(info) = handler.check_for_info().last() {
                match info {
                    AnalysisInfo::Refreshed { combats, .. } => {
                        assert!(!combats.is_empty(), "startup refresh produced no combats");
                        println!("startup loaded {} combats", combats.len());
                        let _ = std::fs::remove_dir_all(&dir);
                        return;
                    }
                    AnalysisInfo::RefreshError => panic!("startup refresh errored"),
                    AnalysisInfo::Combat(_)
                    | AnalysisInfo::Combats(_)
                    | AnalysisInfo::CombatsListRefreshed { .. } => {}
                }
            }
            assert!(
                std::time::Instant::now() <= deadline,
                "no startup refresh within timeout"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    /// The delayed half of automatic refreshing. A change that lands inside the
    /// interval must not refresh at once — it sets a deadline the run loop waits
    /// for on the instruction channel — and one deadline is kept however many
    /// changes arrive, until something refreshes and clears it.
    #[test]
    fn a_change_inside_the_interval_waits_for_one_deadline() {
        let dir = std::env::temp_dir().join(format!("cla-refresh-due-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("combatlog.log");
        std::fs::write(
            &log,
            "26:07:23:20:00:00.0::Kestrel,P[1@2 Kestrel@handle],,*,Target,C[1 Npc_Foo],Phaser Beam,Pn.abc,Phaser,,100,100\n",
        )
        .unwrap();

        let settings = AnalysisSettings {
            combatlog_file: log.to_string_lossy().into_owned(),
            ..Default::default()
        };
        let (instruction_tx, instruction_rx) = unbounded();
        let (main_tx, _main_rx) = unbounded();
        let mut context = AnalysisContext::new(
            instruction_rx,
            HandlerContext {
                tx: main_tx,
                auto_refresh: true,
                id: 0,
                viewport: ViewportId::ROOT,
            },
            instruction_tx,
            settings,
            Context::default(),
            Arc::new(AtomicBool::new(false)),
            // A whole minute, so the change below is well inside the interval
            // and the deadline cannot pass while the test runs.
            60.0,
        );
        context.update_auto_refresh();
        assert!(
            context.auto_refresh.is_some(),
            "the watcher context must exist for the rest of this test to mean anything"
        );
        assert_eq!(None, context.refresh_due_at(), "nothing is waiting yet");

        // A refresh happens, then the log changes right after it.
        context.refresh(false);
        context.auto_refresh();
        let deadline = context
            .refresh_due_at()
            .expect("a change inside the interval must leave a refresh waiting");
        assert!(deadline > Instant::now(), "the deadline is in the future");

        // More changes while one is already waiting keep the same deadline
        // rather than piling up.
        context.auto_refresh();
        context.auto_refresh();
        assert_eq!(
            Some(deadline),
            context.refresh_due_at(),
            "one deadline, not a queue of them"
        );

        // Anything that refreshes in the meantime cancels it.
        context.refresh(false);
        assert_eq!(
            None,
            context.refresh_due_at(),
            "a refresh clears the waiting one"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Regression: an automatic (live) refresh feeds only the overlay and
    /// advances the "did the log grow" marker. A later manual refresh must still
    /// rebuild the main window's combat list so every combat shows up —
    /// otherwise the list gets stuck at the last combat it saw while the overlay
    /// moves on to the current fight.
    #[test]
    fn manual_refresh_rebuilds_list_after_auto_refresh_consumed_change() {
        let dir =
            std::env::temp_dir().join(format!("cla-refresh-gate-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("combatlog.log");

        // One record per combat, far enough apart to stay separate (default
        // separation is 90s); the target is the player so the combat has data.
        let combat = |hh: u32, mm: u32| {
            format!(
                "26:07:23:{hh:02}:{mm:02}:00.0::Kestrel,P[1@2 Kestrel@handle],,*,Target,C[1 Npc_Foo],Phaser Beam,Pn.abc,Phaser,,100,100\n"
            )
        };
        std::fs::write(&log, format!("{}{}", combat(20, 0), combat(20, 5))).unwrap();

        let settings = AnalysisSettings {
            combatlog_file: log.to_string_lossy().into_owned(),
            ..Default::default()
        };

        // Build the analysis context by hand with two handlers, mirroring the
        // app: the main window (no auto refresh) and the overlay (auto refresh).
        let (instruction_tx, instruction_rx) = unbounded();
        let (main_tx, main_rx) = unbounded();
        let main_handler = HandlerContext {
            tx: main_tx,
            auto_refresh: false,
            id: 0,
            viewport: ViewportId::ROOT,
        };
        let mut context = AnalysisContext::new(
            instruction_rx,
            main_handler,
            instruction_tx,
            settings,
            Context::default(),
            Arc::new(AtomicBool::new(false)),
            1.0,
        );
        let (overlay_tx, overlay_rx) = unbounded();
        context.handlers.push(HandlerContext {
            tx: overlay_tx,
            auto_refresh: true,
            id: 1,
            viewport: ViewportId::ROOT,
        });

        let latest_count = |rx: &Receiver<AnalysisInfo>| {
            rx.try_iter()
                .filter_map(|info| match info {
                    AnalysisInfo::Refreshed { combats, .. } => Some(combats.len()),
                    _ => None,
                })
                .last()
        };

        // Initial manual refresh: both windows see the two combats.
        context.refresh(false);
        assert_eq!(latest_count(&main_rx), Some(2));
        assert_eq!(latest_count(&overlay_rx), Some(2));

        // A new combat is logged live.
        {
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().append(true).open(&log).unwrap();
            f.write_all(combat(20, 10).as_bytes()).unwrap();
        }

        // Automatic (live) refresh: only the overlay is notified, and the "did
        // it grow" marker advances past the new combat.
        context.refresh(true);
        assert_eq!(
            latest_count(&overlay_rx),
            Some(3),
            "overlay follows the live combat"
        );
        assert_eq!(
            latest_count(&main_rx),
            None,
            "auto refresh leaves the main window alone"
        );

        // Manual refresh (Refresh Now): the main window must now show all three
        // combats even though the log has not grown since the auto refresh.
        context.refresh(false);
        assert_eq!(
            latest_count(&main_rx),
            Some(3),
            "manual refresh must rebuild the list after an auto refresh consumed the change"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The "Clear Log File" dialog refreshes the combats list via
    /// `refresh_combats_list`. That must deliver the list only to the requesting
    /// handler (the main window) as a `CombatsListRefreshed` — never a
    /// `Refreshed`, which would move the main view onto the newest combat, and
    /// never to the overlay.
    #[test]
    fn refresh_combats_list_updates_only_requesting_handler_without_switching_view() {
        let dir =
            std::env::temp_dir().join(format!("cla-list-refresh-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("combatlog.log");

        let combat = |hh: u32, mm: u32| {
            format!(
                "26:07:23:{hh:02}:{mm:02}:00.0::Kestrel,P[1@2 Kestrel@handle],,*,Target,C[1 Npc_Foo],Phaser Beam,Pn.abc,Phaser,,100,100\n"
            )
        };
        std::fs::write(&log, format!("{}{}", combat(20, 0), combat(20, 5))).unwrap();

        let settings = AnalysisSettings {
            combatlog_file: log.to_string_lossy().into_owned(),
            ..Default::default()
        };

        let (instruction_tx, instruction_rx) = unbounded();
        let (main_tx, main_rx) = unbounded();
        let main_handler = HandlerContext {
            tx: main_tx,
            auto_refresh: false,
            id: 0,
            viewport: ViewportId::ROOT,
        };
        let mut context = AnalysisContext::new(
            instruction_rx,
            main_handler,
            instruction_tx,
            settings,
            Context::default(),
            Arc::new(AtomicBool::new(false)),
            1.0,
        );
        let (overlay_tx, overlay_rx) = unbounded();
        context.handlers.push(HandlerContext {
            tx: overlay_tx,
            auto_refresh: true,
            id: 1,
            viewport: ViewportId::ROOT,
        });

        context.refresh_combats_list(0);

        let main_infos: Vec<_> = main_rx.try_iter().collect();
        assert_eq!(main_infos.len(), 1, "main window gets exactly one message");
        match &main_infos[0] {
            AnalysisInfo::CombatsListRefreshed {
                combats,
                difficulties,
                base_names,
                environments,
                ..
            } => {
                assert_eq!(combats.len(), 2, "the refreshed list holds every combat");
                // The views index these three alongside `combats`; a list-only
                // refresh that left them behind would leave every combat added
                // since the last full refresh without an environment, a level
                // or a map name, and so out of a filtered list.
                assert_eq!(difficulties.len(), combats.len());
                assert_eq!(base_names.len(), combats.len());
                assert_eq!(environments.len(), combats.len());
            }
            AnalysisInfo::Refreshed { .. } => {
                panic!("list-only refresh must not send Refreshed (it switches the main view)")
            }
            _ => panic!("expected CombatsListRefreshed"),
        }
        assert!(
            overlay_rx.try_iter().next().is_none(),
            "the overlay is left untouched by a list-only refresh"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recognizes_combatlog_paths() {
        assert!(is_combatlog_path(Path::new("/logs/combatlog.log")));
        assert!(is_combatlog_path(Path::new(
            "/logs/combatlog_2026-07-22_00-00-00.log"
        )));
        assert!(!is_combatlog_path(Path::new("/logs/chat.log")));
        assert!(!is_combatlog_path(Path::new("/logs/.cla-consolidation")));
        assert!(!is_combatlog_path(Path::new("/logs/combatlog.log.tmp")));
    }

    #[test]
    fn rewrite_file_replaces_contents_atomically() {
        let dir = std::env::temp_dir().join(format!("cla-rewrite-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("combatlog.log");

        std::fs::write(&target, b"old contents that are longer").unwrap();
        rewrite_file(&target, b"new").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"new");
        // No temp file is left behind.
        assert!(!dir.join(".cla-log-rewrite.tmp").exists());

        // Rewriting to empty truncates the file.
        rewrite_file(&target, b"").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
