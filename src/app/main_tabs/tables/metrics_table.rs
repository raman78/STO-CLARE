use std::cmp::Reverse;

use educe::Educe;
use eframe::egui::*;
use rustc_hash::FxHashSet;

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
                show: |t, r| t.$field.show_hull(r),
            },
            ColumnPart {
                name: "Shield",
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
}

#[derive(Educe)]
#[educe(Deref, DerefMut)]
pub struct MetricsTablePart<T> {
    #[educe(Deref, DerefMut)]
    pub data: T,
    pub name: String,
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

/// One half of a split column.
#[derive(Clone, Copy)]
pub struct ColumnPart<T: 'static> {
    pub name: &'static str,
    pub show: fn(&mut MetricsTablePart<T>, &mut TableRow),
}

impl<T: 'static> MetricsTable<T> {
    pub fn empty_base(columns: &'static [ColumnDescriptor<T>]) -> Self {
        Self {
            players: Vec::new(),
            selection: Default::default(),
            columns,
            split_shield_hull: false,
        }
    }

    pub fn new_base<G: AnalysisGroup>(
        settings: &Settings,
        columns: &'static [ColumnDescriptor<T>],
        combat: &Combat,
        mut group: impl FnMut(&Player) -> &G,
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
                        group(p),
                        combat,
                        &mut number_formatter,
                        &mut id_source,
                        data_new,
                    )
                })
                .collect(),
            selection: Default::default(),
        };
        (table.columns[0].sort)(&mut table);

        table
    }

    /// `shown` decides which columns are drawn, and is asked every frame rather
    /// than baked in when the table is built, so the picker takes effect at
    /// once instead of at the next refresh.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        shown: impl Fn(&str) -> bool,
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
        ScrollArea::horizontal().show(ui, |ui| {
            Table::new(ui)
                .cell_spacing(10.0)
                .header(header_height, |r| {
                    r.cell(|ui| {
                        ui.label("Name");
                    });

                    for (index, column) in columns.iter().enumerate() {
                        self.show_column_header(r, column, split);
                        if closes_group(&columns, index, split) {
                            show_group_separator(r);
                        }
                    }
                })
                .body(ROW_HEIGHT, |t| {
                    for player in self.players.iter_mut() {
                        player.show(
                            &columns,
                            t,
                            0.0,
                            &mut self.selection,
                            &mut on_selected,
                            modifiers,
                            split,
                        );
                    }
                });
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
        // from neighbouring metrics run together. Every cell of the group sorts
        // by the same (all-values) key.
        if split && !column.parts.is_empty() {
            show_group_separator(row);
            let name = column.name;
            self.show_header_cell_with(row, column, |ui| {
                ui.label(split_total_header_text(ui, name));
            });
            for part in column.parts.iter() {
                self.show_header_cell(row, &format!("\n{}", part.name), column);
            }
            return;
        }

        let name = if split {
            format!("{}\n", column.name)
        } else {
            column.name.to_string()
        };
        self.show_header_cell(row, &name, column);
    }

    fn show_header_cell(&mut self, row: &mut TableRow, text: &str, column: &ColumnDescriptor<T>) {
        self.show_header_cell_with(row, column, |ui| {
            ui.label(text);
        });
    }

    /// The header cell of `column` with its own contents — sorting on click and
    /// the explanation on hover stay the same whatever is written in it.
    fn show_header_cell_with(
        &mut self,
        row: &mut TableRow,
        column: &ColumnDescriptor<T>,
        contents: impl FnOnce(&mut Ui),
    ) {
        let response = row.selectable_cell(false, contents);
        if response.clicked() {
            (column.sort)(self);
        }
        if let Some(info) = column.name_info {
            response.on_hover_text(info);
        }
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
    ) {
        let response = table.selectable_row(selection.is_selected(self.id), |r| {
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

        if response.clicked() {
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
                sub_part.show(
                    columns,
                    table,
                    indent + 1.0,
                    selection,
                    on_selected,
                    modifiers,
                    split,
                );
            }
        }
    }

    pub fn sort_by_desc<K: Ord>(&mut self, mut key: impl FnMut(&Self) -> K + Copy) {
        self.sub_parts.sort_unstable_by_key(|p| Reverse(key(p)));

        self.sub_parts.iter_mut().for_each(|p| p.sort_by_desc(key));
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
