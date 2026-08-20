use std::cmp::Reverse;

use chrono::Duration;
use eframe::egui::*;

use crate::{
    analyzer::{Player as AnalyzedPlayer, *},
    app::{main_tabs::common::*, settings::Settings},
    custom_widgets::table::*,
    helpers::{number_formatting::NumberFormatter, *},
};

use super::{
    common::Kills,
    metrics_table::{ColumnKey, show_group_separator, show_sortable_header},
};

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
            // Each half orders by its own figure. Ordering all three by the
            // total made two of the three headings a lie.
            parts: &[
                ColumnPart {
                    name: "Hull",
                    sort: |t| t.sort_by_option_f64(|p| p.$field.hull.value),
                    show: |p, r| p.$field.show_hull(r),
                },
                ColumnPart {
                    name: "Shield",
                    sort: |t| t.sort_by_option_f64(|p| p.$field.shield.value),
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
    sort: fn(&mut SummaryTable),
    show: fn(&Player, &mut TableRow),
}

pub struct SummaryTable {
    players: Vec<Player>,
    selected_player: Option<usize>,
    /// See `MetricsTable::split_shield_hull`.
    split_shield_hull: bool,
    /// Which heading is ordering the players, and which way round.
    sort: SortState<ColumnKey>,
}

/// The heading of the player column, which is not one of the metric columns but
/// orders the rows all the same.
const NAME_COLUMN: ColumnKey = ColumnKey::whole("Player");

/// The column the players are in when nothing has been clicked: the first one,
/// which is how `MetricsTable` decides it too. That is DPS — the question a
/// summary is opened with is who was doing the most work, and a long fight can
/// pile up more damage than a short one while going slower.
fn default_sort() -> ColumnKey {
    ColumnKey::whole(COLUMNS[0].name)
}

#[derive(Default)]
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
            sort: Default::default(),
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
            sort: SortState {
                column: Some(default_sort()),
                natural: true,
            },
        };
        (COLUMNS[0].sort)(&mut table);
        table
    }

    /// Carry the order the reader put the table in onto the table replacing it,
    /// so opening another combat does not undo it. See
    /// `MetricsTable::take_state_from`.
    pub fn take_state_from(&mut self, previous: &Self) {
        // See `MetricsTable::take_state_from`: the empty table a tab starts
        // with has picked nothing, and its state must not be taken.
        let Some(key) = previous.sort.column else {
            return;
        };
        self.sort = previous.sort;
        // The player column is not among the metric columns.
        if key == NAME_COLUMN {
            self.sort_by_column(|table| table.sort_by_name());
            return;
        }
        let Some(column) = COLUMNS.iter().find(|column| column.name == key.column()) else {
            return;
        };
        let sort = key
            .part()
            .and_then(|part| column.parts.iter().find(|p| p.name == part))
            .map(|part| part.sort)
            .unwrap_or(column.sort);
        self.sort_by_column(sort);
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
        // The table scrolls sideways by itself; the header is drawn last so it
        // stays level with the columns under it.
        Table::new(ui)
            .header(header_height)
            .body(ROW_HEIGHT, |t| {
                for (i, player) in self.players.iter().enumerate() {
                    let player_selected = Some(i) == self.selected_player;
                    if player.show(&columns, t, player_selected, split).clicked() {
                        self.selected_player = if player_selected { None } else { Some(i) };
                    }
                }
            })
            .header_row(|r| {
                r.cell(|ui| {
                    // Ordering by name, the same as the tables on the damage
                    // tabs: a team of five is found by who, not by how much.
                    if show_sortable_header_cell(
                        ui,
                        self.sort.is_sorted_by(NAME_COLUMN),
                        self.sort.marker(NAME_COLUMN),
                        "Player",
                        |_| {},
                    )
                    .clicked()
                    {
                        self.sort.clicked(NAME_COLUMN);
                        self.sort_by_column(|table| table.sort_by_name());
                    }
                });

                for (index, column) in columns.iter().enumerate() {
                    self.show_column_header(r, column, split);
                    if closes_summary_group(&columns, index, split) {
                        show_group_separator(r);
                    }
                }
            });
    }

    /// See `MetricsTable::show_column_header`: the same headings, drawn by the
    /// same code, so the two tables cannot drift apart.
    fn show_column_header(&mut self, row: &mut TableRow, column: &ColumnDescriptor, split: bool) {
        if split && !column.parts.is_empty() {
            show_group_separator(row);
            let name = column.name;
            self.show_heading(row, ColumnKey::whole(name), column.sort, true, |ui| {
                ui.label(name);
            });
            for part in column.parts.iter() {
                // A half has no name of its own on the first line, but it still
                // needs the room: without it the label would sit a line higher
                // than the one beside it.
                self.show_heading(
                    row,
                    ColumnKey::half(name, part.name),
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

        self.show_heading(
            row,
            ColumnKey::whole(column.name),
            column.sort,
            false,
            |ui| {
                // A metric with no halves still sits in a header built for two lines
                // when the split columns are on, so it takes the first line's room
                // and leaves its heading level with the others.
                if split {
                    let line = ui.text_style_height(&TextStyle::Body);
                    ui.add_space(line);
                }
            },
        );
    }

    fn show_heading(
        &mut self,
        row: &mut TableRow,
        key: ColumnKey,
        sort: fn(&mut Self),
        split_group: bool,
        first_line: impl FnOnce(&mut Ui),
    ) {
        let response = show_sortable_header(row, &self.sort, key, None, split_group, first_line);
        if response.clicked() {
            self.sort.clicked(key);
            self.sort_by_column(sort);
        }
    }

    /// See `MetricsTable::sort_by_column`.
    fn sort_by_column(&mut self, sort: fn(&mut Self)) {
        sort(self);
        if !self.sort.natural {
            self.players.reverse();
        }
    }

    /// By name, A to Z. Case is ignored, so `@Raman` and `@raman` file together.
    fn sort_by_name(&mut self) {
        self.players
            .sort_unstable_by_key(|player| player.name.to_lowercase());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn player(name: &str, all: f64, hull: f64, shield: f64) -> Player {
        Player {
            name: name.to_string(),
            total_out_damage: ShieldAndHullTextValue {
                all: TextValue {
                    text: None,
                    value: Some(all),
                },
                hull: TextValue {
                    text: None,
                    value: Some(hull),
                },
                shield: TextValue {
                    text: None,
                    value: Some(shield),
                },
            },
            ..Default::default()
        }
    }

    fn table(players: Vec<Player>) -> SummaryTable {
        SummaryTable {
            players,
            selected_player: None,
            split_shield_hull: true,
            sort: Default::default(),
        }
    }

    fn names(table: &SummaryTable) -> Vec<&str> {
        table.players.iter().map(|p| p.name.as_str()).collect()
    }

    fn column(name: &str) -> &'static ColumnDescriptor {
        COLUMNS
            .iter()
            .find(|column| column.name == name)
            .expect("the column is one this table has")
    }

    fn part(name: &str, half: &str) -> fn(&mut SummaryTable) {
        column(name)
            .parts
            .iter()
            .find(|part| part.name == half)
            .expect("the column splits into halves")
            .sort
    }

    /// The Hull heading orders by the hull figure, not by the total above it.
    /// One order for all three columns made two of the three headings a lie.
    #[test]
    fn a_half_orders_by_its_own_figure() {
        let mut table = table(vec![
            player("Kestrel", 900.0, 100.0, 800.0),
            player("Talon", 800.0, 700.0, 100.0),
        ]);

        table.sort_by_column(column("Damage Dealt").sort);
        assert_eq!(names(&table), ["Kestrel", "Talon"], "by the total");

        table.sort_by_column(part("Damage Dealt", "Hull"));
        assert_eq!(names(&table), ["Talon", "Kestrel"], "by the hull half");

        table.sort_by_column(part("Damage Dealt", "Shield"));
        assert_eq!(names(&table), ["Kestrel", "Talon"], "by the shield half");
    }

    /// Clicking the heading that is already ordering the rows turns the order
    /// round rather than sorting it the same way again.
    #[test]
    fn clicking_the_same_heading_turns_the_order_round() {
        let mut table = table(vec![
            player("Kestrel", 900.0, 0.0, 0.0),
            player("Talon", 800.0, 0.0, 0.0),
        ]);
        let key = ColumnKey::whole("Damage Dealt");

        table.sort.clicked(key);
        table.sort_by_column(column("Damage Dealt").sort);
        assert_eq!(names(&table), ["Kestrel", "Talon"]);

        table.sort.clicked(key);
        table.sort_by_column(column("Damage Dealt").sort);
        assert_eq!(names(&table), ["Talon", "Kestrel"], "the same click again");

        assert_eq!(
            table.sort.marker(key),
            SORT_MARKERS[1],
            "and the heading says so"
        );
    }

    /// A tab starts with an empty table nobody has clicked. It must not hand
    /// that emptiness to the first real table, which would leave the players in
    /// damage order with no heading saying so.
    #[test]
    fn the_table_a_tab_starts_with_says_nothing_about_the_order() {
        let mut first = table(vec![player("Kestrel", 900.0, 0.0, 0.0)]);
        first.sort = SortState {
            column: Some(default_sort()),
            natural: true,
        };
        first.take_state_from(&SummaryTable::empty());

        assert_eq!(first.sort.column, Some(default_sort()));
    }

    /// A summary opens on DPS, the same question the damage tabs open on. It
    /// opened on damage dealt, which reads differently on a long fight: more
    /// damage piled up at a lower rate.
    #[test]
    fn a_summary_opens_ordered_by_dps() {
        assert_eq!(ColumnKey::whole("DPS Dealt"), default_sort());
    }

    /// Opening another combat builds the table again. The column the reader put
    /// it in order by is theirs and comes along, the way it does in the tables
    /// on the damage tabs.
    #[test]
    fn the_order_survives_opening_another_combat() {
        let mut previous = table(vec![player("Kestrel", 900.0, 100.0, 0.0)]);
        previous
            .sort
            .clicked(ColumnKey::half("Damage Dealt", "Hull"));

        let mut next = table(vec![
            player("Kestrel", 900.0, 100.0, 0.0),
            player("Talon", 800.0, 700.0, 0.0),
        ]);
        next.take_state_from(&previous);

        assert_eq!(
            next.sort.column,
            Some(ColumnKey::half("Damage Dealt", "Hull"))
        );
        assert_eq!(names(&next), ["Talon", "Kestrel"], "and it is applied");
    }
}
