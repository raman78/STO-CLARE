//! The combats list: every fight the log holds, offered as a table.
//!
//! One place decides what a list of combats looks like, because the program
//! offers that list in three: the panel down the side of the main window (this
//! module), the compare view's picker and the delete dialog. A fight that reads
//! as `Infected Space | Team | Elite | 19.08 21:14 | 04:12 | 121.7k` in one of
//! them reads the same in the others.
//!
//! The panel keeps its own filter and its own fold-outs, but it does not keep a
//! *selection*: what is highlighted is whatever combat the window is showing,
//! and what a click does is ask for another one.

use std::cmp::Ordering;

use chrono::NaiveDateTime;
use eframe::egui::*;
use rustc_hash::FxHashSet;

use crate::{
    analyzer::{CombatSummary, Difficulty, PlayerSummary},
    app::{
        combat_filter::{CombatEntry, CombatFilter},
        date_range::DateRange,
        settings::CombatNotes,
        theme,
    },
    custom_widgets::{table::*, toggle::Toggle, tooltip::CloseTooltip},
    helpers::{format_duration_hms, number_formatting::NumberFormatter},
};

/// The height of one row, the tables' own.
const ROW_HEIGHT: f32 = 25.0;
const HEADER_HEIGHT: f32 = 25.0;

/// How wide the panel opens the first time, and the range a drag may take it
/// to. The low end still holds the map name and the DPS beside it; below that
/// the table would be a column of ellipses.
const DEFAULT_WIDTH: f32 = 520.0;
const MIN_WIDTH: f32 = 260.0;
const MAX_WIDTH: f32 = 1100.0;

/// The size of the fold-out arrow, matching the one in the damage tables.
const ARROW_SIZE: Vec2 = vec2(14.0, 14.0);

/// What the reader asked the list to do.
pub enum ListAction {
    /// Put this fight on screen.
    Open(usize),
    /// Rewrite the log keeping only these fights, in the order they are in it.
    /// Everything else was ticked for deletion.
    Keep(Vec<usize>),
}

/// What the list is being shown: the fights, what the reader wrote about them,
/// whose log it is and which fight is on screen.
pub struct CombatsListView<'a> {
    pub combats: &'a [CombatSummary],
    pub notes: &'a CombatNotes,
    /// The handle whose DPS the list shows, when it is known. See
    /// [`crate::analyzer::detect_log_owner`].
    pub my_handle: Option<&'a str>,
    /// The start time of the combat the main window is showing, which is the
    /// row that reads as picked.
    pub shown: Option<NaiveDateTime>,
}

/// The list of combats down the side of the main window.
pub struct CombatsPanel {
    open: bool,
    /// The environment/level/map pickers, the same ones the compare view uses.
    filter: CombatFilter,
    /// Bumped whenever the filter changes and mixed into the table's id, so the
    /// scroll area is measured afresh instead of keeping the height it had
    /// while the list was narrower. Same reason as the old picker's.
    filter_generation: u64,
    search: String,
    /// When the fights were played. The pickers beside it narrow by what a
    /// fight *was*; this one narrows by when — which is how an evening's runs
    /// are picked out of a log that holds a year of them.
    range: DateRange,
    /// Which column the rows are in the order of. Starts on the newest fight
    /// first, which is what a list of fights is nearly always opened for.
    sort: SortState<CombatColumn>,
    /// The fights ticked for deletion while "Clear Log File" is on, by start
    /// time; `None` while it is off and the list is only for reading.
    ///
    /// Ticked by start time rather than by index for the same reason the
    /// fold-outs are: the list is live, and a fight dropped from the log shifts
    /// every index after it — which, here, would delete the wrong fight.
    to_delete: Option<FxHashSet<NaiveDateTime>>,
    /// Set while the reader is being asked whether they mean it. Deleting
    /// rewrites the log and cannot be taken back, so anything past a single
    /// fight is worth one question first.
    confirm_delete: bool,
    /// Whose player lists are folded out, by combat start time. Held by start
    /// time rather than by index because the list is live: a combat dropped
    /// from the log shifts every index after it, and a fold-out would jump to
    /// another fight.
    unfolded: FxHashSet<NaiveDateTime>,
}

impl CombatsPanel {
    pub fn new(open: bool) -> Self {
        Self {
            open,
            filter: Default::default(),
            filter_generation: 0,
            search: String::new(),
            range: Default::default(),
            to_delete: None,
            confirm_delete: false,
            sort: SortState {
                column: Some(CombatColumn::Start),
                natural: true,
            },
            unfolded: Default::default(),
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// Draws the panel and reports what the reader asked of it.
    ///
    /// `width` is the remembered width: read to open the panel at the size it
    /// was left, and written back with whatever the reader has dragged it to.
    pub fn show(
        &mut self,
        view: CombatsListView<'_>,
        width: &mut f32,
        ui: &mut Ui,
    ) -> Option<ListAction> {
        let mut action = None;
        // The panel follows the table: as columns grow — a longer map name, a
        // note somebody wrote — it widens itself to fit rather than making the
        // reader drag it. A floor rather than a fixed width, so it can still be
        // dragged wider, and it stops at `MAX_WIDTH`, where the scroll bars the
        // table already has take over.
        let panel = Panel::left("combats panel")
            .resizable(true)
            .default_size(if *width > 0.0 { *width } else { DEFAULT_WIDTH })
            .size_range(self.fitting_width(ui)..=MAX_WIDTH)
            // The rim goes round the whole panel, not round the table inside
            // it: the search box, the filters and the strip at the bottom are
            // as much a part of the list as its rows. Without it the panel ran
            // into the tabs beside it — same background — and read as part of
            // whatever tab was open rather than as the thing that picks
            // between them.
            .frame(theme::section_frame(ui))
            .show_animated_inside(ui, self.open, |ui| {
                action = self.show_contents(view, ui);
            });
        if let Some(panel) = panel {
            *width = panel.response.rect.width();
        }
        action
    }

    /// The narrowest the panel may be drawn: enough for the whole table, as
    /// wide as it came to when it was last drawn, within the panel's own
    /// limits. Before the first frame there is nothing to measure and the
    /// ordinary minimum stands.
    fn fitting_width(&self, ui: &Ui) -> f32 {
        let Some(content) = table_content_width(ui, self.table_id()) else {
            return MIN_WIDTH;
        };
        // What the table asks for is its columns; the panel around it also has
        // its frame, and the table its own scroll bar.
        let chrome = ui.spacing().scroll.bar_width + ui.spacing().item_spacing.x * 4.0;
        (content + chrome).clamp(MIN_WIDTH, MAX_WIDTH)
    }

    /// The table's id. Carries the filter generation: egui keeps a scroll
    /// area's measured size under it, so a narrowed list would otherwise keep
    /// the height — and now the width — it had while filtered.
    fn table_id(&self) -> Id {
        Id::new(("combats panel table", self.filter_generation))
    }

    fn show_contents(&mut self, view: CombatsListView<'_>, ui: &mut Ui) -> Option<ListAction> {
        ui.horizontal(|ui| {
            ui.heading("Combats");
        });
        ui.separator();

        ui.horizontal(|ui| {
            ui.label("Search:");
            TextEdit::singleline(&mut self.search)
                .desired_width(f32::MAX)
                .show(ui);
        });

        let entries: Vec<CombatEntry> = view
            .combats
            .iter()
            .map(|combat| CombatEntry {
                environment: combat.environment.as_deref(),
                difficulty: combat.difficulty,
                base_name: combat.base_name.as_str(),
                solo: combat.solo,
            })
            .collect();
        ui.horizontal_wrapped(|ui| {
            let before = (self.filter.clone(), self.range.clone());
            self.filter.show("combats panel", &entries, ui);
            if (self.filter.is_active() || self.range.is_active())
                && ui.button("Clear filter").clicked()
            {
                self.filter.clear();
                self.range.clear();
            }
            if (self.filter.clone(), self.range.clone()) != before {
                self.filter_generation = self.filter_generation.wrapping_add(1);
            }
        });
        // Its own row: the two fields and their presets do not fit beside the
        // pickers, and wrapped in with them the row reads as one long filter
        // rather than as "what it was" and "when it was".
        ui.horizontal_wrapped(|ui| {
            let before = self.range.clone();
            self.range.show(
                "combats panel",
                view.combats
                    .first()
                    .map(|combat| combat.start)
                    .zip(view.combats.last().map(|combat| combat.start)),
                ui,
            );
            if self.range != before {
                self.filter_generation = self.filter_generation.wrapping_add(1);
            }
        });

        let visible = self.visible(&view);
        // A tick on a fight the filters have since hidden would delete
        // something nobody can see; the ticks are what is on screen.
        if let Some(ticked) = self.to_delete.as_mut() {
            let shown: FxHashSet<NaiveDateTime> =
                visible.iter().map(|&i| view.combats[i].start).collect();
            ticked.retain(|start| shown.contains(start));
        }

        let mut action = None;
        // Claimed before the table, so the strip sits at the bottom of the
        // panel whatever the table does — and sized by what is in it rather
        // than by a guess at its height, which is what left it half off the
        // edge once the table filled up.
        Panel::bottom("combats footer")
            .frame(footer_frame(ui))
            .show_inside(ui, |ui| {
                action = self.show_footer(&view, &visible, ui);
            });
        let mut clicked_heading = None;
        let mut formatter = NumberFormatter::new();
        // Whatever the strip at the bottom left.
        let table_height = ui.available_height().at_least(ROW_HEIGHT);
        Table::new(ui)
            .id(self.table_id())
            .max_scroll_height(table_height)
            .header(HEADER_HEIGHT)
            .body(ROW_HEIGHT, |t| {
                for index in visible.iter().copied() {
                    let combat = &view.combats[index];
                    let note = view.notes.get(&CombatNotes::key_at(combat.start));
                    let unfolded = self.unfolded.contains(&combat.start);
                    let mut fold = unfolded;
                    // While the log is being cleared the highlight is the tick,
                    // not the fight on screen: what is about to be deleted is
                    // the thing worth seeing at a glance.
                    let ticked = self
                        .to_delete
                        .as_ref()
                        .map(|ticked| ticked.contains(&combat.start));
                    let mut tick = ticked.unwrap_or(false);
                    let highlighted = match ticked {
                        Some(ticked) => ticked,
                        None => view.shown == Some(combat.start),
                    };
                    let row = t.selectable_row(highlighted, |r| {
                        // Kept whether or not there is anything to tick, so
                        // turning "Clear Log File" on makes the boxes appear
                        // rather than shifting every column of the table
                        // sideways under the reader's pointer.
                        r.cell(|ui| {
                            ui.add_visible(ticked.is_some(), Checkbox::without_text(&mut tick));
                        });
                        show_combat_cells(
                            r,
                            combat,
                            note,
                            dps_shown(combat, view.my_handle),
                            Some(&mut fold),
                            &mut formatter,
                        );
                    });
                    // The whole row ticks it, not the box alone — the row is
                    // what lights up under the pointer, so it is what a click
                    // lands on. With nothing to tick, a double click opens the
                    // fight instead.
                    if ticked.is_some() {
                        if row.clicked() {
                            tick = !tick;
                        }
                    } else if row.double_clicked() {
                        action = Some(ListAction::Open(index));
                    }
                    if Some(tick) != ticked
                        && let Some(to_delete) = self.to_delete.as_mut()
                    {
                        if tick {
                            to_delete.insert(combat.start);
                        } else {
                            to_delete.remove(&combat.start);
                        }
                    }
                    row.on_hover_text(&combat.identifier);
                    if fold != unfolded {
                        if fold {
                            self.unfolded.insert(combat.start);
                        } else {
                            self.unfolded.remove(&combat.start);
                        }
                    }
                    if fold {
                        for player in combat.players.iter() {
                            t.row(|r| {
                                show_player_cells(r, player, &mut formatter);
                            });
                        }
                    }
                }
            })
            .header_row(|r| {
                // The tick column's heading stays empty, like the compare
                // picker's.
                r.cell(|_| {});
                clicked_heading = show_header_cells(r, view.my_handle, Some(&self.sort));
            });
        if let Some(column) = clicked_heading {
            self.sort.clicked(column);
        }

        action
    }

    /// The strip that closes the panel off: what the list is holding, and the
    /// one thing done to the log as a whole rather than to a fight in it.
    fn show_footer(
        &mut self,
        view: &CombatsListView<'_>,
        visible: &[usize],
        ui: &mut Ui,
    ) -> Option<ListAction> {
        let mut action = None;
        let counts = ListCounts {
            total: view.combats.len(),
            shown: visible.len(),
            selected: self.to_delete.as_ref().map(FxHashSet::len),
        };
        let select = show_list_footer(ui, counts, None, |ui| {
            // A toggle rather than a button that opens a window: the fights are
            // already listed here, so ticking them is done in the list itself
            // instead of in a second copy of it.
            if ui
                .steady_toggle(self.to_delete.is_some(), "Clear Log File")
                .hover(
                    "Tick the fights to delete from the log. Everything left unticked \
                     stays in it.",
                )
                .clicked()
            {
                self.to_delete = match self.to_delete {
                    Some(_) => None,
                    // Everything but the newest, which is what clearing a log
                    // is nearly always for.
                    None => Some(all_but_newest(view, visible)),
                };
            }

            let ticks = self.to_delete.as_ref().map(FxHashSet::len).unwrap_or(0);
            if ticks > 0 {
                ui.scope(|ui| {
                    theme::accent_rim(ui);
                    if ui
                        .button(format!("Delete {ticks} ticked 🗑"))
                        .hover("Rewrite the log without them. This cannot be undone.")
                        .clicked()
                    {
                        // A single fight goes straight away — it is one row,
                        // ticked deliberately, and asking about it is noise.
                        // Anything more is worth a question, because the log is
                        // rewritten and there is no way back.
                        if ticks > 1 {
                            self.confirm_delete = true;
                        } else {
                            let ticked = self.to_delete.take().unwrap_or_default();
                            action = Some(ListAction::Keep(keep_list(view, &ticked)));
                        }
                    }
                });
            }
        });
        if let Some(confirmed) = self.show_delete_confirmation(view, ui) {
            action = Some(confirmed);
        }
        if let (Some(select), Some(ticked)) = (select, self.to_delete.as_mut()) {
            match select {
                SelectAll::All => {
                    *ticked = visible.iter().map(|&i| view.combats[i].start).collect()
                }
                SelectAll::None => ticked.clear(),
            }
        }
        action
    }

    /// Asks whether the reader means it, and reports the deletion once they say
    /// so.
    ///
    /// A window rather than a second click on the same button: the count is
    /// what the question is about, and a button that changes its own label
    /// under the pointer is how people delete a log by accident.
    fn show_delete_confirmation(
        &mut self,
        view: &CombatsListView<'_>,
        ui: &mut Ui,
    ) -> Option<ListAction> {
        if !self.confirm_delete {
            return None;
        }
        let ticked = self.to_delete.clone().unwrap_or_default();
        // The ticks can go while the question is up — a refresh dropping a
        // fight, the reader clearing them. Nothing left to delete, nothing to
        // ask about.
        if ticked.is_empty() {
            self.confirm_delete = false;
            return None;
        }

        let mut action = None;
        Window::new("Delete combats")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                ui.label(format!(
                    "Delete {} of the log's {} combats?",
                    ticked.len(),
                    view.combats.len()
                ));
                ui.label(
                    RichText::new(
                        "The log is rewritten without them. This cannot be undone — save a \
                         combat first if you want to keep it.",
                    )
                    .weak(),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.scope(|ui| {
                        theme::accent_rim(ui);
                        if ui.button(format!("Delete {} 🗑", ticked.len())).clicked() {
                            self.to_delete = None;
                            self.confirm_delete = false;
                            action = Some(ListAction::Keep(keep_list(view, &ticked)));
                        }
                    });
                    if ui.button("Cancel").clicked() {
                        // The ticks stay: cancelling is about the question, not
                        // about giving up what was picked out.
                        self.confirm_delete = false;
                    }
                });
            });
        action
    }

    /// The rows the filters leave, in the order the headings put them.
    ///
    /// Newest first to begin with: the fight just played is the one being
    /// looked for far more often than the first of the log.
    fn visible(&self, view: &CombatsListView<'_>) -> Vec<usize> {
        let mut visible: Vec<usize> = (0..view.combats.len())
            .filter(|&i| self.matches(&view.combats[i], view.notes))
            .collect();
        if let Some(column) = self.sort.column {
            let natural = self.sort.natural;
            visible.sort_by(|&a, &b| {
                let ordering = compare(
                    &view.combats[a],
                    &view.combats[b],
                    column,
                    view.my_handle,
                    view.notes,
                );
                if natural {
                    ordering
                } else {
                    ordering.reverse()
                }
            });
        }
        visible
    }

    /// The search box reads the note as well as the name, so a fight can be
    /// found by whatever the reader called it.
    fn matches(&self, combat: &CombatSummary, notes: &CombatNotes) -> bool {
        let needle = self.search.trim().to_lowercase();
        if !needle.is_empty() {
            let note = notes.get(&CombatNotes::key_at(combat.start));
            if !combat.identifier.to_lowercase().contains(&needle)
                && !note.to_lowercase().contains(&needle)
            {
                return false;
            }
        }
        if !self.range.matches(combat.start) {
            return false;
        }
        self.filter.matches(
            combat.environment.as_deref(),
            combat.difficulty,
            &combat.base_name,
            combat.solo,
        )
    }
}

/// The strip at the bottom of a list of fights: a little more room above and
/// below than a panel gives by default, since it is the one part of a list with
/// buttons in it.
pub fn footer_frame(ui: &Ui) -> Frame {
    Frame::side_top_panel(ui.style()).inner_margin(Margin::symmetric(8, 4))
}

/// What a list's footer says it is holding.
pub struct ListCounts {
    /// Every fight the list was given.
    pub total: usize,
    /// How many of them the filters leave on screen.
    pub shown: usize,
    /// How many are ticked, where there is anything to tick.
    pub selected: Option<usize>,
}

/// Which way the two buttons in a footer went.
#[derive(PartialEq, Eq)]
pub enum SelectAll {
    All,
    None,
}

/// The strip that closes a list of fights off, wherever one is offered: what it
/// holds, how much of it is picked, the two buttons that pick and unpick it,
/// and whatever the list itself puts at the right-hand end.
///
/// One row of a stated height, laid out from the right. Both halves are then
/// centred in that row rather than in whatever height the strip happens to
/// have: a plain `horizontal` hands its nested layout the whole remaining
/// height, so "centre" meant the centre of that — and the buttons sat above the
/// words beside them.
pub fn show_list_footer(
    ui: &mut Ui,
    counts: ListCounts,
    hint: Option<&str>,
    actions: impl FnOnce(&mut Ui),
) -> Option<SelectAll> {
    let mut select = None;
    let row = vec2(ui.available_width(), ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(row, Layout::right_to_left(Align::Center), |ui| {
        actions(ui);

        // The left-hand half, in reading order again.
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.label(RichText::new(counts.text()).weak());
            if counts.selected.is_some() {
                if ui.button("Select all").clicked() {
                    select = Some(SelectAll::All);
                }
                if ui.button("Unselect all").clicked() {
                    select = Some(SelectAll::None);
                }
            }
            if let Some(hint) = hint {
                ui.label(RichText::new(hint).weak());
            }
        });
    });
    select
}

impl ListCounts {
    /// What the strip says, in one phrase: how much of the list is picked out
    /// of how much there is, and — where the filters are hiding some of it —
    /// how much is on screen to pick from.
    fn text(&self) -> String {
        let of_the_log = match self.shown == self.total {
            true => format!("{} combats", self.total),
            false => format!("{} of {} combats", self.shown, self.total),
        };
        match self.selected {
            Some(selected) => format!("{selected} / {} selected — {of_the_log}", self.total),
            None => of_the_log,
        }
    }
}

/// What is ticked the moment the log starts being cleared: every fight on
/// screen except the newest, which is what clearing a log is nearly always
/// for — and never a fight the filters are hiding, which nobody could untick.
fn all_but_newest(view: &CombatsListView<'_>, visible: &[usize]) -> FxHashSet<NaiveDateTime> {
    let newest = visible.iter().map(|&i| view.combats[i].start).max();
    visible
        .iter()
        .map(|&i| view.combats[i].start)
        .filter(|&start| Some(start) != newest)
        .collect()
}

/// The fights that survive being cleared, by their place in the log: everything
/// that was not ticked, in the order the analyzer holds them — which is the
/// order the log has to be rewritten in.
fn keep_list(view: &CombatsListView<'_>, ticked: &FxHashSet<NaiveDateTime>) -> Vec<usize> {
    (0..view.combats.len())
        .filter(|&i| !ticked.contains(&view.combats[i].start))
        .collect()
}

/// Which DPS figure a row shows, and whose it is: the reader's own where the
/// log says which player they are *and* they were in that fight, else the best
/// anyone in it managed.
///
/// Falling back to the best rather than leaving the cell empty: a fight the
/// reader sat out is still a fight they may be looking for, and the figure that
/// says how it went is the one at the top of it. Whose it is comes back with it
/// so the cell can say.
pub fn dps_shown<'a>(combat: &'a CombatSummary, my_handle: Option<&str>) -> Option<(&'a str, f64)> {
    let mine =
        my_handle.and_then(|handle| combat.players.iter().find(|player| player.handle == handle));
    // The list is sorted by DPS, so the best is the first entry.
    let player = mine.or_else(|| combat.players.first())?;
    Some((player.handle.as_str(), player.dps))
}

/// What the DPS column is headed, which depends on whose it is.
pub fn dps_header(my_handle: Option<&str>) -> &'static str {
    match my_handle {
        Some(_) => "My DPS",
        // Said plainly rather than left as "DPS": a column that quietly means
        // somebody else's figure is worse than one that says so.
        None => "Top DPS",
    }
}

/// A column of a combats table, in the order they are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatColumn {
    /// Where it was fought: space, ground, a shuttle.
    Type,
    Size,
    /// What kind of content it was — a TFO, a patrol, and so on.
    Content,
    Map,
    Level,
    Start,
    Time,
    Dps,
    Note,
}

impl CombatColumn {
    pub const ALL: [CombatColumn; 9] = [
        CombatColumn::Type,
        CombatColumn::Size,
        CombatColumn::Content,
        CombatColumn::Map,
        CombatColumn::Level,
        CombatColumn::Start,
        CombatColumn::Time,
        CombatColumn::Dps,
        CombatColumn::Note,
    ];

    /// The heading. The DPS column says whose figures it holds, so it is the one
    /// that needs telling.
    fn heading(self, my_handle: Option<&str>) -> &'static str {
        match self {
            CombatColumn::Map => "Map",
            CombatColumn::Content => "Content",
            CombatColumn::Type => "Type",
            CombatColumn::Size => "Size",
            CombatColumn::Level => "Level",
            CombatColumn::Start => "Start",
            CombatColumn::Time => "Time",
            CombatColumn::Dps => dps_header(my_handle),
            CombatColumn::Note => "Note",
        }
    }

    fn hover(self) -> Option<&'static str> {
        match self {
            CombatColumn::Content => Some(
                "What kind of content the map is — a TFO, a patrol, and so on. Blank for a \
                 fight on a map the program does not recognize.",
            ),
            CombatColumn::Type => Some(
                "Where it was fought — space, ground, a shuttle. Blank for a fight on a map \
                 the program does not recognize.",
            ),
            CombatColumn::Time => Some(
                "How long the fighting lasted: the first shot to the last, which is the span \
                 the DPS beside it is per second of.",
            ),
            CombatColumn::Dps => Some(
                "Damage per second over the fight — the same figure the Summary tab shows for \
                 that player.",
            ),
            _ => None,
        }
    }
}

/// The heading row every combats table carries.
///
/// With a `sort` the headings order the rows: clicking one picks it, clicking it
/// again turns it round. Without one they are plain words — the compare picker
/// and the delete dialog keep their own order and are not asking to be sorted.
pub fn show_header_cells(
    r: &mut TableRow,
    my_handle: Option<&str>,
    sort: Option<&SortState<CombatColumn>>,
) -> Option<CombatColumn> {
    let mut clicked = None;
    for column in CombatColumn::ALL {
        let heading = column.heading(my_handle);
        r.cell(|ui| {
            let response = match sort {
                Some(sort) => show_sortable_header_cell(
                    ui,
                    sort.is_sorted_by(column),
                    sort.marker(column),
                    heading,
                    |_| {},
                ),
                None => ui.label(RichText::new(heading).strong()),
            };
            if let Some(hover) = column.hover() {
                response.clone().hover(hover);
            }
            if response.clicked() {
                clicked = Some(column);
            }
        });
    }
    clicked
}

/// The order `column` puts the rows in, read the way the column itself reads —
/// [`SortState::natural`] turns it round.
///
/// Ties fall back to when the fight was, newest first, so runs of the same map
/// on the same level do not shuffle about between frames.
fn compare(
    a: &CombatSummary,
    b: &CombatSummary,
    column: CombatColumn,
    my_handle: Option<&str>,
    notes: &CombatNotes,
) -> Ordering {
    let note_of = |combat: &CombatSummary| notes.get(&CombatNotes::key_at(combat.start)).to_owned();
    // A missing figure sorts last whichever way the column runs: a fight the
    // reader was not in has no DPS of theirs, and a map the program does not
    // know has no type — neither belongs at the top of a column about them.
    // The same for a word nobody wrote: a map the program does not know has
    // neither a content type nor an environment, and belongs under the ones it
    // does know rather than above them.
    let by_word = |a: Option<&str>, b: Option<&str>| match (a, b) {
        (Some(a), Some(b)) => a.cmp(b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    };
    let by_option = |a: Option<f64>, b: Option<f64>| match (a, b) {
        (Some(a), Some(b)) => b.total_cmp(&a),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    };
    let ordering = match column {
        CombatColumn::Map => a.map().to_lowercase().cmp(&b.map().to_lowercase()),
        CombatColumn::Content => by_word(a.category.as_deref(), b.category.as_deref()),
        CombatColumn::Type => by_word(a.environment.as_deref(), b.environment.as_deref()),
        // Solo first, which is the shorter word and the smaller team.
        CombatColumn::Size => b.solo.cmp(&a.solo),
        CombatColumn::Level => level_rank(a.difficulty).cmp(&level_rank(b.difficulty)),
        CombatColumn::Start => b.start.cmp(&a.start),
        CombatColumn::Time => b.duration.cmp(&a.duration),
        CombatColumn::Dps => by_option(
            dps_shown(a, my_handle).map(|(_, dps)| dps),
            dps_shown(b, my_handle).map(|(_, dps)| dps),
        ),
        CombatColumn::Note => match (note_of(a).is_empty(), note_of(b).is_empty()) {
            // A fight nobody wrote about goes below the ones somebody did.
            (false, true) => Ordering::Less,
            (true, false) => Ordering::Greater,
            _ => note_of(a).to_lowercase().cmp(&note_of(b).to_lowercase()),
        },
    };
    ordering.then_with(|| b.start.cmp(&a.start))
}

/// Where a level sits among the others, with "not worked out" last.
fn level_rank(difficulty: Option<Difficulty>) -> u8 {
    match difficulty {
        Some(Difficulty::Normal) => 0,
        Some(Difficulty::Advanced) => 1,
        Some(Difficulty::Elite) => 2,
        _ => 3,
    }
}

/// One combat's cells, in the order [`show_header_cells`] names them.
///
/// `fold` is the fold-out state of this row's player list, where the caller
/// offers one; the arrow is drawn (and toggles it) only for a fight with more
/// than one player in it, since folding out a solo run says nothing new.
pub fn show_combat_cells(
    r: &mut TableRow,
    combat: &CombatSummary,
    note: &str,
    dps: Option<(&str, f64)>,
    fold: Option<&mut bool>,
    formatter: &mut NumberFormatter,
) {
    r.cell(|ui| {
        ui.horizontal(|ui| {
            // At the start of the row rather than beside the map name: it folds
            // the whole row out, and that is where a reader looks for it.
            match fold.filter(|_| combat.players.len() > 1) {
                Some(fold) => {
                    let symbol = if *fold { "⏷" } else { "⏵" };
                    if ui
                        .add(Button::selectable(false, symbol).min_size(ARROW_SIZE))
                        .hover("Show what each player did.")
                        .clicked()
                    {
                        *fold = !*fold;
                    }
                }
                // The room is kept either way, so the column stays in line
                // whether a fight has an arrow or not.
                None => {
                    ui.add_space(ARROW_SIZE.x + ui.spacing().item_spacing.x);
                }
            }
            ui.label(combat.environment.as_deref().unwrap_or("—"));
        });
    });
    r.cell(|ui| {
        ui.label(if combat.solo { "Solo" } else { "Team" });
    });
    r.cell(|ui| {
        ui.label(combat.category.as_deref().unwrap_or("—"));
    });
    r.cell(|ui| {
        ui.label(combat.map());
    });
    r.cell(|ui| {
        ui.label(combat.difficulty.and_then(|d| d.label()).unwrap_or("—"));
    });
    r.cell(|ui| {
        ui.label(combat.start.format("%d.%m %H:%M").to_string());
    });
    r.cell(|ui| {
        ui.label(format_duration_hms(combat.duration));
    });
    r.cell(|ui| {
        match dps {
            // Whose figure it is is one hover away, which is what tells a
            // fight the reader was in from one they only have the best of.
            Some((handle, dps)) => {
                ui.label(formatter.format_with_automated_suffixes(dps))
                    .hover(handle);
            }
            // Nobody at all fought it: not a fight, whatever the log says.
            None => {
                ui.label("—");
            }
        }
    });
    r.cell(|ui| {
        ui.label(note);
    });
}

/// A folded-out player: their handle under the map name, their DPS under the
/// DPS column, so the figures stay in one column with the combats above them.
fn show_player_cells(r: &mut TableRow, player: &PlayerSummary, formatter: &mut NumberFormatter) {
    r.cell(|ui| {
        ui.horizontal(|ui| {
            ui.add_space(ARROW_SIZE.x + ui.spacing().item_spacing.x * 2.0);
            ui.label(RichText::new(&player.handle).weak());
        });
    });
    for _ in 0..CombatColumn::ALL.len() - 3 {
        r.cell(|_| {});
    }
    r.cell(|ui| {
        ui.label(
            RichText::new(formatter.format_with_automated_suffixes(player.dps))
                .color(theme::palette().ok),
        );
    });
    r.cell(|_| {});
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::Difficulty;

    fn combat(map: &str, players: &[(&str, f64)]) -> CombatSummary {
        CombatSummary {
            name: format!("[Team] {map}"),
            identifier: format!("[Team] {map} | 2026-08-19 21:14:03 - 21:18:15"),
            base_name: map.to_owned(),
            category: Some("TFO".to_owned()),
            environment: Some("Space".to_owned()),
            difficulty: Some(Difficulty::Elite),
            solo: players.len() == 1,
            start: NaiveDateTime::default(),
            duration: chrono::Duration::seconds(252),
            players: players
                .iter()
                .map(|&(handle, dps)| PlayerSummary {
                    handle: handle.to_owned(),
                    dps,
                })
                .collect(),
        }
    }

    /// The column shows the reader's own figure when the log says who they are,
    /// and the best of the fight when it does not.
    #[test]
    fn the_dps_column_is_mine_when_the_log_knows_me() {
        let combat = combat("Infected Space", &[("@friend", 140.0), ("@me", 100.0)]);
        assert_eq!(Some(("@me", 100.0)), dps_shown(&combat, Some("@me")));
        assert_eq!(Some(("@friend", 140.0)), dps_shown(&combat, None));
        assert_eq!("My DPS", dps_header(Some("@me")));
        assert_eq!("Top DPS", dps_header(None));
    }

    /// A fight the reader sat out still says how it went: the best figure in
    /// it, under the handle it belongs to. An empty cell would read as a fight
    /// with nothing in it.
    #[test]
    fn a_fight_i_was_not_in_falls_back_to_its_best() {
        let combat = combat("Hive Space", &[("@somebody", 140.0)]);
        assert_eq!(Some(("@somebody", 140.0)), dps_shown(&combat, Some("@me")));
    }

    /// Nobody fought it at all — nothing to fall back to.
    #[test]
    fn a_combat_without_players_shows_no_figure() {
        assert_eq!(None, dps_shown(&combat("Empty", &[]), Some("@me")));
    }

    /// Ordering by the content type keeps the kinds together, and a map the
    /// program does not know goes last rather than first — an unnamed row at
    /// the top reads as the list having broken.
    #[test]
    fn the_content_column_orders_by_kind_and_leaves_the_unknown_last() {
        let notes = CombatNotes::default();
        let mut tfo = combat("Infected Space", &[]);
        tfo.category = Some("TFO".to_owned());
        let mut patrol = combat("Rescue and Search", &[]);
        patrol.category = Some("Patrol".to_owned());
        let mut unknown = combat("Combat", &[]);
        unknown.category = None;

        let order = |a: &CombatSummary, b: &CombatSummary| {
            compare(a, b, CombatColumn::Content, None, &notes)
        };
        assert_eq!(Ordering::Greater, order(&tfo, &patrol), "Patrol before TFO");
        assert_eq!(
            Ordering::Less,
            order(&tfo, &unknown),
            "the unknown goes last"
        );
        assert_eq!(Ordering::Less, order(&patrol, &unknown));
    }

    /// Two fights the ordered column cannot tell apart fall back on when they
    /// were, newest first — otherwise rows of the same map and level would
    /// shuffle about between frames.
    #[test]
    fn combats_the_column_cannot_separate_fall_back_on_when_they_were() {
        let notes = CombatNotes::default();
        let mut older = combat("Infected Space", &[]);
        older.start = NaiveDateTime::default();
        let mut newer = combat("Infected Space", &[]);
        newer.start = NaiveDateTime::default() + chrono::Duration::hours(1);

        assert_eq!(
            Ordering::Greater,
            compare(&older, &newer, CombatColumn::Map, None, &notes),
            "the same map, so the newer fight leads"
        );
    }

    /// The DPS column orders by the figure each row actually shows — mine
    /// where I was in the fight, the best of it where I was not.
    #[test]
    fn the_dps_column_orders_by_the_figure_on_screen() {
        let notes = CombatNotes::default();
        let mine = combat("Infected Space", &[("@me", 100.0)]);
        let theirs = combat("Hive Space", &[("@somebody", 400.0)]);

        assert_eq!(
            Ordering::Greater,
            compare(&mine, &theirs, CombatColumn::Dps, Some("@me"), &notes),
            "400k leads 100k, whoever's it is"
        );
        // A fight with nobody in it has no figure, and goes last.
        let empty = combat("Empty", &[]);
        assert_eq!(
            Ordering::Less,
            compare(&mine, &empty, CombatColumn::Dps, Some("@me"), &notes)
        );
    }

    #[test]
    fn the_search_box_reads_the_name_and_the_note() {
        let mut panel = CombatsPanel::new(true);
        let combat = combat("Infected Space", &[("@me", 100.0)]);
        let mut notes = CombatNotes::default();
        notes.set(&CombatNotes::key_at(combat.start), "new build");

        panel.search = "infected".to_owned();
        assert!(panel.matches(&combat, &notes), "by the name");
        panel.search = "NEW BUILD".to_owned();
        assert!(panel.matches(&combat, &notes), "by the note, any case");
        panel.search = "khitomer".to_owned();
        assert!(!panel.matches(&combat, &notes));
    }

    fn view<'a>(combats: &'a [CombatSummary], notes: &'a CombatNotes) -> CombatsListView<'a> {
        CombatsListView {
            combats,
            notes,
            my_handle: None,
            shown: None,
        }
    }

    fn at(minutes: i64) -> NaiveDateTime {
        NaiveDateTime::default() + chrono::Duration::minutes(minutes)
    }

    /// Clearing the log starts ticked on everything but the newest fight —
    /// that is what the button has always meant — and the newest is the newest
    /// of what is *on screen*, not of the log behind a filter.
    #[test]
    fn clearing_the_log_starts_on_everything_but_the_newest() {
        let notes = CombatNotes::default();
        let mut combats = [
            combat("Infected Space", &[]),
            combat("Hive Space", &[]),
            combat("Japori", &[]),
        ];
        combats[0].start = at(0);
        combats[1].start = at(60);
        combats[2].start = at(120);
        let view = view(&combats, &notes);

        let ticked = all_but_newest(&view, &[0, 1, 2]);
        assert_eq!(2, ticked.len());
        assert!(!ticked.contains(&at(120)), "the newest is kept");

        // With the newest filtered out of the list, the newest *shown* is.
        let ticked = all_but_newest(&view, &[0, 1]);
        assert!(ticked.contains(&at(0)));
        assert!(!ticked.contains(&at(60)));
    }

    /// What is deleted is what was ticked, and nothing else — the list handed
    /// to the analyzer is every other fight, in the order the log holds them.
    #[test]
    fn everything_unticked_survives_being_cleared() {
        let notes = CombatNotes::default();
        let mut combats = [
            combat("Infected Space", &[]),
            combat("Hive Space", &[]),
            combat("Japori", &[]),
        ];
        combats[0].start = at(0);
        combats[1].start = at(60);
        combats[2].start = at(120);
        let view = view(&combats, &notes);

        let ticked: FxHashSet<NaiveDateTime> = [at(60)].into_iter().collect();
        assert_eq!(vec![0, 2], keep_list(&view, &ticked));

        // Nothing ticked leaves the log exactly as it was.
        assert_eq!(vec![0, 1, 2], keep_list(&view, &FxHashSet::default()));
    }

    /// The Played window narrows by when a fight was, alongside the pickers
    /// that narrow by what it was — and against the fight's *start*, which is
    /// the one time in it the log fixes.
    #[test]
    fn the_played_window_narrows_by_when_the_fight_was() {
        let notes = CombatNotes::default();
        let mut panel = CombatsPanel::new(true);
        let mut combat = combat("Infected Space", &[("@me", 100.0)]);
        combat.start = NaiveDateTime::parse_from_str("2026-07-23 21:30", "%Y-%m-%d %H:%M").unwrap();

        panel.range.set("2026-07-23 20:00", "2026-07-23 23:00");
        assert!(panel.matches(&combat, &notes));

        panel.range.set("2026-07-24 00:00", "");
        assert!(!panel.matches(&combat, &notes), "played before the window");

        panel.range.clear();
        assert!(panel.matches(&combat, &notes));
    }

    /// Search and the pickers narrow together — a fight has to pass both, or
    /// the pickers would appear to do nothing while a search was typed.
    #[test]
    fn the_search_and_the_filters_both_have_to_pass() {
        let mut panel = CombatsPanel::new(true);
        let combat = combat("Infected Space", &[("@me", 100.0)]);
        let notes = CombatNotes::default();

        panel.search = "infected".to_owned();
        panel.filter.map = Some("Hive Space".to_owned());
        assert!(!panel.matches(&combat, &notes));
        panel.filter.map = Some("Infected Space".to_owned());
        assert!(panel.matches(&combat, &notes));
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use crate::app::theme;
    use eframe::egui::containers::panel::PanelState;

    /// The strip at the bottom of the panel has to reach the bottom of the
    /// panel, bar its rim.
    ///
    /// It is a panel of its own, laid out against the inside of the frame's
    /// margin — so a margin along the bottom left a band of empty panel under
    /// the strip, and everything in the strip read as sitting above the middle
    /// of the footer. Measured rather than eyeballed, because that is twice now.
    #[test]
    fn the_footer_reaches_the_bottom_of_the_panel() {
        let ctx = Context::default();
        theme::apply(&ctx, theme::Theme::Dark);
        let notes = CombatNotes::default();
        let combats = vec![CombatSummary {
            name: "[Solo] Japori".to_owned(),
            identifier: "[Solo] Japori | 2026-08-19 21:14:03 - 21:18:15".to_owned(),
            base_name: "Japori".to_owned(),
            category: None,
            environment: None,
            difficulty: None,
            solo: true,
            start: NaiveDateTime::default(),
            duration: chrono::Duration::seconds(60),
            players: Vec::new(),
        }];
        let mut panel = CombatsPanel::new(true);
        let mut width = 600.0;

        // Twice with the tick boxes out and twice without: the strip changes
        // height as buttons come and go, and it has to sit right either way.
        for frame in 0..4 {
            panel.to_delete = (2..4).contains(&frame).then(FxHashSet::default);
            let _ = ctx.run_ui(RawInput::default(), |ui| {
                panel.show(
                    CombatsListView {
                        combats: &combats,
                        notes: &notes,
                        my_handle: None,
                        shown: None,
                    },
                    &mut width,
                    ui,
                );
            });

            let side = PanelState::load(&ctx, Id::new("combats panel"))
                .expect("the panel was drawn")
                .rect;
            let strip = PanelState::load(&ctx, Id::new("combats footer"))
                .expect("the strip was drawn")
                .rect;
            let below = side.max.y - strip.max.y;
            assert!(
                below <= 1.5,
                "frame {frame}: {below} points of panel left under the strip"
            );
        }
    }
}
