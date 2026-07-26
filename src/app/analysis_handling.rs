use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::SystemTime,
};

use chrono::Duration;
use crossbeam_channel::{Receiver, Sender, unbounded};
use eframe::egui::{Context, ViewportId};
use log::info;
use notify::{RecommendedWatcher, Watcher, recommended_watcher};
use timer::{Guard, Timer};

use crate::{
    analyzer::{Analyzer, Combat, settings::AnalysisSettings},
    unwrap_or_return,
};

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
}

#[derive(Debug)]
struct HandlerContext {
    tx: Sender<AnalysisInfo>,
    auto_refresh: bool,
    id: u32,
    viewport: ViewportId,
}

struct AutoRefreshContext {
    tx: Sender<Instruction>,
    _watcher: RecommendedWatcher,
    timer: Timer,
    state: AutoRefreshState,
    interval: Duration,
    last_refresh: SystemTime,
}

enum AutoRefreshState {
    Idle,
    RefreshScheduled(#[allow(dead_code)] Guard),
}

enum Instruction {
    Refresh(bool),
    AutoRefresh,
    GetCombat(usize, u32),
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
    Refreshed {
        latest_combat: Arc<Combat>,
        combats: Vec<String>,
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

    pub fn get_combat(&self, combat_index: usize) {
        self.tx
            .send(Instruction::GetCombat(combat_index, self.id))
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
            analyzer: Analyzer::new(settings),
            ctx,
            is_busy,
            auto_refresh_interval: AutoRefreshContext::interval(auto_refresh_interval_seconds),
            auto_refresh: None,
            last_file_size: None,
        };
        _self.update_auto_refresh();
        // Load the combats list on startup without waiting for the user to hit
        // Refresh. It is queued so it runs on the analysis thread, keeping the
        // window responsive while the initial parse happens.
        if _self.analyzer.is_some() {
            let _ = _self.instruction_tx.send(Instruction::Refresh(false));
        }
        _self
    }

    fn run(&mut self) {
        loop {
            let instruction = match self.instruction_rx.recv() {
                Ok(i) => i,
                Err(_) => return,
            };

            match instruction {
                Instruction::Refresh(auto_refresh) => self.refresh(auto_refresh),
                Instruction::AutoRefresh => self.auto_refresh(),
                Instruction::GetCombat(combat_index, handler) => {
                    self.get_combat(combat_index, handler);
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
                        if self.handlers.len() == 0 {
                            return;
                        }
                        self.update_auto_refresh();
                    }
                }
                Instruction::SetSettings(settings) => {
                    self.analyzer = Analyzer::new(Arc::into_inner(settings).unwrap());
                    self.update_auto_refresh();
                }
            }

            Self::set_is_busy(&self.is_busy, false);
        }
    }

    fn refresh(&mut self, only_when_auto_refresh: bool) {
        Self::set_is_busy(&self.is_busy, true);
        if let Some(info) = self.try_refresh() {
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

    /// Re-parses the log and returns `None` when it has not changed since the
    /// last notified refresh, so callers do not rebuild the view (which would
    /// collapse expanded damage trees) for nothing.
    fn try_refresh(&mut self) -> Option<AnalysisInfo> {
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
        // Nothing new since the last notified refresh: skip so the view is not
        // rebuilt (which would collapse any expanded tree the user opened).
        if size.is_some() && size == self.last_file_size {
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
            file_size: std::fs::metadata(&analyzer.settings().combatlog_file)
                .ok()
                .map(|m| m.len()),
        }
    }

    fn auto_refresh(&mut self) {
        if let Some(ctx) = &mut self.auto_refresh {
            if let AutoRefreshState::RefreshScheduled(_) = ctx.state {
                return;
            }

            let delta_time = match ctx.last_refresh.elapsed().map(|d| Duration::from_std(d)) {
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

            let delay = ctx.interval - delta_time;
            let tx = ctx.tx.clone();
            let guard = ctx
                .timer
                .schedule_with_delay(delay, move || _ = tx.send(Instruction::Refresh(true)));
            ctx.state = AutoRefreshState::RefreshScheduled(guard);
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

    /// Rewrites the log so it keeps only the combats at `keep` (their byte
    /// ranges), dropping the rest.
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
        self.analyzer = Analyzer::new(settings);
        self.last_file_size = None;
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
        let settings = match &self.analyzer {
            Some(analyzer) => analyzer.settings(),
            None => return,
        };
        if !self.auto_refresh_enabled() {
            self.auto_refresh = None;
            return;
        }
        self.auto_refresh = AutoRefreshContext::new(
            self.instruction_tx.clone(),
            self.auto_refresh_interval,
            &PathBuf::from(&settings.combatlog_file),
        );
    }

    fn auto_refresh_enabled(&self) -> bool {
        self.handlers.iter().any(|h| h.auto_refresh)
    }
}

impl AutoRefreshContext {
    fn new(tx: Sender<Instruction>, interval: Duration, file: &Path) -> Option<Self> {
        let tx_watcher = tx.clone();
        let mut watcher = recommended_watcher(move |_| {
            let _ = tx_watcher.send(Instruction::AutoRefresh);
        })
        .ok()?;

        watcher
            .watch(file, notify::RecursiveMode::NonRecursive)
            .ok()?;

        Some(Self {
            tx,
            timer: Timer::new(),
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
        match self.tx.send(info) {
            Ok(_) => ctx.request_repaint_of(self.viewport),
            Err(_) => (),
        }
    }
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
        // Work on a copy in a scratch dir rather than pointing the handler at
        // the real log.
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
                    AnalysisInfo::Combat(_) => {}
                }
            }
            assert!(
                std::time::Instant::now() <= deadline,
                "no startup refresh within timeout"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
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
