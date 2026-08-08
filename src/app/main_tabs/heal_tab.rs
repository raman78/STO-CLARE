use eframe::egui::Ui;

use crate::{
    analyzer::*,
    app::settings::{Settings, TableKind},
    custom_widgets::splitter::Splitter,
};

use super::{common::*, diagrams::*, tables::*};
use crate::custom_widgets::toggle::Toggle;

pub struct HealTab {
    /// The same healing, nested both ways. Both are built up front so the
    /// grouping toggle is instant — the trees come from the analyzer, which
    /// would otherwise have to re-read the log to re-nest them. A tab without a
    /// grouping toggle leaves `table_by_person` empty; it can never be shown.
    table_by_person: HealTable,
    table_by_ability: HealTable,
    grouping: HealGrouping,
    main_diagrams: HealDiagrams,
    selection_diagrams: Option<HealDiagrams>,
    heal_pool: fn(&Player) -> &HealPool,
    /// What the non-ability level of the tree is called in this tab — the other
    /// party. `None` for self healing: there is nobody else involved, so the
    /// tree is only ever nested by ability and the toggle is left out.
    other_level: Option<&'static str>,
    filter: f64,
    diagram_time_slice: f64,
    active_diagram: DiagramType,
    /// The open combat's length, so a charted selection spans the whole fight
    /// rather than only the stretch where the selected ability was healing.
    combat_duration_s: f64,
    /// Which halves of a heal the chart adds up. Both on by default, which is
    /// the single total line the chart has always drawn.
    components: HealComponents,
}

impl HealTab {
    pub fn empty(heal_pool: fn(&Player) -> &HealPool, other_level: Option<&'static str>) -> Self {
        Self {
            table_by_person: HealTable::empty(),
            table_by_ability: HealTable::empty(),
            grouping: HealGrouping::ByAbility,
            heal_pool,
            other_level,
            main_diagrams: HealDiagrams::empty(),
            selection_diagrams: None,
            filter: 0.4,
            diagram_time_slice: 1.0,
            active_diagram: DiagramType::Hps,
            combat_duration_s: 0.0,
            components: HealComponents::default(),
        }
    }

    pub fn update(&mut self, settings: &Settings, combat: &Combat) {
        self.combat_duration_s = combat_duration_seconds(combat);
        let pool = self.heal_pool;
        self.table_by_person = match self.other_level {
            Some(_) => HealTable::new(settings, combat, move |p| &pool(p).by_person),
            None => HealTable::empty(),
        };
        self.table_by_ability = HealTable::new(settings, combat, move |p| &pool(p).by_ability);
        // Either nesting holds the same ticks, so the per-player chart is the
        // same; build it from one of them.
        self.main_diagrams = HealDiagrams::from_heal_groups(
            combat.players.values().map(move |p| &pool(p).by_person),
            combat,
            self.filter,
            self.diagram_time_slice,
            self.components,
        );
        self.selection_diagrams = None;
    }

    pub fn show(&mut self, settings: &Settings, ui: &mut Ui) {
        Splitter::horizontal()
            .initial_ratio(0.6)
            .ratio_bounds(0.1..=0.9)
            .show(ui, |top_ui, bottom_ui| {
                self.show_grouping_picker(top_ui);

                let table = match self.grouping {
                    HealGrouping::ByPerson => &mut self.table_by_person,
                    HealGrouping::ByAbility => &mut self.table_by_ability,
                };
                let combat_duration_s = self.combat_duration_s;
                let components = self.components;
                let columns = &settings.columns;
                table.show(
                    top_ui,
                    |column| columns.is_shown(TableKind::Heal, column),
                    |p| {
                        Self::process_diagram_change(
                            &mut self.selection_diagrams,
                            p,
                            self.filter,
                            self.diagram_time_slice,
                            combat_duration_s,
                            components,
                        );
                    },
                );

                self.show_diagrams(settings, bottom_ui);
            });
    }

    /// Switches how the tree is nested. Both nestings are already built, so this
    /// only picks which one to draw — no rebuild, no lost data. Tabs without a
    /// second level to group by (self healing) show nothing here.
    /// Which nesting this tab is showing, for the export.
    pub fn grouping(&self) -> HealGrouping {
        self.grouping
    }

    fn show_grouping_picker(&mut self, ui: &mut Ui) {
        let Some(other_level) = self.other_level else {
            return;
        };
        ui.horizontal(|ui| {
            ui.label("Group by:");
            // "⏵" is the same glyph the tree rows use for their expander, so it
            // is known to exist in the bundled font — "→" renders as a blank box.
            let changed = ui
                .steady_toggle_value(
                    &mut self.grouping,
                    HealGrouping::ByPerson,
                    format!("{} ⏵ Ability", other_level),
                )
                .on_hover_text(format!(
                    "Top level is the {}, with the abilities underneath.",
                    other_level.to_lowercase()
                ))
                .clicked()
                | ui.steady_toggle_value(
                    &mut self.grouping,
                    HealGrouping::ByAbility,
                    format!("Ability ⏵ {}", other_level),
                )
                .on_hover_text(format!(
                    "Top level is the ability, with the {} underneath.",
                    other_level.to_lowercase()
                ))
                .clicked();
            // The two tables track their own expanded rows and selection, so a
            // chart built from the old one no longer matches what is on screen.
            if changed {
                self.selection_diagrams = None;
            }
        });
    }

    fn process_diagram_change(
        diagram: &mut Option<HealDiagrams>,
        selection: TableSelectionEvent<HealTablePartData>,
        filter: f64,
        heal_time_slice: f64,
        combat_duration_s: f64,
        components: HealComponents,
    ) {
        match selection {
            TableSelectionEvent::Clear => *diagram = None,
            TableSelectionEvent::Group(part) => {
                *diagram = Some(Self::make_sub_parts_diagram_selection(
                    part,
                    filter,
                    heal_time_slice,
                    combat_duration_s,
                    components,
                ))
            }
            TableSelectionEvent::Single(part) => {
                *diagram = Some(Self::make_single_diagram_selection(
                    part,
                    filter,
                    heal_time_slice,
                    combat_duration_s,
                    components,
                ))
            }
            TableSelectionEvent::AddSingle(part) => match diagram.as_mut() {
                Some(diagram) => {
                    diagram.add_data(
                        Self::make_single_data_set(part, combat_duration_s),
                        filter,
                        heal_time_slice,
                    );
                }
                None => {
                    *diagram = Some(Self::make_single_diagram_selection(
                        part,
                        filter,
                        heal_time_slice,
                        combat_duration_s,
                        components,
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
        part: &HealTablePart,
        filter: f64,
        heal_time_slice: f64,
        combat_duration_s: f64,
        components: HealComponents,
    ) -> HealDiagrams {
        HealDiagrams::from_data(
            part.sub_parts.iter().map(|p| {
                PreparedHealDataSet::new(
                    &p.name,
                    // Each sub-part's own total: it is the sort key, and the
                    // parent's total made every one of them compare equal.
                    p.total_heal(),
                    p.source_ticks.iter(),
                    combat_duration_s,
                )
            }),
            filter,
            heal_time_slice,
            components,
        )
    }

    fn make_single_diagram_selection(
        part: &HealTablePart,
        filter: f64,
        heal_time_slice: f64,
        combat_duration_s: f64,
        components: HealComponents,
    ) -> HealDiagrams {
        HealDiagrams::from_data(
            [Self::make_single_data_set(part, combat_duration_s)].into_iter(),
            filter,
            heal_time_slice,
            components,
        )
    }

    fn make_single_data_set(part: &HealTablePart, combat_duration_s: f64) -> PreparedHealDataSet {
        PreparedHealDataSet::new(
            &part.name,
            part.total_heal(),
            part.source_ticks.iter(),
            combat_duration_s,
        )
    }

    fn update_diagrams(&mut self) {
        self.main_diagrams
            .update(self.filter, self.diagram_time_slice, self.components);
        if let Some(selection_plot) = &mut self.selection_diagrams {
            selection_plot.update(self.filter, self.diagram_time_slice, self.components);
        }
    }

    /// Hull and shield healing on their own or added together. Both on draws
    /// the plain total, which is what the chart shows unless it is changed.
    /// Turning both off would draw nothing, so the last one on stays on.
    ///
    /// Drawn as two toggle buttons that stay lit while on, the same shape as
    /// the diagram and tab switches above them, rather than as check boxes.
    fn show_component_picker(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Show:");
            let mut hull = self.components.hull;
            let mut shield = self.components.shield;
            changed |= ui
                .toggle_value(&mut hull, "Hull")
                .on_hover_text("Include healing that restored hull.")
                .changed();
            changed |= ui
                .toggle_value(&mut shield, "Shield")
                .on_hover_text("Include healing that restored shields.")
                .changed();
            if !hull && !shield {
                // Whichever the user just switched off is the one to keep.
                if !self.components.hull {
                    hull = true;
                } else {
                    shield = true;
                }
            }
            self.components = HealComponents { hull, shield };
        });
        changed
    }

    fn show_diagrams(&mut self, settings: &Settings, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.steady_toggle_value(
                &mut self.active_diagram,
                DiagramType::Hps,
                DiagramType::Hps.name(),
            )
            .on_hover_text(DiagramType::Hps.tooltip());
            ui.steady_toggle_value(
                &mut self.active_diagram,
                DiagramType::Heal,
                DiagramType::Heal.name(),
            )
            .on_hover_text(DiagramType::Heal.tooltip());
            ui.steady_toggle_value(
                &mut self.active_diagram,
                DiagramType::HealTicksPerSecond,
                DiagramType::HealTicksPerSecond.name(),
            )
            .on_hover_text(DiagramType::HealTicksPerSecond.tooltip());
            ui.steady_toggle_value(
                &mut self.active_diagram,
                DiagramType::HealTicksCount,
                DiagramType::HealTicksCount.name(),
            )
            .on_hover_text(DiagramType::HealTicksCount.tooltip());
        });

        let mut update_required = self.show_component_picker(ui);
        update_required |= match self.active_diagram {
            DiagramType::Heal | DiagramType::HealTicksCount => {
                show_time_slice_setting(&mut self.diagram_time_slice, ui)
            }
            DiagramType::Hps | DiagramType::HealTicksPerSecond => {
                show_time_filter_setting(&mut self.filter, self.combat_duration_s, ui)
            }
            _ => unreachable!(),
        };

        if update_required {
            self.update_diagrams();
        }

        if let Some(selection_diagrams) = &mut self.selection_diagrams {
            selection_diagrams.show(settings, ui, self.active_diagram);
        } else {
            self.main_diagrams.show(settings, ui, self.active_diagram);
        }
    }
}
