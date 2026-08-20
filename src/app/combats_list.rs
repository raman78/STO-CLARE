//! The combats list: every fight the log holds, offered as a table.
//!
//! One place decides what a list of combats looks like, because the program
//! offers that list in three: the panel down the side of the main window (this
//! module), the compare view's picker and the delete dialog. A fight that reads
//! as `Infected Space | Team | Elite | 19.08 21:14 | 04:12 | 121.7k` in one of
//! them reads the same in the others.
//!
//! The panel is where a fight is chosen, whatever the choosing is for: reading
//! one (a double click), clearing the log of some, or picking the ones a
//! comparison is built from. The ticks are the same ticks in each case, and so
//! is the strip of buttons at the bottom; only what the mode does with them
//! differs. That is why there is no second list anywhere: the compare view used
//! to carry a picker of its own, and two lists of the same fights disagreed
//! about which of them was filtered how.
//!
//! A run fetched from the ladder is a fight in this list like any other — it is
//! simply not one of the reader's, so it leads the list whatever the headings
//! are ordering the rest by, is drawn in a colour that says so, and carries the
//! button that takes it back out again.

use std::cmp::Ordering;

use chrono::NaiveDateTime;
use eframe::egui::*;
use rustc_hash::FxHashSet;

use crate::{
    analyzer::{CombatSummary, Difficulty, PlayerSummary},
    app::compare::ComparisonSlot,
    app::{
        combat_filter::{CombatEntry, CombatFilter, DifficultyFilter},
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

/// The narrowest the panel may be dragged. It still holds the map name and the
/// DPS beside it; below that the table would be a column of ellipses.
const MIN_WIDTH: f32 = 260.0;
/// How wide the panel will make *itself* to fit its table. Enough for the
/// longest map the program knows — "[TFO] Nukara Prime: Transdimensional
/// Tactics" — beside every other column, including the two a comparison adds
/// (measured; see `print_column_widths`). Past this it stops widening on its
/// own, because a list that takes the whole window leaves nothing to read a
/// fight in.
const AUTO_WIDTH: f32 = 1200.0;

/// How much room the run's number keeps around itself inside its badge.
const BADGE_PADDING: f32 = 4.0;

/// The panel's own id, which its remembered width is kept under.
const PANEL_ID: &str = "combats panel";

/// The gap either side of a column's contents. Narrower than the tables' own
/// default: a column holding "Solo" carried five points of gap on each side,
/// which is a third as much again as the word.
const CELL_SPACING: f32 = 3.0;

/// How wide the picker that says whose figures a run is read for is drawn. Wide
/// enough for an ordinary handle without the column following the longest one
/// in the log about.
const PLAYER_PICKER_WIDTH: f32 = 130.0;

/// The size of the fold-out arrow, matching the one in the damage tables.
const ARROW_SIZE: Vec2 = vec2(14.0, 14.0);

/// What the reader asked the list to do.
pub enum ListAction {
    /// Put this fight on screen, by when it started — which is what names a
    /// fight here, whether it came out of the reader's log or off the ladder.
    Open(NaiveDateTime),
    /// Rewrite the log keeping only these fights, in the order they are in it.
    /// Everything else was ticked for deletion.
    Keep(Vec<usize>),
    /// The fights the comparison should be of, in the order they were ticked.
    /// Fewer than two of them means there is nothing to compare.
    Compare(Vec<NaiveDateTime>),
    /// Take this run fetched from the ladder out of the list.
    DropLadderRun(NaiveDateTime),
    /// Read this fight's figures for another of its players. The fight is named
    /// by when it started: with a run from the ladder on screen, the comparison
    /// is built from a log composed for it, where the same fight sits somewhere
    /// else entirely.
    ComparePlayer {
        start: NaiveDateTime,
        handle: String,
    },
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
    /// Whether the window beside the list is showing a comparison, which is
    /// what the ticks on the rows are for while it is.
    pub comparing: bool,
    /// The runs the comparison on screen is of, where there is one: which row
    /// of the list each is, its number and colour in that comparison, and whose
    /// figures are being read for it.
    pub comparison: &'a [ComparisonSlot],
    /// Runs fetched from the ladder. Fights like any other — ticked, opened,
    /// read — but not the reader's own, so they lead the list whichever way it
    /// is sorted and are drawn in a colour that says where they came from.
    pub ladder_runs: &'a [CombatSummary],
}

/// What ticking a row does, if anything.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PanelMode {
    /// No ticks: a click opens a fight.
    Browse,
    /// Ticked to be deleted from the log.
    Clearing,
    /// Ticked to be compared against each other.
    Comparing,
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
    /// What the ticks on the rows are for, if anything.
    mode: PanelMode,
    /// The fights ticked for deletion, by start time. Kept while the panel is
    /// open even when the ticking is switched off, so turning "Clear Log File"
    /// back on picks up where it was left rather than starting over.
    ///
    /// Ticked by start time rather than by index for the same reason the
    /// fold-outs are: the list is live, and a fight dropped from the log shifts
    /// every index after it — which, here, would delete the wrong fight.
    to_delete: FxHashSet<NaiveDateTime>,
    /// The fights ticked for a comparison, in the order they were ticked —
    /// which is the order they are numbered and coloured in, and the order the
    /// comparison puts its columns in. A set would leave the numbers to fall
    /// out of however the log happens to be ordered, which is not the order the
    /// reader picked them in.
    to_compare: Vec<NaiveDateTime>,
    /// Set when a tick changed this pass, whichever way it was done — a row, or
    /// the two buttons in the strip below. The comparison on screen follows all
    /// of them.
    ticks_changed: bool,
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
            mode: PanelMode::Browse,
            to_delete: Default::default(),
            to_compare: Vec::new(),
            confirm_delete: false,
            ticks_changed: false,
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
        // Folding the list away gives up what was ticked for deletion: the
        // ticks are a train of thought about the list on screen, and coming
        // back to it later is a new one. What is ticked for a comparison stays
        // — the comparison itself is still on screen, and it is those ticks.
        if !self.open {
            self.mode = PanelMode::Browse;
            self.to_delete.clear();
            self.confirm_delete = false;
        }
    }

    /// Starts the list on the map and level of the run being compared against,
    /// because that is what a comparison is nearly always of — the same fight
    /// fought differently. Nothing is locked: these are the ordinary pickers
    /// and the reader can widen them the moment they want something else.
    pub fn suggest_filter(&mut self, map: Option<String>, difficulty: DifficultyFilter) {
        self.filter.map = map;
        self.filter.difficulty = difficulty;
        self.filter.environment = None;
        self.filter_generation = self.filter_generation.wrapping_add(1);
    }

    /// The ticks the current mode is collecting, if it collects any.
    /// Whether this fight is ticked, where the mode ticks anything at all.
    fn is_ticked(&self, start: NaiveDateTime) -> Option<bool> {
        match self.mode {
            PanelMode::Browse => None,
            PanelMode::Clearing => Some(self.to_delete.contains(&start)),
            PanelMode::Comparing => Some(self.to_compare.contains(&start)),
        }
    }

    /// Which fight this is of those ticked for a comparison, from one.
    ///
    /// The number is the panel's own, not the comparison's: ticking the first
    /// fight makes it #1 there and then, before there is a second one to
    /// compare it against.
    fn compare_number(&self, start: NaiveDateTime) -> Option<usize> {
        self.to_compare
            .iter()
            .position(|&ticked| ticked == start)
            .map(|position| position + 1)
    }

    fn tick(&mut self, start: NaiveDateTime, ticked: bool) {
        let list = match self.mode {
            PanelMode::Browse => return,
            PanelMode::Clearing => {
                if ticked {
                    self.to_delete.insert(start);
                } else {
                    self.to_delete.remove(&start);
                }
                return;
            }
            PanelMode::Comparing => &mut self.to_compare,
        };
        list.retain(|&already| already != start);
        if ticked {
            list.push(start);
        }
    }

    /// How many fights the mode has ticked.
    fn ticks(&self) -> Option<usize> {
        match self.mode {
            PanelMode::Browse => None,
            PanelMode::Clearing => Some(self.to_delete.len()),
            PanelMode::Comparing => Some(self.to_compare.len()),
        }
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
        // reader drag it.
        let fits = self.fitting_width(ui);
        // Whether the reader has hold of the panel's edge right now. It is the
        // only thing that sets a width of their own — and the only thing that
        // may change one. A width read back off the panel every frame was worse
        // than useless: open the list while the log is still being read, when
        // the table is a heading and nothing else, and the width they had set
        // was overwritten by that — and written to the settings on the way out.
        let resize_id = Id::new(PANEL_ID).with("__resize");
        let dragging = ui.ctx().dragged_id() == Some(resize_id)
            || ui.ctx().drag_stopped_id() == Some(resize_id);
        // Until they do, the panel *is* its table: exactly as wide as the
        // columns come to, and following them as they change — including as
        // they arrive, which is what the first seconds of a large log look
        // like.
        if *width <= 0.0
            && dragging
            && let Some(fits) = fits
        {
            *width = fits;
        }
        let following_the_table = *width <= 0.0;
        // Pinned to the table while following it. A range is the only way to
        // say so: a panel is drawn at the width it was last drawn at, and what
        // it was last drawn at is the width of its own contents — which is what
        // the reader would be left with, rather than a panel that fits its
        // table. A width of their own is theirs, and only the area we allow
        // ourselves caps it.
        let range = match (following_the_table, fits) {
            (true, Some(fits)) => fits..=fits,
            (true, None) => MIN_WIDTH..=AUTO_WIDTH,
            (false, _) => MIN_WIDTH..=AUTO_WIDTH,
        };
        let panel = Panel::left(PANEL_ID)
            .resizable(true)
            .default_size(if *width > 0.0 { *width } else { AUTO_WIDTH })
            .size_range(range)
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
        if let Some(panel) = panel
            && dragging
        {
            *width = panel.response.rect.width().clamp(MIN_WIDTH, AUTO_WIDTH);
        }
        action
    }

    /// The narrowest the panel may be drawn: enough for the whole table, as
    /// wide as it came to when it was last drawn, up to [`AUTO_WIDTH`]. Before
    /// the first frame there is nothing to measure and the ordinary minimum
    /// stands.
    fn fitting_width(&self, ui: &Ui) -> Option<f32> {
        let content = table_content_width(ui, self.table_id())?;
        // What the table asks for is its columns; the panel around it also has
        // its frame, and the table its own scroll bar.
        let chrome = ui.spacing().scroll.bar_width + ui.spacing().item_spacing.x * 4.0;
        Some((content + chrome).clamp(MIN_WIDTH, AUTO_WIDTH))
    }

    /// The table's id.
    ///
    /// Carries the filter generation, because egui keeps a scroll area's
    /// measured size under it and a narrowed list would otherwise keep the
    /// height — and the width — it had while filtered.
    ///
    /// And the mode, because a table's measured columns are kept *by position*:
    /// picking fights for a comparison puts two columns in front of the rest,
    /// so every column after them would be drawn at the width of the one two
    /// places to its left until it had been measured again. Each mode measures
    /// its own.
    fn table_id(&self) -> Id {
        Id::new((
            "combats panel table",
            self.filter_generation,
            self.mode == PanelMode::Comparing,
        ))
    }

    fn show_contents(&mut self, view: CombatsListView<'_>, ui: &mut Ui) -> Option<ListAction> {
        // Whether a comparison is being put together is the *window's* state,
        // not the list's, so it is read every frame rather than copied when the
        // button was pressed. Copied, the two drifted apart the moment the list
        // was folded away and out again: the window was still comparing and the
        // list had gone back to browsing.
        self.mode = match (view.comparing, self.mode) {
            (true, _) => PanelMode::Comparing,
            (false, PanelMode::Comparing) => PanelMode::Browse,
            (false, mode) => mode,
        };
        let mut action = None;
        ui.horizontal(|ui| {
            ui.heading("Combats");
        });
        ui.separator();

        ui.horizontal(|ui| {
            ui.label("Search:");
            // What is left of the row, not everything egui would give it: a
            // field asking for the maximum reports itself that wide, and the
            // panel sizes itself to what it holds — so the box alone made the
            // panel wider than its table and would not let it be dragged in.
            let room = ui.available_width();
            TextEdit::singleline(&mut self.search)
                .desired_width(room)
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
        // The pickers and the window of time sit in a scroll area of their own.
        // A picker is drawn at the width it was given whether or not the row
        // has the room, and what it overflows by would otherwise push the panel
        // wider — the panel is exactly as wide as its table, and a filter is no
        // reason to widen it. Narrow it far enough and these scroll instead.
        ScrollArea::horizontal()
            .id_salt("combats filters")
            .auto_shrink([false, true])
            .show(ui, |ui| {
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
                // Its own row: the two fields and their presets do not fit
                // beside the pickers, and wrapped in with them the row reads as
                // one long filter rather than as "what it was" and "when it
                // was".
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
            });
        let visible = self.visible(&view);
        // A tick on a fight the filters have since hidden would act on
        // something nobody can see; the ticks are what is on screen.
        // The runs from the ladder count as on screen: they are rows of this
        // list, and the filters below do not reach them — they are not fights
        // out of the log being filtered. Left out of this, a run ticked for a
        // comparison was unticked again by the next frame, which is the whole
        // of what "it works oddly" was.
        let shown: FxHashSet<NaiveDateTime> = view
            .ladder_runs
            .iter()
            .map(|run| run.start)
            .chain(visible.iter().map(|&i| view.combats[i].start))
            .collect();
        match self.mode {
            PanelMode::Browse => (),
            PanelMode::Clearing => self.to_delete.retain(|start| shown.contains(start)),
            PanelMode::Comparing => self.to_compare.retain(|start| shown.contains(start)),
        }

        // Claimed before the table, so the strip sits at the bottom of the
        // panel whatever the table does — and sized by what is in it rather
        // than by a guess at its height, which is what left it half off the
        // edge once the table filled up.
        Panel::bottom("combats footer")
            .frame(footer_frame(ui))
            .show_inside(ui, |ui| {
                if let Some(footer_action) = self.show_footer(&view, &visible, ui) {
                    action = Some(footer_action);
                }
            });
        let comparing = self.mode == PanelMode::Comparing;
        let mut clicked_heading = None;
        let mut formatter = NumberFormatter::new();
        // Whatever the strip at the bottom left.
        let table_height = ui.available_height().at_least(ROW_HEIGHT);
        // Nothing in the table wraps. A row is one row tall, and — more to the
        // point — a cell that wraps asks for less width than its text needs, so
        // the panel sizing itself to its table would never grow to fit a long
        // map name: the name would fold instead, and the panel would settle
        // around the fold.
        ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);
        Table::new(ui)
            .id(self.table_id())
            // Tighter than the tables of figures: this one is columns of short
            // words, where the default gap is a third of a column again.
            .cell_spacing(CELL_SPACING)
            .max_scroll_height(table_height)
            .header(HEADER_HEIGHT)
            .body(ROW_HEIGHT, |t| {
                // The runs fetched from the ladder lead the list, whatever the
                // headings are ordering the rest by: they are not the reader's
                // fights. Whole rows in the theme's own "something is going on"
                // colour, which is what says where they came from without a
                // column for it.
                for run in view.ladder_runs {
                    if let Some(from_row) =
                        self.show_row(t, &view, run, true, comparing, &mut formatter)
                    {
                        action = Some(from_row);
                    }
                }
                for index in visible.iter().copied() {
                    let combat = &view.combats[index];
                    if let Some(from_row) =
                        self.show_row(t, &view, combat, false, comparing, &mut formatter)
                    {
                        action = Some(from_row);
                    }
                }
            })
            .header_row(|r| {
                // The tick column's heading stays empty, like the compare
                // picker's.
                r.cell(|_| {});
                clicked_heading =
                    show_header_cells(r, dps_header(view.my_handle, comparing), Some(&self.sort));
                if comparing {
                    // In the order the cells below them are drawn: whose
                    // figures are being read, then which run of the comparison
                    // this is.
                    r.cell(|ui| {
                        ui.label(RichText::new("Player").strong());
                    });
                    r.cell(|ui| {
                        ui.label(RichText::new("#").strong());
                    });
                }
                // The column the runs from the ladder are taken back out of,
                // which is there only while there are any.
                if !view.ladder_runs.is_empty() {
                    r.cell(|_| {});
                }
            });
        if let Some(column) = clicked_heading {
            self.sort.clicked(column);
        }
        // Reported after the table has drawn: the rows borrow the ticks while
        // they do.
        if self.ticks_changed && comparing {
            self.ticks_changed = false;
            action = Some(ListAction::Compare(self.to_compare.clone()));
        }

        action
    }

    /// One fight of the list, wherever it came from.
    ///
    /// A run fetched from the ladder is drawn by this too: same columns, same
    /// ticks, same fold-out — in the theme's "something is going on" colour,
    /// and with the button that takes it back out of the list where the others
    /// have nothing.
    #[allow(clippy::too_many_arguments)]
    fn show_row(
        &mut self,
        t: &mut TableBody,
        view: &CombatsListView<'_>,
        combat: &CombatSummary,
        from_the_ladder: bool,
        comparing: bool,
        formatter: &mut NumberFormatter,
    ) -> Option<ListAction> {
        let mut action = None;
        let color = from_the_ladder.then(|| theme::palette().busy);
        let note = view.notes.get(&CombatNotes::key_at(combat.start));
        let unfolded = self.unfolded.contains(&combat.start);
        let mut fold = unfolded;
        // While the log is being cleared the highlight is the tick, not the
        // fight on screen: what is about to be deleted is the thing worth
        // seeing at a glance. A run from the ladder is not in the log and
        // cannot be deleted from it, so it has nothing to tick then.
        let ticked = match (self.mode, from_the_ladder) {
            (PanelMode::Clearing, true) => None,
            _ => self.is_ticked(combat.start),
        };
        let mut tick = ticked.unwrap_or(false);
        let highlighted = match ticked {
            Some(ticked) => ticked,
            None => view.shown == Some(combat.start),
        };
        // Which of the ticked fights this is, and whose figures the comparison
        // is reading for it. The number is the panel's own, so ticking one
        // fight numbers it before there is a second to compare it against; the
        // player comes from the comparison once there is one, and from the
        // fight's best until then.
        let number = self.compare_number(combat.start);
        let slot = view
            .comparison
            .iter()
            .find(|slot| slot.start == combat.start);
        let player_shown = slot
            .map(|slot| slot.player.as_str())
            .or_else(|| combat.players.first().map(|player| player.handle.as_str()))
            .unwrap_or("—");
        let mut player = None;
        let mut player_cell = Rect::NOTHING;
        let mut drop_cell = Rect::NOTHING;
        let mut drop_run = false;

        let row = t.selectable_row(highlighted, |r| {
            // Kept whether or not there is anything to tick, so turning "Clear
            // Log File" on makes the boxes appear rather than shifting every
            // column of the table sideways under the reader's pointer.
            r.cell(|ui| {
                ui.visuals_mut().override_text_color = color;
                ui.add_visible(ticked.is_some(), Checkbox::without_text(&mut tick));
            });
            show_combat_cells(
                r,
                combat,
                note,
                // While a comparison is being put together the figure is the
                // one it is reading — the player picked beside it — rather than
                // the reader's own.
                match comparing {
                    true => dps_shown(combat, Some(player_shown)),
                    false => dps_shown(combat, view.my_handle),
                },
                Some(&mut fold),
                color,
                formatter,
            );
            if comparing {
                // At the end of the row, after the fight's own columns: what a
                // fight *is* reads the same whether or not a comparison is
                // being put together, and these two are about the comparison
                // rather than about the fight.
                player_cell = r
                    .cell(|ui| {
                        ui.visuals_mut().override_text_color = color;
                        show_player_picker(ui, combat, player_shown, &mut player);
                    })
                    .rect;
                r.cell(|ui| {
                    ui.set_min_width(text_width(ui, "999") + BADGE_PADDING * 2.0);
                    match number {
                        Some(number) => show_number_badge(ui, number),
                        None => {
                            ui.label("");
                        }
                    }
                });
            }
            if !view.ladder_runs.is_empty() {
                drop_cell = r
                    .cell(|ui| {
                        if !from_the_ladder {
                            return;
                        }
                        ui.visuals_mut().override_text_color = color;
                        if ui
                            .button("✕")
                            .hover("Take this run out of the list.")
                            .clicked()
                        {
                            drop_run = true;
                        }
                    })
                    .rect;
            }
        });

        // A click that landed in one of the buttons is that button's, not the
        // row's.
        let clicked_a_button = row
            .interact_pointer_pos()
            .is_some_and(|pos| player_cell.contains(pos) || drop_cell.contains(pos));
        if ticked.is_some() {
            if row.clicked() && !clicked_a_button {
                tick = !tick;
            }
        } else if row.double_clicked() {
            action = Some(ListAction::Open(combat.start));
        }
        if Some(tick) != ticked {
            self.tick(combat.start, tick);
            // A fight leaves the comparison (or rejoins it) there and then,
            // rather than waiting for the whole thing to be built again: taking
            // a run out and looking is the quickest question there is.
            self.ticks_changed = true;
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
                    show_player_cells(r, player, 1, formatter);
                });
            }
        }

        if drop_run {
            action = Some(ListAction::DropLadderRun(combat.start));
        }
        if let Some(handle) = player {
            action = Some(ListAction::ComparePlayer {
                start: combat.start,
                handle,
            });
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
        let counts = ListCounts {
            total: view.combats.len(),
            shown: visible.len(),
            selected: self.ticks(),
        };
        let mut action = None;
        // Past a certain size a comparison gives something up; the list says
        // so where the ticks are, since that is where it can still be undone.
        let hint = match self.mode {
            PanelMode::Comparing => crate::app::compare::selection_hint(self.to_compare.len()),
            _ => None,
        };
        let select = show_list_footer(ui, counts, hint, |ui| match self.mode {
            // Nothing to press: the comparison is whatever is ticked, and it
            // follows the ticks as they are made. A button would only ask the
            // reader to confirm what they have already said.
            PanelMode::Comparing => {
                if self.to_compare.len() < 2 {
                    ui.label(
                        RichText::new("Tick two fights to compare them against each other.").weak(),
                    );
                }
            }
            // Clearing the log, or offering to.
            PanelMode::Browse | PanelMode::Clearing => {
                // A toggle rather than a button that opens a window: the fights
                // are already listed here, so ticking them is done in the list
                // itself instead of in a second copy of it.
                if ui
                    .steady_toggle(self.mode == PanelMode::Clearing, "Clear Log File")
                    .hover(
                        "Tick the fights to delete from the log. Everything left unticked \
                         stays in it.",
                    )
                    .clicked()
                {
                    self.mode = match self.mode {
                        PanelMode::Clearing => PanelMode::Browse,
                        _ => {
                            // Everything but the newest to begin with, which is
                            // what clearing a log is nearly always for — but
                            // only the first time: what was ticked before is
                            // remembered while the panel stays open.
                            if self.to_delete.is_empty() {
                                self.to_delete = all_but_newest(view, visible);
                            }
                            PanelMode::Clearing
                        }
                    };
                }

                let ticks = self.to_delete.len();
                if self.mode == PanelMode::Clearing && ticks > 0 {
                    ui.scope(|ui| {
                        theme::accent_rim(ui);
                        if ui
                            .button(format!("Delete {ticks} ticked 🗑"))
                            .hover("Rewrite the log without them. This cannot be undone.")
                            .clicked()
                        {
                            // A single fight goes straight away — it is one
                            // row, ticked deliberately, and asking about it is
                            // noise. Anything more is worth a question, because
                            // the log is rewritten and there is no way back.
                            if ticks > 1 {
                                self.confirm_delete = true;
                            } else {
                                let ticked = std::mem::take(&mut self.to_delete);
                                action = Some(ListAction::Keep(keep_list(view, &ticked)));
                            }
                        }
                    });
                }
            }
        });
        if let Some(confirmed) = self.show_delete_confirmation(view, ui) {
            action = Some(confirmed);
        }
        if let Some(select) = select {
            // Everything on screen, which includes the runs standing above the
            // list: they are rows of it like any other.
            let shown = view
                .ladder_runs
                .iter()
                .map(|run| run.start)
                .chain(visible.iter().map(|&i| view.combats[i].start));
            match (select, self.mode) {
                (SelectAll::All, _) => {
                    for start in shown {
                        self.tick(start, true);
                    }
                }
                (SelectAll::None, PanelMode::Clearing) => self.to_delete.clear(),
                (SelectAll::None, PanelMode::Comparing) => self.to_compare.clear(),
                (SelectAll::None, PanelMode::Browse) => (),
            }
            // Ticking or unticking the lot is the same change as ticking a row,
            // and the comparison on screen has to follow it just the same.
            if self.mode == PanelMode::Comparing {
                action = Some(ListAction::Compare(self.to_compare.clone()));
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
        let ticked = self.to_delete.clone();
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
                    "Delete {}/{} combats?",
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
                            self.to_delete.clear();
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
            Some(selected) => format!("{selected}/{} selected", self.total),
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

/// What the DPS column is headed.
///
/// Plain "DPS" wherever the figure is the one it should be — the reader's own,
/// or the player picked beside it in a comparison. "Top DPS" only where the
/// program does not know whose log it is reading and is showing the best of
/// each fight instead: a column that quietly means somebody else's figure is
/// worse than one that says so.
pub fn dps_header(my_handle: Option<&str>, comparing: bool) -> &'static str {
    match (comparing, my_handle) {
        (true, _) | (false, Some(_)) => "DPS",
        (false, None) => "Top DPS",
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

    /// The heading. The DPS column is the one that has to be told what it says:
    /// whose figures it holds depends on the list it is in.
    fn heading(self, dps: &'static str) -> &'static str {
        match self {
            CombatColumn::Map => "Map",
            CombatColumn::Content => "Content",
            CombatColumn::Type => "Type",
            CombatColumn::Size => "Size",
            CombatColumn::Level => "Level",
            CombatColumn::Start => "Start",
            CombatColumn::Time => "Time",
            CombatColumn::Dps => dps,
            CombatColumn::Note => "Note",
        }
    }

    /// The widest value the column can ever hold, or `None` for one whose
    /// contents nobody can put a bound on.
    ///
    /// A column of short words — Solo/Team, a level, a length — is drawn to
    /// that width rather than to whatever the rows on screen happen to need.
    /// Two reasons: it stops a column of four-letter words taking as much room
    /// as the map name beside it, and it makes the list in the panel and the
    /// picker in a comparison the same width down to the point, which they were
    /// not while each measured its own rows.
    fn widest(self) -> Option<&'static str> {
        match self {
            CombatColumn::Type => Some("Shuttle"),
            CombatColumn::Size => Some("Team"),
            CombatColumn::Content => Some("Patrol"),
            CombatColumn::Level => Some("Advanced"),
            CombatColumn::Start => Some("19.08 21:14"),
            // An hour-long fight is rare enough to be allowed to widen its own
            // column rather than have every list carry the room for one.
            CombatColumn::Time => Some("59:59"),
            CombatColumn::Dps => Some("999.9k"),
            // A map's name and a reader's note are as long as they are.
            CombatColumn::Map | CombatColumn::Note => None,
        }
    }

    /// How wide the column is drawn, where it has a width of its own: enough
    /// for its widest value, and for its heading with the sort mark beside it.
    ///
    /// The mark's room is kept in every column, sorting or not, so that
    /// clicking a heading cannot shift the columns under it. What is *not* kept
    /// is the padding the tables of figures put after the mark — a whole space
    /// character on top of a column holding the word "Solo".
    fn width(self, ui: &Ui, dps: &'static str) -> Option<f32> {
        Some(text_width(ui, self.widest()?).max(self.heading_width(ui, dps)))
    }

    /// What the heading itself needs: its words and the room for the sort mark.
    fn heading_width(self, ui: &Ui, dps: &'static str) -> f32 {
        text_width(ui, self.heading(dps)) + sort_marker_width(ui)
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
    dps: &'static str,
    sort: Option<&SortState<CombatColumn>>,
) -> Option<CombatColumn> {
    let mut clicked = None;
    for column in CombatColumn::ALL {
        let heading = column.heading(dps);
        r.cell(|ui| {
            let sorted = sort.is_some_and(|sort| sort.is_sorted_by(column));
            let width = column
                .width(ui, dps)
                .unwrap_or_else(|| column.heading_width(ui, dps));
            ui.set_min_width(width);
            let response = match sort {
                Some(sort) => show_sortable_header_cell_sized(
                    ui,
                    width,
                    sorted,
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
#[allow(clippy::too_many_arguments)]
pub fn show_combat_cells(
    r: &mut TableRow,
    combat: &CombatSummary,
    note: &str,
    dps: Option<(&str, f64)>,
    fold: Option<&mut bool>,
    // A colour for the whole row, where it is not one of the ordinary fights:
    // the run fetched from the ladder, which came from somewhere else.
    color: Option<Color32>,
    formatter: &mut NumberFormatter,
) {
    // A column of short words is drawn to the width its own kind of value
    // needs (see `width`), rather than to whatever the rows on screen happen to
    // come to — so the same column is the same width in the list and in a
    // comparison. The map and the note have no such width: they are as long as
    // they are, and they are what the panel is dragged wider for.
    let cell = |r: &mut TableRow, column: CombatColumn, text: String| {
        r.cell(|ui| {
            ui.visuals_mut().override_text_color = color;
            if let Some(width) = column.width(ui, "DPS") {
                ui.set_min_width(width);
            }
            ui.label(text);
        });
    };
    r.cell(|ui| {
        ui.visuals_mut().override_text_color = color;
        if let Some(width) = CombatColumn::Type.width(ui, "DPS") {
            ui.set_min_width(width);
        }
        ui.horizontal(|ui| {
            // At the start of the row rather than beside the map name: it folds
            // the whole row out, and that is where a reader looks for it.
            // The arrow is drawn either way and only made invisible where
            // there is nothing to fold out, so a fight with players under it
            // and one without line up to the point. Room measured out beside it
            // instead was room of a different size — a button is its glyph plus
            // its own padding — and the column drew two points wider or
            // narrower depending on which rows were in it.
            let can_open = combat.players.len() > 1;
            let symbol = match fold.as_deref() {
                Some(true) => "⏷",
                _ => "⏵",
            };
            let arrow = ui.add_visible(
                can_open,
                Button::selectable(false, symbol).min_size(ARROW_SIZE),
            );
            if can_open
                && arrow.hover("Show what each player did.").clicked()
                && let Some(fold) = fold
            {
                *fold = !*fold;
            }
            ui.label(combat.environment.as_deref().unwrap_or("—"));
        });
    });
    cell(
        r,
        CombatColumn::Size,
        if combat.solo { "Solo" } else { "Team" }.to_owned(),
    );
    cell(
        r,
        CombatColumn::Content,
        combat.category.as_deref().unwrap_or("—").to_owned(),
    );
    r.cell(|ui| {
        ui.visuals_mut().override_text_color = color;
        ui.label(combat.map());
    });
    cell(
        r,
        CombatColumn::Level,
        combat
            .difficulty
            .and_then(|d| d.label())
            .unwrap_or("—")
            .to_owned(),
    );
    cell(
        r,
        CombatColumn::Start,
        combat.start.format("%d.%m %H:%M").to_string(),
    );
    cell(r, CombatColumn::Time, format_duration_hms(combat.duration));
    r.cell(|ui| {
        ui.visuals_mut().override_text_color = color;
        if let Some(width) = CombatColumn::Dps.width(ui, "Top DPS") {
            ui.set_min_width(width);
        }
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
        ui.visuals_mut().override_text_color = color;
        ui.label(note);
    });
}

/// Whose figures a comparison reads for this fight.
///
/// A picker where the fight holds more than one player, and the handle itself
/// where it holds one — a menu of a single entry is a menu that answers nothing.
fn show_player_picker(
    ui: &mut Ui,
    combat: &CombatSummary,
    shown: &str,
    picked: &mut Option<String>,
) {
    ui.set_min_width(PLAYER_PICKER_WIDTH);
    if combat.players.len() < 2 {
        ui.label(shown);
        return;
    }
    ComboBox::new(("combats player", combat.start), "")
        .selected_text(shown)
        .width(PLAYER_PICKER_WIDTH)
        .show_ui(ui, |ui| {
            for candidate in combat.players.iter() {
                if ui
                    .selectable_label(candidate.handle == shown, &candidate.handle)
                    .clicked()
                {
                    *picked = Some(candidate.handle.clone());
                }
            }
        });
}

/// Which run of a comparison this is, drawn as a badge in that run's own
/// colour.
///
/// Filled rather than written in the colour: the series colours are picked to
/// tell lines on a chart apart against the window's background, and several of
/// them disappear into the blue of a row that is picked out. A patch of the
/// colour with the number over it holds up on any row, and says the same thing.
fn show_number_badge(ui: &mut Ui, number: usize) {
    let (fill, on_fill) = theme::badge_colors(theme::series_color(number - 1));
    let font = TextStyle::Body.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(format!("#{number}"), font, on_fill);
    let (rect, _) = ui.allocate_exact_size(
        galley.size() + vec2(BADGE_PADDING * 2.0, 0.0),
        Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, ui.visuals().widgets.inactive.corner_radius, fill);
    let text_at = rect.center() - galley.size() * 0.5;
    ui.painter().galley(text_at, galley, Color32::PLACEHOLDER);
}

/// A folded-out player: their handle under the map name, their DPS under the
/// DPS column, so the figures stay in one column with the combats above them.
fn show_player_cells(
    r: &mut TableRow,
    player: &PlayerSummary,
    leading: usize,
    formatter: &mut NumberFormatter,
) {
    for _ in 0..leading {
        r.cell(|_| {});
    }
    // Under the columns they belong to: the handle where the map's name is —
    // both are names, and a handle under "Type" made that column as wide as the
    // longest one in the log — and the figure under the DPS the fight's own row
    // shows, which is what it is to be read against.
    for column in CombatColumn::ALL {
        match column {
            CombatColumn::Map => {
                r.cell(|ui| {
                    ui.label(RichText::new(&player.handle).weak());
                });
            }
            CombatColumn::Dps => {
                r.cell(|ui| {
                    ui.label(
                        RichText::new(formatter.format_with_automated_suffixes(player.dps))
                            .color(theme::palette().ok),
                    );
                });
            }
            _ => {
                r.cell(|_| {});
            }
        }
    }
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
        assert_eq!("DPS", dps_header(Some("@me"), false));
        assert_eq!("DPS", dps_header(Some("@me"), true));
        // Only where nobody knows whose figures these are does the heading say
        // so.
        assert_eq!("Top DPS", dps_header(None, false));
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
            comparing: false,
            comparison: &[],
            ladder_runs: &[],
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

    /// Folding the list away and out again must not lose track of what the
    /// window beside it is doing: the comparison is still on screen, so the
    /// ticks it is built from are still what the rows are for.
    #[test]
    fn the_list_follows_the_window_rather_than_remembering_it() {
        let notes = CombatNotes::default();
        let combats = [combat("Infected Space", &[("@me", 100.0)])];
        let mut panel = CombatsPanel::new(true);
        let view = |comparing| CombatsListView {
            combats: &combats,
            notes: &notes,
            my_handle: None,
            shown: None,
            comparing,
            comparison: &[],
            ladder_runs: &[],
        };

        let ctx = Context::default();
        crate::app::theme::apply(&ctx, crate::app::theme::Theme::Dark);
        fn draw(ctx: &Context, panel: &mut CombatsPanel, view: CombatsListView<'_>) {
            let mut view = Some(view);
            let _ = ctx.run_ui(RawInput::default(), |ui| {
                if let Some(view) = view.take() {
                    panel.show(view, &mut 600.0, ui);
                }
            });
        }

        draw(&ctx, &mut panel, view(true));
        assert_eq!(PanelMode::Comparing, panel.mode);

        // Folded away and back out while the window keeps comparing.
        panel.toggle();
        draw(&ctx, &mut panel, view(true));
        panel.toggle();
        draw(&ctx, &mut panel, view(true));
        assert_eq!(
            PanelMode::Comparing,
            panel.mode,
            "the window is still comparing, so the list still is"
        );

        draw(&ctx, &mut panel, view(false));
        assert_eq!(PanelMode::Browse, panel.mode);
    }

    /// A run from the ladder is a row of this list like any other: it can be
    /// ticked for a comparison, and it stays ticked.
    ///
    /// The ticks are pruned to what is on screen, so that nothing can be acted
    /// on out of sight. The runs are not fights out of the log the filters are
    /// narrowing, so they were not counted as on screen — and a run ticked for
    /// a comparison was unticked again by the very next frame.
    #[test]
    fn a_run_from_the_ladder_can_be_ticked_like_any_other_fight() {
        let ctx = Context::default();
        crate::app::fonts::install(&ctx);
        theme::apply(&ctx, theme::Theme::Dark);
        let notes = CombatNotes::default();
        let mine = [combat("Japori", &[("@me", 100.0)])];
        let mut run = combat("Infected: The Conduit", &[("@somebody", 140.0)]);
        run.start = NaiveDateTime::default() + chrono::Duration::hours(3);
        let runs = [run];

        let mut panel = CombatsPanel::new(true);
        let mut width = 900.0;
        // Ticked from here rather than by pointing at the box, which is what
        // the row does when it is clicked.
        panel.mode = PanelMode::Comparing;
        panel.tick(runs[0].start, true);
        assert_eq!(Some(1), panel.compare_number(runs[0].start));

        for _ in 0..2 {
            let mut view = Some(CombatsListView {
                combats: &mine,
                notes: &notes,
                my_handle: None,
                shown: None,
                comparing: true,
                comparison: &[],
                ladder_runs: &runs,
            });
            let _ = ctx.run_ui(RawInput::default(), |ui| {
                if let Some(view) = view.take() {
                    panel.show(view, &mut width, ui);
                }
            });
        }

        assert_eq!(
            Some(1),
            panel.compare_number(runs[0].start),
            "the run is still ticked, and still the first of them"
        );
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

    /// A column of short words is the same width whatever is in the rows.
    ///
    /// Otherwise the list in the panel and the picker in a comparison — two
    /// tables of the same columns over different fights — line up at nothing,
    /// and a column of four-letter words takes as much room as the map name
    /// beside it just because its heading is long.
    #[test]
    fn a_column_of_short_words_does_not_follow_its_rows() {
        let short = measured_widths(&[a_combat("Japori", "Solo", None)], false);
        let long = measured_widths(
            &[a_combat(
                "[TFO] Khitomer Vortex: Into the Breach",
                "Team",
                Some(Difficulty::Advanced),
            )],
            false,
        );
        assert!(!short.is_empty(), "the table was drawn");

        // The map's column is what a longer name widens.
        let map = 4;
        assert!(long[map] > short[map], "{short:?} vs {long:?}");
        // Everything of a fixed kind stays put.
        for (column, width) in short.iter().enumerate() {
            if column == map {
                continue;
            }
            assert_eq!(
                *width, long[column],
                "column {column}: {short:?} vs {long:?}"
            );
        }
    }

    /// The same column is the same width whichever the list is drawn for.
    ///
    /// Picking fights for a comparison puts two columns in front of the rest.
    /// A table's measured columns are kept by *position*, so every column after
    /// them was being drawn at the width of the one two places to its left —
    /// which is why Size and Time did not line up between the two views of what
    /// is otherwise the same table, drawn by the same code.
    #[test]
    fn a_column_is_the_same_width_in_both_views() {
        let combats = [a_combat(
            "Infected: The Conduit",
            "Team",
            Some(Difficulty::Elite),
        )];
        let browsing = measured_widths(&combats, false);
        let comparing = measured_widths(&combats, true);

        // The two the comparison adds stand at the end; everything before them
        // is the fight's own columns, and they have to match one for one.
        assert_eq!(browsing.len() + 2, comparing.len());
        assert_eq!(
            browsing[..],
            comparing[..browsing.len()],
            "{browsing:?} against {comparing:?}"
        );
    }

    /// The columns of a list drawn on its own, so one measurement cannot pick
    /// up the widths another left behind: egui keeps a table's measured columns
    /// under its id, and both lists use the same one.
    fn measured_widths(combats: &[CombatSummary], comparing: bool) -> Vec<f32> {
        measured_widths_with(combats, comparing, &[])
    }

    fn measured_widths_with(
        combats: &[CombatSummary],
        comparing: bool,
        comparison: &[ComparisonSlot],
    ) -> Vec<f32> {
        let ctx = Context::default();
        crate::app::fonts::install(&ctx);
        theme::apply(&ctx, theme::Theme::Dark);
        let notes = CombatNotes::default();
        let mut panel = CombatsPanel::new(true);
        let mut widths = Vec::new();
        // Three passes: the fonts arrive, the columns are measured, the table
        // is drawn with what was measured.
        for _ in 0..3 {
            let mut view = Some(CombatsListView {
                combats,
                notes: &notes,
                my_handle: None,
                shown: None,
                comparing,
                comparison,
                ladder_runs: &[],
            });
            let _ = ctx.run_ui(RawInput::default(), |ui| {
                if let Some(view) = view.take() {
                    panel.show(view, &mut 900.0, ui);
                }
                widths = crate::custom_widgets::table::table_column_widths(ui, panel.table_id());
            });
        }
        widths
    }

    /// What the columns come to, printed rather than asserted: the numbers are
    /// what a decision about their widths is made from.
    #[test]
    #[ignore = "prints the column widths rather than asserting anything"]
    fn print_mixed_list_widths() {
        // A list shaped like a real one: solo fights with nobody to fold out,
        // team fights with an arrow, and one the program could not name.
        let mut solo = a_combat("Azure Nebula Rescue", "Solo", Some(Difficulty::Advanced));
        solo.environment = Some("Space".to_owned());
        solo.players = vec![PlayerSummary {
            handle: "@ramanwaleczny".to_owned(),
            dps: 139_000.0,
        }];
        let mut team = a_combat("Gateway To Grethor", "Team", Some(Difficulty::Advanced));
        team.environment = Some("Space".to_owned());
        team.start = NaiveDateTime::default() + chrono::Duration::minutes(10);
        team.players = vec![
            PlayerSummary {
                handle: "@mattman147".to_owned(),
                dps: 8_390.0,
            },
            PlayerSummary {
                handle: "@ramanwaleczny".to_owned(),
                dps: 7_000.0,
            },
        ];
        let mut unknown = a_combat("Combat", "Team", None);
        unknown.environment = None;
        unknown.category = None;
        unknown.start = NaiveDateTime::default() + chrono::Duration::minutes(20);
        unknown.players = vec![
            PlayerSummary {
                handle: "@ramanwaleczny".to_owned(),
                dps: 94_900.0,
            },
            PlayerSummary {
                handle: "@somebody".to_owned(),
                dps: 1_000.0,
            },
        ];
        let combats = [solo, team, unknown];

        let notes = CombatNotes::default();
        let ctx = Context::default();
        crate::app::fonts::install(&ctx);
        theme::apply(&ctx, theme::Theme::Dark);
        let mut panel = CombatsPanel::new(true);
        let mut width = 0.0;
        for (frame, comparing) in [false, false, false, true, true, true]
            .into_iter()
            .enumerate()
        {
            let mut view = Some(CombatsListView {
                combats: &combats,
                notes: &notes,
                my_handle: Some("@ramanwaleczny"),
                shown: Some(combats[0].start),
                comparing,
                comparison: &[],
                ladder_runs: &[],
            });
            let mut widths = Vec::new();
            let _ = ctx.run_ui(RawInput::default(), |ui| {
                if let Some(view) = view.take() {
                    panel.show(view, &mut width, ui);
                }
                widths = crate::custom_widgets::table::table_column_widths(ui, panel.table_id());
            });
            println!("frame {frame} comparing {comparing}: {widths:?}");
        }
    }

    #[test]
    #[ignore = "prints the column widths rather than asserting anything"]
    fn print_column_widths() {
        let mut realistic = a_combat("Infected: The Conduit", "Team", Some(Difficulty::Elite));
        realistic.environment = Some("Space".to_owned());
        realistic.players = vec![
            PlayerSummary {
                handle: "@ramanwaleczny".to_owned(),
                dps: 121_700.0,
            },
            PlayerSummary {
                handle: "@Ettenurb".to_owned(),
                dps: 88_400.0,
            },
        ];
        let realistic = [realistic];
        println!("real browse  {:?}", measured_widths(&realistic, false));
        println!("real compare {:?}", measured_widths(&realistic, true));
        let slots = [ComparisonSlot {
            start: realistic[0].start,
            player: "@ramanwaleczny".to_owned(),
        }];
        println!(
            "real in-comp {:?}",
            measured_widths_with(&realistic, true, &slots)
        );
        let japori = [a_combat("Japori", "Team", None)];
        println!("browse  {:?}", measured_widths(&japori, false));
        println!("compare {:?}", measured_widths(&japori, true));
        let longest = crate::analyzer::curated_map_names()
            .into_iter()
            .max_by_key(String::len)
            .unwrap_or_default();
        println!("longest map name: {longest:?}");
        println!(
            "longest {:?}",
            measured_widths(&[a_combat(&longest, "Team", None)], false)
        );
    }

    /// With nothing the reader has set, the panel opens at exactly the width
    /// its table came to — and once they have dragged it, that is what it opens
    /// at instead.
    #[test]
    fn the_panel_opens_at_the_width_of_its_table() {
        let combats = [a_combat(
            "Infected: The Conduit",
            "Team",
            Some(Difficulty::Elite),
        )];
        let table: f32 = measured_widths(&combats, false).iter().sum();

        // A first run: nothing the reader has set, so the panel is its table.
        let mut width = 0.0;
        let opened = drawn_width(&combats, &mut width);
        assert!(
            opened > table,
            "the panel opened at {opened}, its table came to {table}"
        );
        assert!(
            width <= 0.0,
            "following its table is not a width the reader set: {width}"
        );

        // The next run, with a width of their own: that is what it opens at.
        let mut width = 400.0;
        let opened = drawn_width(&combats, &mut width);
        assert!(
            (opened - 400.0).abs() < 2.0,
            "the panel opened at {opened} rather than at the 400 it was left at"
        );
    }

    /// A width the reader set survives the list being opened before the log has
    /// been read.
    ///
    /// The table is a heading and nothing else for the first seconds of a large
    /// log. Opening the panel then used to hand that width back as if the
    /// reader had chosen it — and it was written to the settings on the way
    /// out, so the panel came up narrow from then on.
    #[test]
    fn a_width_of_my_own_survives_a_log_that_has_not_been_read_yet() {
        let ctx = Context::default();
        crate::app::fonts::install(&ctx);
        theme::apply(&ctx, theme::Theme::Dark);
        let notes = CombatNotes::default();
        let mut panel = CombatsPanel::new(true);
        let mut width = 700.0;

        let draw = |combats: &[CombatSummary], panel: &mut CombatsPanel, width: &mut f32| {
            let mut drawn = 0.0;
            for _ in 0..3 {
                let mut view = Some(CombatsListView {
                    combats,
                    notes: &notes,
                    my_handle: None,
                    shown: None,
                    comparing: false,
                    comparison: &[],
                    ladder_runs: &[],
                });
                let _ = ctx.run_ui(RawInput::default(), |ui| {
                    if let Some(view) = view.take() {
                        panel.show(view, width, ui);
                    }
                    drawn = PanelState::load(ui.ctx(), Id::new(PANEL_ID))
                        .map(|state| state.rect.width())
                        .unwrap_or_default();
                });
            }
            drawn
        };

        // Opened while the log is still being read: no fights, so no table to
        // speak of.
        let drawn = draw(&[], &mut panel, &mut width);
        assert!(
            (width - 700.0).abs() < 1.0,
            "the width they set is still theirs: {width}"
        );
        assert!(
            (drawn - 700.0).abs() < 2.0,
            "and the panel is drawn at it: {drawn}"
        );

        // The fights arrive.
        let combats = [a_combat(
            "Infected: The Conduit",
            "Team",
            Some(Difficulty::Elite),
        )];
        let drawn = draw(&combats, &mut panel, &mut width);
        assert!((width - 700.0).abs() < 1.0, "still theirs: {width}");
        assert!((drawn - 700.0).abs() < 2.0, "still drawn at it: {drawn}");
    }

    /// How wide the panel comes out, from a context of its own — a fresh run of
    /// the program, in other words.
    fn drawn_width(combats: &[CombatSummary], width: &mut f32) -> f32 {
        let ctx = Context::default();
        crate::app::fonts::install(&ctx);
        theme::apply(&ctx, theme::Theme::Dark);
        let notes = CombatNotes::default();
        let mut panel = CombatsPanel::new(true);
        let mut drawn = 0.0;
        // The fonts arrive, the columns are measured, the panel settles.
        for _ in 0..4 {
            let mut view = Some(CombatsListView {
                combats,
                notes: &notes,
                my_handle: None,
                shown: None,
                comparing: false,
                comparison: &[],
                ladder_runs: &[],
            });
            let _ = ctx.run_ui(RawInput::default(), |ui| {
                if let Some(view) = view.take() {
                    panel.show(view, width, ui);
                }
                drawn = PanelState::load(ui.ctx(), Id::new(PANEL_ID))
                    .map(|state| state.rect.width())
                    .unwrap_or_default();
            });
        }
        drawn
    }

    /// The strip at the bottom of the panel has to reach the bottom of the
    /// panel, bar its rim.
    ///
    /// It is a panel of its own, laid out against the inside of the frame's
    /// margin — so a margin along the bottom left a band of empty panel under
    /// the strip, and everything in the strip read as sitting above the middle
    /// of the footer. Measured rather than eyeballed, because that is twice now.
    /// A fight with everything filled in, for the layout tests.
    fn a_combat(map: &str, size: &str, difficulty: Option<Difficulty>) -> CombatSummary {
        CombatSummary {
            name: map.to_owned(),
            identifier: format!("{map} | 2026-08-19 21:14:03 - 21:18:15"),
            base_name: map.to_owned(),
            category: Some("Patrol".to_owned()),
            environment: Some("Shuttle".to_owned()),
            difficulty,
            solo: size == "Solo",
            start: NaiveDateTime::default(),
            duration: chrono::Duration::seconds(3700),
            players: vec![PlayerSummary {
                handle: "@ramanwaleczny".to_owned(),
                dps: 121_700.0,
            }],
        }
    }

    #[test]
    fn the_footer_reaches_the_bottom_of_the_panel() {
        let ctx = Context::default();
        theme::apply(&ctx, theme::Theme::Dark);
        let notes = CombatNotes::default();
        let combats = vec![CombatSummary {
            name: "[Space] [Team] [TFO] Infected: The Conduit [Elite]".to_owned(),
            identifier: "[Space] [Team] [TFO] Infected: The Conduit [Elite] | 2026-08-19 21:14:03 - 21:18:15"
                .to_owned(),
            base_name: "[TFO] Infected: The Conduit".to_owned(),
            category: Some("Patrol".to_owned()),
            environment: Some("Shuttle".to_owned()),
            difficulty: Some(Difficulty::Advanced),
            solo: false,
            start: NaiveDateTime::default(),
            duration: chrono::Duration::seconds(3700),
            players: vec![PlayerSummary {
                handle: "@ramanwaleczny".to_owned(),
                dps: 121700.0,
            }],
        }];
        let mut panel = CombatsPanel::new(true);
        let mut width = 600.0;

        // Twice with the tick boxes out and twice without: the strip changes
        // height as buttons come and go, and it has to sit right either way.
        for frame in 0..4 {
            panel.mode = match (2..4).contains(&frame) {
                true => PanelMode::Clearing,
                false => PanelMode::Browse,
            };
            let _ = ctx.run_ui(RawInput::default(), |ui| {
                panel.show(
                    CombatsListView {
                        combats: &combats,
                        notes: &notes,
                        my_handle: None,
                        shown: None,
                        comparing: false,
                        comparison: &[],
                        ladder_runs: &[],
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
