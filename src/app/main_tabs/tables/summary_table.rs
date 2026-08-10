use std::cmp::Reverse;

use chrono::Duration;
use eframe::egui::*;

use crate::{
    analyzer::{Player as AnalyzedPlayer, *},
    app::{main_tabs::common::*, settings::Settings},
    custom_widgets::table::*,
    helpers::{number_formatting::NumberFormatter, *},
};

use super::{common::Kills, metrics_table::show_group_separator};

macro_rules! col {
    ($name:expr, $sort:expr, $show:expr $(,)?) => {
        ColumnDescriptor {
            name: $name,
            sort: $sort,
            show: $show,
            parts: &[],
        }
    };
}

/// A column whose value splits into a hull and a shield half. Renders as one
/// cell with the halves in a tooltip, or as `all | Hull | Shield` under a shared
/// header when `general.split_shield_hull_columns` is on.
macro_rules! shield_hull_col {
    ($name:expr, $sort:expr, $field:ident $(,)?) => {
        ColumnDescriptor {
            name: $name,
            sort: $sort,
            show: |p, r| p.$field.show(r, p.halves_in_tooltip),
            parts: &[
                ColumnPart {
                    name: "Hull",
                    show: |p, r| p.$field.show_hull(r),
                },
                ColumnPart {
                    name: "Shield",
                    show: |p, r| p.$field.show_shield(r),
                },
            ],
        }
    };
}

static COLUMNS: &[ColumnDescriptor] = &[
    shield_hull_col!(
        "DPS Dealt",
        |t| t.sort_by_option_f64(|p| p.dps_out.all.value),
        dps_out,
    ),
    shield_hull_col!(
        "Damage Dealt",
        |t| t.sort_by_option_f64(|p| p.total_out_damage.all.value),
        total_out_damage,
    ),
    shield_hull_col!(
        "Damage Dealt %",
        |t| t.sort_by_option_f64(|p| p.total_out_damage_percentage.all.value),
        total_out_damage_percentage,
    ),
    shield_hull_col!(
        "Damage Taken",
        |t| t.sort_by_option_f64(|p| p.total_in_damage.all.value),
        total_in_damage,
    ),
    shield_hull_col!(
        "Damage Taken %",
        |t| t.sort_by_option_f64(|p| p.total_in_damage_percentage.all.value),
        total_in_damage_percentage,
    ),
    col!(
        "Combat Duration",
        |t| t.sort_by_key(|p| p.combat_duration.duration),
        |p, r| {
            p.combat_duration.show(r);
        },
    ),
    col!(
        "Combat Duration %",
        |t| t.sort_by_option_f64(|p| p.combat_duration_percentage.value),
        |p, r| {
            p.combat_duration_percentage.show(r);
        },
    ),
    col!(
        "Active Duration",
        |t| t.sort_by_key(|p| p.active_duration.duration),
        |p, r| {
            p.active_duration.show(r);
        },
    ),
    col!("Deaths", |t| t.sort_by_key(|p| p.deaths.count), |p, r| {
        p.deaths.show(r);
    }),
    col!(
        "Kills",
        |t| t.sort_by_key(|p| p.kills.total_count),
        |p, r| p.kills.show(r),
    ),
    col!(
        "Player Kills",
        |t| t.sort_by_key(|p| p.player_kills.count),
        |p, r| {
            p.player_kills.show(r);
        },
    ),
    col!(
        "NPC Kills",
        |t| t.sort_by_key(|p| p.npc_kills.count),
        |p, r| {
            p.npc_kills.show(r);
        },
    ),
];

struct ColumnDescriptor {
    name: &'static str,
    sort: fn(&mut SummaryTable),
    show: fn(&Player, &mut TableRow),
    /// Hull/Shield cells appended after `show` in split-columns mode.
    parts: &'static [ColumnPart],
}

/// See `metrics_table::closes_group`.
fn closes_summary_group(columns: &[&ColumnDescriptor], index: usize, split: bool) -> bool {
    if !split || columns[index].parts.is_empty() {
        return false;
    }
    columns
        .get(index + 1)
        .map(|next| next.parts.is_empty())
        .unwrap_or(true)
}

struct ColumnPart {
    name: &'static str,
    show: fn(&Player, &mut TableRow),
}

pub struct SummaryTable {
    players: Vec<Player>,
    selected_player: Option<usize>,
    /// See `MetricsTable::split_shield_hull`.
    split_shield_hull: bool,
}

struct Player {
    name: String,
    total_out_damage: ShieldAndHullTextValue,
    dps_out: ShieldAndHullTextValue,
    total_out_damage_percentage: ShieldAndHullTextValue,
    total_in_damage: ShieldAndHullTextValue,
    total_in_damage_percentage: ShieldAndHullTextValue,
    combat_duration: TextDuration,
    combat_duration_percentage: TextValue,
    active_duration: TextDuration,
    kills: Kills,
    npc_kills: TextCount,
    player_kills: TextCount,
    deaths: TextCount,
    /// See `DamageTablePartData::halves_in_tooltip`.
    halves_in_tooltip: bool,
}

impl SummaryTable {
    pub fn empty() -> Self {
        Self {
            players: Default::default(),
            selected_player: None,
            split_shield_hull: false,
        }
    }

    pub fn new(settings: &Settings, combat: &Combat) -> Self {
        let combat_duration = time_range_to_duration_or_zero(&combat.combat_time);
        let mut number_formatter = NumberFormatter::new();
        let mut table = Self {
            players: combat
                .players
                .values()
                .map(|p| {
                    Player::new(
                        settings,
                        combat_duration,
                        p,
                        &combat.name_manager,
                        &mut number_formatter,
                    )
                })
                .collect(),
            selected_player: None,
            split_shield_hull: settings.general.split_shield_hull_columns,
        };
        table.sort_by_option_f64(|p| p.total_out_damage.all.value);
        table
    }

    /// Every column this table has, for the column picker.
    pub fn column_names() -> Vec<&'static str> {
        COLUMNS.iter().map(|column| column.name).collect()
    }

    /// `shown` decides which columns are drawn; see `MetricsTable::show`.
    pub fn show(&mut self, ui: &mut Ui, shown: impl Fn(&str) -> bool) {
        let split = self.split_shield_hull;
        let columns: Vec<&ColumnDescriptor> =
            COLUMNS.iter().filter(|column| shown(column.name)).collect();
        let header_height = if split {
            SPLIT_HEADER_HEIGHT
        } else {
            HEADER_HEIGHT
        };
        ScrollArea::new([true, false]).show(ui, |ui| {
            Table::new(ui)
                .header(header_height, |r| {
                    r.cell(|ui| {
                        ui.horizontal(|ui| {
                            ui.label("Player");
                        });
                    });

                    for (index, column) in columns.iter().enumerate() {
                        if split && !column.parts.is_empty() {
                            show_group_separator(r);
                        }
                        for name in Self::header_names(column, split) {
                            Self::show_column_header(r, &name, || {
                                (column.sort)(self);
                            });
                        }
                        if closes_summary_group(&columns, index, split) {
                            show_group_separator(r);
                        }
                    }
                })
                .body(ROW_HEIGHT, |t| {
                    for (i, player) in self.players.iter().enumerate() {
                        let player_selected = Some(i) == self.selected_player;
                        if player.show(&columns, t, player_selected, split).clicked() {
                            self.selected_player = if player_selected { None } else { Some(i) };
                        }
                    }
                });
        });
    }

    /// Header text per cell of a column: one cell normally, or the metric name
    /// plus an All/Hull/Shield line per cell when the column is split.
    fn header_names(column: &ColumnDescriptor, split: bool) -> Vec<String> {
        if split && !column.parts.is_empty() {
            let mut names = vec![format!("{}\nAll", column.name)];
            names.extend(column.parts.iter().map(|p| format!("\n{}", p.name)));
            return names;
        }
        if split {
            return vec![format!("{}\n", column.name)];
        }
        vec![column.name.to_string()]
    }

    fn show_column_header(row: &mut TableRow, column_name: &str, sort: impl FnOnce()) {
        if row
            .selectable_cell(false, |ui| {
                ui.label(column_name);
            })
            .clicked()
        {
            sort();
        }
    }

    fn sort_by_option_f64(&mut self, mut value: impl FnMut(&Player) -> Option<f64>) {
        self.players
            .sort_unstable_by_key(|p| Reverse(value(p).map(F64TotalOrd)))
    }

    fn sort_by_key<K: Ord>(&mut self, mut key: impl FnMut(&Player) -> K) {
        self.players.sort_unstable_by_key(|p| Reverse(key(p)));
    }
}

impl Player {
    fn new(
        settings: &Settings,
        combat_duration: Duration,
        player: &AnalyzedPlayer,
        name_manager: &NameManager,
        number_formatter: &mut NumberFormatter,
    ) -> Self {
        let player_combat_duration = time_range_to_duration_or_zero(&player.combat_time);
        let player_combat_duration_percentage = if combat_duration.num_milliseconds() == 0 {
            0.0
        } else {
            player_combat_duration.num_milliseconds() as f64
                / combat_duration.num_milliseconds() as f64
                * 100.0
        };
        let player_active_duration = time_range_to_duration_or_zero(&player.active_time);
        let npc_kills: u32 = player
            .damage_out
            .kills
            .iter()
            .filter_map(|(n, k)| {
                if !name_manager.info(*n).flags.contains(NameFlags::PLAYER) {
                    Some(*k)
                } else {
                    None
                }
            })
            .sum();
        let player_kills: u32 = player
            .damage_out
            .kills
            .iter()
            .filter_map(|(n, k)| {
                if name_manager.info(*n).flags.contains(NameFlags::PLAYER) {
                    Some(*k)
                } else {
                    None
                }
            })
            .sum();
        Self {
            name: player.damage_out.name().get(name_manager).to_string(),
            total_out_damage: ShieldAndHullTextValue::new(
                &player.damage_out.total_damage,
                if settings.general.more_decimals { 2 } else { 0 },
                number_formatter,
            ),
            total_out_damage_percentage: ShieldAndHullTextValue::option(
                &player.damage_out.damage_percentage,
                if settings.general.more_decimals { 3 } else { 2 },
                number_formatter,
            ),
            dps_out: ShieldAndHullTextValue::new(
                &player.damage_out.dps,
                if settings.general.more_decimals { 2 } else { 0 },
                number_formatter,
            ),
            total_in_damage: ShieldAndHullTextValue::new(
                &player.damage_in.total_damage,
                if settings.general.more_decimals { 2 } else { 0 },
                number_formatter,
            ),
            total_in_damage_percentage: ShieldAndHullTextValue::option(
                &player.damage_in.damage_percentage,
                if settings.general.more_decimals { 3 } else { 2 },
                number_formatter,
            ),
            combat_duration: TextDuration::new(player_combat_duration),
            combat_duration_percentage: TextValue::new(
                player_combat_duration_percentage,
                if settings.general.more_decimals { 3 } else { 2 },
                number_formatter,
            ),
            active_duration: TextDuration::new(player_active_duration),
            kills: Kills::new(&player.damage_out, name_manager),
            deaths: TextCount::new(player.damage_in.kills.values().copied().sum::<u32>() as _),
            npc_kills: TextCount::new(npc_kills as _),
            player_kills: TextCount::new(player_kills as _),
            halves_in_tooltip: !settings.general.split_shield_hull_columns,
        }
    }

    pub fn show(
        &self,
        columns: &[&ColumnDescriptor],
        table: &mut TableBody,
        selected: bool,
        split: bool,
    ) -> Response {
        table.selectable_row(selected, |r| {
            r.cell(|ui| {
                ui.label(&self.name);
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
                if closes_summary_group(columns, index, split) {
                    show_group_separator(r);
                }
            }
        })
    }
}
