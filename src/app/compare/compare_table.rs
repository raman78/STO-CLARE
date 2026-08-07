//! Builds and renders the side-by-side comparison of the outgoing damage ability
//! tree of a chosen player across up to a few combats, plus a chart (reusing the
//! main window's diagrams) of the selected ability branch across those combats.
//!
//! The trees are aligned by ability name (name handles differ per combat, so we
//! key on the resolved name string). Rows are sorted by the first (reference)
//! combat's DPS, and every value in combats 2+ carries a colored +/- delta
//! against the reference.

use std::sync::Arc;

use eframe::egui::{text::LayoutJob, *};
use rustc_hash::FxHashMap;

use crate::{
    analyzer::{AnalysisGroup, Combat, DamageGroup, Hit, HitsManager, NameHandle, NameManager},
    app::main_tabs::diagrams::{
        DamageDiagrams, DiagramType, PreparedDamageDataSet, combat_duration_seconds,
    },
    app::main_tabs::tables::show_group_separator,
    app::settings::{CombatNotes, Settings},
    app::theme,
    custom_widgets::{slider_text_edit::SliderTextEdit, splitter::Splitter, table::*},
    helpers::number_formatting::NumberFormatter,
};

use super::CompareMetric;

const ROW_HEIGHT: f32 = 25.0;
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
    filter: f64,
    time_slice: f64,
}

struct CompareNode {
    name: String,
    id: u32,
    /// One entry per slot; `None` when that combat's player has no such node.
    cells: Vec<Option<SlotCell>>,
    /// Per-slot hit series for charting (`None` when the slot lacks this node).
    series: Vec<Option<SeriesData>>,
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
    /// Delta versus the reference combat (only for slots after the first).
    delta: Option<DeltaCell>,
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
            filter: 0.4,
            time_slice: 1.0,
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
        let cells = build_cells(&parents, &self.columns);
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
            series,
            sort_key,
            sub_nodes,
            open: true,
        }];

        // Chart the overall total by default so a chart shows immediately.
        self.selected = Some(root_id);
        self.rebuild_diagram();
    }

    /// (Re)build the chart for the currently selected ability node: one line per
    /// combat over that branch's hits.
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
        self.diagrams = find_node(&self.nodes, id).map(|node| {
            let data = (0..n_slots).filter_map(|slot_i| {
                let series = node.series.get(slot_i)?.as_ref()?;
                Some(PreparedDamageDataSet::new(
                    &chart_label(slot_i, note_of(notes, slot_i)),
                    series.total,
                    series.hits.iter(),
                    series.combat_duration_s,
                ))
            });
            DamageDiagrams::from_data(data, filter, time_slice)
        });
    }

    pub fn show(&mut self, ui: &mut Ui, settings: &mut Settings) {
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

        self.show_column_picker(ui, settings);

        // Pick up column changes from the picker (or an external settings edit).
        if self.columns != settings.compare.columns {
            self.columns = settings.compare.columns.clone();
            self.rebuild();
        }

        // Legend + per-combat player picker.
        let colors = self.slot_colors();
        let mut player_change: Option<(usize, NameHandle)> = None;
        for (slot_i, slot) in self.slots.iter().enumerate() {
            ui.horizontal(|ui| {
                // The user's own note, where they wrote one, tells apart runs
                // the identifier alone cannot.
                let note = note_of(&self.notes, slot_i);
                // The number and the note are drawn in the colour this combat
                // has on the chart, so the legend, the table and the chart all
                // say the same thing about which run is which.
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
                            "(reference — every difference is measured against this combat, and \
                             changing the player here moves the others to the same player)",
                        )
                        .weak(),
                    );
                }
            });
        }
        if let Some((slot_i, handle)) = player_change {
            self.slots[slot_i].player = handle;
            // Changing who the reference is about moves the others with it,
            // where that player took part: the deltas are all measured against
            // the reference, so leaving them on somebody else would compare two
            // different people again.
            if slot_i == 0 {
                follow_the_reference_player(&mut self.slots);
            }
            self.rebuild();
        }

        ui.label(
            RichText::new(
                "Each column group holds one metric, one column per combat. The small coloured \
                 number beside a value is its difference against combat #1 — green when it moved \
                 the better way.",
            )
            .weak(),
        );

        // Comparing two different people's numbers looks like a build changed
        // when nothing did, so say so rather than let it pass unnoticed.
        let names = slot_player_names(&self.slots);
        if names.iter().any(|n| n != &names[0]) {
            ui.label(
                RichText::new(format!(
                    "⚠ The combats are showing different players ({}). The differences compare \
                     those players against each other, not one player's runs — pick the same \
                     player above to compare like with like.",
                    names.join(", ")
                ))
                .color(theme::palette().worse),
            );
        }

        ui.separator();

        Splitter::horizontal()
            .initial_ratio(0.6)
            .ratio_bounds(0.15..=0.9)
            .show(ui, |top_ui, bottom_ui| {
                self.show_table(top_ui, settings.compare.show_dps_breakdown, &colors);
                self.show_diagram(bottom_ui, settings);
            });
    }

    /// The colour each slot's combat is drawn in on the chart, `None` for a
    /// slot the chart has no line for — nothing is charted yet, or the selected
    /// ability is absent from that combat.
    ///
    /// Asked of the chart rather than worked out here: the colours follow the
    /// order the series sorted into (by total, largest first), so they depend
    /// on the numbers and change with the ability picked. Reading them off the
    /// chart is what keeps the table and the legend from drifting out of step
    /// with it.
    fn slot_colors(&self) -> Vec<Option<Color32>> {
        (0..self.slots.len())
            .map(|slot_i| {
                let diagrams = self.diagrams.as_ref()?;
                diagrams.series_color(&chart_label(slot_i, note_of(&self.notes, slot_i)))
            })
            .collect()
    }

    fn show_column_picker(&self, ui: &mut Ui, settings: &mut Settings) {
        ui.menu_button("Columns ▾", |ui| {
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

    fn show_table(&mut self, ui: &mut Ui, show_breakdown: bool, colors: &[Option<Color32>]) {
        let n_slots = self.slots.len();
        let n_metrics = self.columns.len();
        // The note line is only there when some combat carries a note, so a
        // comparison of runs nobody named keeps the two-line header it had.
        let with_notes = self.notes.iter().any(|note| !note.is_empty());
        let font = TextStyle::Body.resolve(ui.style());
        let text_color = ui.visuals().text_color();
        // Columns are grouped by metric: the metric name spans its group (shown
        // on the first column), with the combat number below each column.
        // A rule opens each group: without one, three combats' worth of the
        // same-looking numbers run into the next metric.
        let mut headers: Vec<HeaderCell> = Vec::new();
        for column in self.columns.iter() {
            headers.push(HeaderCell::Separator);
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
        {
            let nodes = &mut self.nodes;
            ScrollArea::horizontal().show(ui, |ui| {
                Table::new(ui)
                    .cell_spacing(10.0)
                    .header(header_height(with_notes), |r| {
                        r.cell(|ui| {
                            ui.label("Name");
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
                                0.0,
                                n_slots,
                                n_metrics,
                                show_breakdown,
                                &mut selected,
                                &mut selection_changed,
                            );
                        }
                    });
            });
        }

        self.selected = selected;
        if selection_changed {
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
                ui.selectable_value(&mut self.active_diagram, diagram, diagram.name())
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
        indent: f32,
        n_slots: usize,
        n_metrics: usize,
        show_breakdown: bool,
        selected: &mut Option<u32>,
        selection_changed: &mut bool,
    ) {
        let is_selected = *selected == Some(self.id);
        let response = t.selectable_row(is_selected, |r| {
            r.cell(|ui| {
                ui.horizontal(|ui| {
                    ui.add_space(indent * 20.0);
                    let symbol = if self.open { "⏷" } else { "⏵" };
                    let can_open = !self.sub_nodes.is_empty();
                    if ui
                        .add_visible(can_open, Button::selectable(false, symbol))
                        .clicked()
                    {
                        self.open = !self.open;
                    }
                    ui.label(&self.name);
                });
            });

            // Column groups by metric: for each metric, one cell per combat.
            for metric_i in 0..n_metrics {
                show_group_separator(r);
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

        if response.clicked() {
            *selected = Some(self.id);
            *selection_changed = true;
        }

        if self.open {
            for sub in self.sub_nodes.iter_mut() {
                sub.show(
                    t,
                    indent + 1.0,
                    n_slots,
                    n_metrics,
                    show_breakdown,
                    selected,
                    selection_changed,
                );
            }
        }
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
            let cells = build_cells(per_slot, columns);
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
                series,
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

fn build_cells(
    per_slot: &[Option<&DamageGroup>],
    columns: &[CompareMetric],
) -> Vec<Option<SlotCell>> {
    let mut formatter = NumberFormatter::new();

    // Raw metric values per slot, so combats 2+ can be compared to slot 0.
    let raw: Vec<Option<Vec<Option<f64>>>> = per_slot
        .iter()
        .map(|g| g.map(|g| columns.iter().map(|c| c.extract(g)).collect::<Vec<_>>()))
        .collect();
    let base = raw.first().and_then(|o| o.as_ref());

    raw.iter()
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
                        MetricCell { text, delta }
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
        header_text(
            &FontId::default(),
            Color32::WHITE,
            "DPS",
            "#2",
            note,
            color,
        )
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
        assert!(job.sections.iter().all(|s| s.format.color == Color32::WHITE));
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
