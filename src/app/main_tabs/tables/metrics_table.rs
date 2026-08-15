use std::borrow::Cow;
use std::cmp::Reverse;

use educe::Educe;
use eframe::egui::*;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::custom_widgets::popup_button::PopupButton;
use crate::custom_widgets::table::SortState;
use crate::custom_widgets::tooltip::CloseTooltip;
use crate::{
    analyzer::*,
    app::{main_tabs::common::*, settings::Settings},
    custom_widgets::table::*,
    helpers::{F64TotalOrd, number_formatting::NumberFormatter},
};

/// The size the open/close arrow of a tree row is drawn at.
///
/// Pinned rather than left to the text in it: the arrow is frameless while
/// resting and framed under the pointer, and egui sizes those two differently
/// under this app's themes (see `custom_widgets::toggle`), so without a fixed
/// size pointing at an arrow nudged the name beside it.
const ARROW_SIZE: Vec2 = vec2(22.0, 18.0);

#[macro_export]
macro_rules! col {
    ($name:expr, $sort:expr, $show:expr $(,)?) => {
        ColumnDescriptor {
            name: $name,
            name_info: None,
            sort: $sort,
            show: $show,
            parts: &[],
        }
    };

    ($name:expr, $name_info:expr, $sort:expr, $show:expr $(,)?) => {
        ColumnDescriptor {
            name: $name,
            name_info: Some($name_info),
            sort: $sort,
            show: $show,
            parts: &[],
        }
    };
}

/// A column whose value splits into a shield and a hull half, e.g. `Total
/// Damage` or `Hits`. Renders as a single "all" column with the halves in a
/// tooltip, or — when the split-columns setting is on — as `all | Hull |
/// Shield` under one header. `$field` must be a `ShieldAndHullTextValue` or
/// `ShieldAndHullTextCount` on the row data, which must also carry a
/// `halves_in_tooltip` flag (the row data is built per settings, so the flag
/// rides along with the formatting).
#[macro_export]
macro_rules! shield_hull_col {
    ($name:expr, $sort:expr, $field:ident $(,)?) => {
        ColumnDescriptor {
            name: $name,
            name_info: None,
            sort: $sort,
            show: |t, r| t.$field.show(r, t.halves_in_tooltip),
            parts: $crate::shield_hull_parts!($field),
        }
    };

    ($name:expr, $name_info:expr, $sort:expr, $field:ident $(,)?) => {
        ColumnDescriptor {
            name: $name,
            name_info: Some($name_info),
            sort: $sort,
            show: |t, r| t.$field.show(r, t.halves_in_tooltip),
            parts: $crate::shield_hull_parts!($field),
        }
    };
}

#[macro_export]
macro_rules! shield_hull_parts {
    ($field:ident) => {
        &[
            ColumnPart {
                name: "Hull",
                // Each half orders by its own figure. Ordering all three by the
                // total, as they used to, made two of the three headings a lie.
                sort: |t| {
                    t.sort_by_option_f64_desc(|p| {
                        $crate::app::main_tabs::common::OrderingValue::ordering_value(
                            &p.$field.hull,
                        )
                    })
                },
                show: |t, r| t.$field.show_hull(r),
            },
            ColumnPart {
                name: "Shield",
                sort: |t| {
                    t.sort_by_option_f64_desc(|p| {
                        $crate::app::main_tabs::common::OrderingValue::ordering_value(
                            &p.$field.shield,
                        )
                    })
                },
                show: |t, r| t.$field.show_shield(r),
            },
        ]
    };
}

pub struct MetricsTable<T: 'static> {
    columns: &'static [ColumnDescriptor<T>],
    /// Whether the Hull/Shield halves get their own columns (setting
    /// `general.split_shield_hull_columns`). Baked in when the table is built,
    /// like the other formatting settings.
    split_shield_hull: bool,
    players: Vec<MetricsTablePart<T>>,
    selection: SelectionTracker,
    /// Which column the rows are ordered by, and which way round.
    sort: SortState<ColumnKey>,
}

#[derive(Educe)]
#[educe(Deref, DerefMut)]
pub struct MetricsTablePart<T> {
    #[educe(Deref, DerefMut)]
    pub data: T,
    pub name: String,
    /// The group this row is of. Two rows can be called the same thing — a
    /// group named after one of the abilities it collects, say — so anything
    /// that has to pick one row out of a table goes by this rather than by the
    /// name.
    handle: NameHandle,
    id: u32,

    pub sub_parts: Vec<Self>,

    open: bool,
}

#[derive(Clone, Copy)]
pub struct ColumnDescriptor<T: 'static> {
    pub name: &'static str,
    pub name_info: Option<&'static str>,
    pub sort: fn(&mut MetricsTable<T>),
    pub show: fn(&mut MetricsTablePart<T>, &mut TableRow),
    /// Extra cells appended after `show` when the split-columns setting is on
    /// (the Hull and Shield halves). Empty for columns that have no such split.
    pub parts: &'static [ColumnPart<T>],
}

/// What tells one sortable heading from another: the metric, and which half of
/// it when the halves have columns of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColumnKey {
    column: &'static str,
    part: Option<&'static str>,
}

impl ColumnKey {
    pub const fn whole(column: &'static str) -> Self {
        Self { column, part: None }
    }

    pub const fn half(column: &'static str, part: &'static str) -> Self {
        Self {
            column,
            part: Some(part),
        }
    }

    /// Which half of a split column this is, if it is one.
    pub const fn part(&self) -> Option<&'static str> {
        self.part
    }

    /// The metric this heading orders by.
    pub const fn column(&self) -> &'static str {
        self.column
    }
}

/// The heading of the name column, which is not one of the metric columns but
/// can order the rows all the same.
const NAME_COLUMN: ColumnKey = ColumnKey::whole("Name");

/// One half of a split column.
#[derive(Clone, Copy)]
pub struct ColumnPart<T: 'static> {
    pub name: &'static str,
    pub sort: fn(&mut MetricsTable<T>),
    pub show: fn(&mut MetricsTablePart<T>, &mut TableRow),
}

impl<T: 'static> MetricsTable<T> {
    pub fn empty_base(columns: &'static [ColumnDescriptor<T>]) -> Self {
        Self {
            players: Vec::new(),
            selection: Default::default(),
            columns,
            split_shield_hull: false,
            sort: Default::default(),
        }
    }

    pub fn new_base<G: AnalysisGroup>(
        settings: &Settings,
        columns: &'static [ColumnDescriptor<T>],
        combat: &Combat,
        // A group by value rather than by reference: a table whose rows can be
        // ticked off shows a group that is not in the analyzer's tree — the
        // player's damage with the unticked rows taken out, worked out from the
        // hits (`app::damage_subset`). `Cow` keeps the ordinary case free.
        mut group: impl FnMut(&Player) -> Cow<'_, G>,
        data_new: fn(&Settings, &G, &Combat, &mut NumberFormatter) -> T,
    ) -> Self {
        let mut number_formatter = NumberFormatter::new();
        let mut id_source = 0;
        let mut table = Self {
            columns,
            split_shield_hull: settings.general.split_shield_hull_columns,
            players: combat
                .players
                .values()
                .map(|p| {
                    MetricsTablePart::new(
                        settings,
                        group(p).as_ref(),
                        combat,
                        &mut number_formatter,
                        &mut id_source,
                        data_new,
                    )
                })
                .collect(),
            selection: Default::default(),
            sort: Default::default(),
        };
        // The first column is what a table opens ordered by, as it always has
        // been. The state has to say so too, or the heading it is ordered by
        // carries no mark until somebody clicks something.
        table.sort.column = Some(ColumnKey::whole(table.columns[0].name));
        let first = table.columns[0].sort;
        table.sort_by_column(first);

        table
    }

    /// `shown` decides which columns are drawn, and is asked every frame rather
    /// than baked in when the table is built, so the picker takes effect at
    /// once instead of at the next refresh.
    /// Draws the table, with a tick column in front of the names when `ticks`
    /// is given: the rows under a player can then be taken out of that player's
    /// own figures, the way the ticks in a comparison decide its Total.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        shown: impl Fn(&str) -> bool,
        ticks: &mut RowTicks,
        mut on_selected: impl FnMut(TableSelectionEvent<T>),
    ) {
        let modifiers = ui.input(|i| i.modifiers);
        let split = self.split_shield_hull;
        // Split columns need a second header line for the All/Hull/Shield labels.
        let header_height = if split {
            SPLIT_HEADER_HEIGHT
        } else {
            HEADER_HEIGHT
        };
        // The visible ones, gathered once so the header and every row walk the
        // same list in step.
        let columns: Vec<&ColumnDescriptor<T>> = self
            .columns
            .iter()
            .filter(|column| shown(column.name))
            .collect();
        // The table scrolls both ways by itself, so its bars stay at the edges
        // of the view; the header is drawn last, level with the columns.
        Table::new(ui)
            .cell_spacing(10.0)
            .header(header_height)
            .body(ROW_HEIGHT, |t| {
                for player in self.players.iter_mut() {
                    let handle = player.handle;
                    player.show(
                        &columns,
                        t,
                        0.0,
                        &mut self.selection,
                        &mut on_selected,
                        modifiers,
                        split,
                        ticks,
                        handle,
                    );
                }
            })
            .header_row(|r| {
                // The tick column's own header stays empty; the eye that hides
                // the unticked rows sits beside the name, as it does in a
                // comparison.
                r.cell(|_| {});
                r.cell(|ui| {
                    // The name orders the rows too — by name, which is how an
                    // ability is looked for when it is known what it is called
                    // and not what it is worth. The eye and the type picker sit
                    // on the heading and keep their own clicks.
                    if show_sortable_header_cell(
                        ui,
                        self.sort.is_sorted_by(NAME_COLUMN),
                        self.sort.marker(NAME_COLUMN),
                        "Name",
                        |ui| {
                            ticks.show_eye(ui);
                            ticks.show_types(ui);
                        },
                    )
                    .clicked()
                    {
                        self.sort.clicked(NAME_COLUMN);
                        self.sort_by_column(|table| table.sort_by_name());
                    }
                });

                for (index, column) in columns.iter().enumerate() {
                    self.show_column_header(r, column, split);
                    if closes_group(&columns, index, split) {
                        show_group_separator(r);
                    }
                }
            });
    }

    fn show_column_header(
        &mut self,
        row: &mut TableRow,
        column: &ColumnDescriptor<T>,
        split: bool,
    ) {
        // Unsplit: one cell holding the metric name. Split: a rule opens the
        // group, the metric name sits above its first cell, and All/Hull/Shield
        // label the second line. Without the rule, three same-looking numbers
        // from neighbouring metrics run together.
        //
        // Each cell of the group orders by its own figure — the total, the hull
        // or the shield — which is why the halves are named for the sort state
        // as `Metric Hull` rather than sharing the metric's name. Ordering all
        // three by the total made two of the three headings a lie.
        if split && !column.parts.is_empty() {
            show_group_separator(row);
            let name = column.name;
            self.show_split_header(
                row,
                ColumnKey::whole(name),
                column.name_info,
                column.sort,
                true,
                |ui| {
                    ui.label(RichText::new(name).color(ui.visuals().text_color()));
                },
            );
            for part in column.parts.iter() {
                // A half has no name of its own on the first line, but it still
                // needs the room: without it the label would sit a line higher
                // than the one beside it.
                self.show_split_header(
                    row,
                    ColumnKey::half(name, part.name),
                    None,
                    part.sort,
                    true,
                    |ui| {
                        let line = ui.text_style_height(&TextStyle::Body);
                        ui.add_space(line);
                    },
                );
            }
            return;
        }

        // One heading, one control — the same one a split group's halves get,
        // so every heading in the table looks and behaves alike.
        self.show_split_header(
            row,
            ColumnKey::whole(column.name),
            column.name_info,
            column.sort,
            false,
            |ui| {
                // A metric with no halves still sits in a header built for two
                // lines when the split columns are on, so it takes the first
                // line's room and leaves its heading level with the others.
                if split {
                    let line = ui.text_style_height(&TextStyle::Body);
                    ui.add_space(line);
                }
            },
        );
    }

    /// One heading of a split group: the metric name (or nothing, over a half)
    /// on the first line, and under it the label that does the ordering.
    ///
    /// Only that label takes the click and lights up under the pointer. It used
    /// to be one cell holding both lines, so pointing anywhere in the heading
    /// lit the whole two-line block and said nothing about which of All, Hull
    /// or Shield was about to be ordered by.
    fn show_split_header(
        &mut self,
        row: &mut TableRow,
        key: ColumnKey,
        info: Option<&'static str>,
        sort: fn(&mut Self),
        split_group: bool,
        first_line: impl FnOnce(&mut Ui),
    ) {
        let response = show_sortable_header(row, &self.sort, key, info, split_group, first_line);
        if response.clicked() {
            self.sort.clicked(key);
            self.sort_by_column(sort);
        }
    }

    /// Order the rows by `column`, the way `SortState` says.
    ///
    /// A column knows one order — largest first for most of them, smallest for
    /// the few where small is the good end — so the other way round is that one
    /// reversed rather than a second sort with its own idea of what is best.
    fn sort_by_column(&mut self, sort: fn(&mut Self)) {
        sort(self);
        if !self.sort.natural {
            self.reverse_order();
        }
    }

    /// Turn the order round, at every level of the tree.
    fn reverse_order(&mut self) {
        self.players.reverse();
        self.players.iter_mut().for_each(|p| p.reverse_order());
    }

    pub fn sort_by_option_f64_desc(
        &mut self,
        mut key: impl FnMut(&MetricsTablePart<T>) -> Option<f64> + Copy,
    ) {
        self.sort_by_desc(move |p| key(p).map(F64TotalOrd));
    }

    pub fn sort_by_option_f64_asc(
        &mut self,
        mut key: impl FnMut(&MetricsTablePart<T>) -> Option<f64> + Copy,
    ) {
        self.sort_by_asc(move |p| key(p).map(F64TotalOrd));
    }

    /// Carries over which rows were open, and what the rows are ordered by,
    /// from the table this one replaces.
    ///
    /// A table is rebuilt whenever a tick or a damage type moves, and a rebuilt
    /// row is closed and unsorted by default — so the tree a reader had opened
    /// folded itself up under them every time they ticked something. Matched by
    /// name, which is what a reader recognises a row by; a row that was not
    /// there before simply starts closed.
    pub fn take_state_from(&mut self, previous: &Self) {
        take_open_state(&mut self.players, &previous.players);
        // The table being replaced may be the empty one a tab starts life with,
        // which has picked no column. Taking its state would leave the fresh
        // table ordered by its first column with no heading saying so.
        if previous.sort.column.is_none() {
            return;
        }
        self.sort = previous.sort;
        // The name column is not one of the metric columns, so it is not found
        // among them; it orders the rows all the same.
        if self.sort.is_sorted_by(NAME_COLUMN) {
            self.sort_by_column(|table| table.sort_by_name());
            return;
        }
        let column = self
            .sort
            .column
            .and_then(|key| self.columns.iter().find(|c| c.name == key.column))
            .map(|column| (column.sort, column.parts, self.sort.column));
        if let Some((whole, parts, key)) = column {
            let sort = key
                .and_then(|key| key.part)
                .and_then(|part| parts.iter().find(|p| p.name == part))
                .map(|part| part.sort)
                .unwrap_or(whole);
            self.sort_by_column(sort);
        }
    }

    /// By name, A to Z, at every level of the tree. Case is ignored: a reader
    /// looking for `Gravity Well` does not think of it as filed apart from
    /// `gravity well`.
    pub fn sort_by_name(&mut self) {
        self.sort_by_asc(|part| part.name.to_lowercase());
    }

    pub fn sort_by_desc<K: Ord>(&mut self, mut key: impl FnMut(&MetricsTablePart<T>) -> K + Copy) {
        self.players.sort_unstable_by_key(|p| Reverse(key(p)));

        self.players.iter_mut().for_each(|p| p.sort_by_desc(key));
    }

    pub fn sort_by_asc<K: Ord>(&mut self, key: impl FnMut(&MetricsTablePart<T>) -> K + Copy) {
        self.players.sort_unstable_by_key(key);

        self.players.iter_mut().for_each(|p| p.sort_by_asc(key));
    }
}

impl<T> MetricsTablePart<T> {
    fn new<G: AnalysisGroup>(
        settings: &Settings,
        source: &G,
        combat: &Combat,
        number_formatter: &mut NumberFormatter,
        id_source: &mut u32,
        data_new: fn(&Settings, &G, &Combat, &mut NumberFormatter) -> T,
    ) -> Self {
        let id = *id_source;
        *id_source += 1;
        let sub_parts = source
            .sub_groups()
            .values()
            .map(|s| {
                MetricsTablePart::new(settings, s, combat, number_formatter, id_source, data_new)
            })
            .collect();

        Self {
            data: data_new(settings, source, combat, number_formatter),
            name: source.name().get(&combat.name_manager).to_string(),
            handle: source.name(),
            id,
            sub_parts,
            open: false,
        }
    }

    // Drawing context threaded through; a struct of the same fields would
    // only move the list somewhere else.
    #[allow(clippy::too_many_arguments)]
    fn show(
        &mut self,
        columns: &[&ColumnDescriptor<T>],
        table: &mut TableBody,
        indent: f32,
        selection: &mut SelectionTracker,
        on_selected: &mut impl FnMut(TableSelectionEvent<T>),
        modifiers: Modifiers,
        split: bool,
        ticks: &mut RowTicks,
        // Whose rows these are: the ticks are per player.
        player: NameHandle,
    ) {
        let mut tick_rect = Rect::NOTHING;
        let response = table.selectable_row(selection.is_selected(self.id), |r| {
            tick_rect = ticks.show_cell(
                r,
                indent,
                player,
                self.handle,
                self.sub_parts.iter().map(|part| part.handle),
            );
            r.cell(|ui| {
                ui.horizontal(|ui| {
                    ui.add_space(indent * 30.0);
                    let symbol = if self.open { "⏷" } else { "⏵" };
                    let can_open = !self.sub_parts.is_empty();
                    if ui
                        .add_visible(
                            can_open,
                            Button::selectable(false, symbol).min_size(ARROW_SIZE),
                        )
                        .clicked()
                    {
                        self.open = !self.open;
                    }

                    ui.label(&self.name);
                });
            });

            for (index, column) in columns.iter().enumerate() {
                if split && !column.parts.is_empty() {
                    show_group_separator(r);
                }
                (column.show)(self, r);
                if split {
                    for part in column.parts.iter() {
                        (part.show)(self, r);
                    }
                }
                if closes_group(columns, index, split) {
                    show_group_separator(r);
                }
            }
        });

        // A click that landed in the tick column is about the tick, not about
        // charting the row.
        let clicked_the_tick = response
            .interact_pointer_pos()
            .is_some_and(|pos| tick_rect.contains(pos));
        if response.clicked() && !clicked_the_tick {
            if modifiers.contains(Modifiers::CTRL) {
                selection.select_or_unselect_single(self, on_selected);
            } else {
                selection.select_group(self, on_selected);
            }
        }

        response.context_menu(|ui| {
            if ui.button("copy name to clipboard").clicked() {
                ui.ctx().copy_text(self.name.clone());
                ui.close_kind(UiKind::Menu);
            }

            if ui.button("show diagrams for this").clicked() && !selection.is_selected(self.id) {
                selection.select_or_unselect_single(self, on_selected);
                ui.close_kind(UiKind::Menu);
            }
        });

        if self.open {
            for sub_part in self.sub_parts.iter_mut() {
                if ticks.is_hidden(indent, self.handle, sub_part.handle) {
                    continue;
                }
                sub_part.show(
                    columns,
                    table,
                    indent + 1.0,
                    selection,
                    on_selected,
                    modifiers,
                    split,
                    ticks,
                    self.handle,
                );
            }
        }
    }

    pub fn sort_by_desc<K: Ord>(&mut self, mut key: impl FnMut(&Self) -> K + Copy) {
        self.sub_parts.sort_unstable_by_key(|p| Reverse(key(p)));

        self.sub_parts.iter_mut().for_each(|p| p.sort_by_desc(key));
    }

    /// Turn this row's children round, and theirs, so a reversed order reaches
    /// the whole tree rather than only its top level.
    fn reverse_order(&mut self) {
        self.sub_parts.reverse();
        self.sub_parts.iter_mut().for_each(|p| p.reverse_order());
    }

    pub fn sort_by_asc<K: Ord>(&mut self, key: impl FnMut(&Self) -> K + Copy) {
        self.sub_parts.sort_unstable_by_key(key);

        self.sub_parts.iter_mut().for_each(|p| p.sort_by_asc(key));
    }
}

/// Whether a closing rule belongs after the column at `index`: it ends a split
/// group and what follows is not another one. Between two adjacent groups the
/// next group's opening rule already separates them, so only the last of a run
/// is closed.
pub fn closes_group<T>(columns: &[&ColumnDescriptor<T>], index: usize, split: bool) -> bool {
    if !split || columns[index].parts.is_empty() {
        return false;
    }
    columns
        .get(index + 1)
        .map(|next| next.parts.is_empty())
        .unwrap_or(true)
}

/// A narrow cell holding a vertical rule, drawn where a split column group
/// starts so the All/Hull/Shield triples do not read as one run of numbers.
/// Used in the header and in every body row, so the rule is continuous.
pub fn show_group_separator(row: &mut TableRow) {
    row.cell(|ui| {
        ui.add(Separator::default().vertical().spacing(0.0));
    });
}

#[derive(Default)]
enum SelectionTracker {
    #[default]
    None,
    Group(u32),
    Multi(FxHashSet<u32>),
}

pub enum TableSelectionEvent<'a, T> {
    Clear,
    Group(&'a MetricsTablePart<T>),
    Single(&'a MetricsTablePart<T>),
    AddSingle(&'a MetricsTablePart<T>),
    Unselect(&'a str),
}

impl SelectionTracker {
    fn is_selected(&self, id: u32) -> bool {
        match &self {
            Self::None => false,
            Self::Group(i) => *i == id,
            Self::Multi(g) => g.contains(&id),
        }
    }

    fn select_group<T>(
        &mut self,
        part: &MetricsTablePart<T>,
        on_selected: &mut impl FnMut(TableSelectionEvent<T>),
    ) {
        match self {
            SelectionTracker::Group(id) if *id == part.id => {
                *self = Self::None;
                on_selected(TableSelectionEvent::Clear);
            }
            _ => {
                *self = Self::Group(part.id);
                on_selected(TableSelectionEvent::Group(part));
            }
        }
    }

    fn select_or_unselect_single<T>(
        &mut self,
        part: &MetricsTablePart<T>,
        on_selected: &mut impl FnMut(TableSelectionEvent<T>),
    ) {
        match self {
            SelectionTracker::None | SelectionTracker::Group(_) => {
                let mut group: FxHashSet<_> = Default::default();
                group.insert(part.id);
                *self = Self::Multi(group);
                on_selected(TableSelectionEvent::Single(part));
            }
            SelectionTracker::Multi(group) => {
                if !group.contains(&part.id) {
                    group.insert(part.id);
                    on_selected(TableSelectionEvent::AddSingle(part));
                } else if group.len() > 1 {
                    group.remove(&part.id);
                    on_selected(TableSelectionEvent::Unselect(&part.name));
                } else {
                    *self = Self::None;
                    on_selected(TableSelectionEvent::Clear);
                }
            }
        }
    }
}

/// One heading: whatever the caller draws on the first line, and under it the
/// strip that does the ordering.
///
/// Only that strip takes the click and lights up under the pointer, and it is
/// drawn the way a pickable table cell is drawn rather than as a button —
/// filled while it is the one ordering the rows, rimmed under the pointer. It
/// used to be one cell holding both lines, so pointing anywhere in the heading
/// lit the whole two-line block and said nothing about which of All, Hull or
/// Shield was about to be ordered by.
///
/// Shared with `SummaryTable`, so a heading looks and behaves the same
/// wherever it is.
pub(super) fn show_sortable_header(
    row: &mut TableRow,
    sort: &SortState<ColumnKey>,
    key: ColumnKey,
    info: Option<&'static str>,
    split_group: bool,
    first_line: impl FnOnce(&mut Ui),
) -> Response {
    let marker = sort.marker(key);
    // A whole column is labelled with its metric; a split group labels its
    // three columns All, Hull and Shield under the metric name.
    let label = match (key.part, split_group) {
        (Some(part), _) => part,
        (None, true) => "All",
        (None, false) => key.column,
    };
    let picked = sort.is_sorted_by(key);
    let mut strip = None;
    row.cell(|ui| {
        // Headings are as narrow as their column and must not wrap: a "Shield"
        // folded into "Shie / ld" costs the line under it.
        // Headings are as narrow as their column and must not wrap: a "Shield"
        // folded into "Shie / ld" costs the line under it.
        ui.style_mut().wrap_mode = Some(TextWrapMode::Extend);

        // The whole cell, both lines of it, takes the click and lights up: a
        // column is one thing to point at, and half of it reacting while the
        // other half did not read as two.
        let line = ui.text_style_height(&TextStyle::Body);
        let width = heading_width(
            ui.available_width(),
            text_width(ui, label),
            sort_marker_width(ui),
            ui.spacing().item_spacing.x,
        );
        let height = ui.available_height();
        let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
        draw_cell_visuals(ui, picked, &response);

        ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.vertical(|ui| {
                first_line(ui);
                ui.label(label);
            });
        });
        // Against the line the label is on, not the middle of the cell: the
        // mark belongs beside the word it is about.
        let label_line = Rect::from_min_size(
            rect.left_bottom() - vec2(0.0, line.min(rect.height())),
            vec2(rect.width(), line.min(rect.height())),
        );
        show_sort_marker(ui, label_line, marker);
        strip = Some(response);
    });
    let response = strip.expect("the cell's contents are drawn before it returns");
    if let Some(info) = info {
        response.clone().hover(info);
    }
    response
}

/// How wide a heading has to be: what it is called, the room its sort mark
/// needs whether or not it is carrying one, and the gap between the two — or
/// the column's own width where that is wider.
///
/// The room is kept unconditionally so that a heading taking charge of the
/// order cannot widen its column under itself.
fn heading_width(available: f32, label: f32, marker_room: f32, spacing: f32) -> f32 {
    available.max(label + marker_room + spacing)
}

/// Copies `open` from the rows of the table being replaced onto the rows that
/// took their place, by name, down the whole tree.
fn take_open_state<T>(rows: &mut [MetricsTablePart<T>], previous: &[MetricsTablePart<T>]) {
    for row in rows.iter_mut() {
        let Some(was) = previous.iter().find(|old| old.name == row.name) else {
            continue;
        };
        row.open = was.open;
        take_open_state(&mut row.sub_parts, &was.sub_parts);
    }
}

/// The tick column of a table, and what it decides.
///
/// The rows under a player can be ticked off; what is left is what that
/// player's own row is worked out from (`app::damage_subset`), the way a
/// comparison's Total is worked out from the rows ticked under it. The eye
/// beside the name takes the unticked rows off the screen as well.
pub struct RowTicks<'a> {
    /// The rows that are out, per player: one tree of ticks each, because one
    /// player's rows are their own. Two players who both flew a Phaser Beam
    /// Array can have it ticked off in one and in for the other.
    ///
    /// Keyed by the group's name handle, not by the name itself: a grouping
    /// rule can give a group the name of an ability it collects, and then two
    /// different rows read the same. Ticking one would take both.
    pub excluded: &'a mut FxHashMap<NameHandle, FxHashSet<NameHandle>>,
    pub hide_unticked: &'a mut bool,
    /// The damage types the reader is looking at, and every type there is to
    /// pick. Empty means every type, which is how it starts.
    pub types: &'a mut FxHashSet<String>,
    pub all_types: &'a [String],
    /// Set when this pass changed any of them, so the caller rebuilds.
    pub changed: bool,
}

impl RowTicks<'_> {
    /// The type picker: which damage types the figures are of.
    fn show_types(&mut self, ui: &mut Ui) {
        if self.all_types.is_empty() {
            return;
        }
        let label = if self.types.is_empty() {
            "☰ Type".to_string()
        } else {
            format!("☰ Type ({})", self.types.len())
        };
        PopupButton::new(label)
            .with_id_source("table damage type picker")
            .show(ui, |ui| {
                ui.label(RichText::new("Show only these damage types").weak());
                // No separator: it takes the whole width it is offered, and in
                // a window sized to its contents that is the width of the
                // screen — a list of words like "Phaser" opened as a banner.
                ui.add_space(4.0);
                if ui
                    .add_enabled(!self.types.is_empty(), Button::new("Every type"))
                    .hover("Back to showing every type")
                    .clicked()
                {
                    self.types.clear();
                    self.changed = true;
                }
                for damage_type in self.all_types {
                    let mut on = self.types.contains(damage_type);
                    if ui.checkbox(&mut on, damage_type).changed() {
                        self.changed = true;
                        if on {
                            self.types.insert(damage_type.clone());
                        } else {
                            self.types.remove(damage_type);
                        }
                    }
                }
            })
            .response
            .hover(
                "Show only what was dealt in the damage types you pick, with every figure worked \
                 out for those types alone. Pick as many as you like; the list stays open.",
            );
    }

    /// The eye that hides the rows that are out.
    fn show_eye(&mut self, ui: &mut Ui) {
        if ui
            .add(Button::selectable(*self.hide_unticked, "👁"))
            .hover(
                "Hide the rows that are not ticked. They are out of the player's figures either \
                 way — this only takes them off the screen.",
            )
            .clicked()
        {
            *self.hide_unticked = !*self.hide_unticked;
        }
    }

    /// The tick of one row, and the cell it was drawn in.
    ///
    /// A player's row carries the tick that stands for all of theirs, half
    /// filled while only some are in — the same tick the Total row of a
    /// comparison carries, because it is the same thing: the row those below it
    /// add up to. The rows under it carry their own. Deeper in the tree there
    /// is nothing to tick: a row goes in with the branch it belongs to.
    fn show_cell(
        &mut self,
        row: &mut TableRow,
        indent: f32,
        player: NameHandle,
        handle: NameHandle,
        children: impl Iterator<Item = NameHandle> + Clone,
    ) -> Rect {
        match indent {
            0.0 => self.show_all_tick(row, handle, children),
            1.0 => self.show_row_tick(row, player, handle),
            _ => row.cell(|_| {}).rect,
        }
    }

    /// The player's own tick: every row of theirs, or none.
    fn show_all_tick(
        &mut self,
        row: &mut TableRow,
        player: NameHandle,
        children: impl Iterator<Item = NameHandle> + Clone,
    ) -> Rect {
        let out = self.excluded.get(&player);
        let rows = children.clone().count();
        let kept = children
            .clone()
            .filter(|handle| !out.is_some_and(|out| out.contains(handle)))
            .count();
        let mut all = kept == rows;
        let cell = row.cell(|ui| {
            let response = ui
                .add(Checkbox::new(&mut all, "").indeterminate(kept != rows && kept != 0))
                .hover(
                    "Count every row below in this player's figures, or none of them. Untick one \
                     and the figures above are worked out again without it.",
                );
            if response.changed() {
                self.changed = true;
                let out = self.excluded.entry(player).or_default();
                for handle in children {
                    if all {
                        out.remove(&handle);
                    } else {
                        out.insert(handle);
                    }
                }
            }
        });
        cell.rect
    }

    fn show_row_tick(
        &mut self,
        row: &mut TableRow,
        player: NameHandle,
        handle: NameHandle,
    ) -> Rect {
        let mut ticked = !self
            .excluded
            .get(&player)
            .is_some_and(|out| out.contains(&handle));
        let cell = row.cell(|ui| {
            if ui
                .checkbox(&mut ticked, "")
                .hover("Count this row in the player's figures above")
                .changed()
            {
                self.changed = true;
                let out = self.excluded.entry(player).or_default();
                if ticked {
                    out.remove(&handle);
                } else {
                    out.insert(handle);
                }
            }
        });
        cell.rect
    }

    /// Whether a row is not drawn at all: only the rows under a player can be,
    /// and only while the eye is on.
    fn is_hidden(&self, parent_indent: f32, player: NameHandle, handle: NameHandle) -> bool {
        parent_indent == 0.0
            && *self.hide_unticked
            && self
                .excluded
                .get(&player)
                .is_some_and(|out| out.contains(&handle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analyzer::NameFlags;

    fn ticks<'a>(
        excluded: &'a mut FxHashMap<NameHandle, FxHashSet<NameHandle>>,
        hide: &'a mut bool,
        types: &'a mut FxHashSet<String>,
    ) -> RowTicks<'a> {
        RowTicks {
            excluded,
            hide_unticked: hide,
            types,
            all_types: &[],
            changed: false,
        }
    }

    fn row(name: &str, children: Vec<MetricsTablePart<()>>) -> MetricsTablePart<()> {
        MetricsTablePart {
            data: (),
            name: name.to_string(),
            handle: NameHandle::default(),
            id: 0,
            sub_parts: children,
            open: false,
        }
    }

    fn names(parts: &[MetricsTablePart<()>]) -> Vec<&str> {
        parts.iter().map(|part| part.name.as_str()).collect()
    }

    /// The name column orders the rows by what they are called, at every level
    /// of the tree, and files `gravity well` with `Gravity Well` rather than
    /// under a heading of its own.
    #[test]
    fn the_name_column_orders_the_rows_by_name() {
        let mut table = MetricsTable::<()>::empty_base(&[]);
        table.players = vec![
            row("Talon", vec![row("torpedo", vec![]), row("Beam", vec![])]),
            row("kestrel", vec![]),
        ];

        table.sort_by_name();

        assert_eq!(["kestrel", "Talon"], names(&table.players)[..]);
        assert_eq!(["Beam", "torpedo"], names(&table.players[1].sub_parts)[..]);
    }

    /// The name column is not one of the metric columns, so its ordering has to
    /// survive a rebuild by a route of its own — otherwise a tick left the table
    /// ordered by name with no heading saying so.
    #[test]
    fn the_name_ordering_survives_a_rebuild() {
        let mut previous = MetricsTable::<()>::empty_base(&[]);
        previous.sort.clicked(NAME_COLUMN);

        let mut next = MetricsTable::<()>::empty_base(&[]);
        next.players = vec![row("Talon", vec![]), row("kestrel", vec![])];
        next.take_state_from(&previous);

        assert!(next.sort.is_sorted_by(NAME_COLUMN));
        assert_eq!(["kestrel", "Talon"], names(&next.players)[..]);
    }

    /// A heading keeps the room its sort mark needs before it has one, so
    /// taking charge of the order cannot widen the column under it — which is
    /// what pushed the numbers sideways under a long heading.
    #[test]
    fn a_heading_keeps_room_for_a_mark_it_is_not_carrying_yet() {
        let (label, mark, spacing) = (120.0, 9.0, 4.0);
        let narrow_column = 10.0;

        let width = heading_width(narrow_column, label, mark, spacing);
        assert!(
            width >= label + mark,
            "the name and the mark both fit: {width}"
        );
        assert_eq!(
            width,
            heading_width(narrow_column, label, mark, spacing),
            "and the width does not depend on whether the mark is drawn"
        );
        assert_eq!(
            300.0,
            heading_width(300.0, label, mark, spacing),
            "a column wider than its heading keeps its own width"
        );
    }

    /// One player's ticks are their own. Two players who both flew a Phaser
    /// Beam Array can have it ticked off in one and left in for the other —
    /// their trees are separate, and so are their figures.
    #[test]
    fn a_row_ticked_off_for_one_player_stays_in_for_another() {
        let mut names = NameManager::default();
        let raman = names.insert("Raman", NameFlags::NONE);
        let martinez = names.insert("Martinez", NameFlags::NONE);
        let beams = names.insert("Phaser Beam Array", NameFlags::NONE);
        let torpedo = names.insert("Photon Torpedo", NameFlags::NONE);

        let mut excluded: FxHashMap<NameHandle, FxHashSet<NameHandle>> = Default::default();
        excluded.entry(raman).or_default().insert(beams);
        let (mut hide, mut types) = (true, FxHashSet::default());
        let ticks = ticks(&mut excluded, &mut hide, &mut types);

        assert!(ticks.is_hidden(0.0, raman, beams));
        assert!(
            !ticks.is_hidden(0.0, martinez, beams),
            "the other player never ticked it off"
        );
        assert!(
            !ticks.is_hidden(0.0, raman, torpedo),
            "and this player's other rows are untouched"
        );
        assert!(
            !ticks.is_hidden(1.0, raman, beams),
            "a row deeper in the tree goes with the branch it belongs to"
        );
    }

    /// The rows under one parent are keyed by their group, and the analyzer
    /// interns a name once, so two rows under the same parent cannot read the
    /// same. Keying the ticks by the handle rather than by the name is
    /// therefore free of the question of what a name means — and cheaper.
    #[test]
    fn a_row_is_told_by_its_group_not_by_its_name() {
        let mut names = NameManager::default();
        let once = names.insert("Gravity Well I", NameFlags::NONE);
        let again = names.insert("Gravity Well I", NameFlags::VALUE);
        assert_eq!(once, again, "one name, one handle");
    }

    /// Nothing hides while the eye is off, whatever is ticked.
    #[test]
    fn the_unticked_rows_stay_on_screen_until_the_eye_is_on() {
        let mut names = NameManager::default();
        let player = names.insert("Raman", NameFlags::NONE);
        let beams = names.insert("Phaser Beam Array", NameFlags::NONE);
        let mut excluded: FxHashMap<NameHandle, FxHashSet<NameHandle>> = Default::default();
        excluded.entry(player).or_default().insert(beams);
        let (mut hide, mut types) = (false, FxHashSet::default());
        let ticks = ticks(&mut excluded, &mut hide, &mut types);
        assert!(!ticks.is_hidden(0.0, player, beams));
    }
}
