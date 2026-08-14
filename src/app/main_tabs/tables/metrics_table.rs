use std::borrow::Cow;
use std::cmp::Reverse;

use educe::Educe;
use eframe::egui::*;
use rustc_hash::FxHashSet;

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
struct ColumnKey {
    column: &'static str,
    part: Option<&'static str>,
}

impl ColumnKey {
    const fn whole(column: &'static str) -> Self {
        Self { column, part: None }
    }

    const fn half(column: &'static str, part: &'static str) -> Self {
        Self {
            column,
            part: Some(part),
        }
    }
}

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
        ScrollArea::horizontal().show(ui, |ui| {
            Table::new(ui)
                .cell_spacing(10.0)
                .header(header_height, |r| {
                    // The tick column's own header stays empty; the eye that
                    // hides the unticked rows sits beside the name, as it does
                    // in a comparison.
                    r.cell(|_| {});
                    r.cell(|ui| {
                        ui.horizontal(|ui| {
                            ui.label("Name");
                            ticks.show_eye(ui);
                            ticks.show_types(ui);
                        });
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
                            ticks,
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
        // from neighbouring metrics run together.
        //
        // Each cell of the group orders by its own figure — the total, the hull
        // or the shield — which is why the halves are named for the sort state
        // as `Metric Hull` rather than sharing the metric's name. Ordering all
        // three by the total made two of the three headings a lie.
        if split && !column.parts.is_empty() {
            show_group_separator(row);
            let name = column.name;
            self.show_header_cell_with(
                row,
                ColumnKey::whole(name),
                column.name_info,
                column.sort,
                |ui| split_total_header_text(ui, name).into(),
            );
            for part in column.parts.iter() {
                let label = format!("\n{}", part.name);
                self.show_header_cell_with(
                    row,
                    ColumnKey::half(column.name, part.name),
                    None,
                    part.sort,
                    move |_| label.into(),
                );
            }
            return;
        }

        let name = if split {
            format!("{}\n", column.name)
        } else {
            column.name.to_string()
        };
        self.show_header_cell_with(
            row,
            ColumnKey::whole(column.name),
            column.name_info,
            column.sort,
            move |_| name.clone().into(),
        );
    }

    /// The header cell of a column: ordered by on click, marked while it is the
    /// one doing the ordering, and explained on hover.
    ///
    /// The mark goes to the right of the text, in a cell of its own inside the
    /// heading, so it lands in the same place whether the heading is one line
    /// or two.
    fn show_header_cell_with(
        &mut self,
        row: &mut TableRow,
        key: ColumnKey,
        info: Option<&'static str>,
        sort: fn(&mut Self),
        text: impl FnOnce(&mut Ui) -> WidgetText,
    ) {
        let marker = self.sort.marker(key);
        // Not drawn as picked: a filled heading cell over a two-line header
        // leaves no room for the second line, and the mark already says which
        // column is doing the ordering.
        let response = row.selectable_cell(false, |ui| {
            ui.horizontal(|ui| {
                let text = text(ui);
                ui.label(text);
                // Only when there is one: an empty label still takes the
                // spacing beside it, and headings are tight enough as it is.
                if !marker.is_empty() {
                    ui.label(marker);
                }
            });
        });
        if response.clicked() {
            self.sort.clicked(key);
            self.sort_by_column(sort);
        }
        if let Some(info) = info {
            response.hover(info);
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
        ticks: &mut RowTicks,
    ) {
        let mut tick_rect = Rect::NOTHING;
        let response = table.selectable_row(selection.is_selected(self.id), |r| {
            tick_rect = ticks.show_cell(r, indent, &self.name);
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
                if ticks.is_hidden(indent, &sub_part.name) {
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

/// The tick column of a table, and what it decides.
///
/// The rows under a player can be ticked off; what is left is what that
/// player's own row is worked out from (`app::damage_subset`), the way a
/// comparison's Total is worked out from the rows ticked under it. The eye
/// beside the name takes the unticked rows off the screen as well.
pub struct RowTicks<'a> {
    /// The rows that are out, by name. Names rather than ids because the table
    /// is rebuilt whenever the combat, the settings or the ticks change.
    pub excluded: &'a mut FxHashSet<String>,
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
                ui.separator();
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
    /// Only the rows directly under a player carry one: they are what the
    /// player's figures are added up from, and a row deeper in the tree goes in
    /// with the branch it belongs to.
    fn show_cell(&mut self, row: &mut TableRow, indent: f32, name: &str) -> Rect {
        if indent != 1.0 {
            return row.cell(|_| {}).rect;
        }
        let mut ticked = !self.excluded.contains(name);
        let cell = row.cell(|ui| {
            if ui
                .checkbox(&mut ticked, "")
                .hover("Count this row in the player's figures above")
                .changed()
            {
                self.changed = true;
                if ticked {
                    self.excluded.remove(name);
                } else {
                    self.excluded.insert(name.to_string());
                }
            }
        });
        cell.rect
    }

    /// Whether a row is not drawn at all: only the rows under a player can be,
    /// and only while the eye is on.
    fn is_hidden(&self, parent_indent: f32, name: &str) -> bool {
        parent_indent == 0.0 && *self.hide_unticked && self.excluded.contains(name)
    }
}
