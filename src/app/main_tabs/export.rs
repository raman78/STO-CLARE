//! Writing the open combat out as a spreadsheet, one sheet per tab.
//!
//! The tables on screen show what fits; the file holds every metric the tab's
//! kind has, for every player and every row of its tree. A spreadsheet has its
//! own ways of hiding a column, and a file that leaves a number out is a file
//! that has to be exported again.
//!
//! The metrics are read straight off the analyzer's groups rather than out of
//! the built tables: a table cell holds text ("1.2M") and a spreadsheet wants a
//! number.

use crate::{
    analyzer::{Combat, DamageGroup, HealGroup, HealGrouping, NameManager, Player},
    app::export::{Column, Row, Sheet},
};

use super::MainTab;

/// One exported column: its heading, how many decimals it is written with, and
/// where its number comes from. `None` where the thing it is read off has no
/// value for it, which is left as an empty cell.
type Metric<T> = (&'static str, usize, fn(&T) -> Option<f64>);

/// The damage metrics, in the order they are written.
const DAMAGE_METRICS: &[Metric<DamageGroup>] = &[
    ("DPS", 0, |g| Some(g.dps.all)),
    ("Total Damage", 0, |g| Some(g.total_damage.all)),
    ("Damage %", 2, |g| g.damage_percentage.all),
    ("Resistance %", 2, |g| g.damage_resistance_percentage),
    ("Max One-Hit", 0, |g| Some(g.max_one_hit.damage)),
    ("Average Hit", 0, |g| g.average_hit.all),
    ("Critical %", 2, |g| g.critical_percentage),
    ("Flanking %", 2, |g| g.flanking),
    ("Accuracy %", 2, |g| g.accuracy_percentage),
    ("Hits", 0, |g| Some(g.damage_metrics.hits.all as f64)),
    ("Hits Hull", 0, |g| Some(g.damage_metrics.hits.hull as f64)),
    ("Hits Shield", 0, |g| {
        Some(g.damage_metrics.hits.shield as f64)
    }),
    ("Hits/s", 1, |g| Some(g.hits_per_second.all)),
    ("Hits %", 2, |g| g.hits_percentage.all),
    ("Misses", 0, |g| Some(g.damage_metrics.misses as f64)),
    ("Base Damage", 0, |g| Some(g.total_base_damage)),
    ("Base DPS", 0, |g| Some(g.base_dps)),
    ("Shield Drain", 0, |g| Some(g.total_shield_drain)),
    ("Crit Damage", 0, |g| Some(g.total_crit_damage)),
    ("Non-Crit Hull Damage", 0, |g| {
        Some(g.total_non_crit_hull_damage)
    }),
    ("Average Crit Hit", 0, |g| g.average_crit_hit),
    ("Average Non-Crit Hull Hit", 0, |g| {
        g.average_non_crit_hull_hit
    }),
    ("Damage Hull", 0, |g| Some(g.total_damage.hull)),
    ("Damage Shield", 0, |g| Some(g.total_damage.shield)),
];

/// The same for a healing tab.
const HEAL_METRICS: &[Metric<HealGroup>] = &[
    ("HPS", 0, |g| Some(g.hps.all)),
    ("Total Heal", 0, |g| Some(g.total_heal.all)),
    ("Heal %", 2, |g| g.heal_percentage.all),
    ("Average Heal", 0, |g| g.average_heal.all),
    ("Critical %", 2, |g| g.critical_percentage),
    ("Ticks", 0, |g| Some(g.heal_metrics.ticks.all as f64)),
    ("Ticks/s", 1, |g| Some(g.ticks_per_second.all)),
    ("Ticks %", 2, |g| g.ticks_percentage.all),
    ("Heal Hull", 0, |g| Some(g.total_heal.hull)),
    ("Heal Shield", 0, |g| Some(g.total_heal.shield)),
];

/// The per-player figures of the Summary tab, which has no tree under it.
const SUMMARY_METRICS: &[Metric<Player>] = &[
    ("DPS Dealt", 0, |p| Some(p.damage_out.dps.all)),
    ("Damage Dealt", 0, |p| Some(p.damage_out.total_damage.all)),
    ("Damage Dealt %", 2, |p| p.damage_out.damage_percentage.all),
    ("Damage Taken", 0, |p| Some(p.damage_in.total_damage.all)),
    ("Damage Taken %", 2, |p| p.damage_in.damage_percentage.all),
    ("Heal to Others", 0, |p| {
        Some(p.heal_ally.by_ability.total_heal.all)
    }),
    ("Self Heal", 0, |p| {
        Some(p.heal_self.by_ability.total_heal.all)
    }),
    ("Heal Received", 0, |p| {
        Some(p.heal_received.by_ability.total_heal.all)
    }),
    ("Combat Duration (s)", 1, |p| {
        Some(duration_seconds(p.combat_time.as_ref()))
    }),
    ("Active Duration (s)", 1, |p| {
        Some(duration_seconds(p.active_time.as_ref()))
    }),
    ("Kills", 0, |p| {
        Some(p.damage_out.kills.values().sum::<u32>() as f64)
    }),
];

/// One sheet per tab, in the order the tabs sit in. A run's numbers are one
/// file: the tab that happened to be open when the button was pressed is not
/// what the user wants to have to remember next week.
pub fn all_sheets(combat: &Combat, grouping: HealGrouping) -> Vec<Sheet> {
    [
        MainTab::Summary,
        MainTab::DamageDealt,
        MainTab::DamageTaken,
        MainTab::SelfHealing,
        MainTab::HealingAlly,
        MainTab::HealingReceived,
    ]
    .into_iter()
    .map(|tab| sheet(combat, tab, grouping))
    .collect()
}

/// The sheet for `tab` of `combat`. `grouping` is the healing tabs' by-ability
/// / by-target switch, ignored by the rest.
pub fn sheet(combat: &Combat, tab: MainTab, grouping: HealGrouping) -> Sheet {
    let names = &combat.name_manager;
    let mut sheet = match tab {
        MainTab::Summary => summary_sheet(combat),
        MainTab::DamageDealt => damage_sheet(combat, |p| &p.damage_out),
        MainTab::DamageTaken => damage_sheet(combat, |p| &p.damage_in),
        MainTab::SelfHealing => heal_sheet(combat, names, grouping, |p| &p.heal_self),
        MainTab::HealingAlly => heal_sheet(combat, names, grouping, |p| &p.heal_ally),
        MainTab::HealingReceived => heal_sheet(combat, names, grouping, |p| &p.heal_received),
    };
    sheet.name = tab.name().to_string();
    sheet
}

fn summary_sheet(combat: &Combat) -> Sheet {
    let rows = combat
        .players
        .iter()
        .map(|(handle, player)| Row {
            name: handle.get(&combat.name_manager).to_string(),
            level: 0,
            values: SUMMARY_METRICS
                .iter()
                .map(|(_, _, get)| get(player))
                .collect(),
        })
        .collect();
    Sheet {
        name: String::new(),
        combats: vec![combat_info(combat)],
        columns: columns(SUMMARY_METRICS.iter().map(|(name, decimals, _)| Column {
            header: name.to_string(),
            decimals: *decimals,
        })),
        rows,
    }
}

fn damage_sheet(combat: &Combat, pick: fn(&Player) -> &DamageGroup) -> Sheet {
    let mut rows = Vec::new();
    for player in combat.players.values() {
        collect_damage(pick(player), &combat.name_manager, 0, &mut rows);
    }
    Sheet {
        name: String::new(),
        combats: vec![combat_info(combat)],
        columns: columns(DAMAGE_METRICS.iter().map(|(name, decimals, _)| Column {
            header: name.to_string(),
            decimals: *decimals,
        })),
        rows,
    }
}

fn heal_sheet(
    combat: &Combat,
    names: &NameManager,
    grouping: HealGrouping,
    pick: fn(&Player) -> &crate::analyzer::HealPool,
) -> Sheet {
    let mut rows = Vec::new();
    for player in combat.players.values() {
        let pool = pick(player);
        let root = match grouping {
            HealGrouping::ByAbility => &pool.by_ability,
            HealGrouping::ByPerson => &pool.by_person,
        };
        collect_heal(root, names, 0, &mut rows);
    }
    Sheet {
        name: String::new(),
        combats: vec![combat_info(combat)],
        columns: columns(HEAL_METRICS.iter().map(|(name, decimals, _)| Column {
            header: name.to_string(),
            decimals: *decimals,
        })),
        rows,
    }
}

fn collect_damage(group: &DamageGroup, names: &NameManager, level: usize, rows: &mut Vec<Row>) {
    rows.push(Row {
        name: group.segment.name().get(names).to_string(),
        level,
        values: DAMAGE_METRICS
            .iter()
            .map(|(_, _, get)| get(group))
            .collect(),
    });
    for sub in group.sub_groups.values() {
        collect_damage(sub, names, level + 1, rows);
    }
}

fn collect_heal(group: &HealGroup, names: &NameManager, level: usize, rows: &mut Vec<Row>) {
    rows.push(Row {
        name: group.segment.name().get(names).to_string(),
        level,
        values: HEAL_METRICS.iter().map(|(_, _, get)| get(group)).collect(),
    });
    for sub in group.sub_groups.values() {
        collect_heal(sub, names, level + 1, rows);
    }
}

fn columns(columns: impl Iterator<Item = Column>) -> Vec<Column> {
    columns.collect()
}

fn combat_info(combat: &Combat) -> crate::app::export::Combat {
    crate::app::export::Combat {
        identifier: combat.identifier(),
        note: String::new(),
        player: String::new(),
    }
}

fn duration_seconds(range: Option<&std::ops::Range<chrono::NaiveDateTime>>) -> f64 {
    range
        .map(|range| {
            range
                .end
                .signed_duration_since(range.start)
                .num_milliseconds()
                .max(0) as f64
                / 1e3
        })
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every metric list has to line up with the columns written for it, or a
    /// value lands under somebody else's heading.
    #[test]
    fn each_metric_list_names_every_column_it_writes() {
        assert!(!DAMAGE_METRICS.is_empty());
        assert!(!HEAL_METRICS.is_empty());
        assert!(!SUMMARY_METRICS.is_empty());
        for (name, decimals, _) in DAMAGE_METRICS {
            assert!(!name.is_empty());
            assert!(*decimals <= 2, "{name} asks for more decimals than exist");
        }
        for (name, decimals, _) in HEAL_METRICS {
            assert!(!name.is_empty());
            assert!(*decimals <= 2, "{name} asks for more decimals than exist");
        }
        for (name, decimals, _) in SUMMARY_METRICS {
            assert!(!name.is_empty());
            assert!(*decimals <= 2, "{name} asks for more decimals than exist");
        }
    }

    /// Every tab has to be able to be a sheet: Excel wants names that are
    /// distinct, no longer than 31 characters and free of `: \\ / ? * [ ]`.
    #[test]
    fn every_tab_name_can_be_a_sheet_name() {
        let tabs = [
            MainTab::Summary,
            MainTab::DamageDealt,
            MainTab::DamageTaken,
            MainTab::SelfHealing,
            MainTab::HealingAlly,
            MainTab::HealingReceived,
        ];
        let mut names: Vec<&str> = tabs.iter().map(|tab| tab.name()).collect();
        assert_eq!(6, names.len());
        for name in &names {
            assert!(name.len() <= 31, "{name} is too long for a sheet name");
            assert!(
                !name.contains([':', '\\', '/', '?', '*', '[', ']']),
                "{name} holds a character a sheet name may not"
            );
        }
        names.sort_unstable();
        names.dedup();
        assert_eq!(6, names.len(), "two tabs share a name");
    }

    /// A metric name may not repeat: two identical headings in a spreadsheet
    /// are two columns nobody can tell apart.
    #[test]
    fn no_metric_name_repeats() {
        for names in [
            DAMAGE_METRICS
                .iter()
                .map(|(n, _, _)| *n)
                .collect::<Vec<_>>(),
            HEAL_METRICS.iter().map(|(n, _, _)| *n).collect(),
            SUMMARY_METRICS.iter().map(|(n, _, _)| *n).collect(),
        ] {
            let mut sorted = names.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(
                names.len(),
                sorted.len(),
                "a metric name repeats: {names:?}"
            );
        }
    }
}
