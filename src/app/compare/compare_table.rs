//! Builds and renders the side-by-side comparison of the outgoing damage ability
//! tree of a chosen player across the picked combats, plus a chart (reusing the
//! main window's diagrams) of the selected ability branch across those combats.
//!
//! The trees are aligned by ability name (name handles differ per combat, so we
//! key on the resolved name string). Rows are sorted by the first (reference)
//! combat's DPS, and every value in combats 2+ carries a colored +/- delta
//! against the reference.
//!
//! Two ways out of the table: the averages toggle, which collapses the columns
//! into one mean per metric, and the export, which writes the same table to a
//! spreadsheet (`export`).

use std::{ops::Range, path::PathBuf, sync::Arc};

use chrono::NaiveDateTime;
use eframe::{
    Frame,
    egui::{text::LayoutJob, *},
};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    analyzer::{
        AnalysisGroup, Combat, DamageGroup, DamageMetrics, Hit, Hits, HitsManager, MaxOneHit,
        NameHandle, NameManager, ShieldHullOptionalValues, ShieldHullValues, ValueFlags,
    },
    app::main_tabs::diagrams::{
        DamageDiagrams, DiagramType, PreparedDamageDataSet, PreparedHit, combat_duration_seconds,
    },
    app::main_tabs::tables::show_group_separator,
    app::settings::{CombatNotes, Settings},
    app::theme,
    custom_widgets::{
        slider_text_edit::SliderTextEdit, splitter::Splitter, table::*, toggle::Toggle,
    },
    helpers::{number_formatting::NumberFormatter, time_range_to_duration_or_zero},
};

use super::CompareMetric;
use crate::app::export;

const ROW_HEIGHT: f32 = 25.0;
/// The size the open/close arrow of a tree row is drawn at.
///
/// Pinned rather than left to the text in it: the arrow is frameless while
/// resting and framed under the pointer, and egui sizes those two differently
/// under this app's themes (see `custom_widgets::toggle`), so without a fixed
/// size pointing at an arrow nudged the name beside it.
const ARROW_SIZE: Vec2 = vec2(22.0, 18.0);
// The header is two lines — the metric name on top, the combat number below —
// and a third when any combat carries a note.
const HEADER_LINE_HEIGHT: f32 = 17.0;

/// Headers of the two breakdown column groups, with what each one means, in the
/// order they are drawn.
const BREAKDOWN_LABELS: [(&str, &str); 2] = [
    (
        "ΔDPS from rate",
        "How much of the DPS difference came from landing more (or fewer) times per second. Added to the hit-size share this is the whole DPS difference — each share on its own can be far larger than that difference when the two point opposite ways",
    ),
    (
        "ΔDPS from hit size",
        "How much of the DPS difference came from each hit landing harder (or softer). Added to the rate share this is the whole DPS difference; hover a value to see the two and their sum",
    ),
];

/// One cell of the header row: a label, or the rule that opens a column group.
///
/// The label is a laid-out job rather than a string because its lines are not
/// all the same colour: the combat number is drawn in the colour of that
/// combat's line on the chart.
enum HeaderCell {
    Separator,
    Cell { text: LayoutJob, tooltip: String },
}

struct Slot {
    /// Index in the combats list (shown in the legend as combat 1/2/3).
    #[allow(dead_code)]
    index: usize,
    combat: Arc<Combat>,
    player: NameHandle,
}

pub struct Comparison {
    slots: Vec<Slot>,
    nodes: Vec<CompareNode>,
    columns: Vec<CompareMetric>,
    /// The user's note for each slot's combat, empty where they wrote none.
    /// Held here rather than read where it is drawn, because the chart bakes
    /// its series names in when it is built: a note written while the table is
    /// up has to be noticed and the chart rebuilt for it.
    notes: Vec<String>,
    /// The ability node whose chart is shown (by node id).
    selected: Option<u32>,
    diagrams: Option<DamageDiagrams>,
    active_diagram: DiagramType,
    /// Whether the chart currently holds the averaged line rather than one line
    /// per combat. Followed from the settings so the toggle rebuilds it.
    averages: bool,
    /// The rows under Total the reader has taken out of it, by name.
    ///
    /// By name rather than by node id because a rebuild (another column, a
    /// different player) hands out fresh ids, and a selection that survived
    /// picking a column but not unpicking one would be a puzzle.
    excluded: FxHashSet<String>,
    /// Whether the rows that are out are hidden rather than only left out of
    /// the Total.
    hide_unticked: bool,
    /// The damage types the reader is looking at. Empty means every type, which
    /// is the state the picker opens in.
    types: FxHashSet<String>,
    /// Whether the rows the combats agree on are hidden, leaving what they
    /// differ over.
    show_differences: bool,
    /// What "differ" is measured in, and by how much a row has to differ to
    /// stay on screen. A threshold each, because one is percentage points of a
    /// combat and the other is DPS — a number that means anything in one is
    /// meaningless in the other.
    difference_measure: DifferenceMeasure,
    share_threshold: f64,
    dps_threshold: f64,
    filter: f64,
    time_slice: f64,
    /// What came of the last export, shown beside the button: where the file
    /// went, or why it did not.
    export_status: Option<Result<PathBuf, String>>,
}

struct CompareNode {
    name: String,
    id: u32,
    /// One entry per slot; `None` when that combat's player has no such node.
    cells: Vec<Option<SlotCell>>,
    /// One entry per configured column: this row averaged across the combats.
    /// Always built (it costs a division per column) so the averages toggle is
    /// a redraw rather than a rebuild.
    averages: Vec<Option<AverageCell>>,
    /// Per-slot hit series for charting (`None` when the slot lacks this node).
    series: Vec<Option<SeriesData>>,
    /// What this row is of each combat, per slot: its share of that combat's
    /// own total (percent) and its DPS. Held apart from `cells` because those
    /// follow the reader's chosen columns, while these two decide which rows
    /// are on screen and have to be there whether or not they are shown.
    shares: Vec<Option<f64>>,
    dps: Vec<Option<f64>>,
    /// The damage types this row dealt, over all the combats — what the type
    /// picker filters on. A group of several weapons carries all of theirs.
    damage_types: Vec<String>,
    /// Reference (first slot) DPS, used to sort rows; `-inf` when absent.
    sort_key: f64,
    sub_nodes: Vec<CompareNode>,
    open: bool,
}

struct SeriesData {
    hits: Vec<Hit>,
    total: f64,
    /// The slot combat's length, so each line spans its own whole fight even
    /// when the compared combats are of different lengths.
    combat_duration_s: f64,
}

struct SlotCell {
    /// One entry per configured column.
    metrics: Vec<MetricCell>,
    /// How this slot's DPS difference against the reference splits up. `None`
    /// on the reference itself, which has nothing to differ from.
    breakdown: Option<DpsBreakdown>,
}

/// A DPS difference split into where it came from.
///
/// `DPS = hits per second x average hit`, so a change in DPS is a change in how
/// often something landed, a change in how hard each one landed, or both. The
/// two shares are taken at the midpoint of the pair:
///
/// ```text
/// rate share = (r2 - r1) * (m1 + m2) / 2
/// size share = (m2 - m1) * (r1 + r2) / 2
/// ```
///
/// which add up to `r2*m2 - r1*m1` exactly — the whole difference, with no
/// leftover cross term to attribute by hand.
struct DpsBreakdown {
    rate: f64,
    size: f64,
}

struct MetricCell {
    text: String,
    /// The number behind the text, kept for the spreadsheet export — a
    /// spreadsheet wants a number it can add up, not "1.2M".
    value: Option<f64>,
    /// Delta versus the reference combat (only for slots after the first).
    delta: Option<DeltaCell>,
}

/// One metric of one row, averaged over the combats that have that row.
///
/// A plain mean of the combats, each counting once — the number a player means
/// by "my average DPS on this map". Combats the row is absent from are left out
/// rather than counted as zero, which is why the count is carried and shown:
/// an ability flown in two runs out of ten averages the two, and says so.
struct AverageCell {
    value: f64,
    text: String,
    count: usize,
    min: f64,
    max: f64,
}

struct DeltaCell {
    text: String,
    improvement: bool,
}

impl Comparison {
    pub fn new(fetched: Vec<(usize, Arc<Combat>)>, settings: &Settings) -> Self {
        let mut slots: Vec<Slot> = fetched
            .into_iter()
            .map(|(index, combat)| {
                let player = top_dps_player(&combat);
                Slot {
                    index,
                    combat,
                    player,
                }
            })
            .collect();
        follow_the_reference_player(&mut slots);
        let notes = slot_notes(&slots, &settings.combat_notes);
        let mut comparison = Self {
            slots,
            nodes: Vec::new(),
            columns: settings.compare.columns.clone(),
            notes,
            selected: None,
            diagrams: None,
            active_diagram: DiagramType::Dps,
            averages: settings.compare.show_averages,
            excluded: Default::default(),
            hide_unticked: false,
            types: Default::default(),
            show_differences: false,
            difference_measure: DifferenceMeasure::Share,
            share_threshold: 3.0,
            dps_threshold: 5_000.0,
            filter: 0.4,
            time_slice: 1.0,
            export_status: None,
        };
        comparison.rebuild();
        comparison
    }

    fn rebuild(&mut self) {
        let parents: Vec<Option<&DamageGroup>> = self
            .slots
            .iter()
            .map(|s| s.combat.players.get(&s.player).map(|p| &p.damage_out))
            .collect();
        let name_managers: Vec<&NameManager> =
            self.slots.iter().map(|s| &s.combat.name_manager).collect();
        let hits_managers: Vec<&HitsManager> =
            self.slots.iter().map(|s| &s.combat.hits_manger).collect();
        let durations: Vec<f64> = self
            .slots
            .iter()
            .map(|s| combat_duration_seconds(&s.combat))
            .collect();

        let mut id_source = 0u32;
        let root_id = id_source;
        id_source += 1;

        // Top row is the player's overall total (root of the damage tree); the
        // ability groups hang under it, expanded by default.
        let (cells, averages) = build_row(&parents, &self.columns);
        let series = build_series(&parents, &hits_managers, &durations);
        let sub_nodes = build_level(
            &parents,
            &name_managers,
            &hits_managers,
            &durations,
            &self.columns,
            &mut id_source,
        );
        let sort_key = parents
            .first()
            .and_then(|p| *p)
            .map(|g| g.dps.all)
            .unwrap_or(f64::NEG_INFINITY);
        self.nodes = vec![CompareNode {
            name: "Total".to_string(),
            id: root_id,
            cells,
            averages,
            series,
            // The Total is not a row anything is filtered by: it is what the
            // rows are filtered against, and is always drawn.
            shares: Vec::new(),
            dps: Vec::new(),
            damage_types: Vec::new(),
            sort_key,
            sub_nodes,
            open: true,
        }];

        // A rebuild starts from the whole tree again, so a row that was out of
        // the Total has to be taken back out of the one just built.
        if !self.excluded.is_empty() {
            self.refresh_total();
        }

        // Chart the overall total by default so a chart shows immediately.
        self.selected = Some(root_id);
        self.rebuild_diagram();
    }

    /// (Re)build the Total row from the rows that are ticked — its values, the
    /// averages beside them, the series the chart draws it from, and the name
    /// that says how much of the tree went into it.
    ///
    /// With nothing unticked it is built from the player's own damage group,
    /// so an unfiltered Total is the analyzer's own figure to the last digit
    /// rather than the pieces added back together in a different order.
    fn refresh_total(&mut self) {
        let filtered: Vec<Option<DamageGroup>> = if self.excluded.is_empty() {
            Vec::new()
        } else {
            self.slots
                .iter()
                .map(|slot| subset_total(slot, &self.excluded))
                .collect()
        };

        let (cells, averages, series) = {
            let parents: Vec<Option<&DamageGroup>> = if filtered.is_empty() {
                self.slots
                    .iter()
                    .map(|s| s.combat.players.get(&s.player).map(|p| &p.damage_out))
                    .collect()
            } else {
                filtered.iter().map(|group| group.as_ref()).collect()
            };
            let hits_managers: Vec<&HitsManager> =
                self.slots.iter().map(|s| &s.combat.hits_manger).collect();
            let durations: Vec<f64> = self
                .slots
                .iter()
                .map(|s| combat_duration_seconds(&s.combat))
                .collect();
            let (cells, averages) = build_row(&parents, &self.columns);
            let series = build_series(&parents, &hits_managers, &durations);
            (cells, averages, series)
        };

        let excluded = &self.excluded;
        let Some(total) = self.nodes.first_mut() else {
            return;
        };
        total.cells = cells;
        total.averages = averages;
        total.series = series;
        total.name = total_row_name(
            ticked_count(&total.sub_nodes, excluded),
            total.sub_nodes.len(),
        );
    }

    /// (Re)build the chart for the currently selected ability node: one line per
    /// combat over that branch's hits — or, with the averages on, the one line
    /// those lines average out to.
    fn rebuild_diagram(&mut self) {
        let id = match self.selected {
            Some(id) => id,
            None => {
                self.diagrams = None;
                return;
            }
        };
        let n_slots = self.slots.len();
        let filter = self.filter;
        let time_slice = self.time_slice;
        let notes = &self.notes;
        let averages = self.averages;
        self.diagrams = find_node(&self.nodes, id).map(|node| {
            if averages {
                let data = average_series(node).into_iter();
                return DamageDiagrams::from_data(data, filter, time_slice);
            }
            let data = (0..n_slots).filter_map(|slot_i| {
                let series = node.series.get(slot_i)?.as_ref()?;
                Some(
                    PreparedDamageDataSet::new(
                        &chart_label(slot_i, note_of(notes, slot_i)),
                        series.total,
                        series.hits.iter(),
                        series.combat_duration_s,
                    )
                    // A combat's colour is which combat it is, so it is pinned
                    // to the slot rather than left to the chart's own order —
                    // that order is by total, and every tick in the table moves
                    // the totals.
                    .with_color(theme::series_color(slot_i)),
                )
            });
            DamageDiagrams::from_data(data, filter, time_slice)
        });
    }

    pub fn show(&mut self, ui: &mut Ui, settings: &mut Settings, frame: &Frame) {
        if self.slots.is_empty() {
            ui.label("No combats selected.");
            return;
        }

        // A note can be written (or changed) in the main window while a
        // comparison is still up, and the chart's series names are built from
        // these, so check them rather than take them once and keep them.
        let notes = slot_notes(&self.slots, &settings.combat_notes);
        if notes != self.notes {
            self.notes = notes;
            self.rebuild_diagram();
        }

        self.show_toolbar(ui, settings, frame);

        // Pick up column changes from the picker (or an external settings edit).
        if self.columns != settings.compare.columns {
            self.columns = settings.compare.columns.clone();
            self.rebuild();
        }
        // The chart follows the toggle as well as the table: with the averages
        // on it draws the one line the combats average out to, so switching has
        // to rebuild it. Only the chart — the table's averages are always built.
        if self.averages != settings.compare.show_averages {
            self.averages = settings.compare.show_averages;
            self.rebuild_diagram();
        }

        let colors = self.slot_colors();

        ui.label(
            RichText::new(if settings.compare.show_averages {
                "Each column holds one metric averaged over the combats above — every combat \
                 counting once, and only those a row appears in. Hover a value for how many \
                 combats went into it, and its best and worst."
            } else {
                "Each column group holds one metric, one column per combat. The small coloured \
                 number beside a value is its difference against combat #1 — green when it moved \
                 the better way."
            })
            .weak(),
        );

        // Comparing two different people's numbers looks like a build changed
        // when nothing did, so say so rather than let it pass unnoticed — in
        // one short line, with the particulars a hover away.
        if let Some(warning) = odd_player_warning(&slot_player_names(&self.slots)) {
            ui.label(RichText::new(warning.line).color(theme::palette().worse))
                .on_hover_text(warning.detail);
        }

        ui.separator();

        // Three stacked panes, two draggable boundaries: the list of runs, the
        // table, the chart. The list starts at the height of what it holds (up
        // to a few rows) rather than at a fixed share, so an ordinary
        // comparison of two runs does not open with a third of the window given
        // to two lines — and a session's worth can be dragged open from there.
        let row = ui.spacing().interact_size.y + ui.spacing().item_spacing.y;
        let legend_ratio = legend_ratio(row, self.slots.len(), ui.available_height());
        Splitter::horizontal()
            .initial_ratio(legend_ratio)
            .ratio_bounds(MIN_LEGEND_RATIO..=MAX_LEGEND_RATIO)
            .show(ui, |legend_ui, rest_ui| {
                if let Some((slot_i, handle)) = self.show_legend(legend_ui, &colors) {
                    self.slots[slot_i].player = handle;
                    // Changing who the reference is about moves the others with
                    // it, where that player took part: the deltas are all
                    // measured against the reference, so leaving them on
                    // somebody else would compare two different people again.
                    if slot_i == 0 {
                        follow_the_reference_player(&mut self.slots);
                    }
                    self.rebuild();
                }

                Splitter::horizontal()
                    .initial_ratio(0.6)
                    .ratio_bounds(0.15..=0.9)
                    .show(rest_ui, |top_ui, bottom_ui| {
                        self.show_table(
                            top_ui,
                            settings.compare.show_dps_breakdown,
                            settings.compare.show_averages,
                            &colors,
                        );
                        self.show_diagram(bottom_ui, settings);
                    });
            });
    }

    /// The list of runs the comparison is of, one row each: which combat it is,
    /// and whose numbers are being shown for it. Returns the slot whose player
    /// was picked, if one was.
    ///
    /// It scrolls inside whatever height the pane has been given, so a session's
    /// worth of runs is reachable without pushing the table off the window.
    fn show_legend(&self, ui: &mut Ui, colors: &[Option<Color32>]) -> Option<(usize, NameHandle)> {
        let mut player_change = None;
        ScrollArea::vertical()
            .id_salt("compare legend")
            .show(ui, |ui| {
                for (slot_i, slot) in self.slots.iter().enumerate() {
                    ui.horizontal(|ui| {
                        // The user's own note, where they wrote one, tells apart
                        // runs the identifier alone cannot.
                        let note = note_of(&self.notes, slot_i);
                        // The number and the note are drawn in the colour this
                        // combat has on the chart, so the legend, the table and
                        // the chart all say the same thing about which run is
                        // which.
                        ui.label(legend_text(
                            &TextStyle::Body.resolve(ui.style()),
                            ui.visuals().text_color(),
                            slot_i,
                            &slot.combat.identifier(),
                            note,
                            colors[slot_i],
                        ));
                        let current = slot.player.get(&slot.combat.name_manager).to_string();
                        ComboBox::new(("compare player", slot_i), "player")
                            .selected_text(current)
                            .show_ui(ui, |ui| {
                                for (handle, name) in players_by_dps(&slot.combat) {
                                    if ui.selectable_label(handle == slot.player, name).clicked() {
                                        player_change = Some((slot_i, handle));
                                    }
                                }
                            });
                        if slot_i == 0 {
                            ui.label(
                                RichText::new(
                                    "(reference — every difference is measured against this \
                                     combat, and changing the player here moves the others to the \
                                     same player)",
                                )
                                .weak(),
                            );
                        }
                    });
                }
            });
        player_change
    }

    /// The colour each slot's combat is drawn in on the chart, `None` for a
    /// slot the chart has no line for — nothing is charted yet, the selected
    /// ability is absent from that combat, or the chart holds the one averaged
    /// line instead of one per combat.
    ///
    /// The colour itself is the slot's own and never moves: it says which
    /// combat this is, and a combat is still the same combat after a row is
    /// unticked. The charts sort their series by total, so a colour taken from
    /// that order would change under every tick — and did, which is what this
    /// replaced.
    fn slot_colors(&self) -> Vec<Option<Color32>> {
        let charted = (!self.averages)
            .then(|| self.selected.and_then(|id| find_node(&self.nodes, id)))
            .flatten();
        (0..self.slots.len())
            .map(|slot_i| {
                let node = charted?;
                node.series
                    .get(slot_i)?
                    .as_ref()
                    .map(|_| theme::series_color(slot_i))
            })
            .collect()
    }

    /// Every damage type the rows under Total dealt, in name order — what the
    /// type picker offers. Taken from the rows rather than from a list of the
    /// game's types, so it never offers a type this comparison has none of.
    fn all_damage_types(&self) -> Vec<String> {
        let mut types: Vec<String> = self
            .nodes
            .first()
            .into_iter()
            .flat_map(|total| total.sub_nodes.iter())
            .flat_map(|node| node.damage_types.iter().cloned())
            .collect();
        types.sort_unstable();
        types.dedup();
        types
    }

    /// The row above the legend: which columns to show, whether to average them,
    /// and the export. The export sits on the right, away from the two menus
    /// that change what is on screen — it only takes a copy of it.
    fn show_toolbar(&mut self, ui: &mut Ui, settings: &mut Settings, frame: &Frame) {
        ui.horizontal(|ui| {
            self.show_column_picker(ui, settings);

            // A framed button that lights up when it is on, rather than a
            // frameless toggle: it stands between two ordinary buttons, and the
            // frameless kind reads as a stray label between them. (The top bar
            // uses the frameless kind because there it stands among its own.)
            if ui
                .add(Button::new("Σ Averages").selected(settings.compare.show_averages))
                .on_hover_text(
                    "Collapse the per-combat columns into one average per metric, over every \
                     combat in the comparison. Useful once there are more runs on screen than \
                     can be read side by side.",
                )
                .clicked()
            {
                settings.compare.show_averages = !settings.compare.show_averages;
                settings.save();
            }

            self.show_difference_controls(ui);

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .button("Export XLSX 🖹")
                    .on_hover_text(
                        "Save what the table shows as a spreadsheet — the plain numbers, without \
                         the differences.",
                    )
                    .clicked()
                {
                    self.export(settings.compare.show_averages, frame);
                }
                match &self.export_status {
                    Some(Ok(path)) => {
                        ui.label(RichText::new(format!("Saved to {}", path.display())).weak());
                    }
                    Some(Err(error)) => {
                        ui.label(
                            RichText::new(format!("Export failed: {error}"))
                                .color(theme::palette().worse),
                        );
                    }
                    None => (),
                }
            });
        });
    }

    /// The differences toggle, and — while it is on — what a difference is
    /// measured in and how large one has to be to stay on screen.
    ///
    /// The two controls only appear with the toggle, because they say nothing
    /// while nothing is being hidden, and the toolbar is already a row of
    /// things that are always there.
    fn show_difference_controls(&mut self, ui: &mut Ui) {
        if ui
            .add(Button::new("Δ Differences").selected(self.show_differences))
            .on_hover_text(
                "Hide the rows the combats agree on, leaving what they differ over — what a run \
                 flew that the others did not, and what it leaned on far harder. The rows stay in \
                 the order they are in; only the ones nobody differs over go.",
            )
            .clicked()
        {
            self.show_differences = !self.show_differences;
        }
        if !self.show_differences {
            return;
        }

        for measure in [DifferenceMeasure::Share, DifferenceMeasure::Dps] {
            ui.steady_toggle_value(&mut self.difference_measure, measure, measure.label())
                .on_hover_text(match measure {
                    DifferenceMeasure::Share => {
                        "Measure a difference in what the row was of its own combat. A shorter or \
                         weaker run then does not read as a different build in every row at once."
                    }
                    DifferenceMeasure::Dps => {
                        "Measure a difference in DPS — what the row was actually worth, whatever \
                         share of the run it came to."
                    }
                });
        }

        let (threshold, range, step) = match self.difference_measure {
            DifferenceMeasure::Share => (&mut self.share_threshold, 0.5..=20.0, 0.5),
            DifferenceMeasure::Dps => (&mut self.dps_threshold, 500.0..=50_000.0, 500.0),
        };
        SliderTextEdit::new(threshold, range, "compare difference threshold")
            .clamp_min(0.0)
            .clamp_max(1e9)
            .desired_text_edit_width(50.0)
            .display_precision(6)
            .step_by(step)
            .show(ui);
        ui.label(format!(
            "min difference ({})",
            self.difference_measure.unit()
        ))
        .on_hover_text(
            "A row stays on screen when its largest combat and its smallest are at least this \
                 far apart. A combat that does not have the row at all counts as zero, so a row \
                 flown in some runs and not others is a difference of its whole size.",
        );
    }

    /// Asks for a file and writes the table into it. Whatever comes of it is
    /// kept for the toolbar to show: a save dialog closing on its own says
    /// nothing about whether anything was written.
    fn export(&mut self, averages: bool, frame: &Frame) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Export Comparison")
            .add_filter("Excel workbook", &["xlsx"])
            .set_file_name(export::default_file_name(&self.slots[0].combat))
            .set_parent(frame)
            .save_file()
        else {
            return;
        };
        let sheet = self.export_sheet(averages);
        self.export_status = Some(match export::write(&path, std::slice::from_ref(&sheet)) {
            Ok(()) => {
                log::info!("exported comparison to {}", path.display());
                Ok(path)
            }
            Err(error) => {
                log::error!("failed to export comparison to {}: {error}", path.display());
                Err(error.to_string())
            }
        });
    }

    /// The table as plain data: the combats it is about, one column per metric
    /// (per combat, or one averaged), and every row of the tree — including the
    /// ones collapsed on screen, since a spreadsheet has its own way of hiding
    /// what is not wanted.
    fn export_sheet(&self, averages: bool) -> export::Sheet {
        let combats = self
            .slots
            .iter()
            .enumerate()
            .map(|(slot_i, slot)| export::Combat {
                identifier: slot.combat.identifier(),
                note: note_of(&self.notes, slot_i).to_string(),
                player: slot.player.get(&slot.combat.name_manager).to_string(),
            })
            .collect();

        let mut columns = Vec::new();
        for column in self.columns.iter() {
            if averages {
                columns.push(export::Column {
                    header: format!("{} (avg)", column.label()),
                    decimals: column.precision(),
                });
            } else {
                for slot_i in 0..self.slots.len() {
                    columns.push(export::Column {
                        header: format!("{} #{}", column.label(), slot_i + 1),
                        decimals: column.precision(),
                    });
                }
            }
        }

        let mut rows = Vec::new();
        collect_export_rows(&self.nodes, 0, self.slots.len(), averages, &mut rows);
        export::Sheet {
            name: "Comparison".to_string(),
            combats,
            columns,
            rows,
        }
    }

    fn show_column_picker(&self, ui: &mut Ui, settings: &mut Settings) {
        // ⏷ rather than ▾: the bundled fonts have no U+25BE, which drew as an
        // empty box. This is the same arrow the tree rows open with.
        ui.menu_button("Columns ⏷", |ui| {
            let mut changed = false;
            for &metric in CompareMetric::ALL {
                let mut on = settings.compare.columns.contains(&metric);
                if ui.checkbox(&mut on, metric.label()).changed() {
                    changed = true;
                    if on {
                        settings.compare.columns.push(metric);
                    } else {
                        settings.compare.columns.retain(|&m| m != metric);
                    }
                }
            }

            ui.separator();
            // Not a metric of a single combat but a pair of columns all the
            // same, so it belongs with the others rather than beside the menu.
            changed |= ui
                .checkbox(&mut settings.compare.show_dps_breakdown, "ΔDPS breakdown")
                .on_hover_text(
                    "Two more columns splitting each DPS difference against the reference: the \
                     share that came from landing more often, and the share that came from each \
                     hit landing harder. The two add up to the whole difference.",
                )
                .changed();

            if changed {
                // Keep a stable column order regardless of toggle order.
                settings.compare.columns.sort_by_key(|m| {
                    CompareMetric::ALL
                        .iter()
                        .position(|x| x == m)
                        .unwrap_or(usize::MAX)
                });
                settings.save();
            }
        });
    }

    fn show_table(
        &mut self,
        ui: &mut Ui,
        show_breakdown: bool,
        show_averages: bool,
        colors: &[Option<Color32>],
    ) {
        let n_slots = self.slots.len();
        let n_metrics = self.columns.len();
        // Averaged columns belong to every combat at once, so there is no note
        // and no breakdown against a reference to put under them.
        let show_breakdown = show_breakdown && !show_averages;
        // The note line is only there when some combat carries a note, so a
        // comparison of runs nobody named keeps the two-line header it had.
        let with_notes = !show_averages && self.notes.iter().any(|note| !note.is_empty());
        let font = TextStyle::Body.resolve(ui.style());
        let text_color = ui.visuals().text_color();
        // Columns are grouped by metric: the metric name spans its group (shown
        // on the first column), with the combat number below each column.
        // A rule opens each group: without one, three combats' worth of the
        // same-looking numbers run into the next metric.
        let mut headers: Vec<HeaderCell> = Vec::new();
        for column in self.columns.iter() {
            headers.push(HeaderCell::Separator);
            if show_averages {
                headers.push(HeaderCell::Cell {
                    text: header_text(&font, text_color, column.label(), "Avg", None, None),
                    tooltip: format!(
                        "{} averaged over the {} combats in this comparison",
                        column.label(),
                        n_slots
                    ),
                });
                continue;
            }
            for slot_i in 0..n_slots {
                headers.push(HeaderCell::Cell {
                    text: header_text(
                        &font,
                        text_color,
                        if slot_i == 0 { column.label() } else { "" },
                        &if slot_i == 0 {
                            format!("#{} (ref)", slot_i + 1)
                        } else {
                            format!("#{}", slot_i + 1)
                        },
                        with_notes.then(|| note_of(&self.notes, slot_i)),
                        colors.get(slot_i).copied().flatten(),
                    ),
                    tooltip: format!(
                        "{} in combat #{}{}{}",
                        column.label(),
                        slot_i + 1,
                        note_suffix(note_of(&self.notes, slot_i)),
                        if slot_i == 0 {
                            " — the reference every other column is compared against"
                        } else {
                            ", with its difference against combat #1 beside it"
                        }
                    ),
                });
            }
        }
        // The breakdown has nothing to say about the reference, so its groups
        // start at the second combat.
        if show_breakdown {
            for (label, tooltip) in BREAKDOWN_LABELS {
                headers.push(HeaderCell::Separator);
                for slot_i in 1..n_slots {
                    headers.push(HeaderCell::Cell {
                        text: header_text(
                            &font,
                            text_color,
                            if slot_i == 1 { label } else { "" },
                            &format!("#{}", slot_i + 1),
                            with_notes.then(|| note_of(&self.notes, slot_i)),
                            colors.get(slot_i).copied().flatten(),
                        ),
                        tooltip: format!(
                            "{} (combat #{}{} against #1)",
                            tooltip,
                            slot_i + 1,
                            note_suffix(note_of(&self.notes, slot_i))
                        ),
                    });
                }
            }
        }

        let mut selected = self.selected;
        let mut selection_changed = false;
        let hide_unticked = self.hide_unticked;
        let mut hide_toggled = false;
        let all_types = self.all_damage_types();
        let differences = self.show_differences.then_some((
            self.difference_measure,
            match self.difference_measure {
                DifferenceMeasure::Share => self.share_threshold,
                DifferenceMeasure::Dps => self.dps_threshold,
            },
        ));
        // The picker writes to a copy: the set it edits is the one the rows are
        // being filtered by as it draws, and a pick takes effect on the next
        // pass — which egui runs anyway, having just been clicked.
        let mut picked_types = self.types.clone();
        let mut ticks = Ticks {
            excluded: &mut self.excluded,
            hide_unticked,
            types: &self.types,
            differences,
            changed: false,
        };
        {
            let nodes = &mut self.nodes;
            ScrollArea::horizontal().show(ui, |ui| {
                Table::new(ui)
                    .cell_spacing(10.0)
                    .header(header_height(with_notes), |r| {
                        // The tick column's header stays empty: the row it
                        // belongs to (Total) carries the tick that stands for
                        // all of them.
                        r.cell(|_| {});
                        r.cell(|ui| {
                            ui.horizontal(|ui| {
                                ui.label("Name");
                                if ui
                                    .add(Button::selectable(hide_unticked, "👁"))
                                    .on_hover_text(
                                        "Hide the rows that are not ticked, leaving only what the \
                                         Total is added up from. They stay out of the Total either \
                                         way — this only takes them off the screen.",
                                    )
                                    .clicked()
                                {
                                    hide_toggled = true;
                                }
                                show_type_picker(ui, &all_types, &mut picked_types);
                            });
                        });
                        for header in &headers {
                            match header {
                                HeaderCell::Separator => show_group_separator(r),
                                HeaderCell::Cell { text, tooltip } => {
                                    r.cell(|ui| {
                                        ui.label(text.clone()).on_hover_text(tooltip);
                                    });
                                }
                            }
                        }
                    })
                    .body(ROW_HEIGHT, |t| {
                        for node in nodes.iter_mut() {
                            node.show(
                                t,
                                0,
                                n_slots,
                                n_metrics,
                                show_breakdown,
                                show_averages,
                                &mut ticks,
                                &mut selected,
                                &mut selection_changed,
                            );
                        }
                    });
            });
        }

        let ticks_changed = ticks.changed;
        self.selected = selected;
        self.types = picked_types;
        if hide_toggled {
            self.hide_unticked = !self.hide_unticked;
        }
        if ticks_changed {
            self.refresh_total();
        }
        // The chart follows the ticks only while it is the Total that is
        // charted — an ability's own line is the same line whether or not it is
        // counted in the total above it.
        let total_charted = self.selected == self.nodes.first().map(|node| node.id);
        if selection_changed || (ticks_changed && total_charted) {
            self.rebuild_diagram();
        }
    }

    fn show_diagram(&mut self, ui: &mut Ui, settings: &Settings) {
        ui.horizontal(|ui| {
            for diagram in [
                DiagramType::Dps,
                DiagramType::Damage,
                DiagramType::DamageResistance,
                DiagramType::HitsPerSecond,
                DiagramType::HitsCount,
            ] {
                ui.steady_toggle_value(&mut self.active_diagram, diagram, diagram.name())
                    .on_hover_text(diagram.tooltip());
            }
        });

        let changed = match self.active_diagram {
            DiagramType::Damage | DiagramType::DamageResistance | DiagramType::HitsCount => {
                show_time_slice_setting(&mut self.time_slice, ui)
            }
            _ => show_time_filter_setting(&mut self.filter, ui),
        };
        if changed && let Some(diagrams) = &mut self.diagrams {
            diagrams.update(self.filter, self.time_slice);
        }

        match &mut self.diagrams {
            Some(diagrams) => diagrams.show(settings, ui, self.active_diagram),
            None => {
                ui.label("Select an ability row above to chart it across the combats.");
            }
        }
    }
}

impl CompareNode {
    // Drawing context threaded through; a struct of the same fields would
    // only move the list somewhere else.
    #[allow(clippy::too_many_arguments)]
    fn show(
        &mut self,
        t: &mut TableBody,
        depth: usize,
        n_slots: usize,
        n_metrics: usize,
        show_breakdown: bool,
        show_averages: bool,
        ticks: &mut Ticks,
        selected: &mut Option<u32>,
        selection_changed: &mut bool,
    ) {
        let is_selected = *selected == Some(self.id);
        let mut tick_rect = Rect::NOTHING;
        let response = t.selectable_row(is_selected, |r| {
            tick_rect = self.show_tick(r, depth, ticks);

            r.cell(|ui| {
                ui.horizontal(|ui| {
                    ui.add_space(depth as f32 * 20.0);
                    let symbol = if self.open { "⏷" } else { "⏵" };
                    let can_open = !self.sub_nodes.is_empty();
                    if ui
                        .add_visible(can_open, Button::selectable(false, symbol).min_size(ARROW_SIZE))
                        .clicked()
                    {
                        self.open = !self.open;
                    }
                    ui.label(&self.name);
                    // While the differences are being read, a row missing from
                    // some of the combats says so: "flown in two runs out of
                    // five" and "flown in all five, unevenly" are different
                    // findings, and the numbers alone do not tell them apart at
                    // a glance.
                    if let Some(missing) = self.missing_from(depth, n_slots, ticks) {
                        ui.label(RichText::new(missing).weak());
                    }
                });
            });

            // Column groups by metric: for each metric, one cell per combat —
            // or a single averaged cell standing for all of them.
            for metric_i in 0..n_metrics {
                show_group_separator(r);
                if show_averages {
                    match self.averages.get(metric_i).and_then(|a| a.as_ref()) {
                        Some(average) => {
                            let tooltip = average_tooltip(average, n_slots);
                            r.cell_with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(&average.text).on_hover_text(tooltip);
                            });
                        }
                        None => {
                            r.cell(|_| {});
                        }
                    }
                    continue;
                }
                for slot_i in 0..n_slots {
                    match self
                        .cells
                        .get(slot_i)
                        .and_then(|c| c.as_ref())
                        .and_then(|c| c.metrics.get(metric_i))
                    {
                        Some(metric) => {
                            r.cell_with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if let Some(delta) = &metric.delta {
                                    let palette = theme::palette();
                                    let color = if delta.improvement {
                                        palette.improve
                                    } else {
                                        palette.worse
                                    };
                                    ui.colored_label(color, &delta.text);
                                }
                                ui.label(&metric.text);
                            });
                        }
                        None => {
                            r.cell(|_| {});
                        }
                    }
                }
            }

            // Where each DPS difference came from: firing more often, or each
            // hit landing harder. Green when that share pushed DPS up.
            if show_breakdown {
                for pick in [
                    (|b: &DpsBreakdown| b.rate) as fn(&DpsBreakdown) -> f64,
                    |b: &DpsBreakdown| b.size,
                ] {
                    show_group_separator(r);
                    for slot_i in 1..n_slots {
                        match self
                            .cells
                            .get(slot_i)
                            .and_then(|c| c.as_ref())
                            .and_then(|c| c.breakdown.as_ref())
                        {
                            Some(breakdown) => {
                                let share = pick(breakdown);
                                let palette = theme::palette();
                                let color = if share >= 0.0 {
                                    palette.improve
                                } else {
                                    palette.worse
                                };
                                let mut formatter = NumberFormatter::new();
                                let mut signed = |value: f64| {
                                    format!(
                                        "{}{}",
                                        if value >= 0.0 { "+" } else { "-" },
                                        formatter.format(value.abs(), 0)
                                    )
                                };
                                let text = signed(share);
                                // The two shares often point opposite ways, and
                                // each can then dwarf their sum — so spell the
                                // sum out rather than leave it to be noticed.
                                let tooltip = format!(
                                    "{} from landing more often\n{} from each hit landing harder\n= {} DPS against combat #1",
                                    signed(breakdown.rate),
                                    signed(breakdown.size),
                                    signed(breakdown.rate + breakdown.size)
                                );
                                r.cell_with_layout(
                                    Layout::right_to_left(Align::Center),
                                    |ui| {
                                        ui.colored_label(color, text).on_hover_text(tooltip);
                                    },
                                );
                            }
                            None => {
                                r.cell(|_| {});
                            }
                        }
                    }
                }
            }
        });

        // The tick sits inside the row, so ticking clicks the row as well —
        // including when the pointer lands in the tick's cell but beside the
        // box itself. The whole cell is therefore out of bounds for charting,
        // or aiming at a tick and missing it by two points would chart that row
        // instead of ticking it.
        let clicked_the_tick = response
            .interact_pointer_pos()
            .is_some_and(|pos| tick_rect.contains(pos));
        if response.clicked() && !clicked_the_tick {
            *selected = Some(self.id);
            *selection_changed = true;
        }

        if self.open {
            for sub in self.sub_nodes.iter_mut() {
                if is_hidden(depth, ticks, sub) {
                    continue;
                }
                sub.show(
                    t,
                    depth + 1,
                    n_slots,
                    n_metrics,
                    show_breakdown,
                    show_averages,
                    ticks,
                    selected,
                    selection_changed,
                );
            }
        }
    }

    /// The first column: whether this row is counted in the Total. Returns the
    /// cell it drew into, which the row uses to tell a tick from a click on the
    /// row itself.
    ///
    /// Only the rows directly under Total carry one — they are what the Total
    /// is added up from, and a row deeper in the tree goes in with the branch
    /// it belongs to. Total itself carries the tick that stands for all of
    /// them, half-filled while only some are in.
    fn show_tick(&self, r: &mut TableRow, depth: usize, ticks: &mut Ticks) -> Rect {
        match depth {
            0 => {
                let rows = self.sub_nodes.len();
                let kept = ticked_count(&self.sub_nodes, ticks.excluded);
                let mut all = kept == rows;
                let cell = r.cell(|ui| {
                    let response = ui
                        .add(Checkbox::new(&mut all, "").indeterminate(kept != rows && kept != 0))
                        .on_hover_text(
                            "Count every row below in the Total, or none of them. Untick a row to \
                             leave it out and the Total is added up again without it.",
                        );
                    if response.changed() {
                        ticks.changed = true;
                        for sub in self.sub_nodes.iter() {
                            if all {
                                ticks.excluded.remove(&sub.name);
                            } else {
                                ticks.excluded.insert(sub.name.clone());
                            }
                        }
                    }
                });
                cell.rect
            }
            1 => {
                let mut ticked = !ticks.excluded.contains(&self.name);
                let cell = r.cell(|ui| {
                    if ui
                        .checkbox(&mut ticked, "")
                        .on_hover_text("Count this row in the Total above")
                        .changed()
                    {
                        ticks.changed = true;
                        if ticked {
                            ticks.excluded.remove(&self.name);
                        } else {
                            ticks.excluded.insert(self.name.clone());
                        }
                    }
                });
                cell.rect
            }
            // Deeper in the tree there is nothing to tick: the row is part of
            // the branch above it, which has a tick of its own.
            _ => r.cell(|_| {}).rect,
        }
    }
}

/// What the tick column needs while the tree draws itself: which rows are out
/// of the Total, which rows are on screen at all, and whether this pass changed
/// any of that.
struct Ticks<'a> {
    excluded: &'a mut FxHashSet<String>,
    hide_unticked: bool,
    /// The damage types to show. Empty is every type.
    types: &'a FxHashSet<String>,
    /// How far a row has to differ between the combats to stay on screen, and
    /// in what. `None` while the toggle is off.
    differences: Option<(DifferenceMeasure, f64)>,
    changed: bool,
}

/// What a difference between combats is measured in.
///
/// Neither is the right answer on its own. A share says what a row was *of* its
/// combat, so a shorter or worse run does not read as a different build in
/// every row at once; DPS says what it was worth, which is what a reader chasing
/// a number wants. Both are offered rather than guessed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DifferenceMeasure {
    /// Percentage points of the combat's own damage.
    Share,
    Dps,
}

impl DifferenceMeasure {
    fn label(self) -> &'static str {
        match self {
            DifferenceMeasure::Share => "share of combat",
            DifferenceMeasure::Dps => "DPS",
        }
    }

    fn unit(self) -> &'static str {
        match self {
            DifferenceMeasure::Share => "pp",
            DifferenceMeasure::Dps => "DPS",
        }
    }
}

/// How far apart the combats are on one row: the largest of its values less the
/// smallest, counting a combat that does not have the row at all as zero.
///
/// The zero is the point. A row flown in two runs out of five and absent from
/// the rest is exactly the kind of difference this is looking for, and treating
/// the absence as "no data" would rank it as agreement.
fn spread(values: &[Option<f64>]) -> f64 {
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for value in values.iter().map(|v| v.unwrap_or(0.0)) {
        low = low.min(value);
        high = high.max(value);
    }
    if low > high { 0.0 } else { high - low }
}

/// The damage type picker in the `Name` header: every type the comparison holds,
/// each on or off, and a way back to all of them.
///
/// Off is drawn as "every type" rather than as "no types", because a picker
/// nobody has touched is showing everything, and a list of empty boxes over a
/// full table reads as the opposite.
fn show_type_picker(ui: &mut Ui, all: &[String], picked: &mut FxHashSet<String>) {
    if all.is_empty() {
        return;
    }
    let label = if picked.is_empty() {
        "☰ Type".to_string()
    } else {
        format!("☰ Type ({})", picked.len())
    };
    ui.menu_button(label, |ui| {
        if !picked.is_empty() && ui.button("Every type").clicked() {
            picked.clear();
        }
        for damage_type in all {
            let mut on = picked.contains(damage_type);
            if ui.checkbox(&mut on, damage_type).changed() {
                if on {
                    picked.insert(damage_type.clone());
                } else {
                    picked.remove(damage_type);
                }
            }
        }
    })
    .response
    .on_hover_text(
        "Show only the rows that dealt a damage type you pick — the beams of one flavour out of a \
         rainbow build, say. A row that dealt several types is shown for each of them.",
    );
}

impl CompareNode {
    /// How far the combats are apart on this row, in the given measure.
    fn difference(&self, measure: DifferenceMeasure) -> f64 {
        match measure {
            DifferenceMeasure::Share => spread(&self.shares),
            DifferenceMeasure::Dps => spread(&self.dps),
        }
    }

    /// In how many of the combats this row appears at all — the other half of
    /// what makes a row stand out, and the one a number alone cannot show.
    fn present_in(&self) -> usize {
        self.dps.iter().filter(|value| value.is_some()).count()
    }

    /// What to say beside the name about the combats this row is missing from,
    /// while the differences are being read. `None` when it is in all of them,
    /// when the toggle is off, or for a row the toggle is not about.
    fn missing_from(&self, depth: usize, n_slots: usize, ticks: &Ticks) -> Option<String> {
        if depth != 1 || ticks.differences.is_none() {
            return None;
        }
        let present = self.present_in();
        (present < n_slots).then(|| format!("(in {present} of {n_slots})"))
    }
}

/// How many of the rows under Total are counted in it.
fn ticked_count(sub_nodes: &[CompareNode], excluded: &FxHashSet<String>) -> usize {
    sub_nodes
        .iter()
        .filter(|node| !excluded.contains(&node.name))
        .count()
}

/// What the Total row is called: plain "Total" while the whole tree is in it,
/// and how much of it went in when it is not — a filtered total read as the
/// run's own figure is the one way this can mislead, on screen and in the
/// exported sheet alike.
fn total_row_name(kept: usize, rows: usize) -> String {
    if kept == rows {
        "Total".to_string()
    } else {
        format!("Total ({kept} of {rows} rows)")
    }
}

/// The player's outgoing damage in one combat, added up from the ticked rows
/// only.
///
/// Built out of the hits themselves and run through the analyzer's own metrics
/// pass, so every column — resistance, criticals, accuracy, the lot — comes out
/// the way the analyzer would have it for a group holding exactly those rows,
/// rather than out of an average of averages.
fn subset_total(slot: &Slot, excluded: &FxHashSet<String>) -> Option<DamageGroup> {
    let player = slot.combat.players.get(&slot.player)?;
    let name_manager = &slot.combat.name_manager;
    let rows = player.damage_out.sub_groups().values().map(|sub| {
        (
            sub.name().get(name_manager),
            sub.hits.get(&slot.combat.hits_manger),
        )
    });
    Some(subset_group(
        subset_hits(rows, excluded)?,
        metrics_duration(&player.combat_time),
        &slot.combat.total_damage_out,
    ))
}

/// The hits of the ticked rows, pooled — or `None` when this combat holds none
/// of the ticked rows at all.
///
/// A branch's hits are the whole branch's, so a row goes in with everything
/// under it, which is why only the rows under Total can be ticked.
///
/// The `None` is the difference between "this combat did nothing with what you
/// ticked" and "this combat has none of it": the second is an empty cell
/// everywhere else in the table, is left out of the averages, and is not
/// charted. A zero would instead draw a flat line along the bottom of the chart
/// and drag every average down for a run that simply flew something else.
fn subset_hits<'a>(
    rows: impl Iterator<Item = (&'a str, &'a [Hit])>,
    excluded: &FxHashSet<String>,
) -> Option<Vec<Hit>> {
    let kept: Vec<&[Hit]> = rows
        .filter(|(name, _)| !excluded.contains(*name))
        .map(|(_, hits)| hits)
        .collect();
    if kept.is_empty() {
        return None;
    }
    Some(kept.into_iter().flatten().copied().collect())
}

/// Whether a row is not drawn at all.
///
/// Only the rows under Total (`parent_depth` 0 is Total itself) can be hidden:
/// they are the ones the three filters are about, and a sub-row is drawn with
/// the branch it belongs to. The filters are `and`ed — each one is a separate
/// question the reader asked.
fn is_hidden(parent_depth: usize, ticks: &Ticks, node: &CompareNode) -> bool {
    if parent_depth != 0 {
        return false;
    }
    if ticks.hide_unticked && ticks.excluded.contains(&node.name) {
        return true;
    }
    if !ticks.types.is_empty()
        && !node
            .damage_types
            .iter()
            .any(|damage_type| ticks.types.contains(damage_type))
    {
        return true;
    }
    if let Some((measure, threshold)) = ticks.differences
        && node.difference(measure) < threshold
    {
        return true;
    }
    false
}

/// A damage group standing for a set of hits, with every metric recalculated
/// from them the way [`DamageGroup::recalculate_metrics`] does it, so the rest
/// of the table can read it like any other group.
fn subset_group(hits: Vec<Hit>, duration: f64, combat_total: &ShieldHullValues) -> DamageGroup {
    let mut damage_metrics = DamageMetrics::default();
    damage_metrics.calc_and_apply_delta(&hits);
    damage_metrics.recalculate_time_based_metrics(duration);
    let mut max_one_hit = MaxOneHit::default();
    max_one_hit.update_from_hits(NameHandle::UNKNOWN, &hits);
    DamageGroup {
        damage_percentage: ShieldHullOptionalValues::percentage(
            &damage_metrics.total_damage,
            combat_total,
        ),
        damage_metrics,
        max_one_hit,
        hits: Hits::Leaf(hits),
        ..Default::default()
    }
}

/// The combat duration the analyzer measures a player's outgoing damage
/// against: the time they were in combat.
///
/// Not `combat_duration_seconds`, which the charts use — that one is the length
/// of the fight (`active_time`), and dividing by it would give a DPS no other
/// part of the program states.
fn metrics_duration(combat_time: &Option<Range<NaiveDateTime>>) -> f64 {
    time_range_to_duration_or_zero(combat_time)
        .num_milliseconds()
        .max(0) as f64
        / 1e3
}

/// What an averaged value is made of: how many of the combats had this row at
/// all, and how far apart the best and the worst of them were. An average of
/// two runs out of twelve means something quite different from an average of
/// all twelve, and the number alone cannot say which it is.
fn average_tooltip(average: &AverageCell, n_slots: usize) -> String {
    let mut formatter = NumberFormatter::new();
    format!(
        "Average of {} of the {} combats\nlowest {}, highest {}",
        average.count,
        n_slots,
        formatter.format(average.min, 2),
        formatter.format(average.max, 2)
    )
}

/// Every row of the tree, in the order it is drawn, flattened for the export.
/// Collapsed rows are taken too: the file is the whole comparison, not the part
/// that happened to be unfolded when the button was pressed.
fn collect_export_rows(
    nodes: &[CompareNode],
    level: usize,
    n_slots: usize,
    averages: bool,
    rows: &mut Vec<export::Row>,
) {
    for node in nodes {
        let mut values = Vec::new();
        // One entry per configured column, whether or not this row has a value
        // for it, so every row lines up under the same headers.
        for metric_i in 0..node.averages.len() {
            if averages {
                values.push(node.averages[metric_i].as_ref().map(|a| a.value));
            } else {
                for slot_i in 0..n_slots {
                    values.push(
                        node.cells
                            .get(slot_i)
                            .and_then(|c| c.as_ref())
                            .and_then(|c| c.metrics.get(metric_i))
                            .and_then(|m| m.value),
                    );
                }
            }
        }
        rows.push(export::Row {
            name: node.name.clone(),
            level,
            values,
        });
        collect_export_rows(&node.sub_nodes, level + 1, n_slots, averages, rows);
    }
}

/// The one line the combats average out to, for the chart in averages mode.
///
/// Every combat's hits are pooled onto one time axis — each hit already carries
/// its offset from the start of its own combat — and every value is divided by
/// the number of combats that have this row. The charts are linear in the hit
/// values (a smoothed sum for the per-second lines, a bucketed sum for the
/// bars), so a pooled series scaled by `1/n` *is* the mean of the individual
/// lines, at every point, rather than an approximation of it.
///
/// Combats without the selected ability are left out of `n`, exactly as the
/// table's averages leave them out: charting a run that never fired the thing
/// as a zero would drag the line down for a reason that never happened.
///
/// The window is the longest of the pooled combats, so no hit falls outside it.
/// The tail is then the average of fewer runs than the head — unavoidable when
/// runs differ in length, and the alternative (cutting at the shortest) throws
/// away the end of every longer fight.
fn average_series(node: &CompareNode) -> Option<PreparedDamageDataSet> {
    let present: Vec<&SeriesData> = node.series.iter().flatten().collect();
    let n = present.len();
    if n == 0 {
        return None;
    }
    let scale = 1.0 / n as f64;
    let points: Vec<PreparedHit> = present
        .iter()
        .flat_map(|series| series.hits.iter())
        // The same hits the ordinary series drop: one that did nothing because
        // the target was immune is not damage that happened.
        .filter(|hit| !hit.flags.contains(ValueFlags::IMMUNE))
        .map(|hit| scaled_point(hit.into(), scale))
        .collect();
    let total = present.iter().map(|series| series.total).sum::<f64>() * scale;
    let duration = present
        .iter()
        .map(|series| series.combat_duration_s)
        .fold(0.0, f64::max);
    Some(PreparedDamageDataSet::base_new(
        &average_label(n),
        total,
        points.into_iter(),
        duration,
    ))
}

/// One hit's share of an average: every figure it carries scaled the same way —
/// the hit count included, which is what makes the hits-per-second chart show
/// an average rather than the runs added up. Scaling all of them together keeps
/// the parts adding up to the whole, and leaves the resistance chart (a ratio
/// of two of them) unchanged.
fn scaled_point(mut point: PreparedHit, scale: f64) -> PreparedHit {
    point.value.damage *= scale;
    point.value.hull_damage *= scale;
    point.value.shield_damage *= scale;
    point.value.base_damage *= scale;
    point.value.drain_damage *= scale;
    point.value.damage_prevented_to_hull *= scale;
    point.value.hits_count *= scale;
    point
}

/// What the averaged line is called. It says how many combats went into it,
/// because that is the one thing the line itself cannot show — and it is not
/// always every combat in the comparison.
fn average_label(n: usize) -> String {
    if n == 1 {
        "average of 1 combat".to_string()
    } else {
        format!("average of {n} combats")
    }
}

fn find_node(nodes: &[CompareNode], id: u32) -> Option<&CompareNode> {
    for node in nodes {
        if node.id == id {
            return Some(node);
        }
        if let Some(found) = find_node(&node.sub_nodes, id) {
            return Some(found);
        }
    }
    None
}

/// Points every slot at the same player as the reference, where that player
/// took part.
///
/// Each combat is otherwise opened on its own top-DPS player, and in a team
/// those are rarely the same person — the deltas would then compare one player
/// against another rather than one player's runs against each other, which
/// makes them read as noise. A slot the reference player was not in keeps its
/// own top player, and the picker in the legend says whose numbers it is
/// showing.
fn follow_the_reference_player(slots: &mut [Slot]) {
    let Some(reference) = slots.first() else {
        return;
    };
    let reference_name = reference
        .player
        .get(&reference.combat.name_manager)
        .to_string();
    for slot in slots.iter_mut().skip(1) {
        if let Some(handle) = slot.combat.name_manager.get_handle(&reference_name)
            && slot.combat.players.contains_key(&handle)
        {
            slot.player = handle;
        }
    }
}

/// The user's note for each slot's combat, empty where they wrote none.
fn slot_notes(slots: &[Slot], notes: &CombatNotes) -> Vec<String> {
    slots
        .iter()
        .map(|slot| notes.get(&CombatNotes::key(&slot.combat)).to_owned())
        .collect()
}

/// One slot's note, or an empty string when the slot is out of range.
fn note_of(notes: &[String], slot_i: usize) -> &str {
    notes.get(slot_i).map(String::as_str).unwrap_or("")
}

/// A note as it reads appended to a sentence in a tooltip.
fn note_suffix(note: &str) -> String {
    if note.is_empty() {
        String::new()
    } else {
        format!(" — {note}")
    }
}

/// How many legend rows the list opens at before the user drags its boundary.
/// Six: enough that an ordinary comparison shows every run without scrolling,
/// few enough that a session's worth still leaves the table its room.
const LEGEND_ROWS: usize = 6;

/// How small and how large the list of runs may be dragged, as a share of the
/// space the three panes share. The lower bound leaves a row visible rather
/// than letting the list be closed by accident; the upper leaves the table
/// something to be a table in.
const MIN_LEGEND_RATIO: f32 = 0.05;
const MAX_LEGEND_RATIO: f32 = 0.5;

/// How tall the list of runs wants to be, for `row` points per row: the height
/// of what it holds, capped at [`LEGEND_ROWS`].
fn legend_height(row: f32, combats: usize) -> f32 {
    row * combats.min(LEGEND_ROWS) as f32
}

/// Where the boundary between the list and the table starts out, as a share of
/// `available`. Sized to what the list holds, so two runs do not open with a
/// third of the window under them, and thirty do not open with the table
/// squeezed to nothing.
fn legend_ratio(row: f32, combats: usize, available: f32) -> f32 {
    if available <= 0.0 {
        return MIN_LEGEND_RATIO;
    }
    (legend_height(row, combats) / available).clamp(MIN_LEGEND_RATIO, MAX_LEGEND_RATIO)
}

/// The height of the header: two lines, and a third when the notes are shown.
fn header_height(with_notes: bool) -> f32 {
    HEADER_LINE_HEIGHT * if with_notes { 3.0 } else { 2.0 }
}

/// One header cell: the metric name (only on the group's first column, where it
/// stands for the whole group), the combat number under it, and the user's note
/// under that.
///
/// The number and the note are drawn in the colour that combat's line has on
/// the chart, so a column and a line can be paired off by eye; the metric name
/// stays in the ordinary text colour, because it belongs to the whole group of
/// columns rather than to one combat. `note` is `None` when no combat in the
/// comparison has one and the line is left out entirely.
fn header_text(
    font: &FontId,
    text_color: Color32,
    metric: &str,
    number: &str,
    note: Option<&str>,
    color: Option<Color32>,
) -> LayoutJob {
    let combat = TextFormat::simple(font.clone(), color.unwrap_or(text_color));
    let mut job = LayoutJob::default();
    job.append(
        &format!("{metric}\n"),
        0.0,
        TextFormat::simple(font.clone(), text_color),
    );
    job.append(number, 0.0, combat.clone());
    if let Some(note) = note {
        job.append(&format!("\n{note}"), 0.0, combat);
    }
    job
}

/// One line of the legend above the table: the combat's number, what the
/// program calls it, and the user's note where they wrote one.
///
/// The number and the note carry the combat's colour from the chart. The
/// identifier between them does not: it is the longest part of the line, and a
/// whole row of it in a chart colour reads as a warning rather than a label.
fn legend_text(
    font: &FontId,
    text_color: Color32,
    slot_i: usize,
    identifier: &str,
    note: &str,
    color: Option<Color32>,
) -> LayoutJob {
    let combat = TextFormat::simple(font.clone(), color.unwrap_or(text_color));
    let mut job = LayoutJob::default();
    job.append(&format!("{}:", slot_i + 1), 0.0, combat.clone());
    job.append(
        &format!(" {identifier}"),
        0.0,
        TextFormat::simple(font.clone(), text_color),
    );
    if !note.is_empty() {
        job.append(&format!(" — {note}"), 0.0, combat);
    }
    job
}

/// One combat's chart label: its slot number, and the user's note where they
/// wrote one.
///
/// The number stays in front of the note, because it is what the table columns
/// (`#1 (ref)`, `#2`) and the deltas are labelled with — a legend of notes
/// alone would name the lines but no longer say which column each one is.
fn chart_label(slot_i: usize, note: &str) -> String {
    let number = slot_i + 1;
    if note.is_empty() {
        number.to_string()
    } else {
        format!("{number} — {note}")
    }
}

/// A warning that not every combat is showing the reference's player: the line
/// on screen, and what a hover says.
struct OddPlayerWarning {
    line: String,
    detail: String,
}

/// Whether the comparison is about one player's runs, and what to say when it
/// is not.
///
/// A slot falls back to its own top-DPS player when the reference player was
/// not in that fight (`follow_the_reference_player`), and then a difference is
/// one person measured against another rather than one person's build against
/// itself — which reads exactly like a build that changed. Worth a word, but
/// only a word: the line names how many and the hover names which, because a
/// comparison of a whole session would otherwise put a paragraph of handles
/// above the table.
fn odd_player_warning(names: &[String]) -> Option<OddPlayerWarning> {
    let reference = names.first()?;
    let odd: Vec<String> = names
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, name)| *name != reference)
        .map(|(slot_i, name)| format!("#{} is {name}", slot_i + 1))
        .collect();
    if odd.is_empty() {
        return None;
    }
    Some(OddPlayerWarning {
        line: format!(
            "⚠ {} of {} combats {} not showing {reference}",
            odd.len(),
            names.len(),
            if odd.len() == 1 { "is" } else { "are" }
        ),
        detail: format!(
            "{}.\nTheir figures are compared against {reference}'s, so a difference here can be \
             one player against another rather than one player's runs. Where that player took \
             part, pick them in the list above.",
            odd.join(", ")
        ),
    })
}

/// The players a comparison is showing, one per slot, for the warning above the
/// table.
fn slot_player_names(slots: &[Slot]) -> Vec<String> {
    slots
        .iter()
        .map(|s| s.player.get(&s.combat.name_manager).to_string())
        .collect()
}

fn top_dps_player(combat: &Combat) -> NameHandle {
    players_by_dps(combat)
        .into_iter()
        .next()
        .map(|(handle, _)| handle)
        .unwrap_or(NameHandle::UNKNOWN)
}

fn players_by_dps(combat: &Combat) -> Vec<(NameHandle, String)> {
    let mut players: Vec<(NameHandle, f64, String)> = combat
        .players
        .iter()
        .map(|(handle, player)| {
            (
                *handle,
                player.damage_out.dps.all,
                handle.get(&combat.name_manager).to_string(),
            )
        })
        .collect();
    players.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    players
        .into_iter()
        .map(|(handle, _, name)| (handle, name))
        .collect()
}

/// Build one level of the aligned tree from the parent damage groups of each
/// slot (union of child ability names), recursing into sub-groups.
fn build_level(
    parents: &[Option<&DamageGroup>],
    name_managers: &[&NameManager],
    hits_managers: &[&HitsManager],
    durations: &[f64],
    columns: &[CompareMetric],
    id_source: &mut u32,
) -> Vec<CompareNode> {
    let n = parents.len();
    let mut order: Vec<String> = Vec::new();
    let mut index: FxHashMap<String, usize> = Default::default();
    let mut children: Vec<Vec<Option<&DamageGroup>>> = Vec::new();

    for (slot_i, parent) in parents.iter().enumerate() {
        let Some(parent) = parent else { continue };
        for sub in parent.sub_groups().values() {
            let name = sub.name().get(name_managers[slot_i]).to_string();
            let idx = *index.entry(name.clone()).or_insert_with(|| {
                order.push(name);
                children.push(vec![None; n]);
                order.len() - 1
            });
            children[idx][slot_i] = Some(sub);
        }
    }

    let mut nodes: Vec<CompareNode> = order
        .into_iter()
        .enumerate()
        .map(|(idx, name)| {
            let per_slot = &children[idx];
            let id = *id_source;
            *id_source += 1;
            let sort_key = per_slot[0].map(|g| g.dps.all).unwrap_or(f64::NEG_INFINITY);
            let (cells, averages) = build_row(per_slot, columns);
            let series = build_series(per_slot, hits_managers, durations);
            let sub_nodes = build_level(
                per_slot,
                name_managers,
                hits_managers,
                durations,
                columns,
                id_source,
            );
            CompareNode {
                name,
                id,
                cells,
                averages,
                series,
                shares: per_slot
                    .iter()
                    .map(|g| g.and_then(|g| g.damage_percentage.all))
                    .collect(),
                dps: per_slot.iter().map(|g| g.map(|g| g.dps.all)).collect(),
                damage_types: damage_types(per_slot, name_managers),
                sort_key,
                sub_nodes,
                open: false,
            }
        })
        .collect();

    nodes.sort_by(|a, b| {
        b.sort_key
            .partial_cmp(&a.sort_key)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    nodes
}

/// Every damage type this row dealt, over all the combats, in name order.
///
/// The analyzer already keeps `Shield` out of a group that has a real type of
/// its own (`DamageGroup::add_damage_type_non_pool`) — the log gives no energy
/// type for a hit on shields — so what comes out here is the weapon types, and
/// a group of several weapons carries all of theirs.
fn damage_types(per_slot: &[Option<&DamageGroup>], name_managers: &[&NameManager]) -> Vec<String> {
    let mut types: Vec<String> = per_slot
        .iter()
        .enumerate()
        .filter_map(|(slot_i, group)| Some((slot_i, (*group)?)))
        .flat_map(|(slot_i, group)| {
            group
                .damage_types
                .iter()
                .map(move |t| t.get(name_managers[slot_i]).to_string())
        })
        .collect();
    types.sort_unstable();
    types.dedup();
    types
}

fn build_series(
    per_slot: &[Option<&DamageGroup>],
    hits_managers: &[&HitsManager],
    durations: &[f64],
) -> Vec<Option<SeriesData>> {
    per_slot
        .iter()
        .enumerate()
        .map(|(slot_i, g)| {
            g.map(|g| SeriesData {
                hits: g.hits.get(hits_managers[slot_i]).to_vec(),
                total: g.total_damage.all,
                combat_duration_s: durations[slot_i],
            })
        })
        .collect()
}

/// A group's hits per second and average hit, the two factors of its DPS. An
/// absent group contributes nothing, which is what a zero pair means.
fn dps_factors(group: Option<&DamageGroup>) -> (f64, f64) {
    match group {
        Some(group) => (
            group.hits_per_second.all,
            group.average_hit.all.unwrap_or(0.0),
        ),
        None => (0.0, 0.0),
    }
}

fn dps_breakdown(reference: Option<&DamageGroup>, slot: Option<&DamageGroup>) -> DpsBreakdown {
    let (r1, m1) = dps_factors(reference);
    let (r2, m2) = dps_factors(slot);
    split_dps_difference(r1, m1, r2, m2)
}

fn split_dps_difference(r1: f64, m1: f64, r2: f64, m2: f64) -> DpsBreakdown {
    DpsBreakdown {
        rate: (r2 - r1) * (m1 + m2) / 2.0,
        size: (m2 - m1) * (r1 + r2) / 2.0,
    }
}

/// One row of the table: a cell per slot, and the same row averaged across the
/// slots. Both come out of one pass over the metrics, since they read the same
/// numbers.
fn build_row(
    per_slot: &[Option<&DamageGroup>],
    columns: &[CompareMetric],
) -> (Vec<Option<SlotCell>>, Vec<Option<AverageCell>>) {
    let mut formatter = NumberFormatter::new();

    // Raw metric values per slot, so combats 2+ can be compared to slot 0.
    let raw: Vec<Option<Vec<Option<f64>>>> = per_slot
        .iter()
        .map(|g| g.map(|g| columns.iter().map(|c| c.extract(g)).collect::<Vec<_>>()))
        .collect();
    let base = raw.first().and_then(|o| o.as_ref());

    let cells = raw
        .iter()
        .enumerate()
        .map(|(slot_i, values)| {
            values.as_ref().map(|values| {
                let metrics = values
                    .iter()
                    .enumerate()
                    .map(|(m, value)| {
                        let column = columns[m];
                        let text = value
                            .map(|v| formatter.format(v, column.precision()))
                            .unwrap_or_default();
                        let delta = if slot_i == 0 {
                            None
                        } else {
                            make_delta(base.and_then(|b| b[m]), *value, column, &mut formatter)
                        };
                        MetricCell {
                            text,
                            value: *value,
                            delta,
                        }
                    })
                    .collect();
                SlotCell {
                    metrics,
                    breakdown: (slot_i > 0).then(|| {
                        dps_breakdown(per_slot.first().copied().flatten(), per_slot[slot_i])
                    }),
                }
            })
        })
        .collect();

    let averages = build_averages(&raw, columns, &mut formatter);
    (cells, averages)
}

/// Each column averaged over the combats that have a value for this row.
///
/// A plain mean, every combat counting once — including for the percentages,
/// so what the column shows is the average of the numbers above it rather than
/// a differently-weighted figure that no column states.
fn build_averages(
    raw: &[Option<Vec<Option<f64>>>],
    columns: &[CompareMetric],
    formatter: &mut NumberFormatter,
) -> Vec<Option<AverageCell>> {
    columns
        .iter()
        .enumerate()
        .map(|(m, column)| {
            let values: Vec<f64> = raw
                .iter()
                .filter_map(|slot| slot.as_ref()?.get(m).copied().flatten())
                .collect();
            let count = values.len();
            if count == 0 {
                return None;
            }
            let value = values.iter().sum::<f64>() / count as f64;
            Some(AverageCell {
                value,
                text: formatter.format(value, column.precision()),
                count,
                min: values.iter().copied().fold(f64::INFINITY, f64::min),
                max: values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            })
        })
        .collect()
}

/// Formatted, colored delta of `current` versus `base` for one metric. `None`
/// when `current` is missing (the cell is empty) or the values are equal. When
/// the reference combat has no value to compare against (the ability is absent
/// there), the baseline is treated as zero, so the whole value still shows as a
/// colored +/- delta.
fn make_delta(
    base: Option<f64>,
    current: Option<f64>,
    metric: CompareMetric,
    formatter: &mut NumberFormatter,
) -> Option<DeltaCell> {
    let current = current?;
    let base = base.unwrap_or(0.0);
    let diff = current - base;
    if diff.abs() < 1e-9 {
        return None;
    }
    let improvement = if metric.higher_is_better() {
        diff > 0.0
    } else {
        diff < 0.0
    };
    let sign = if diff > 0.0 { "+" } else { "-" };
    Some(DeltaCell {
        text: format!(
            "{}{}",
            sign,
            formatter.format(diff.abs(), metric.precision())
        ),
        improvement,
    })
}

fn show_time_slice_setting(time_slice: &mut f64, ui: &mut Ui) -> bool {
    ui.horizontal(|ui| {
        let changed = SliderTextEdit::new(time_slice, 0.1..=6.0, "compare time slice slider")
            .clamp_min(0.1)
            .clamp_max(120.0)
            .desired_text_edit_width(30.0)
            .display_precision(4)
            .step_by(0.1)
            .show(ui)
            .changed();
        ui.label("Time Slice (s)");
        changed
    })
    .inner
}

fn show_time_filter_setting(filter: &mut f64, ui: &mut Ui) -> bool {
    ui.horizontal(|ui| {
        let changed = SliderTextEdit::new(filter, 0.4..=6.0, "compare filter slider")
            .clamp_min(0.1)
            .clamp_max(120.0)
            .desired_text_edit_width(30.0)
            .display_precision(4)
            .step_by(0.1)
            .show(ui)
            .changed();
        ui.label("Gauss Filter Standard Deviation (how much to smooth the graph)");
        changed
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two shares must add up to the whole DPS difference, whatever the
    /// numbers — that is the point of taking them at the midpoint of the pair
    /// rather than against one end, which leaves a cross term over.
    #[test]
    fn the_two_shares_add_up_to_the_whole_difference() {
        for (r1, m1, r2, m2) in [
            (10.0, 500.0, 12.0, 600.0), // fired more often and hit harder
            (10.0, 500.0, 20.0, 250.0), // twice the rate, half the size
            (10.0, 500.0, 4.0, 900.0),  // slower but much harder
            (0.0, 0.0, 7.0, 300.0),     // the ability is new in this combat
            (7.0, 300.0, 0.0, 0.0),     // ...and the other way round
            (10.0, 500.0, 10.0, 500.0), // no change at all
        ] {
            let split = split_dps_difference(r1, m1, r2, m2);
            let whole = r2 * m2 - r1 * m1;
            assert!(
                (split.rate + split.size - whole).abs() < 1e-9,
                "{r1}x{m1} -> {r2}x{m2}: {} + {} != {whole}",
                split.rate,
                split.size
            );
        }
    }

    /// Each share is attributed to the factor that actually moved.
    #[test]
    fn a_change_in_one_factor_alone_lands_on_that_factor() {
        let faster = split_dps_difference(10.0, 500.0, 15.0, 500.0);
        assert_eq!(2500.0, faster.rate, "firing 5 more per second at 500 each");
        assert_eq!(0.0, faster.size);

        let harder = split_dps_difference(10.0, 500.0, 10.0, 700.0);
        assert_eq!(0.0, harder.rate);
        assert_eq!(2000.0, harder.size, "200 more per hit at 10 per second");
    }

    /// A trade — more hits, each weaker — shows up as one share up and the
    /// other down, which is the case a single DPS number hides.
    #[test]
    fn a_trade_shows_up_as_opposite_shares() {
        let split = split_dps_difference(10.0, 500.0, 20.0, 300.0);
        assert!(split.rate > 0.0, "the extra hits helped");
        assert!(split.size < 0.0, "each one landing softer cost");
        assert!((split.rate + split.size - 1000.0).abs() < 1e-9);
    }

    /// A combat the user named is charted under that name, so the legend says
    /// which run a line is rather than only which slot it sits in.
    #[test]
    fn a_noted_combat_is_charted_under_its_note() {
        assert_eq!("1 — Cheops build", chart_label(0, "Cheops build"));
        assert_eq!("2 — FAW build", chart_label(1, "FAW build"));
    }

    /// Without a note there is nothing to say beyond the slot number, which is
    /// what the table columns are labelled with.
    #[test]
    fn a_combat_without_a_note_is_charted_under_its_number() {
        assert_eq!("1", chart_label(0, ""));
        assert_eq!("3", chart_label(2, ""));
    }

    fn header(note: Option<&str>, color: Option<Color32>) -> LayoutJob {
        header_text(&FontId::default(), Color32::WHITE, "DPS", "#2", note, color)
    }

    /// The note is a line of its own under the combat number, so a column says
    /// which run it is about and not only which slot it sits in.
    #[test]
    fn a_column_header_carries_the_note_under_the_number() {
        assert_eq!("DPS\n#2\nFAW build", header(Some("FAW build"), None).text);
    }

    /// A comparison of runs nobody named keeps the two-line header it had,
    /// rather than a blank third line under every column.
    #[test]
    fn a_column_header_without_notes_stays_two_lines() {
        assert_eq!("DPS\n#2", header(None, None).text);
        assert_eq!(2.0 * HEADER_LINE_HEIGHT, header_height(false));
        assert_eq!(3.0 * HEADER_LINE_HEIGHT, header_height(true));
    }

    /// The number and the note take the colour of that combat's line on the
    /// chart; the metric name does not, since it stands for the whole group of
    /// columns rather than for one combat.
    #[test]
    fn the_number_and_the_note_take_the_series_colour() {
        let job = header(Some("FAW build"), Some(Color32::RED));
        let colors: Vec<Color32> = job.sections.iter().map(|s| s.format.color).collect();
        assert_eq!(
            vec![Color32::WHITE, Color32::RED, Color32::RED],
            colors,
            "metric name, combat number, note"
        );
    }

    /// The legend colours the same two things the header does, and leaves the
    /// identifier between them alone.
    #[test]
    fn the_legend_colours_the_number_and_the_note() {
        let job = legend_text(
            &FontId::default(),
            Color32::WHITE,
            1,
            "Infected Space",
            "FAW build",
            Some(Color32::RED),
        );
        assert_eq!("2: Infected Space — FAW build", job.text);
        let colors: Vec<Color32> = job.sections.iter().map(|s| s.format.color).collect();
        assert_eq!(
            vec![Color32::RED, Color32::WHITE, Color32::RED],
            colors,
            "number, identifier, note"
        );
    }

    /// A run with no note is just the number and the identifier — no dash left
    /// hanging at the end of the line.
    #[test]
    fn the_legend_of_an_unnamed_run_ends_at_the_identifier() {
        let job = legend_text(
            &FontId::default(),
            Color32::WHITE,
            0,
            "Infected Space",
            "",
            None,
        );
        assert_eq!("1: Infected Space", job.text);
    }

    /// The header's lines are counted by hand at [`HEADER_LINE_HEIGHT`], since
    /// the table reserves the height before it draws anything. A font whose
    /// rows are taller than that would push the note line out of the space
    /// reserved for it and into the first row of the table.
    #[test]
    fn a_header_line_fits_the_font_it_is_drawn_in() {
        let ctx = Context::default();
        crate::app::fonts::install(&ctx);
        // The fonts only exist once a pass has run.
        let _ = ctx.run_ui(Default::default(), |_| {});
        let font = TextStyle::Body.resolve(&Style::default());
        let row_height = ctx.fonts_mut(|fonts| fonts.row_height(&font));
        assert!(
            row_height <= HEADER_LINE_HEIGHT,
            "a line of the header is {row_height} high, more than the {HEADER_LINE_HEIGHT} \
             reserved for it"
        );
    }

    /// Nothing is charted for that combat — the number is left in the ordinary
    /// text colour rather than picked out in a colour that means nothing.
    #[test]
    fn a_combat_the_chart_has_no_line_for_is_left_uncoloured() {
        let job = header(None, None);
        assert!(
            job.sections
                .iter()
                .all(|s| s.format.color == Color32::WHITE)
        );
    }

    /// Every character the compare view draws has to exist in the fonts the app
    /// bundles, or it comes out as an empty box that says nothing.
    ///
    /// This caught two: the arrow between the two date fields (U+2192) and the
    /// one the Columns menu was labelled with (U+25BE, `▾`). Known missing:
    /// `▾`, `→`, `🗎`, `⇒`. Known present: the ones asserted below — check a new
    /// one here before drawing it.
    #[test]
    fn every_glyph_the_compare_view_draws_exists_in_the_font() {
        let ctx = Context::default();
        crate::app::fonts::install(&ctx);
        // The fonts only exist once a pass has run.
        let _ = ctx.run_ui(Default::default(), |_| {});
        let font = TextStyle::Body.resolve(&Style::default());
        for glyph in "Σ🖹⚠🆚◀⏷⏵👁Δ☰".chars() {
            assert!(
                ctx.fonts_mut(|fonts| fonts.has_glyph(&font, glyph)),
                "'{glyph}' (U+{:04X}) has no glyph and would draw as an empty box",
                glyph as u32
            );
        }
    }

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    /// One player's runs are what the comparison is normally about, and that
    /// says nothing at all.
    #[test]
    fn one_player_throughout_is_not_worth_a_warning() {
        assert!(odd_player_warning(&names(&["Kestrel", "Kestrel", "Kestrel"])).is_none());
        assert!(odd_player_warning(&names(&["Kestrel"])).is_none());
        assert!(odd_player_warning(&[]).is_none());
    }

    /// The line is a count, not a list: a session's worth of handles above the
    /// table was a paragraph nobody read.
    #[test]
    fn the_warning_counts_on_screen_and_names_on_hover() {
        let warning = odd_player_warning(&names(&[
            "Kestrel", "Kestrel", "Somebody", "Kestrel", "Else", "Kestrel",
        ]))
        .expect("two of them are showing somebody else");

        assert_eq!("⚠ 2 of 6 combats are not showing Kestrel", warning.line);
        assert!(warning.line.len() < 60, "{}", warning.line);
        // Which ones, by the number the columns are labelled with.
        assert!(warning.detail.starts_with("#3 is Somebody, #5 is Else"));
    }

    /// One odd combat reads as one, not as "1 combats are".
    #[test]
    fn a_single_odd_combat_reads_as_one() {
        let warning = odd_player_warning(&names(&["Kestrel", "Somebody"])).unwrap();
        assert_eq!("⚠ 1 of 2 combats is not showing Kestrel", warning.line);
    }

    /// The list opens at the height of what it holds — no room reserved for
    /// rows that are not there.
    #[test]
    fn a_short_legend_takes_only_the_rows_it_has() {
        assert_eq!(0.0, legend_height(21.0, 0));
        assert_eq!(63.0, legend_height(21.0, 3));
        assert_eq!(126.0, legend_height(21.0, LEGEND_ROWS));
    }

    /// Past the cap it stops growing, whatever the selection. Measured at 21
    /// points a row, a legend of fifty combats would otherwise fill a 1080-point
    /// window on its own and leave the table and the chart drawn past the bottom
    /// edge, with no way to scroll to them.
    #[test]
    fn a_long_legend_stops_at_the_cap() {
        let capped = legend_height(21.0, LEGEND_ROWS);
        for combats in [LEGEND_ROWS + 1, 20, 50, 500] {
            assert_eq!(capped, legend_height(21.0, combats), "{combats} combats");
        }
        assert!(
            capped < 720.0,
            "the cap has to leave a short window something to draw the table in"
        );
    }

    /// Two runs open with the list at a couple of rows, not at a fixed share of
    /// the window — the boundary is there to be dragged, not to start in the
    /// way.
    #[test]
    fn the_list_opens_at_the_height_of_what_it_holds() {
        // Four rows of 21 points in a 900-point window is a tenth of it.
        let ratio = legend_ratio(21.0, 4, 900.0);
        assert!((ratio - 84.0 / 900.0).abs() < 1e-6, "{ratio}");

        // Two rows want less than the floor allows, so they get the floor —
        // which in that window is about two rows anyway.
        assert_eq!(MIN_LEGEND_RATIO, legend_ratio(21.0, 2, 900.0));
    }

    /// However many runs are compared, the table keeps at least half the room
    /// until the user says otherwise — and however few, the boundary stays
    /// grabbable rather than collapsing onto the edge.
    #[test]
    fn the_opening_split_stays_between_its_bounds() {
        for (combats, available) in [(1, 900.0), (50, 900.0), (500, 200.0), (3, 40.0)] {
            let ratio = legend_ratio(21.0, combats, available);
            assert!(
                (MIN_LEGEND_RATIO..=MAX_LEGEND_RATIO).contains(&ratio),
                "{combats} combats in {available}: {ratio}"
            );
        }
    }

    /// A window with no room yet — the first frame, before egui has laid
    /// anything out — must not divide by zero or open at a NaN.
    #[test]
    fn a_window_with_no_room_opens_at_the_smallest_split() {
        assert_eq!(MIN_LEGEND_RATIO, legend_ratio(21.0, 3, 0.0));
        assert_eq!(MIN_LEGEND_RATIO, legend_ratio(21.0, 3, -10.0));
    }

    /// The row height is asked of the style rather than written down, so the
    /// list follows the UI scale instead of being six rows at one zoom level
    /// and four at another.
    #[test]
    fn the_cap_follows_the_row_height_it_is_given() {
        assert_eq!(
            2.0 * legend_height(10.0, 4),
            legend_height(20.0, 4),
            "twice the row height is twice the height"
        );
    }

    fn averages_of(raw: &[Option<Vec<Option<f64>>>]) -> Vec<Option<AverageCell>> {
        build_averages(
            raw,
            &[CompareMetric::Dps, CompareMetric::Critical],
            &mut NumberFormatter::new(),
        )
    }

    /// Every combat counts once, including for the percentages — the column
    /// says what the numbers above it average out to.
    #[test]
    fn an_average_counts_every_combat_once() {
        let raw = vec![
            Some(vec![Some(100.0), Some(40.0)]),
            Some(vec![Some(200.0), Some(44.0)]),
            Some(vec![Some(300.0), Some(48.0)]),
        ];
        let averages = averages_of(&raw);

        let dps = averages[0].as_ref().unwrap();
        assert_eq!(200.0, dps.value);
        assert_eq!(3, dps.count);
        assert_eq!(100.0, dps.min);
        assert_eq!(300.0, dps.max);

        assert_eq!(44.0, averages[1].as_ref().unwrap().value);
    }

    /// An ability flown in two runs out of five averages those two rather than
    /// counting the other three as zero, which would read as a nerf that never
    /// happened. The count is carried so the tooltip can say which it is.
    #[test]
    fn an_absent_row_is_left_out_rather_than_counted_as_zero() {
        let raw = vec![
            Some(vec![Some(100.0), Some(40.0)]),
            None,
            Some(vec![Some(200.0), None]),
        ];
        let averages = averages_of(&raw);

        let dps = averages[0].as_ref().unwrap();
        assert_eq!(150.0, dps.value, "the two combats that have it, not three");
        assert_eq!(2, dps.count);

        let critical = averages[1].as_ref().unwrap();
        assert_eq!(40.0, critical.value, "a metric can be missing on its own");
        assert_eq!(1, critical.count);
    }

    /// Nothing to average is an empty cell, not a zero.
    #[test]
    fn a_metric_no_combat_has_leaves_the_cell_empty() {
        let raw = vec![Some(vec![None, Some(40.0)]), None];
        assert!(averages_of(&raw)[0].is_none());
    }

    /// The tooltip says how much of the comparison went into the number, which
    /// is what tells an average of everything from an average of a corner of it.
    #[test]
    fn the_tooltip_says_how_many_combats_the_average_is_of() {
        let raw = vec![
            Some(vec![Some(100.0), None]),
            None,
            Some(vec![Some(300.0), None]),
        ];
        let tooltip = average_tooltip(averages_of(&raw)[0].as_ref().unwrap(), 3);
        assert!(tooltip.contains("2 of the 3"), "{tooltip}");
        assert!(tooltip.contains("100"), "{tooltip}");
        assert!(tooltip.contains("300"), "{tooltip}");
    }

    fn hit(damage: f64, time_millis: u32) -> Hit {
        flagged_hit(damage, time_millis, ValueFlags::NONE)
    }

    fn crit(damage: f64, time_millis: u32) -> Hit {
        flagged_hit(damage, time_millis, ValueFlags::CRITICAL)
    }

    fn flagged_hit(damage: f64, time_millis: u32, flags: ValueFlags) -> Hit {
        use crate::analyzer::{BaseHit, SpecificHit};
        Hit {
            hit: BaseHit {
                damage,
                flags,
                specific: SpecificHit::Hull {
                    base_damage: damage,
                },
            },
            time_millis,
        }
    }

    fn combat_total(all: f64) -> ShieldHullValues {
        ShieldHullValues {
            all,
            shield: 0.0,
            hull: all,
        }
    }

    fn excluded(names: &[&str]) -> FxHashSet<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    fn at(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").unwrap()
    }

    /// The Total of a set of ticked rows is not the columns added up — it is
    /// their hits put back through the analyzer's own metrics pass, so every
    /// figure comes out the way it would for a group holding exactly those
    /// rows.
    #[test]
    fn a_filtered_total_is_recalculated_from_the_hits_it_keeps() {
        let group = subset_group(
            vec![hit(100.0, 0), hit(300.0, 2000)],
            10.0,
            &combat_total(1000.0),
        );

        assert_eq!(400.0, group.total_damage.all);
        assert_eq!(40.0, group.dps.all, "400 damage over 10 seconds");
        assert_eq!(2, group.damage_metrics.hits.all);
        assert_eq!(Some(200.0), group.average_hit.all);
        assert_eq!(300.0, group.max_one_hit.damage);
        assert_eq!(
            Some(40.0),
            group.damage_percentage.all,
            "still a share of the whole fight, as the unfiltered Total is"
        );
    }

    /// The percentages are recalculated with it, rather than carried over from
    /// the rows that are still in — one crit in two hull hits is a 50% crit
    /// rate whatever the rest of the run did.
    #[test]
    fn a_filtered_total_recalculates_the_percentages_too() {
        let group = subset_group(
            vec![hit(100.0, 0), crit(300.0, 1000)],
            10.0,
            &combat_total(400.0),
        );

        assert_eq!(Some(50.0), group.critical_percentage);
        assert_eq!(Some(100.0), group.damage_percentage.all);
    }

    /// Unticking a row takes its hits out of the Total, and leaves the rest of
    /// them exactly as they were.
    #[test]
    fn an_unticked_row_is_left_out_of_the_total() {
        let beams = [hit(100.0, 0), hit(100.0, 1000)];
        let torpedoes = [hit(500.0, 500)];
        let rows = || {
            [
                ("Phaser Beam Array", beams.as_slice()),
                ("Photon Torpedo", torpedoes.as_slice()),
            ]
            .into_iter()
        };

        let everything = subset_hits(rows(), &excluded(&[])).expect("nothing is unticked");
        assert_eq!(3, everything.len());
        assert_eq!(
            700.0,
            subset_group(everything, 10.0, &combat_total(700.0))
                .total_damage
                .all
        );

        let kept = subset_hits(rows(), &excluded(&["Photon Torpedo"])).expect("the beams are in");
        assert_eq!(2, kept.len());
        let total = subset_group(kept, 10.0, &combat_total(700.0));
        assert_eq!(200.0, total.total_damage.all, "the torpedo is out");
        assert_eq!(20.0, total.dps.all);
    }

    /// A row name that is not in the tree (a leftover from another player, say)
    /// takes nothing out with it.
    #[test]
    fn an_unknown_row_name_leaves_the_total_alone() {
        let beams = [hit(100.0, 0)];
        let rows = [("Phaser Beam Array", beams.as_slice())].into_iter();
        assert_eq!(
            1,
            subset_hits(rows, &excluded(&["Tetryon Cannon"]))
                .expect("the row it does name is still in")
                .len()
        );
    }

    /// A combat that has none of the ticked rows leaves an empty cell, exactly
    /// as an absent row does everywhere else in the table — not a zero, which
    /// would draw a line along the bottom of the chart and pull every average
    /// down for a run that simply flew something else.
    #[test]
    fn a_combat_without_any_ticked_row_is_left_out_entirely() {
        let torpedoes = [hit(500.0, 0)];
        let rows = [("Photon Torpedo", torpedoes.as_slice())].into_iter();
        assert!(subset_hits(rows, &excluded(&["Photon Torpedo"])).is_none());

        // Unticking everything is the same case, for every combat at once.
        let beams = [hit(100.0, 0)];
        let rows = [
            ("Phaser Beam Array", beams.as_slice()),
            ("Photon Torpedo", torpedoes.as_slice()),
        ]
        .into_iter();
        assert!(subset_hits(rows, &excluded(&["Phaser Beam Array", "Photon Torpedo"])).is_none());
    }

    /// The Total is measured against the time the player was in combat, which
    /// is what the analyzer divides by — not the length of the fight, which is
    /// what the charts use.
    #[test]
    fn the_filtered_total_uses_the_time_in_combat() {
        let start = at("2026-07-23 20:07");
        let end = start + chrono::Duration::seconds(30);
        assert_eq!(30.0, metrics_duration(&Some(start..end)));
        assert_eq!(0.0, metrics_duration(&None));
    }

    /// The name says how much of the tree the Total is of, so a filtered figure
    /// cannot be read as the run's own — on screen or in the exported sheet.
    #[test]
    fn a_filtered_total_says_so_in_its_name() {
        assert_eq!("Total", total_row_name(5, 5));
        assert_eq!("Total (2 of 5 rows)", total_row_name(2, 5));
        assert_eq!("Total (0 of 5 rows)", total_row_name(0, 5));
    }

    /// The tick on the Total row stands for the rows below it: full when they
    /// are all in, empty when none are, half-filled in between.
    #[test]
    fn the_total_counts_the_rows_that_are_ticked() {
        let rows = vec![
            node("Phaser Beam Array", vec![Some(0.0)], Vec::new()),
            node("Photon Torpedo", vec![Some(0.0)], Vec::new()),
        ];

        assert_eq!(2, ticked_count(&rows, &excluded(&[])));
        assert_eq!(1, ticked_count(&rows, &excluded(&["Photon Torpedo"])));
        assert_eq!(
            0,
            ticked_count(&rows, &excluded(&["Photon Torpedo", "Phaser Beam Array"]))
        );
    }

    /// A row as the filters see it: what it is called, what it dealt, and what
    /// it was worth in each combat.
    fn filter_row(name: &str, types: &[&str], shares: &[Option<f64>]) -> CompareNode {
        let mut node = node(name, vec![Some(0.0)], Vec::new());
        node.damage_types = types.iter().map(|t| t.to_string()).collect();
        node.shares = shares.to_vec();
        node.dps = shares.iter().map(|s| s.map(|s| s * 1_000.0)).collect();
        node
    }

    fn filters<'a>(
        excluded_names: &'a mut FxHashSet<String>,
        types: &'a FxHashSet<String>,
    ) -> Ticks<'a> {
        Ticks {
            excluded: excluded_names,
            hide_unticked: false,
            types,
            differences: None,
            changed: false,
        }
    }

    /// The toggle in the Name header hides the rows that are out, and nothing
    /// else: Total stays, the ticked rows stay, and a sub-row is never hidden
    /// on account of a name that matches its own branch's.
    #[test]
    fn hiding_takes_out_the_unticked_rows_only() {
        let mut names = excluded(&["Photon Torpedo"]);
        let no_types = FxHashSet::default();
        let mut ticks = filters(&mut names, &no_types);
        ticks.hide_unticked = true;

        assert!(is_hidden(
            0,
            &ticks,
            &filter_row("Photon Torpedo", &[], &[])
        ));
        assert!(!is_hidden(
            0,
            &ticks,
            &filter_row("Phaser Beam Array", &[], &[])
        ));
        assert!(
            !is_hidden(1, &ticks, &filter_row("Photon Torpedo", &[], &[])),
            "a row deeper in the tree is drawn with the branch it belongs to"
        );
    }

    /// With the toggle off the rows stay on screen, only out of the Total.
    #[test]
    fn the_unticked_rows_stay_on_screen_until_the_toggle_is_on() {
        let mut names = excluded(&["Photon Torpedo"]);
        let no_types = FxHashSet::default();
        let ticks = filters(&mut names, &no_types);
        assert!(!is_hidden(
            0,
            &ticks,
            &filter_row("Photon Torpedo", &[], &[])
        ));
    }

    /// The type picker keeps the rows that dealt a picked type. A row of
    /// several types — a group holding more than one weapon — is kept for any
    /// one of them, since it is partly that type.
    #[test]
    fn the_type_picker_keeps_the_rows_of_that_type() {
        let mut names = FxHashSet::default();
        let picked: FxHashSet<String> = ["Phaser".to_string()].into_iter().collect();
        let ticks = filters(&mut names, &picked);

        assert!(!is_hidden(
            0,
            &ticks,
            &filter_row("Beams", &["Phaser"], &[])
        ));
        assert!(!is_hidden(
            0,
            &ticks,
            &filter_row("Rainbow", &["Phaser", "Polaron"], &[])
        ));
        assert!(is_hidden(
            0,
            &ticks,
            &filter_row("Torpedo", &["Kinetic"], &[])
        ));
        // A row the log gave no type for is not claimed by any of them.
        assert!(is_hidden(0, &ticks, &filter_row("Placate", &[], &[])));
    }

    /// An untouched picker is showing every type, not none of them.
    #[test]
    fn an_empty_type_picker_hides_nothing() {
        let mut names = FxHashSet::default();
        let none = FxHashSet::default();
        let ticks = filters(&mut names, &none);
        assert!(!is_hidden(
            0,
            &ticks,
            &filter_row("Torpedo", &["Kinetic"], &[])
        ));
    }

    /// A difference is the largest combat less the smallest, and a combat
    /// without the row counts as zero — a row flown in two runs out of five is
    /// a difference of its whole size, which is the case this is looking for.
    #[test]
    fn a_difference_is_the_spread_across_the_combats() {
        assert_eq!(0.0, spread(&[]));
        assert_eq!(0.0, spread(&[Some(5.0), Some(5.0)]));
        assert_eq!(3.0, spread(&[Some(5.0), Some(2.0), Some(4.0)]));
        assert_eq!(
            22.0,
            spread(&[None, None, Some(22.0)]),
            "absent from a combat is a difference, not missing data"
        );
    }

    /// The rows the combats agree on go; the ones they differ over stay. The
    /// two measures are read off the row separately, so a threshold in one is
    /// never applied to the other.
    #[test]
    fn the_differences_toggle_hides_what_the_combats_agree_on() {
        let mut names = FxHashSet::default();
        let no_types = FxHashSet::default();
        let agreed = filter_row("Broadside", &["Phaser"], &[Some(15.0), Some(16.0)]);
        let differs = filter_row("Ba'ul", &["AntiProton"], &[Some(3.9), Some(27.4)]);

        let mut ticks = filters(&mut names, &no_types);
        ticks.differences = Some((DifferenceMeasure::Share, 3.0));
        assert!(is_hidden(0, &ticks, &agreed), "one point apart");
        assert!(!is_hidden(0, &ticks, &differs));

        // The same rows in DPS, where the numbers are a thousand times larger.
        ticks.differences = Some((DifferenceMeasure::Dps, 3.0));
        assert!(
            !is_hidden(0, &ticks, &agreed),
            "a thousand DPS apart is over a threshold of three"
        );
        ticks.differences = Some((DifferenceMeasure::Dps, 5_000.0));
        assert!(is_hidden(0, &ticks, &agreed));
        assert!(!is_hidden(0, &ticks, &differs));
    }

    /// How many combats a row appears in at all, which is what tells "flown
    /// once" from "flown everywhere, unevenly".
    #[test]
    fn a_row_says_how_many_combats_it_appears_in() {
        let row = filter_row("Ba'ul", &[], &[Some(3.0), None, Some(27.0), None]);
        assert_eq!(2, row.present_in());
    }

    fn charted(node: &CompareNode) -> PreparedDamageDataSet {
        average_series(node).expect("the row has a series to average")
    }

    /// The averaged line is the mean of the combats' lines: their hits pooled
    /// onto one axis with every value divided by how many combats there were.
    /// The charts sum the values, so a pooled series scaled that way is the
    /// mean at every point rather than three runs stacked on top of each other.
    #[test]
    fn the_charted_average_scales_the_pooled_hits_down() {
        let mut node = node("Total", vec![Some(0.0)], Vec::new());
        node.series = vec![
            Some(SeriesData {
                hits: vec![hit(100.0, 0), hit(100.0, 1000)],
                total: 200.0,
                combat_duration_s: 10.0,
            }),
            Some(SeriesData {
                hits: vec![hit(300.0, 0)],
                total: 300.0,
                combat_duration_s: 20.0,
            }),
        ];

        let data = charted(&node);
        assert_eq!(250.0, data.total_value, "the mean of 200 and 300");
        assert_eq!(20.0, data.duration_s, "the window covers the longer run");
        let damage: f64 = data
            .values
            .iter()
            .map(|point| point.value.damage)
            .sum::<f64>();
        assert_eq!(250.0, damage, "500 of pooled damage over two combats");
        let hits: f64 = data.values.iter().map(|point| point.value.hits_count).sum();
        assert_eq!(1.5, hits, "three pooled hits over two combats");
    }

    /// A combat that never used the ability is left out of the average rather
    /// than charted as a flat zero, which would drag the line down for a reason
    /// that never happened — the same rule the table's averages follow.
    #[test]
    fn a_combat_without_the_ability_is_not_charted_as_zero() {
        let mut node = node("Kemocite", vec![Some(0.0)], Vec::new());
        node.series = vec![
            Some(SeriesData {
                hits: vec![hit(100.0, 0)],
                total: 100.0,
                combat_duration_s: 10.0,
            }),
            None,
        ];

        let data = charted(&node);
        assert_eq!(100.0, data.total_value, "one combat, not halved");
        assert_eq!("average of 1 combat", data.name);
    }

    /// Nothing to average is no line at all, not an empty one.
    #[test]
    fn a_row_no_combat_has_is_not_charted() {
        let mut node = node("Nothing", vec![None], Vec::new());
        node.series = vec![None, None];
        assert!(average_series(&node).is_none());
    }

    fn node(name: &str, values: Vec<Option<f64>>, sub_nodes: Vec<CompareNode>) -> CompareNode {
        CompareNode {
            name: name.to_string(),
            id: 0,
            cells: vec![Some(SlotCell {
                metrics: values
                    .iter()
                    .map(|value| MetricCell {
                        text: String::new(),
                        value: *value,
                        delta: None,
                    })
                    .collect(),
                breakdown: None,
            })],
            averages: values
                .iter()
                .map(|value| {
                    value.map(|value| AverageCell {
                        value,
                        text: String::new(),
                        count: 1,
                        min: value,
                        max: value,
                    })
                })
                .collect(),
            series: vec![None],
            shares: Vec::new(),
            dps: Vec::new(),
            damage_types: Vec::new(),
            sort_key: 0.0,
            sub_nodes,
            open: false,
        }
    }

    /// The export takes the whole tree in the order it is drawn, collapsed rows
    /// included — the file is the comparison, not the part that happened to be
    /// unfolded when the button was pressed.
    #[test]
    fn the_export_takes_every_row_of_the_tree() {
        let nodes = vec![node(
            "Total",
            vec![Some(85231.0)],
            vec![node(
                "Phaser Beam",
                vec![Some(21004.0)],
                vec![node("Overload", vec![None], Vec::new())],
            )],
        )];
        let mut rows = Vec::new();
        collect_export_rows(&nodes, 0, 1, false, &mut rows);

        let shape: Vec<(&str, usize)> = rows.iter().map(|r| (r.name.as_str(), r.level)).collect();
        assert_eq!(
            vec![("Total", 0), ("Phaser Beam", 1), ("Overload", 2)],
            shape
        );
        assert_eq!(vec![Some(85231.0)], rows[0].values);
        // A row a combat has no value for leaves an empty cell rather than a
        // zero, which would average and chart as a real number in the sheet.
        assert_eq!(vec![None], rows[2].values);
    }

    /// In averages mode the file holds the same one column per metric that the
    /// table does, not one per combat.
    #[test]
    fn the_export_follows_the_averages_toggle() {
        let nodes = vec![node("Total", vec![Some(100.0), Some(40.0)], Vec::new())];

        let mut per_combat = Vec::new();
        collect_export_rows(&nodes, 0, 1, false, &mut per_combat);
        assert_eq!(2, per_combat[0].values.len());

        let mut averaged = Vec::new();
        collect_export_rows(&nodes, 0, 1, true, &mut averaged);
        assert_eq!(vec![Some(100.0), Some(40.0)], averaged[0].values);
    }

    #[test]
    fn delta_direction_for_higher_is_better() {
        let mut f = NumberFormatter::new();
        let up = make_delta(Some(100.0), Some(120.0), CompareMetric::Dps, &mut f).unwrap();
        assert!(up.improvement);
        assert_eq!(up.text, "+20");

        let down = make_delta(Some(120.0), Some(100.0), CompareMetric::Dps, &mut f).unwrap();
        assert!(!down.improvement);
        assert_eq!(down.text, "-20");
    }

    #[test]
    fn delta_direction_for_resistance_is_inverted() {
        // Lower resistance faced is better, so a drop is an improvement (green).
        let mut f = NumberFormatter::new();
        let lower = make_delta(Some(50.0), Some(40.0), CompareMetric::Resistance, &mut f).unwrap();
        assert!(lower.improvement);
        let higher = make_delta(Some(40.0), Some(50.0), CompareMetric::Resistance, &mut f).unwrap();
        assert!(!higher.improvement);
    }

    #[test]
    fn equal_or_missing_current_has_no_delta() {
        let mut f = NumberFormatter::new();
        assert!(make_delta(Some(100.0), Some(100.0), CompareMetric::Dps, &mut f).is_none());
        assert!(make_delta(Some(100.0), None, CompareMetric::Dps, &mut f).is_none());
    }

    #[test]
    fn missing_base_treats_baseline_as_zero() {
        // The ability is absent from the reference combat, so its whole value
        // shows as a colored +/- delta against a zero baseline.
        let mut f = NumberFormatter::new();
        let new_dps = make_delta(None, Some(100.0), CompareMetric::Dps, &mut f).unwrap();
        assert!(new_dps.improvement);
        assert_eq!(new_dps.text, "+100");

        // For resistance, lower is better, so a value appearing is worse (red).
        let new_res = make_delta(None, Some(30.0), CompareMetric::Resistance, &mut f).unwrap();
        assert!(!new_res.improvement);
    }
}
