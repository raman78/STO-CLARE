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
/// `ShieldAndHullTextCount` on the row data.
#[macro_export]
macro_rules! shield_hull_col {
    ($name:expr, $sort:expr, $field:ident $(,)?) => {
        ColumnDescriptor {
            name: $name,
            name_info: None,
            sort: $sort,
            show: |t, r| t.$field.show(r),
            parts: $crate::shield_hull_parts!($field),
        }
    };

    ($name:expr, $name_info:expr, $sort:expr, $field:ident $(,)?) => {
        ColumnDescriptor {
            name: $name,
            name_info: Some($name_info),
            sort: $sort,
            show: |t, r| t.$field.show(r),
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

    pub fn show(&mut self, ui: &mut Ui, mut on_selected: impl FnMut(TableSelectionEvent<T>)) {
        let modifiers = ui.input(|i| i.modifiers);
        let split = self.split_shield_hull;
        // Split columns need a second header line for the All/Hull/Shield labels.
        let header_height = if split {
            SPLIT_HEADER_HEIGHT
        } else {
            HEADER_HEIGHT
        };
        ScrollArea::horizontal().show(ui, |ui| {
            Table::new(ui)
                .cell_spacing(10.0)
                .header(header_height, |mut r| {
                    r.cell(|ui| {
                        ui.label("Name");
                    });

                    for column in self.columns.iter() {
                        self.show_column_header(&mut r, column, split);
                    }
                })
                .body(ROW_HEIGHT, |mut t| {
                    for player in self.players.iter_mut() {
                        player.show(
                            &self.columns,
                            &mut t,
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

    fn show_column_header(&mut self, row: &mut TableRow, column: &ColumnDescriptor<T>, split: bool) {
        // Unsplit: one cell holding the metric name. Split: a rule opens the
        // group, the metric name sits above its first cell, and All/Hull/Shield
        // label the second line. Without the rule, three same-looking numbers
        // from neighbouring metrics run together. Every cell of the group sorts
        // by the same (all-values) key.
        if split && !column.parts.is_empty() {
            show_group_separator(row);
            self.show_header_cell(row, &format!("{}\n{}", column.name, "All"), column);
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
        let response = row.selectable_cell(false, |ui| {
            ui.label(text);
        });
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
        self.sort_by_desc(move |p| key(p).map(|v| F64TotalOrd(v)));
    }

    pub fn sort_by_option_f64_asc(
        &mut self,
        mut key: impl FnMut(&MetricsTablePart<T>) -> Option<f64> + Copy,
    ) {
        self.sort_by_asc(move |p| key(p).map(|v| F64TotalOrd(v)));
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

    fn show(
        &mut self,
        columns: &[ColumnDescriptor<T>],
        table: &mut TableBody,
        indent: f32,
        selection: &mut SelectionTracker,
        on_selected: &mut impl FnMut(TableSelectionEvent<T>),
        modifiers: Modifiers,
        split: bool,
    ) {
        let response = table.selectable_row(selection.is_selected(self.id), |mut r| {
            r.cell(|ui| {
                ui.horizontal(|ui| {
                    ui.add_space(indent * 30.0);
                    let symbol = if self.open { "⏷" } else { "⏵" };
                    let can_open = self.sub_parts.len() > 0;
                    if ui
                        .add_visible(can_open, Button::selectable(false, symbol))
                        .clicked()
                    {
                        self.open = !self.open;
                    }

                    ui.label(&self.name);
                });
            });

            for column in columns.iter() {
                if split && !column.parts.is_empty() {
                    show_group_separator(&mut r);
                }
                (column.show)(self, &mut r);
                if split {
                    for part in column.parts.iter() {
                        (part.show)(self, &mut r);
                    }
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
            if ui
                .selectable_label(false, "copy name to clipboard")
                .clicked()
            {
                ui.ctx().copy_text(self.name.clone());
                ui.close_kind(UiKind::Menu);
            }

            if ui
                .selectable_label(false, "show diagrams for this")
                .clicked()
                && !selection.is_selected(self.id)
            {
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
