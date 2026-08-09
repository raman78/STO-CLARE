use std::{fs::File, io::Write, path::PathBuf, thread::JoinHandle, time::Duration};

use chrono::DateTime;
use eframe::{Frame, egui::*};
use flate2::write::GzDecoder;
use itertools::{Either, Itertools};
use reqwest::{
    Url,
    blocking::{Client, ClientBuilder},
};
use rustc_hash::FxHashMap;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    app::theme,
    custom_widgets::{
        number_edit::NumberEdit,
        table::{Table, TableRow},
    },
    helpers::number_formatting::NumberFormatter,
};

use super::common::*;
use crate::custom_widgets::toggle::Toggle;

const PAGE_SIZE: i32 = 50;

/// The largest page the server will hand over, whatever is asked for
/// (`core/pagination.py`: `max_page_size`). Asking for more is not an error —
/// it is capped in silence, which is the part worth guarding against.
const MAX_PAGE_SIZE: i32 = 10_000;

/// Every request to the ladder is made with this.
///
/// Two things it settles. It names us: a server operator reading their logs can
/// see which program is calling and, if it ever misbehaves, has something to
/// block or somebody to tell — the default is an anonymous library string that
/// says nothing. And it bounds the wait, so a connection that never answers
/// cannot leave a thread and a window hanging for good, which is also how a
/// stalled client turns into a pile of stalled clients.
fn ladder_client() -> Client {
    ClientBuilder::new()
        .user_agent(concat!("sto-clare/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap()
}

/// What a table says about the runs in it, for naming them.
#[derive(Clone)]
struct LadderFacts {
    map: String,
    space: bool,
    difficulty: LadderDifficulty,
    solo: bool,
}

/// A run from the ladder, named the way the program names a combat of its own:
/// `[Solo] [TFO] Hive Onslaught (Space) [Elite]`.
///
/// Whether it was solo is the server's own test — a run enters a solo table only
/// where the log held exactly one player (`combatlog/models/combatlog.py`:
/// `if ladder.is_solo and len(players) != 1: continue`) — so being in one is
/// decisive and being in a team table says nothing, since every run is in those.
///
/// The ladder keeps the pieces apart — the map on the table, the level and
/// whether it was solo on which tables the run was entered into — and spells
/// some maps differently from us ("Hive: Onslaught" against our "Hive
/// Onslaught"). So the map is matched against our own curated names, ignoring
/// punctuation and case, and the rest is appended in the order the program uses.
/// A map we do not know keeps the ladder's spelling rather than being dropped.
fn ladder_combat_name(map: &str, space: bool, level: LadderDifficulty, solo: bool) -> String {
    let ours = crate::analyzer::curated_map_names()
        .into_iter()
        .find(|name| {
            let bare = name.rsplit("] ").next().unwrap_or(name);
            squashed(bare) == squashed(map)
        })
        .unwrap_or_else(|| map.to_owned());
    let mut name = String::new();
    name.push_str(if solo { "[Solo] " } else { "[Team] " });
    name.push_str(&ours);
    name.push_str(if space { " (Space)" } else { " (Ground)" });
    if let Some(level) = level.api_value() {
        name.push_str(&format!(" [{level}]"));
    }
    name
}

/// Letters and digits only, folded to lower case: what two spellings of the same
/// map have in common.
fn squashed(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Warns when the server sent fewer rows than it says it has.
///
/// Today's ladder is far below the cap, so this never fires — but a silently
/// short list would not look wrong anywhere: the filters would simply stop
/// offering maps nobody could then find.
fn warn_if_truncated(what: &str, count: i32, received: usize) {
    if (received as i32) < count {
        log::warn!(
            "ladder: the server sent {received} of {count} {what} — the page limit cut the rest, \
             so anything after them is missing from the window"
        );
    }
}
static REDUCED_COLUMNS: &[&str] = &[
    "Rank",
    "Combat",
    "Player",
    "DPS",
    "debuff",
    "combat time",
    "damage share",
    "Date",
];

#[derive(Default)]
pub enum Records {
    #[default]
    Collapsed,
    Loading(Option<JoinHandle<Self>>),
    #[allow(private_interfaces)]
    Loaded(Box<LoadedLadders>),
    LoadError(String),
}

impl Records {
    /// Returns the run the reader asked to look at, once it is on disk.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        frame: &Frame,
        url: &str,
        position: &mut Option<[f32; 2]>,
    ) -> Option<PathBuf> {
        let mut open_run = None;
        let url = match Url::parse(url) {
            Ok(u) => u,
            Err(_) => {
                ui.label("the provided upload URL is invalid (change in Settings->Upload)");
                return None;
            }
        };
        if ui.steady_toggle(!self.collapsed(), "Ladder").clicked() {
            // A toggle both ways. It lit up while the window was open but
            // pressing it again fetched the whole ladder afresh instead of
            // putting the window away, which is not what a lit button offers.
            *self = if self.collapsed() {
                Self::begin_load_ladders(ui.ctx().clone(), url.clone())
            } else {
                Self::Collapsed
            };
        }

        let mut open = !self.collapsed();
        // A window of its own, not one drawn inside the main one: the whole
        // point of opening a run from here is to read it in the main window,
        // which cannot happen while this sits on top of it.
        //
        // `show_viewport_immediate` runs the contents there and then, on this
        // thread, so the state stays plain `&mut self` — a deferred viewport
        // would want all of it `Send + Sync`.
        //
        // The viewport id is its own, because that is what tells egui the two
        // windows apart. The **app id is deliberately the main window's**: on
        // Wayland a window carries no icon of its own, the compositor looks up
        // `<app id>.desktop` for one, and that is also what it groups by on the
        // task bar. A window with an app id nothing is installed under gets the
        // generic icon and a place of its own — which is what a suffixed one
        // did here. The icon below is for X11, where it does come from the
        // window.
        if open {
            let saved = position.map(|p| pos2(p[0], p[1]));
            let mut builder = ViewportBuilder::default()
                .with_title("Ladder")
                .with_app_id(crate::app::desktop_install::APP_ID)
                .with_icon(crate::app::app_icon::window_icon())
                .with_inner_size(vec2(1280.0, 720.0));
            if let Some(saved) = saved {
                builder = builder.with_position(saved);
            }
            ui.ctx().show_viewport_immediate(
                ViewportId::from_hash_of("sto-clare-ladder"),
                builder,
                |viewport_ui, _class| {
                    CentralPanel::default().show_inside(viewport_ui, |ui| match self {
                        Self::Collapsed => (),
                        Self::Loading(join_handle) => {
                            if join_handle.as_ref().unwrap().is_finished() {
                                *self = join_handle.take().unwrap().join().unwrap();
                                ui.ctx().request_repaint_of(ViewportId::ROOT);
                            }
                            Self::show_loading_ladders(ui);
                        }
                        Self::Loaded(loaded_ladders) => {
                            loaded_ladders.show(ui, frame, url.clone(), &mut open_run)
                        }
                        Self::LoadError(err) => {
                            ui.label(&*err);
                        }
                    });
                    // The window's own close button, and where it was left.
                    let ctx = viewport_ui.ctx();
                    if ctx.input(|i| i.viewport().close_requested()) {
                        open = false;
                    }
                    if let Some(outer) = ctx.input(|i| i.viewport().outer_rect) {
                        *position = Some([outer.min.x, outer.min.y]);
                    }
                },
            );
        }

        if !open {
            *self = Self::Collapsed;
        }
        open_run
    }

    fn collapsed(&self) -> bool {
        matches!(self, Self::Collapsed)
    }

    fn show_loading_ladders(ui: &mut Ui) {
        ui.add_space(20.0);
        ui.label("loading record tables...");
        ui.add_space(40.0);
        ui.label(WidgetText::from("⏳").color(theme::palette().busy));
        ui.add_space(20.0);
    }

    fn begin_load_ladders(ctx: Context, url: Url) -> Self {
        let join_handle = spawn_request(move || Self::load_ladders(ctx, url));
        Self::Loading(Some(join_handle))
    }

    fn load_ladders(ctx: Context, url: Url) -> Self {
        let state = match Self::do_load_ladders(url.clone()) {
            Ok(ladders) => {
                if ladders.results.is_empty() {
                    return Self::LoadError("Failed to load records tables.".into());
                }
                Self::Loaded(Box::new(LoadedLadders::new(ladders, &ctx, url)))
            }
            Err(err) => Self::LoadError(format!(
                "{}",
                err.action_error("Failed to load records tables.")
            )),
        };
        ctx.request_repaint_after_for(Duration::from_millis(10), ViewportId::ROOT);
        state
    }

    fn do_load_ladders(url: Url) -> Result<LaddersModel, RequestError> {
        let client = ladder_client();
        let ladders_url = url.join("/ladder/").unwrap();
        let response = client
            .get(ladders_url)
            .query(&[("page_size", &MAX_PAGE_SIZE.to_string())])
            .send()?;
        if !response.status().is_success() {
            return Err(RequestError::from(response));
        }
        let mut ladders = response.json::<LaddersModel>()?;
        warn_if_truncated("record tables", ladders.count, ladders.results.len());
        ladders.seasons_newest_first = Self::do_load_seasons(&client, url).unwrap_or_default();
        Ok(ladders)
    }

    /// The seasons, newest first — the order the season picker is put in.
    ///
    /// A ladder names its season but says nothing about when the season was, and
    /// the names cannot be sorted into any sensible order by themselves ("Season
    /// 31", "Season 9", "Default", "Pre-OSCR records"). The dates live on the
    /// season itself, so they are asked for separately; it is one small request
    /// (a dozen rows) beside the several hundred ladders.
    ///
    /// A failure here is not worth failing the window for: the picker then keeps
    /// the order the ladders arrived in, which is what it always used to have.
    fn do_load_seasons(client: &Client, url: Url) -> Result<Vec<String>, RequestError> {
        let url = url.join("/variant/").unwrap();
        let response = client
            .get(url)
            .query(&[
                ("ordering", "-start_date"),
                ("page_size", &MAX_PAGE_SIZE.to_string()),
            ])
            .send()?;
        if !response.status().is_success() {
            return Err(RequestError::from(response));
        }
        let seasons = response.json::<SeasonsModel>()?;
        warn_if_truncated("seasons", seasons.count, seasons.results.len());
        Ok(seasons
            .results
            .into_iter()
            .map(|season| season.name)
            .collect())
    }
}

struct LoadedLadders {
    ladders: Ladders,
    filter: LadderFilter,
    entries: Entries,
}

impl LoadedLadders {
    fn new(ladders: LaddersModel, ctx: &Context, url: Url) -> Self {
        let ladders = Ladders::from(ladders);
        // Opens on the newest season, which is what somebody opening this window
        // is almost always after; "All seasons" is one click away for the times
        // it is a player being looked for rather than a table.
        let filter = LadderFilter {
            season: ladders.seasons.first().cloned(),
            ..Default::default()
        };
        Self {
            entries: Entries::begin_load(ctx.clone(), url, &ladders, &filter, 1, false),
            filter,
            ladders,
        }
    }

    fn show(&mut self, ui: &mut Ui, frame: &Frame, url: Url, open_run: &mut Option<PathBuf>) {
        let changed = self.show_filters(ui);
        // What the filters left, so a search across several tables does not look
        // like one table with a strange number of first places in it.
        let matching = self.ladders.matching(&self.filter);
        ui.label(
            RichText::new(match matching.as_slice() {
                [] => "no table matches".to_owned(),
                [only] => only.name.clone(),
                several => format!("{} tables", several.len()),
            })
            .weak(),
        );
        ui.separator();

        let page = self
            .entries
            .show(ui, frame, &url, &mut self.filter, open_run);
        // A changed filter starts again at the first page; a changed page keeps
        // the filter, player and all.
        if let Some(page) = changed.then_some(1).or(page) {
            self.entries = Entries::begin_load(
                ui.ctx().clone(),
                url,
                &self.ladders,
                &self.filter,
                page,
                self.entries.shows_full_data(),
            );
        }
    }

    /// The four menus, each offering only what the other three leave reachable.
    /// Returns whether any of them moved.
    fn show_filters(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            changed |= ComboBox::new("ladder_season", "")
                .selected_text(self.filter.season.as_deref().unwrap_or("All seasons"))
                .width(260.0)
                .show_ui(ui, |ui| {
                    let mut changed = ui
                        .selectable_label(self.filter.season.is_none(), "All seasons")
                        .clicked();
                    if changed {
                        self.filter.season = None;
                    }
                    for season in &self.ladders.seasons {
                        if ui
                            .selectable_label(self.filter.season.as_ref() == Some(season), season)
                            .clicked()
                        {
                            self.filter.season = Some(season.clone());
                            changed = true;
                        }
                    }
                    changed
                })
                .inner
                .unwrap_or(false);

            let mut maps =
                self.ladders
                    .reachable(&self.filter, |f| f.map = None, |l| l.map.clone());
            maps.sort();
            changed |= self.show_choice(
                ui,
                "ladder_map",
                "Any map",
                260.0,
                maps,
                |map| map.clone(),
                |filter| &mut filter.map,
            );

            // Space first, because that is where most of the ladder is.
            let mut environments =
                self.ladders
                    .reachable(&self.filter, |f| f.environment = None, |l| l.is_space);
            environments.sort_by(|a, b| b.cmp(a));
            changed |= self.show_choice(
                ui,
                "ladder_environment",
                "Space and ground",
                90.0,
                environments,
                |space| if *space { "Space" } else { "Ground" }.to_owned(),
                |filter| &mut filter.environment,
            );

            let mut sizes = self
                .ladders
                .reachable(&self.filter, |f| f.solo = None, |l| l.is_solo);
            sizes.sort_by(|a, b| b.cmp(a));
            changed |= self.show_choice(
                ui,
                "ladder_solo",
                "Solo and team",
                90.0,
                sizes,
                |solo| if *solo { "Solo" } else { "Team" }.to_owned(),
                |filter| &mut filter.solo,
            );

            // The levels come from the fixed list rather than from whatever
            // order the tables happened to arrive in, so the menu always reads
            // Normal, Advanced, Elite, Any.
            let reachable =
                self.ladders
                    .reachable(&self.filter, |f| f.difficulty = None, |l| l.difficulty);
            let levels = LadderDifficulty::ALL
                .iter()
                .copied()
                .filter(|level| reachable.contains(level))
                .collect();
            changed |= self.show_choice(
                ui,
                "ladder_difficulty",
                "All levels",
                100.0,
                levels,
                |difficulty| difficulty.label().to_owned(),
                |filter| &mut filter.difficulty,
            );
        });
        if changed {
            self.ladders.settle(&mut self.filter);
        }
        changed
    }

    /// One menu: "everything" at the top, then whatever the other three leave.
    #[allow(clippy::too_many_arguments)]
    fn show_choice<T: PartialEq + Clone>(
        &mut self,
        ui: &mut Ui,
        id: &str,
        anything: &str,
        width: f32,
        options: Vec<T>,
        label: impl Fn(&T) -> String,
        field: impl Fn(&mut LadderFilter) -> &mut Option<T>,
    ) -> bool {
        let selected = field(&mut self.filter)
            .as_ref()
            .map(&label)
            .unwrap_or_else(|| anything.to_owned());
        ComboBox::new(id, "")
            .selected_text(selected)
            .width(width)
            .show_ui(ui, |ui| {
                let mut changed = ui
                    .selectable_label(field(&mut self.filter).is_none(), anything)
                    .clicked();
                if changed {
                    *field(&mut self.filter) = None;
                }
                for option in options {
                    let picked = field(&mut self.filter).as_ref() == Some(&option);
                    if ui.selectable_label(picked, label(&option)).clicked() {
                        *field(&mut self.filter) = Some(option);
                        changed = true;
                    }
                }
                changed
            })
            .inner
            .unwrap_or(false)
    }
}

enum Entries {
    Loading(Option<JoinHandle<Self>>),
    Loaded(LoadedEntries),
    LoadError(String),
}

impl Entries {
    /// Returns the page to load again, when something here asked for one.
    fn show(
        &mut self,
        ui: &mut Ui,
        frame: &Frame,
        url: &Url,
        filter: &mut LadderFilter,
        open_run: &mut Option<PathBuf>,
    ) -> Option<i32> {
        let mut reload = None;
        match self {
            Entries::Loading(join_handle) => {
                if join_handle.as_ref().unwrap().is_finished() {
                    let join_handle = join_handle.take().unwrap();
                    *self = join_handle.join().unwrap();
                    ui.ctx().request_repaint_of(ViewportId::ROOT);
                }

                ui.add_space(20.0);
                ui.label("loading table entries...");
                ui.add_space(40.0);
                ui.label(WidgetText::from("⏳").color(theme::palette().busy));
                ui.add_space(20.0);
            }
            Entries::Loaded(entries) => {
                let search = ui
                    .horizontal(|ui| {
                        let mut search = TextEdit::singleline(&mut filter.player)
                            .desired_width(400.0)
                            .hint_text("search for Player")
                            .show(ui)
                            .response
                            .lost_focus()
                            && ui.input(|i| i.key_pressed(Key::Enter));
                        search |= ui.button("Search").clicked();
                        search
                    })
                    .inner;

                let mut change_page = None;
                ui.horizontal(|ui| {
                    ui.label("Page:");
                    ui.add_enabled_ui(entries.page > 1, |ui| {
                        if ui.button("⏴").clicked() {
                            change_page = Some(entries.page - 1);
                        }
                    });
                    if NumberEdit::new(&mut entries.entered_page, "page edit")
                        .clamp_min(1)
                        .clamp_max(entries.page_count)
                        .desired_text_edit_width(40.0)
                        .show(ui)
                        .lost_focus()
                        && entries.page != entries.entered_page
                    {
                        change_page = Some(entries.entered_page);
                    }
                    ui.add_enabled_ui(entries.page < entries.page_count, |ui| {
                        if ui.button("⏵").clicked() {
                            change_page = Some(entries.page + 1);
                        }
                    });

                    ui.add_space(20.0);
                    ui.checkbox(&mut entries.show_full_data, "Show full data");
                });
                entries.show(ui, frame, url, open_run);
                if search {
                    reload = Some(1);
                } else if let Some(change_page) = change_page {
                    reload = Some(change_page);
                }
            }
            Entries::LoadError(err) => {
                ui.label(&*err);
            }
        }
        reload
    }

    /// Whether the wide set of columns is on, so a reload comes back the same
    /// way the reader left it.
    fn shows_full_data(&self) -> bool {
        matches!(self, Entries::Loaded(entries) if entries.show_full_data)
    }

    fn begin_load(
        ctx: Context,
        url: Url,
        ladders: &Ladders,
        filter: &LadderFilter,
        page: i32,
        show_full_data: bool,
    ) -> Entries {
        // The question is settled here, where both the filter and the tables
        // are to hand; the thread only carries it to the server.
        let query = filter.query(&ladders.all);
        let team_only = filter.solo == Some(false);
        let metric = ladders
            .matching(filter)
            .first()
            .map(|ladder| ladder.metric.clone())
            .unwrap_or_else(|| "DPS".to_owned());
        // What each table is called, for the rows to point back at when the
        // answer spans more than one of them.
        let tables: FxHashMap<i32, LadderFacts> = ladders
            .all
            .iter()
            .map(|ladder| {
                (
                    ladder.id,
                    LadderFacts {
                        map: ladder.map.clone(),
                        space: ladder.is_space,
                        difficulty: ladder.difficulty,
                        solo: ladder.is_solo,
                    },
                )
            })
            .collect();
        let join_handle = spawn_request(move || {
            Self::load_ladder_entries(
                ctx,
                url,
                query,
                metric,
                page,
                tables,
                team_only,
                show_full_data,
            )
        });
        Entries::Loading(Some(join_handle))
    }

    fn load_ladder_entries(
        ctx: Context,
        url: Url,
        query: Vec<(String, String)>,
        metric: String,
        page: i32,
        tables: FxHashMap<i32, LadderFacts>,
        team_only: bool,
        show_full_data: bool,
    ) -> Entries {
        let state = match Self::do_load_ladder_entries(url, &query, &metric, page) {
            Ok(entries) => Entries::Loaded(LoadedEntries::new(
                page,
                entries,
                &tables,
                team_only,
                show_full_data,
            )),
            Err(err) => Entries::LoadError(format!(
                "{}",
                err.action_error("Failed to load record table entries.")
            )),
        };
        ctx.request_repaint_after_for(Duration::from_millis(10), ViewportId::ROOT);
        state
    }

    fn do_load_ladder_entries(
        url: Url,
        filter_query: &[(String, String)],
        metric: &str,
        page: i32,
    ) -> Result<LadderEntriesModel, RequestError> {
        let client = ladder_client();
        let url = url.join("/ladder-entries/").unwrap();
        let mut query: Vec<(&str, String)> = vec![
            ("page_size", PAGE_SIZE.to_string()),
            ("ordering", format!("-data__{metric}")),
            ("page", page.to_string()),
        ];
        query.extend(
            filter_query
                .iter()
                .map(|(key, value)| (key.as_str(), value.clone())),
        );
        let response = client.get(url).query(&query).send()?;
        if !response.status().is_success() {
            return Err(RequestError::from(response));
        }
        let ladder_entries = response.json::<LadderEntriesModel>()?;
        Ok(ladder_entries)
    }
}

struct LoadedEntries {
    page: i32,
    entered_page: i32,
    selected_row: Option<usize>,
    page_count: i32,
    reduced_columns_count: usize,
    entries: Vec<TableColumn>,
    combat_log_ids: Vec<i32>,
    download_log_state: DownloadLogState,
    show_full_data: bool,
}

impl LoadedEntries {
    fn new(
        page: i32,
        model: LadderEntriesModel,
        tables: &FxHashMap<i32, LadderFacts>,
        team_only: bool,
        show_full_data: bool,
    ) -> Self {
        let mut formatter = NumberFormatter::new();
        let (reduced_columns_count, entries) =
            TableColumn::build_table(&model, tables, team_only, &mut formatter);
        let combat_log_ids = model.results.iter().map(|e| e.combatlog).collect();
        Self {
            page_count: model.count / PAGE_SIZE + if model.count % PAGE_SIZE > 0 { 1 } else { 0 },
            page,
            entered_page: page,
            reduced_columns_count,
            entries,
            combat_log_ids,
            selected_row: None,
            download_log_state: DownloadLogState::Idle,
            show_full_data,
        }
    }

    fn show(&mut self, ui: &mut Ui, frame: &Frame, url: &Url, open_run: &mut Option<PathBuf>) {
        if self.entries.is_empty() {
            ui.label("no entries");
            return;
        }

        let columns = if self.show_full_data {
            Either::Left(self.entries.iter())
        } else {
            Either::Right(self.entries.iter().take(self.reduced_columns_count))
        };
        let entries_count = self.entries.first().map(|c| c.values.len()).unwrap_or(0);
        ScrollArea::horizontal().show(ui, |ui| {
            Table::new(ui)
                .header(15.0, |r| {
                    for column in columns.clone() {
                        r.cell(|ui| {
                            ui.label(&column.name);
                        });
                    }
                    r.cell(|ui| {
                        ui.label("📥");
                    })
                    .on_hover_text("download log");
                    r.cell(|ui| {
                        ui.label("🔍");
                    })
                    .on_hover_text("open this run in the main window");
                })
                .body(25.0, |b| {
                    for index in 0..entries_count {
                        if b.selectable_row(self.selected_row == Some(index), |r| {
                            for column in columns.clone() {
                                let data = &column.values[index];
                                if data.is_number {
                                    r.cell_with_layout(
                                        Layout::right_to_left(Align::Center),
                                        |ui| {
                                            ui.label(&data.value);
                                        },
                                    );
                                } else {
                                    r.cell(|ui| {
                                        ui.label(&data.value);
                                    });
                                }
                            }

                            self.download_log_state.show_download_button(
                                r,
                                frame,
                                url,
                                self.combat_log_ids[index],
                            );
                            self.download_log_state.show_open_button(
                                r,
                                url,
                                self.combat_log_ids[index],
                                open_run,
                            );
                        })
                        .clicked()
                        {
                            if self.selected_row == Some(index) {
                                self.selected_row = None
                            } else {
                                self.selected_row = Some(index)
                            }
                        }
                    }
                });
        });

        self.download_log_state.show_download(ui, open_run);
    }
}

enum DownloadLogState {
    Idle,
    Downloading(String, Option<JoinHandle<Self>>),
    /// A fetch made so the run could be looked at here, rather than saved
    /// somewhere of the reader's choosing. It carries where it was put, and is
    /// handed on the moment the file is there.
    Opening(PathBuf, Option<JoinHandle<Self>>),
    DownloadFailed(String),
}

impl DownloadLogState {
    fn is_idle(&self) -> bool {
        matches!(self, DownloadLogState::Idle)
    }

    /// Fetches the run and hands its path back, for the main window to show.
    /// Downloaded to a scratch file named after the run, so asking twice costs
    /// one fetch.
    fn show_open_button(
        &mut self,
        row: &mut TableRow,
        url: &Url,
        log_id: i32,
        open_run: &mut Option<PathBuf>,
    ) {
        if row
            .selectable_cell(false, |ui| {
                ui.add_enabled_ui(self.is_idle(), |ui| {
                    ui.label("🔍");
                });
            })
            .on_hover_text("open this run in the main window")
            .clicked()
        {
            let path = crate::helpers::paths::ladder_run(log_id);
            if path.is_file() {
                // Fetched once already; asking again costs nothing.
                *open_run = Some(path);
            } else {
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let url = url.clone();
                let fetching = path.clone();
                *self = DownloadLogState::Opening(
                    path,
                    Some(spawn_request(move || {
                        match Self::do_download_log(url, fetching, log_id) {
                            Ok(()) => DownloadLogState::Idle,
                            Err(err) => DownloadLogState::DownloadFailed(format!(
                                "{}",
                                err.action_error("Failed to download the combatlog.")
                            )),
                        }
                    })),
                );
            }
        }
    }

    fn show_download_button(&mut self, row: &mut TableRow, frame: &Frame, url: &Url, log_id: i32) {
        if row
            .selectable_cell(false, |ui| {
                ui.add_enabled_ui(self.is_idle(), |ui| {
                    ui.label("📥");
                });
            })
            .on_hover_text("download log")
            .clicked()
            && let Some(file) = rfd::FileDialog::new()
                .set_parent(frame)
                .set_title("Download combatlog File")
                .add_filter("combatlog", &["log"])
                .save_file()
        {
            *self = Self::begin_download_log(url.clone(), file, log_id);
        }
    }

    fn show_download(&mut self, ui: &Ui, open_run: &mut Option<PathBuf>) {
        match self {
            DownloadLogState::Idle => (),
            DownloadLogState::Opening(path, join_handle) => {
                Window::new("Download log")
                    .auto_sized()
                    .constrain(true)
                    .collapsible(false)
                    .show(ui.ctx(), |ui| {
                        ui.add_space(20.0);
                        ui.label("fetching the run...");
                        ui.add_space(40.0);
                        ui.label(WidgetText::from("⏳").color(theme::palette().busy));
                        ui.add_space(20.0);
                    });
                if join_handle.as_ref().unwrap().is_finished() {
                    let path = path.clone();
                    let finished = join_handle.take().unwrap().join().unwrap();
                    // Only a fetch that worked has a file to show.
                    if matches!(finished, DownloadLogState::Idle) {
                        *open_run = Some(path);
                    }
                    *self = finished;
                    ui.ctx().request_repaint_of(ViewportId::ROOT);
                }
            }
            DownloadLogState::Downloading(message, join_handle) => {
                Window::new("Download log")
                    .auto_sized()
                    .constrain(true)
                    .collapsible(false)
                    .show(ui.ctx(), |ui| {
                        ui.add_space(20.0);
                        ui.label(&*message);
                        ui.add_space(40.0);
                        ui.label(WidgetText::from("⏳").color(theme::palette().busy));
                        ui.add_space(20.0);
                    });
                if join_handle.as_ref().unwrap().is_finished() {
                    *self = join_handle.take().unwrap().join().unwrap();
                    ui.ctx().request_repaint_of(ViewportId::ROOT);
                }
            }
            DownloadLogState::DownloadFailed(error) => {
                let mut open = true;
                Window::new("Download log failed")
                    .auto_sized()
                    .constrain(true)
                    .collapsible(false)
                    .open(&mut open)
                    .show(ui.ctx(), |ui| {
                        ui.label(&*error);
                    });

                if !open {
                    *self = DownloadLogState::Idle;
                }
            }
        }
    }

    fn begin_download_log(url: Url, path: PathBuf, log_id: i32) -> DownloadLogState {
        DownloadLogState::Downloading(
            format!("downloading log to {:?}...", path),
            Some(spawn_request(move || Self::download_log(url, path, log_id))),
        )
    }

    fn download_log(url: Url, path: PathBuf, log_id: i32) -> DownloadLogState {
        match Self::do_download_log(url, path, log_id) {
            Ok(_) => DownloadLogState::Idle,
            Err(err) => DownloadLogState::DownloadFailed(
                err.action_error("Failed to download log.").to_string(),
            ),
        }
    }

    fn do_download_log(url: Url, path: PathBuf, log_id: i32) -> Result<(), RequestError> {
        let client = ladder_client();
        let url = url
            .join(&format!("/combatlog/{}/download/", log_id))
            .unwrap();
        let mut response = client.get(url).send()?;
        if !response.status().is_success() {
            return Err(RequestError::from(response));
        }

        let mut data = Vec::new();
        response.copy_to(&mut data)?;

        let file = File::create(path)?;
        GzDecoder::new(file).write_all(&data)?;

        Ok(())
    }
}

#[derive(Deserialize, Debug)]
struct LaddersModel {
    /// How many the server has, against how many it sent. See [`sane_client`]:
    /// a page size beyond what the server allows is capped in silence.
    count: i32,
    results: Vec<LadderModel>,
    /// Filled in after the fact from the seasons endpoint — the ladders
    /// themselves carry no dates. Empty when that request failed.
    #[serde(skip)]
    seasons_newest_first: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct SeasonsModel {
    count: i32,
    results: Vec<SeasonModel>,
}

#[derive(Deserialize, Debug)]
struct SeasonModel {
    #[serde(deserialize_with = "null_to_default")]
    name: String,
}

#[derive(Deserialize, Debug, Clone)]
struct LadderModel {
    id: i32,
    #[serde(deserialize_with = "null_to_default")]
    name: String,
    difficulty: Option<String>,
    #[serde(deserialize_with = "null_to_default")]
    metric: String,
    is_solo: bool,
    /// Space rather than ground. The server has it on every table but does not
    /// accept it as a filter — see [`LadderFilter::query`].
    #[serde(default)]
    is_space: bool,
    #[serde(deserialize_with = "null_to_default")]
    variant: String,
}

#[derive(Deserialize, Debug)]
struct LadderEntriesModel {
    count: i32,
    results: Vec<LadderEntryModel>,
}

#[derive(Deserialize, Debug)]
struct LadderEntryModel {
    /// Which table the entry is in. Only worth showing when the filters leave
    /// more than one, which is what a player search across a season does.
    ladder: i32,
    #[serde(deserialize_with = "null_to_default")]
    date: String,
    #[serde(deserialize_with = "null_to_default")]
    player: String,
    rank: i32,
    combatlog: i32,
    data: serde_json::Map<String, serde_json::Value>,
}

/// Every table the ladder holds, kept flat: what the window shows is decided by
/// the filters, not by which bucket a table was put in when it arrived.
struct Ladders {
    seasons: Vec<String>,
    all: Vec<Ladder>,
}

impl From<LaddersModel> for Ladders {
    fn from(value: LaddersModel) -> Self {
        // Newest season first, which is the one somebody opening this window is
        // almost always after. Seasons the dates did not cover keep their old
        // place, at the end, rather than disappearing from the picker.
        let seasons: Vec<_> = value
            .seasons_newest_first
            .iter()
            .filter(|season| value.results.iter().any(|l| &l.variant == *season))
            .chain(
                value
                    .results
                    .iter()
                    .map(|l| &l.variant)
                    .filter(|season| !value.seasons_newest_first.contains(season)),
            )
            .unique()
            .cloned()
            .collect();
        Self {
            all: value.results.iter().map(Ladder::from).collect(),
            seasons,
        }
    }
}

impl Ladders {
    /// The tables a filter leaves, in the order the picker offers them: by map,
    /// then by level as the game orders it, then solo beside its team twin. The
    /// server hands them over in no order worth keeping.
    fn matching(&self, filter: &LadderFilter) -> Vec<&Ladder> {
        self.all
            .iter()
            .filter(|ladder| filter.matches(ladder))
            .sorted_by(|a, b| {
                a.map
                    .cmp(&b.map)
                    .then(a.difficulty.cmp(&b.difficulty))
                    .then(a.is_solo.cmp(&b.is_solo))
            })
            .collect()
    }

    /// Tidies the filter after a choice.
    ///
    /// A map is space or ground by its nature, so choosing one answers that
    /// question too — the menu then shows what the map is rather than leaving
    /// the reader to set it. And a choice made for one map need not hold for the
    /// next: a level the new map has no table for is let go rather than left to
    /// produce an empty list nothing on screen explains.
    fn settle(&self, filter: &mut LadderFilter) {
        if let Some(map) = filter.map.clone()
            && let Some(ladder) = self.all.iter().find(|l| l.map == map)
        {
            filter.environment = Some(ladder.is_space);
        }
        if self.matching(filter).is_empty() {
            for drop in [
                |f: &mut LadderFilter| f.difficulty = None,
                |f: &mut LadderFilter| f.solo = None,
                |f: &mut LadderFilter| f.environment = None,
                |f: &mut LadderFilter| f.map = None,
            ] {
                drop(filter);
                if !self.matching(filter).is_empty() {
                    break;
                }
            }
        }
    }

    /// What one menu can still offer, given every *other* menu — so no choice
    /// on offer can empty the list. Reads the same way as the main window's
    /// combat filter, which has the same problem with three menus instead of
    /// four.
    fn reachable<T: PartialEq>(
        &self,
        filter: &LadderFilter,
        without: impl Fn(&mut LadderFilter),
        of: impl Fn(&Ladder) -> T,
    ) -> Vec<T> {
        let mut relaxed = filter.clone();
        without(&mut relaxed);
        let mut reachable = Vec::new();
        for ladder in self.all.iter().filter(|ladder| relaxed.matches(ladder)) {
            let value = of(ladder);
            if !reachable.contains(&value) {
                reachable.push(value);
            }
        }
        reachable
    }
}

/// A table on the ladder, with the pieces the filters work on kept apart from
/// the line the picker shows.
#[derive(Clone)]
struct Ladder {
    id: i32,
    metric: String,
    name: String,
    season: String,
    map: String,
    difficulty: LadderDifficulty,
    is_solo: bool,
    is_space: bool,
}

/// The level a table is for.
///
/// A table either names a level or does not, and one that does not is the map's
/// unsplit table. The server writes that two ways — the string `Any` and no
/// value at all — and never both for the same map, so they are the same thing
/// and are read as one here. Ordered as the game orders them, which is the order
/// the picker offers them in; alphabetically they would read Advanced, Any,
/// Elite, Normal, which means nothing to anybody.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum LadderDifficulty {
    Normal,
    Advanced,
    Elite,
    Any,
}

impl LadderDifficulty {
    const ALL: &'static [Self] = &[Self::Normal, Self::Advanced, Self::Elite, Self::Any];

    fn from_api(difficulty: Option<&str>) -> Self {
        match difficulty {
            Some("Normal") => Self::Normal,
            Some("Advanced") => Self::Advanced,
            Some("Elite") => Self::Elite,
            _ => Self::Any,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Advanced => "Advanced",
            Self::Elite => "Elite",
            Self::Any => "Any",
        }
    }

    /// What the server calls it, where it has a name to call it. The unsplit
    /// tables have none — the column is empty for them, and no filter can ask
    /// for an empty column here (see [`LadderFilter::query`]).
    fn api_value(self) -> Option<&'static str> {
        match self {
            Self::Any => None,
            other => Some(other.label()),
        }
    }
}

impl<'a> From<&'a LadderModel> for Ladder {
    fn from(value: &'a LadderModel) -> Self {
        let difficulty = LadderDifficulty::from_api(value.difficulty.as_deref());
        let level = match difficulty {
            LadderDifficulty::Any => String::new(),
            other => format!(" ({})", other.label()),
        };
        Self {
            name: format!(
                "{}{}{} - {}",
                if value.is_solo { "[Solo] " } else { "" },
                value.name,
                level,
                value.metric
            ),
            id: value.id,
            metric: value.metric.clone(),
            season: value.variant.clone(),
            map: value.name.clone(),
            difficulty,
            is_solo: value.is_solo,
            is_space: value.is_space,
        }
    }
}

struct DataValue {
    value: String,
    is_number: bool,
}

impl DataValue {
    fn from_json_value(value: &Value, formatter: &mut NumberFormatter) -> Self {
        match value {
            Value::Null => Self::non_number(String::new()),
            Value::Bool(bool) => Self::non_number(if *bool { "✔" } else { "✖" }.into()),
            Value::Number(number) => Self::number(if number.is_f64() {
                formatter.format(number.as_f64().unwrap(), 2)
            } else {
                number.to_string()
            }),
            Value::String(str) => Self::non_number(str.into()),
            Value::Array(array) => Self::non_number(format!("{:?}", array)),
            Value::Object(object) => Self::non_number(format!("{:?}", object)),
        }
    }

    fn number(value: String) -> Self {
        Self {
            value,
            is_number: true,
        }
    }

    fn non_number(value: String) -> Self {
        Self {
            value,
            is_number: false,
        }
    }

    fn empty() -> Self {
        Self {
            value: Default::default(),
            is_number: false,
        }
    }
}

struct TableColumn {
    name: String,
    values: Vec<DataValue>,
}

impl TableColumn {
    fn build_table(
        entries: &LadderEntriesModel,
        tables: &FxHashMap<i32, LadderFacts>,
        team_only: bool,
        formatter: &mut NumberFormatter,
    ) -> (usize, Vec<Self>) {
        // Rank is per table, so a page drawn from several of them repeats "1"
        // with nothing to say which race each was won. Named only then: with one
        // table in play the column would say the same thing on every row.
        let mixed = entries
            .results
            .iter()
            .map(|entry| entry.ladder)
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            > 1;
        // One fight is entered into several tables — the catch-all for its map
        // and the one for its level, and a solo run into the team table as
        // well — so a page drawn from more than one of them repeats it, rank
        // and figures identical. Folded back into one row here, which loses
        // nothing: measured over a season's page, no two of those rows ever
        // disagreed on rank or on a single figure.
        //
        // It gains something, in fact. A row carries no map and no level of its
        // own; both live on the tables it is in. The set of them says which map,
        // which level (the one that is not the catch-all) and whether it was
        // solo (a run in a solo table was solo — the team entry is the
        // catch-all), which no single row can say.
        let mut tables_of: FxHashMap<i32, Vec<&LadderFacts>> = FxHashMap::default();
        if mixed {
            for entry in entries.results.iter() {
                if let Some(facts) = tables.get(&entry.ladder) {
                    tables_of.entry(entry.combatlog).or_default().push(facts);
                }
            }
        }
        // Asked for team, a run is only team where it is in no solo table: the
        // server admits one to a solo table exactly when the log held a single
        // player, and every run is in the team tables regardless. This is why
        // the query was not narrowed to team tables — with the solo ones out of
        // the answer there would be nothing left to recognise a solo run by.
        let solo_run = |combatlog: &i32| {
            tables_of
                .get(combatlog)
                .is_some_and(|facts| facts.iter().any(|f| f.solo))
        };
        let folded: Vec<&LadderEntryModel> = if mixed {
            let mut seen = std::collections::BTreeSet::new();
            entries
                .results
                .iter()
                .filter(|entry| !team_only || !solo_run(&entry.combatlog))
                .filter(|entry| seen.insert(entry.combatlog))
                .collect()
        } else {
            entries.results.iter().collect()
        };
        let mut from = Vec::new();
        let mut ranks = Vec::new();
        let mut players = Vec::new();
        let mut dates = Vec::new();
        let mut columns: FxHashMap<&str, Vec<DataValue>> = FxHashMap::default();

        for (i, entry) in folded.iter().enumerate() {
            if mixed {
                let of = tables_of.get(&entry.combatlog);
                let name = match of.and_then(|facts| facts.first()) {
                    Some(first) => ladder_combat_name(
                        &first.map,
                        first.space,
                        of.map(|facts| {
                            facts
                                .iter()
                                .map(|f| f.difficulty)
                                .filter(|d| *d != LadderDifficulty::Any)
                                .min()
                                .unwrap_or(LadderDifficulty::Any)
                        })
                        .unwrap_or(LadderDifficulty::Any),
                        of.is_some_and(|facts| facts.iter().any(|f| f.solo)),
                    ),
                    None => "—".to_owned(),
                };
                from.push(DataValue::non_number(name));
            }
            ranks.push(DataValue::number(entry.rank.to_string()));
            players.push(DataValue::non_number(entry.player.clone()));
            let date_time = DateTime::parse_from_str(&entry.date, "%+")
                .map(|d| format!("{}", d.format("%v %T")))
                .unwrap_or_else(|_| entry.date.clone());
            dates.push(DataValue::non_number(date_time));

            for (name, value) in entry.data.iter() {
                columns
                    .entry(name)
                    .or_default()
                    .push(DataValue::from_json_value(value, formatter));
            }

            columns
                .values_mut()
                .filter(|c| c.len() != i + 1)
                .for_each(|c| c.push(DataValue::empty()));
        }

        let named = if mixed {
            vec![
                ("Rank", ranks),
                ("Combat", from),
                ("Player", players),
                ("Date", dates),
            ]
        } else {
            vec![("Rank", ranks), ("Player", players), ("Date", dates)]
        };
        let mut columns: Vec<Self> = named
            .into_iter()
            .chain(columns)
            .map(|(n, c)| Self {
                name: n.replace('_', " "),
                values: c,
            })
            .collect();

        let mut new_index = 0;
        for cherry_pick in REDUCED_COLUMNS.iter() {
            if let Some(index) = columns
                .iter()
                .position(|c| str_equal_ignore_case(&c.name, cherry_pick))
            {
                let column = columns.remove(index);
                columns.insert(new_index, column);
                new_index += 1;
            }
        }

        (new_index, columns)
    }
}

fn str_equal_ignore_case(str1: &str, str2: &str) -> bool {
    str1.chars()
        .flat_map(|c| c.to_lowercase())
        .eq(str2.chars().flat_map(|c| c.to_lowercase()))
}

/// What the ladder has been narrowed to.
///
/// Everything here is answered by the server in one request. The environment is
/// the odd one out: the server carries `is_space` on every table but does not
/// accept it as a filter — sending it changes nothing at all — so it is asked
/// for as the set of maps that are of it, which the server does understand.
#[derive(Clone, Default, PartialEq)]
struct LadderFilter {
    /// Unset to look across every season at once, which is how you find a
    /// player who has not been near this one.
    season: Option<String>,
    /// `Some(true)` for space, `Some(false)` for ground.
    environment: Option<bool>,
    /// `Some(true)` for solo tables, `Some(false)` for team ones.
    solo: Option<bool>,
    difficulty: Option<LadderDifficulty>,
    map: Option<String>,
    /// Kept across every other change: narrowing the level while looking for a
    /// player is narrowing *that* search, not starting a new one.
    player: String,
}

impl LadderFilter {
    fn matches(&self, ladder: &Ladder) -> bool {
        self.season
            .as_ref()
            .is_none_or(|season| *season == ladder.season)
            && self
                .environment
                .is_none_or(|space| space == ladder.is_space)
            && self.solo.is_none_or(|solo| solo == ladder.is_solo)
            && self
                .difficulty
                .is_none_or(|difficulty| difficulty == ladder.difficulty)
            && self.map.as_ref().is_none_or(|map| *map == ladder.map)
    }

    /// The query for `/ladder-entries/`, given every table the ladder holds.
    ///
    /// Narrowed to a single table, it asks for that table by id, which is exact
    /// and needs nothing else. Otherwise it sends what the server understands:
    ///
    /// * booleans as `True`/`False` — lowercase `true` makes the server answer
    ///   `500` (reported as STOCD/OSCR-server#113);
    /// * the level by name, but only where it has one. The unsplit tables have
    ///   an empty level, and no filter can ask for an empty column, so those are
    ///   reached through their map names instead;
    /// * the maps as one regular expression, which is how the environment is
    ///   asked for as well.
    fn query(&self, ladders: &[Ladder]) -> Vec<(String, String)> {
        let matching: Vec<_> = ladders.iter().filter(|l| self.matches(l)).collect();
        let mut query = Vec::new();

        if let [only] = matching.as_slice() {
            query.push(("ladder".to_owned(), only.id.to_string()));
        } else {
            if let Some(season) = &self.season {
                query.push(("ladder__variant__name".to_owned(), season.clone()));
            }
            // Solo is exact on the server: a run is in a solo table only where
            // the log held exactly one player. Team is not — every run is in a
            // team table, solo ones included — so asking for team tables would
            // hand back the very runs it is meant to exclude, with nothing left
            // in the answer to recognise them by. Team is therefore asked for as
            // "any table" and the solo runs are dropped once the tables each run
            // is in can be seen (see `build_table`).
            if self.solo == Some(true) {
                query.push(("ladder__is_solo".to_owned(), "True".to_owned()));
            }
            if let Some(level) = self.difficulty.and_then(LadderDifficulty::api_value) {
                query.push(("ladder__difficulty".to_owned(), level.to_owned()));
            }
            // Only when it says something: a pattern of every map the season has
            // is a long way of saying nothing.
            let maps: Vec<_> = matching.iter().map(|l| l.map.as_str()).unique().collect();
            let all_maps = ladders
                .iter()
                .filter(|l| {
                    self.season
                        .as_ref()
                        .is_none_or(|season| *season == l.season)
                })
                .map(|l| l.map.as_str())
                .unique()
                .count();
            if !maps.is_empty() && maps.len() < all_maps {
                query.push((
                    "ladder__name__iregex".to_owned(),
                    format!("^({})$", maps.iter().map(|m| escape_regex(m)).join("|")),
                ));
            }
        }

        if !self.player.is_empty() {
            query.push(("player__icontains".to_owned(), self.player.clone()));
        }
        query
    }
}

/// Map names carry no regular-expression syntax today — there are ten of them
/// and none has so much as a bracket — but one that did would quietly change
/// what the pattern means rather than fail, so they are escaped anyway.
fn escape_regex(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if r".^$*+?()[]{}|\".contains(character) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEASON: &str = "Season 36 - Undiscovered";

    fn ladder(
        id: i32,
        map: &str,
        difficulty: LadderDifficulty,
        is_solo: bool,
        is_space: bool,
    ) -> Ladder {
        Ladder {
            id,
            metric: "DPS".into(),
            name: map.into(),
            season: SEASON.into(),
            map: map.into(),
            difficulty,
            is_solo,
            is_space,
        }
    }

    /// Two space maps and one ground one, the space pair split by level the way
    /// the real ladder splits them.
    fn ladders() -> Vec<Ladder> {
        vec![
            ladder(
                1,
                "Infected: The Conduit",
                LadderDifficulty::Elite,
                false,
                true,
            ),
            ladder(
                2,
                "Infected: The Conduit",
                LadderDifficulty::Advanced,
                false,
                true,
            ),
            ladder(3, "Hive: Onslaught", LadderDifficulty::Elite, false, true),
            ladder(4, "Bug Hunt", LadderDifficulty::Any, false, false),
            ladder(5, "Bug Hunt", LadderDifficulty::Any, true, false),
        ]
    }

    fn filter() -> LadderFilter {
        LadderFilter {
            season: Some(SEASON.into()),
            ..Default::default()
        }
    }

    fn value<'a>(query: &'a [(String, String)], key: &str) -> Option<&'a str> {
        query
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// A run from the ladder should read like one of your own combats, so the
    /// two sit side by side in a comparison without looking like they came from
    /// different programs.
    #[test]
    fn a_ladder_run_is_named_the_way_the_program_names_a_combat() {
        assert_eq!(
            "[Team] [TFO] Hive Onslaught (Space) [Elite]",
            ladder_combat_name("Hive: Onslaught", true, LadderDifficulty::Elite, false)
        );
    }

    /// The ladder spells some maps differently from the curated names — a colon
    /// here, none there — so the match ignores punctuation rather than failing
    /// and falling back to the ladder's spelling.
    #[test]
    fn a_map_spelled_differently_still_finds_our_name() {
        assert!(
            ladder_combat_name("Hive: Onslaught", true, LadderDifficulty::Any, false)
                .starts_with("[Team] [TFO] Hive Onslaught")
        );
    }

    /// Solo goes in front, the way the ladder itself writes it, and a table
    /// with no level of its own adds nothing.
    #[test]
    fn solo_leads_and_an_unsplit_level_adds_nothing() {
        assert_eq!(
            "[Solo] [TFO] Bug Hunt (Ground)",
            ladder_combat_name("Bug Hunt", false, LadderDifficulty::Any, true)
        );
    }

    /// A map nobody has curated keeps the ladder's own spelling rather than
    /// disappearing from the row.
    #[test]
    fn an_unknown_map_keeps_the_ladders_spelling() {
        assert_eq!(
            "[Team] Fictional Map (Space) [Elite]",
            ladder_combat_name("Fictional Map", true, LadderDifficulty::Elite, false)
        );
    }

    #[test]
    fn one_table_is_asked_for_by_id() {
        let filter = LadderFilter {
            map: Some("Hive: Onslaught".into()),
            ..filter()
        };
        let query = filter.query(&ladders());
        assert_eq!(Some("3"), value(&query, "ladder"));
        assert_eq!(None, value(&query, "ladder__name__iregex"));
    }

    /// Solo is exact on the server — a run is in a solo table only where the log
    /// held one player — so it is asked for directly. Lowercase `true` makes the
    /// server answer 500 (STOCD/OSCR-server#113), so the spelling is not a style
    /// choice.
    #[test]
    fn solo_is_asked_for_the_way_the_server_accepts_it() {
        // Two solo tables, so the answer is not one table — narrowed to a single
        // one it would be asked for by id and need nothing else.
        let mut ladders = ladders();
        ladders.push(ladder(
            6,
            "Hive: Onslaught",
            LadderDifficulty::Elite,
            true,
            true,
        ));
        let filter = LadderFilter {
            solo: Some(true),
            ..filter()
        };
        assert_eq!(
            Some("True"),
            value(&filter.query(&ladders), "ladder__is_solo")
        );
    }

    /// Team is not asked for at all. Every run is in a team table, solo ones
    /// included, so narrowing to them would hand back the runs it is meant to
    /// exclude — with the solo tables out of the answer, nothing would be left
    /// to recognise them by. They are dropped once the tables each run is in can
    /// be seen.
    #[test]
    fn team_is_not_narrowed_on_the_server() {
        let filter = LadderFilter {
            solo: Some(false),
            ..filter()
        };
        assert_eq!(None, value(&filter.query(&ladders()), "ladder__is_solo"));
    }

    /// The server carries `is_space` but ignores it as a filter, so the
    /// environment is asked for as the maps that are of it.
    #[test]
    fn the_environment_is_asked_for_as_its_maps() {
        let filter = LadderFilter {
            environment: Some(true),
            ..filter()
        };
        let query = filter.query(&ladders());
        let regex = value(&query, "ladder__name__iregex").unwrap();
        assert!(regex.contains("Infected: The Conduit"));
        assert!(regex.contains("Hive: Onslaught"));
        assert!(!regex.contains("Bug Hunt"));
    }

    /// A table with no level of its own cannot be asked for by level — the
    /// column is empty and no filter matches an empty column — so the level is
    /// left out and its maps carry the question instead.
    #[test]
    fn the_unsplit_level_is_not_asked_for_by_name() {
        let filter = LadderFilter {
            difficulty: Some(LadderDifficulty::Any),
            ..filter()
        };
        let query = filter.query(&ladders());
        assert_eq!(None, value(&query, "ladder__difficulty"));
        assert_eq!(Some("^(Bug Hunt)$"), value(&query, "ladder__name__iregex"));
    }

    #[test]
    fn a_named_level_is_asked_for_by_name() {
        let filter = LadderFilter {
            difficulty: Some(LadderDifficulty::Elite),
            ..filter()
        };
        assert_eq!(
            Some("Elite"),
            value(&filter.query(&ladders()), "ladder__difficulty")
        );
    }

    /// The player is part of the same question as the rest, not a separate one:
    /// narrowing the level while looking for somebody narrows that search.
    #[test]
    fn the_player_survives_every_other_choice() {
        let filter = LadderFilter {
            player: "somebody".into(),
            difficulty: Some(LadderDifficulty::Elite),
            environment: Some(true),
            ..filter()
        };
        assert_eq!(
            Some("somebody"),
            value(&filter.query(&ladders()), "player__icontains")
        );
    }

    /// Naming every map the season has is a long way of saying nothing.
    #[test]
    fn nothing_narrowed_asks_for_no_maps() {
        let query = filter().query(&ladders());
        assert_eq!(None, value(&query, "ladder__name__iregex"));
        assert_eq!(Some(SEASON), value(&query, "ladder__variant__name"));
    }

    /// A map is space or ground by its nature, so choosing one answers that
    /// question too.
    /// Looking across every season at once is how you find a player who has not
    /// been near the newest one, so the season has to be droppable like the rest.
    #[test]
    fn every_season_at_once_asks_for_no_season() {
        let filter = LadderFilter {
            season: None,
            player: "somebody".into(),
            ..filter()
        };
        let query = filter.query(&ladders());
        assert_eq!(None, value(&query, "ladder__variant__name"));
        assert_eq!(Some("somebody"), value(&query, "player__icontains"));
    }

    #[test]
    fn choosing_a_map_settles_where_it_is_fought() {
        let ladders = Ladders {
            seasons: vec![SEASON.into()],
            all: ladders(),
        };
        let mut filter = LadderFilter {
            map: Some("Bug Hunt".into()),
            ..filter()
        };
        ladders.settle(&mut filter);
        assert_eq!(Some(false), filter.environment);
    }

    /// A level picked for one map need not exist on the next. Left alone it
    /// would produce an empty list with nothing on screen to explain it, so the
    /// choice that cannot hold is let go.
    #[test]
    fn a_level_the_new_map_has_no_table_for_is_let_go() {
        let ladders = Ladders {
            seasons: vec![SEASON.into()],
            all: ladders(),
        };
        let mut filter = LadderFilter {
            difficulty: Some(LadderDifficulty::Elite),
            map: Some("Bug Hunt".into()),
            ..filter()
        };
        ladders.settle(&mut filter);
        assert_eq!(None, filter.difficulty);
        assert_eq!(Some("Bug Hunt".to_owned()), filter.map);
        assert!(!ladders.matching(&filter).is_empty());
    }

    /// Today's map names carry nothing a pattern would read as syntax, and must
    /// come through untouched; one that did would change what the pattern means
    /// rather than fail, so it is escaped.
    #[test]
    fn map_names_are_escaped_into_the_pattern() {
        assert_eq!("Hive: Onslaught", escape_regex("Hive: Onslaught"));
        assert_eq!(r"a\.b\+c", escape_regex("a.b+c"));
        assert_eq!(r"\(x\)", escape_regex("(x)"));
    }
}
