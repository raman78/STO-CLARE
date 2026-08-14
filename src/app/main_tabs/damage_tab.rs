use std::borrow::Cow;

use eframe::egui::*;
use rustc_hash::FxHashSet;

use crate::{
    analyzer::*,
    app::{
        damage_subset,
        settings::{Settings, TableKind},
    },
    custom_widgets::splitter::Splitter,
};

use super::{common::*, diagrams::*, tables::*};
use crate::custom_widgets::toggle::Toggle;
use crate::custom_widgets::tooltip::CloseTooltip;

pub struct DamageTab {
    table: DamageTable,
    dmg_main_diagrams: DamageDiagrams,
    dmg_selection_diagrams: Option<DamageDiagrams>,
    damage_group: for<'a> fn(&'a Player) -> &'a DamageGroup,
    /// The rows the reader has ticked off, by name. They are left out of every
    /// player's figures, and — with `hide_unticked` — off the screen as well.
    excluded: FxHashSet<String>,
    hide_unticked: bool,
    /// The damage types the figures are of, empty for all of them, and every
    /// type this combat holds.
    types: FxHashSet<String>,
    all_types: Vec<String>,
    filter: f64,
    diagram_time_slice: f64,
    active_diagram: DiagramType,
    /// The open combat's length, so a charted selection spans the whole fight
    /// rather than only the part where the selected ability was firing.
    combat_duration_s: f64,
}

impl DamageTab {
    pub fn empty(damage_group: fn(&Player) -> &DamageGroup) -> Self {
        Self {
            table: DamageTable::empty(),
            dmg_main_diagrams: DamageDiagrams::empty(),
            damage_group,
            excluded: Default::default(),
            hide_unticked: false,
            types: Default::default(),
            all_types: Vec::new(),
            filter: 0.4,
            diagram_time_slice: 1.0,
            dmg_selection_diagrams: None,
            active_diagram: DiagramType::Dps,
            combat_duration_s: 0.0,
        }
    }

    pub fn update(&mut self, settings: &Settings, combat: &Combat) {
        self.combat_duration_s = combat_duration_seconds(combat);
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
            .map(|player| self.kept_damage(player, combat).into_owned())
            .collect();
        self.dmg_main_diagrams = DamageDiagrams::from_damage_groups(
            kept.iter(),
            combat,
            self.filter,
            self.diagram_time_slice,
        );
        self.dmg_selection_diagrams = None;
    }

    /// One player's damage with the unticked rows left out, or the whole of it
    /// when nothing is ticked off.
    fn kept_damage<'a>(&self, player: &'a Player, combat: &Combat) -> Cow<'a, DamageGroup> {
        let whole = (self.damage_group)(player);
        if self.types.is_empty() && self.excluded.is_empty() {
            return Cow::Borrowed(whole);
        }
        // The type first — it decides what the rows even are — and the ticks
        // over what it left.
        let of_types = if self.types.is_empty() {
            None
        } else {
            damage_subset::damage_of_types(
                player,
                whole,
                &combat.name_manager,
                &combat.hits_manger,
                &self.types,
                &combat.total_damage_out,
            )
        };
        let narrowed = of_types.as_ref().unwrap_or(whole);
        if self.excluded.is_empty() {
            return match of_types {
                Some(kept) => Cow::Owned(kept),
                None => Cow::Borrowed(whole),
            };
        }
        Cow::Owned(damage_subset::player_damage_without(
            player,
            narrowed,
            &combat.name_manager,
            &combat.hits_manger,
            &self.excluded,
            &combat.total_damage_out,
        ))
    }

    fn rebuild_table(&mut self, settings: &Settings, combat: &Combat) {
        // Both filters go through one place, so the table and the chart under
        // it are always of the same rows.
        let table = DamageTable::new(settings, combat, |player| self.kept_damage(player, combat));
        self.table = table;
    }

    pub fn show(&mut self, settings: &Settings, combat: Option<&Combat>, ui: &mut Ui) {
        let mut ticks_changed = false;
        Splitter::horizontal()
            .initial_ratio(0.6)
            .ratio_bounds(0.1..=0.9)
            .show(ui, |top_ui, bottom_ui| {
                let columns = &settings.columns;
                let mut ticks = RowTicks {
                    excluded: &mut self.excluded,
                    hide_unticked: &mut self.hide_unticked,
                    types: &mut self.types,
                    all_types: &self.all_types,
                    changed: false,
                };
                self.table.show(
                    top_ui,
                    |column| columns.is_shown(TableKind::Damage, column),
                    &mut ticks,
                    |p| {
                        Self::process_diagram_change(
                            &mut self.dmg_selection_diagrams,
                            p,
                            self.filter,
                            self.diagram_time_slice,
                            self.combat_duration_s,
                        );
                    },
                );

                ticks_changed = ticks.changed;
                self.show_diagrams(settings, bottom_ui);
            });

        // A tick changes what every player's row is of, so the table and the
        // chart are both built again from the rows that are left.
        if ticks_changed && let Some(combat) = combat {
            self.rebuild(settings, combat);
        }
    }

    fn process_diagram_change(
        diagram: &mut Option<DamageDiagrams>,
        selection: TableSelectionEvent<DamageTablePartData>,
        filter: f64,
        damage_time_slice: f64,
        combat_duration_s: f64,
    ) {
        match selection {
            TableSelectionEvent::Clear => *diagram = None,
            TableSelectionEvent::Group(part) => {
                *diagram = Some(Self::make_sub_parts_diagram_selection(
                    part,
                    filter,
                    damage_time_slice,
                    combat_duration_s,
                ))
            }
            TableSelectionEvent::Single(part) => {
                *diagram = Some(Self::make_single_diagram_selection(
                    part,
                    filter,
                    damage_time_slice,
                    combat_duration_s,
                ))
            }
            TableSelectionEvent::AddSingle(part) => match diagram.as_mut() {
                Some(diagram) => {
                    diagram.add_data(
                        Self::make_single_data_set(part, combat_duration_s),
                        filter,
                        damage_time_slice,
                    );
                }
                None => {
                    *diagram = Some(Self::make_single_diagram_selection(
                        part,
                        filter,
                        damage_time_slice,
                        combat_duration_s,
                    ))
                }
            },
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
            }),
            filter,
            damage_time_slice,
        )
    }

    fn make_single_diagram_selection(
        part: &DamageTablePart,
        filter: f64,
        damage_time_slice: f64,
        combat_duration_s: f64,
    ) -> DamageDiagrams {
        DamageDiagrams::from_data(
            [Self::make_single_data_set(part, combat_duration_s)].into_iter(),
            filter,
            damage_time_slice,
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
    }

    fn update_diagrams(&mut self) {
        self.dmg_main_diagrams
            .update(self.filter, self.diagram_time_slice);
        if let Some(selection_plot) = &mut self.dmg_selection_diagrams {
            selection_plot.update(self.filter, self.diagram_time_slice);
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
        });

        let updated_required = match self.active_diagram {
            DiagramType::Damage | DiagramType::DamageResistance | DiagramType::HitsCount => {
                show_time_slice_setting(&mut self.diagram_time_slice, ui)
            }
            DiagramType::Dps | DiagramType::HitsPerSecond => {
                show_time_filter_setting(&mut self.filter, self.combat_duration_s, ui)
            }
            _ => unreachable!(),
        };

        if updated_required {
            self.update_diagrams();
        }

        if let Some(selection_diagrams) = &mut self.dmg_selection_diagrams {
            selection_diagrams.show(settings, ui, self.active_diagram);
        } else {
            self.dmg_main_diagrams
                .show(settings, ui, self.active_diagram);
        }
    }
}
