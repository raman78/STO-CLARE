use std::{borrow::Cow, ops::Range};

use chrono::NaiveDateTime;

use eframe::egui::*;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    analyzer::*,
    app::{
        damage_sections::{SectionRules, gather_normal_fire, into_sections},
        damage_subset,
        settings::{Settings, TableKind},
    },
    custom_widgets::splitter::Splitter,
    helpers::number_formatting::NumberFormatter,
};

use super::{common::*, diagrams::*, tables::*};
use crate::custom_widgets::toggle::Toggle;
use crate::custom_widgets::tooltip::CloseTooltip;

/// Which way a damage tab nests its tree: the weapon with the modes it was
/// fired in under it, or the mode with the weapons that fired it under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageGrouping {
    ByWeapon,
    ByAbility,
}

pub struct DamageTab {
    /// The same damage, nested both ways, both built up front so the grouping
    /// toggle is instant — the second nesting comes from the analyzer, the way
    /// the healing tabs' does. A tab without the toggle leaves
    /// `table_by_ability` empty; it can never be shown.
    table: DamageTable,
    table_by_ability: DamageTable,
    grouping: DamageGrouping,
    /// Whether the rows under each player are split into weapons, consoles
    /// and abilities, and pets (`app::damage_sections`). Offered where the
    /// ability-first tree is, since both are about reading the player's own
    /// build.
    sectioned: bool,
    /// Whether the ability-first order gathers every weapon shot fired in no
    /// mode under one row (`damage_sections::gather_normal_fire`).
    normal_fire: bool,
    /// The ability-first tree, for the tab that offers it (Damage Dealt).
    damage_group_by_ability: Option<for<'a> fn(&'a Player) -> &'a DamageGroup>,
    dmg_main_diagrams: DamageDiagrams,
    dmg_selection_diagrams: Option<DamageDiagrams>,
    damage_group: for<'a> fn(&'a Player) -> &'a DamageGroup,
    /// The time this tab's DPS is divided by, which the Active column's share
    /// is of as well: the analyzer measures outgoing damage against the time
    /// a player was dealing damage, incoming against the time they were doing
    /// anything.
    time_base: for<'a> fn(&'a Player) -> &'a Option<Range<NaiveDateTime>>,
    /// That time per player, by the name their row carries.
    player_time_s: FxHashMap<String, f64>,
    /// The rows the reader has ticked off, by name. They are left out of every
    /// player's figures, and — with `hide_unticked` — off the screen as well.
    /// One tree of ticks per player: a row ticked off for one player says
    /// nothing about the same row under another.
    excluded: FxHashMap<NameHandle, FxHashSet<NameHandle>>,
    /// The same, for the ability-first nesting. Its rows are other rows, so a
    /// tick there says nothing about the weapon-first tree, and the reverse.
    excluded_by_ability: FxHashMap<NameHandle, FxHashSet<NameHandle>>,
    /// The same, for the ability-first nesting with the plain fire gathered.
    excluded_normal_fire: FxHashMap<NameHandle, FxHashSet<NameHandle>>,
    hide_unticked: bool,
    /// The damage types the figures are of, empty for all of them, and every
    /// type this combat holds.
    types: FxHashSet<String>,
    all_types: Vec<String>,
    filter: f64,
    diagram_time_slice: f64,
    activity_gap_s: f64,
    /// Whether the Activity chart draws its rows as one lane.
    activity_total: bool,
    active_diagram: DiagramType,
    /// The open combat's length, so a charted selection spans the whole fight
    /// rather than only the part where the selected ability was firing.
    combat_duration_s: f64,
}

impl DamageTab {
    pub fn empty(
        damage_group: fn(&Player) -> &DamageGroup,
        damage_group_by_ability: Option<fn(&Player) -> &DamageGroup>,
        time_base: fn(&Player) -> &Option<Range<NaiveDateTime>>,
    ) -> Self {
        Self {
            time_base,
            player_time_s: Default::default(),
            table: DamageTable::empty(),
            table_by_ability: DamageTable::empty(),
            grouping: DamageGrouping::ByWeapon,
            sectioned: false,
            normal_fire: false,
            damage_group_by_ability,
            excluded_by_ability: Default::default(),
            excluded_normal_fire: Default::default(),
            dmg_main_diagrams: DamageDiagrams::empty(),
            damage_group,
            excluded: Default::default(),
            hide_unticked: false,
            types: Default::default(),
            all_types: Vec::new(),
            filter: 0.4,
            diagram_time_slice: 1.0,
            activity_gap_s: DEFAULT_ACTIVITY_GAP_S,
            activity_total: false,
            dmg_selection_diagrams: None,
            active_diagram: DiagramType::Dps,
            combat_duration_s: 0.0,
        }
    }

    pub fn update(&mut self, settings: &Settings, combat: &Combat) {
        self.combat_duration_s = combat_duration_seconds(combat);
        self.player_time_s = combat
            .players
            .values()
            .map(|player| {
                (
                    (self.damage_group)(player)
                        .name()
                        .get(&combat.name_manager)
                        .to_string(),
                    damage_subset::metrics_duration((self.time_base)(player)),
                )
            })
            .collect();
        self.rebuild(settings, combat);
    }

    /// The table and the chart under it, both of the rows that are ticked.
    ///
    /// The chart follows because the reason to tick rows off is to look at part
    /// of a build — the beams alone, the front weapons alone — and a chart that
    /// kept drawing everything would answer a different question from the table
    /// above it.
    fn rebuild(&mut self, settings: &Settings, combat: &Combat) {
        // Taken from the players' own groups: a branch carries the types of
        // everything under it, and reading them off the filtered tree would
        // leave the picker offering only what was already picked.
        let mut all_types: Vec<String> = combat
            .players
            .values()
            .flat_map(|player| {
                (self.damage_group)(player)
                    .damage_types
                    .iter()
                    .map(|damage_type| damage_type.get(&combat.name_manager).to_string())
            })
            .collect();
        all_types.sort_unstable();
        all_types.dedup();
        self.all_types = all_types;

        self.rebuild_table(settings, combat);
        let kept: Vec<DamageGroup> = combat
            .players
            .values()
            .map(|player| {
                self.kept_damage(self.grouping, player, combat, settings)
                    .into_owned()
            })
            .collect();
        let player_time_s = &self.player_time_s;
        self.dmg_main_diagrams = DamageDiagrams::from_damage_groups(
            kept.iter(),
            combat,
            |player| player_time_s.get(player).copied().unwrap_or(0.0),
            self.filter,
            self.diagram_time_slice,
            self.activity_gap_s,
        );
        self.dmg_selection_diagrams = None;
    }

    /// One player's damage as the table shows it: the unticked rows left out
    /// of the figures, and — when the table is split — the rows put in their
    /// sections.
    fn kept_damage<'a>(
        &self,
        grouping: DamageGrouping,
        player: &'a Player,
        combat: &Combat,
        settings: &Settings,
    ) -> Cow<'a, DamageGroup> {
        let kept = self.filtered_damage(grouping, player, combat, settings);
        if !self.sectioned || self.damage_group_by_ability.is_none() {
            return kept;
        }
        let excluded = self.excluded_of(grouping);
        // Told apart by everything the player hit, so taken from the whole of
        // their damage rather than from the rows that are ticked.
        let rules = SectionRules::new((self.damage_group)(player), combat, &settings.sections);
        // The ticks are this player's own, filed under the player's row.
        let excluded = excluded.get(&kept.name());
        Cow::Owned(into_sections(
            kept.into_owned(),
            &rules,
            excluded,
            &combat.hits_manger,
            damage_subset::metrics_duration((self.time_base)(player)),
        ))
    }

    /// One player's damage with the unticked rows left out, or the whole of it
    /// when nothing is ticked off.
    fn filtered_damage<'a>(
        &self,
        grouping: DamageGrouping,
        player: &'a Player,
        combat: &Combat,
        settings: &Settings,
    ) -> Cow<'a, DamageGroup> {
        let whole = match (grouping, self.damage_group_by_ability) {
            (DamageGrouping::ByAbility, Some(by_ability)) if self.normal_fire => {
                // Gathered before the ticks, so the gathered row is ticked as
                // one, as a mode's row is.
                let rules =
                    SectionRules::new((self.damage_group)(player), combat, &settings.sections);
                Cow::Owned(gather_normal_fire(
                    by_ability(player).clone(),
                    player,
                    &rules,
                    &combat.hits_manger,
                    damage_subset::metrics_duration((self.time_base)(player)),
                ))
            }
            (DamageGrouping::ByAbility, Some(by_ability)) => Cow::Borrowed(by_ability(player)),
            _ => Cow::Borrowed((self.damage_group)(player)),
        };
        // The ticks are this player's own.
        let excluded = self.excluded_of(grouping).get(&whole.name());
        if self.types.is_empty() && excluded.is_none_or(FxHashSet::is_empty) {
            return whole;
        }
        // The type first — it decides what the rows even are — and the ticks
        // over what it left.
        let of_types = if self.types.is_empty() {
            None
        } else {
            damage_subset::damage_of_types(
                player,
                &whole,
                &combat.name_manager,
                &combat.hits_manger,
                &self.types,
                &combat.total_damage_out,
            )
        };
        let Some(excluded) = excluded.filter(|out| !out.is_empty()) else {
            return match of_types {
                Some(kept) => Cow::Owned(kept),
                None => whole,
            };
        };
        Cow::Owned(damage_subset::player_damage_without(
            player,
            of_types.as_ref().unwrap_or(&whole),
            &combat.hits_manger,
            excluded,
            &combat.total_damage_out,
        ))
    }

    /// The ticks of the rows on screen in this order. Gathering the plain fire
    /// into one row makes other rows, so it keeps its ticks apart, the way the
    /// two orders do.
    fn excluded_of(
        &self,
        grouping: DamageGrouping,
    ) -> &FxHashMap<NameHandle, FxHashSet<NameHandle>> {
        match grouping {
            DamageGrouping::ByWeapon => &self.excluded,
            DamageGrouping::ByAbility if self.normal_fire => &self.excluded_normal_fire,
            DamageGrouping::ByAbility => &self.excluded_by_ability,
        }
    }

    fn rebuild_table(&mut self, settings: &Settings, combat: &Combat) {
        // Both filters go through one place, so the table and the chart under
        // it are always of the same rows.
        let mut table = DamageTable::new(settings, combat, |player| {
            self.kept_damage(DamageGrouping::ByWeapon, player, combat, settings)
        });
        // A tick rebuilds the table; the tree the reader had opened, and the
        // column they had ordered it by, are theirs and stay put.
        table.take_state_from(&self.table);
        self.reserve_view_room(&mut table, DamageGrouping::ByWeapon);
        self.table = table;

        if self.damage_group_by_ability.is_some() {
            let mut table = DamageTable::new(settings, combat, |player| {
                self.kept_damage(DamageGrouping::ByAbility, player, combat, settings)
            });
            table.take_state_from(&self.table_by_ability);
            self.reserve_view_room(&mut table, DamageGrouping::ByAbility);
            self.table_by_ability = table;
        }
        self.update_active(settings);
    }

    /// The gap the Activity chart and the Active column split stretches at,
    /// which the spreadsheet export follows too.
    pub fn activity_gap_s(&self) -> f64 {
        self.activity_gap_s
    }

    /// Keeps room in the Name column for the levels that Sections and Normal
    /// weapon fire add while they are off, so switching either does not move
    /// the figures sideways. Sections put every row a level down; gathering
    /// puts the plain fire a level down, and room is kept for every row
    /// rather than guessing which ones those are — with it on, every row but
    /// the gathered ones keeps that room, so the column is the same width both
    /// ways.
    fn reserve_view_room(&self, table: &mut DamageTable, grouping: DamageGrouping) {
        if self.damage_group_by_ability.is_none() {
            return;
        }
        if !self.sectioned {
            table.reserve_name_room(1, |_| 1.0);
        }
        if grouping == DamageGrouping::ByAbility {
            let depth = if self.sectioned { 2 } else { 1 };
            table.reserve_name_room(
                depth,
                |row| {
                    if row == NORMAL_WEAPON_FIRE { 0.0 } else { 1.0 }
                },
            );
        }
    }

    /// The Active column of both tables, from the Activity chart's gap. Done
    /// over the rows already built rather than by rebuilding them, so moving
    /// the slider does not fold the tree up.
    fn update_active(&mut self, settings: &Settings) {
        let gap_s = self.activity_gap_s;
        let player_time_s = &self.player_time_s;
        let more_decimals = settings.general.more_decimals;
        let mut formatter = NumberFormatter::new();
        for table in [&mut self.table, &mut self.table_by_ability] {
            table.for_each_part_mut(|player, part| {
                // Every row is under a player, and every player is in the map;
                // a row the map does not know would be a table built from
                // another combat, and gets no share rather than a wrong one.
                let time = player_time_s.get(player).copied().unwrap_or(0.0);
                part.data
                    .set_active(gap_s, time, more_decimals, &mut formatter)
            });
            table.reapply_sort();
        }
    }

    /// Switches how the tree is nested, and whether it is split into
    /// sections. Answers whether either was clicked, because the table and the
    /// chart under it are then of other rows until they are built again. Tabs
    /// without the second nesting show nothing here.
    fn show_grouping_picker(&mut self, ui: &mut Ui) -> bool {
        if self.damage_group_by_ability.is_none() {
            return false;
        }
        ui.horizontal(|ui| {
            ui.label("Group by:");
            // "⏵" is the glyph the tree rows use for their expander, so it is
            // known to exist in the bundled font.
            let regrouped = ui
                .steady_toggle_value(
                    &mut self.grouping,
                    DamageGrouping::ByWeapon,
                    "Weapon ⏵ Ability",
                )
                .hover(
                    "Top level is the weapon, with the modes it was fired in underneath: \
                 Phaser Beam Array, then its Fire at Will III and Overload I.",
                )
                .clicked()
                | ui.steady_toggle_value(
                    &mut self.grouping,
                    DamageGrouping::ByAbility,
                    "Ability ⏵ Weapon",
                )
                .hover(
                    "Top level is the mode a weapon was fired in, with the weapons \
                     underneath: Fire at Will III, then every weapon fired with it. \
                     Damage without a mode — a weapon's plain fire, a console — is \
                     grouped as in the other order, and a pet's modes stay under the pet.",
                )
                .clicked();
            ui.add_space(12.0);
            let sections = ui
                .steady_toggle(self.sectioned, "Sections")
                .hover(
                    "Split each player's rows into Weapons, Consoles and abilities, and \
                     Pets and summons, each with its own totals. A row in the wrong one \
                     can be moved with a right click.",
                )
                .clicked();
            if sections {
                self.sectioned = !self.sectioned;
            }
            // Only the ability-first order has the modes this row stands
            // beside; read by weapon, a weapon's plain fire is already next to
            // its modes.
            let mut gathered = false;
            if self.grouping == DamageGrouping::ByAbility {
                gathered = ui
                    .steady_toggle(self.normal_fire, NORMAL_WEAPON_FIRE)
                    .hover(
                        "Put every weapon shot fired in no bridge officer mode into one \
                         row, beside Fire at Will III and the other modes, with each \
                         weapon underneath. A pet's plain fire stays with the pet.",
                    )
                    .clicked();
                if gathered {
                    self.normal_fire = !self.normal_fire;
                }
            }
            regrouped | sections | gathered
        })
        .inner
    }

    pub fn show(&mut self, settings: &mut Settings, combat: Option<&Combat>, ui: &mut Ui) {
        let mut ticks_changed = false;
        let mut grouping_changed = false;
        let mut section_move = None;
        Splitter::horizontal()
            .initial_ratio(0.6)
            .ratio_bounds(0.1..=0.9)
            .show(ui, |top_ui, bottom_ui| {
                grouping_changed = self.show_grouping_picker(top_ui);
                let (table, excluded) = match self.grouping {
                    DamageGrouping::ByWeapon => (&mut self.table, &mut self.excluded),
                    DamageGrouping::ByAbility if self.normal_fire => {
                        (&mut self.table_by_ability, &mut self.excluded_normal_fire)
                    }
                    DamageGrouping::ByAbility => {
                        (&mut self.table_by_ability, &mut self.excluded_by_ability)
                    }
                };
                let columns = &settings.columns;
                let choices = &settings.sections;
                let is_moved = |row: &str| choices.moved(row).is_some();
                let mut ticks = RowTicks {
                    excluded,
                    hide_unticked: &mut self.hide_unticked,
                    types: &mut self.types,
                    all_types: &self.all_types,
                    sections: self.sectioned.then_some(RowSections {
                        is_moved: &is_moved,
                        request: None,
                    }),
                    changed: false,
                };
                table.show(
                    top_ui,
                    |column| columns.is_shown(TableKind::Damage, column),
                    &mut ticks,
                    |p| {
                        Self::process_diagram_change(
                            &mut self.dmg_selection_diagrams,
                            p,
                            self.filter,
                            self.diagram_time_slice,
                            self.activity_gap_s,
                            self.combat_duration_s,
                        );
                    },
                );

                ticks_changed = ticks.changed;
                section_move = ticks.sections.and_then(|sections| sections.request);
                self.show_diagrams(settings, bottom_ui);
            });

        // A moved row is written down at once, so it holds for the next fight
        // and the next start, and the table is split again to show it there.
        if let Some((row, section)) = section_move {
            settings.sections.set(&row, section);
            settings.save();
            ticks_changed = true;
        }

        // A tick changes what every player's row is of, so the table and the
        // chart are both built again from the rows that are left.
        // The other nesting has other rows: the chart is drawn again from the
        // ticks that nesting holds, and a selection made in the other table
        // no longer names anything on screen.
        if (ticks_changed || grouping_changed)
            && let Some(combat) = combat
        {
            self.rebuild(settings, combat);
        }
    }

    fn process_diagram_change(
        diagram: &mut Option<DamageDiagrams>,
        selection: TableSelectionEvent<DamageTablePartData>,
        filter: f64,
        damage_time_slice: f64,
        activity_gap_s: f64,
        combat_duration_s: f64,
    ) {
        match selection {
            TableSelectionEvent::Clear => *diagram = None,
            TableSelectionEvent::Group(part) => {
                *diagram = Some(Self::make_sub_parts_diagram_selection(
                    part,
                    filter,
                    damage_time_slice,
                    activity_gap_s,
                    combat_duration_s,
                ))
            }
            TableSelectionEvent::Single(part) => {
                *diagram = Some(Self::make_single_diagram_selection(
                    part,
                    filter,
                    damage_time_slice,
                    activity_gap_s,
                    combat_duration_s,
                ))
            }
            TableSelectionEvent::AddSingle(part) => match diagram.as_mut() {
                Some(diagram) => {
                    diagram.add_data(
                        Self::make_single_data_set(part, combat_duration_s),
                        filter,
                        damage_time_slice,
                        activity_gap_s,
                    );
                }
                None => {
                    *diagram = Some(Self::make_single_diagram_selection(
                        part,
                        filter,
                        damage_time_slice,
                        activity_gap_s,
                        combat_duration_s,
                    ))
                }
            },
            TableSelectionEvent::KeepGroupAndAdd(part) => {
                // The row the chart was opened on becomes one lane of its own,
                // beside the row just added.
                let kept = diagram.as_ref().and_then(DamageDiagrams::activity_whole);
                let added = Self::make_single_data_set(part, combat_duration_s);
                *diagram = Some(DamageDiagrams::from_data(
                    kept.into_iter().chain([added]),
                    filter,
                    damage_time_slice,
                    activity_gap_s,
                ));
            }
            TableSelectionEvent::Unselect(part) => {
                if let Some(diagram) = diagram.as_mut() {
                    diagram.remove_data(part);
                }
            }
        }
    }

    fn make_sub_parts_diagram_selection(
        part: &DamageTablePart,
        filter: f64,
        damage_time_slice: f64,
        activity_gap_s: f64,
        combat_duration_s: f64,
    ) -> DamageDiagrams {
        DamageDiagrams::from_data(
            part.sub_parts.iter().map(|p| {
                PreparedDamageDataSet::new(
                    &p.name,
                    // Each sub-part's own total: it is the sort key, and the
                    // parent's total made every one of them compare equal.
                    p.total_damage(),
                    p.source_hits.iter(),
                    combat_duration_s,
                )
                .with_active_base(p.data.active_base_s())
            }),
            filter,
            damage_time_slice,
            activity_gap_s,
        )
        .with_activity_whole(Self::make_single_data_set(part, combat_duration_s))
    }

    fn make_single_diagram_selection(
        part: &DamageTablePart,
        filter: f64,
        damage_time_slice: f64,
        activity_gap_s: f64,
        combat_duration_s: f64,
    ) -> DamageDiagrams {
        DamageDiagrams::from_data(
            [Self::make_single_data_set(part, combat_duration_s)].into_iter(),
            filter,
            damage_time_slice,
            activity_gap_s,
        )
    }

    fn make_single_data_set(
        part: &DamageTablePart,
        combat_duration_s: f64,
    ) -> PreparedDamageDataSet {
        PreparedDamageDataSet::new(
            &part.name,
            part.total_damage(),
            part.source_hits.iter(),
            combat_duration_s,
        )
        // So the chart's tooltip gives the share the Active column gives.
        .with_active_base(part.data.active_base_s())
    }

    fn update_diagrams(&mut self) {
        self.dmg_main_diagrams
            .update(self.filter, self.diagram_time_slice, self.activity_gap_s);
        if let Some(selection_plot) = &mut self.dmg_selection_diagrams {
            selection_plot.update(self.filter, self.diagram_time_slice, self.activity_gap_s);
        }
    }

    fn show_diagrams(&mut self, settings: &Settings, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.steady_toggle_value(
                &mut self.active_diagram,
                DiagramType::Dps,
                DiagramType::Dps.name(),
            )
            .hover(DiagramType::Dps.tooltip());
            ui.steady_toggle_value(
                &mut self.active_diagram,
                DiagramType::Damage,
                DiagramType::Damage.name(),
            )
            .hover(DiagramType::Damage.tooltip());
            ui.steady_toggle_value(
                &mut self.active_diagram,
                DiagramType::DamageResistance,
                DiagramType::DamageResistance.name(),
            )
            .hover(DiagramType::DamageResistance.tooltip());
            ui.steady_toggle_value(
                &mut self.active_diagram,
                DiagramType::HitsPerSecond,
                DiagramType::HitsPerSecond.name(),
            )
            .hover(DiagramType::HitsPerSecond.tooltip());
            ui.steady_toggle_value(
                &mut self.active_diagram,
                DiagramType::HitsCount,
                DiagramType::HitsCount.name(),
            )
            .hover(DiagramType::HitsCount.tooltip());
            ui.steady_toggle_value(
                &mut self.active_diagram,
                DiagramType::Activity,
                DiagramType::Activity.name(),
            )
            .hover(DiagramType::Activity.tooltip());
        });

        let updated_required = match self.active_diagram {
            DiagramType::Damage | DiagramType::DamageResistance | DiagramType::HitsCount => {
                show_time_slice_setting(&mut self.diagram_time_slice, ui)
            }
            DiagramType::Dps | DiagramType::HitsPerSecond => {
                show_time_filter_setting(&mut self.filter, self.combat_duration_s, ui)
            }
            DiagramType::Activity => {
                show_activity_gap_setting(&mut self.activity_gap_s, &mut self.activity_total, ui)
            }
            _ => unreachable!(),
        };

        if updated_required {
            self.update_diagrams();
            if self.active_diagram == DiagramType::Activity {
                self.update_active(settings);
            }
        }

        if let Some(selection_diagrams) = &mut self.dmg_selection_diagrams {
            selection_diagrams.show(settings, ui, self.active_diagram, self.activity_total);
        } else {
            self.dmg_main_diagrams
                .show(settings, ui, self.active_diagram, self.activity_total);
        }
    }
}
