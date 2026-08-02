use std::borrow::BorrowMut;

use eframe::egui::*;

use super::Settings;
use crate::analyzer::{Combat, curated_map_identifiers, curated_map_names};
use crate::app::theme;
use crate::custom_widgets::table::Table;
use crate::unwrap_or_return;
use crate::{analyzer::settings::*, custom_widgets::popup_button::PopupButton};

const HEADER_HEIGHT: f32 = 15.0;
const ROW_HEIGHT: f32 = 25.0;

#[derive(Default)]
pub struct AnalysisTab {
    list_selected_combat_occurred_names: bool,
    occurred_combat_names_search_term: String,
    selected_section: AnalysisSection,
    indirect_source_reversal_rules: IndirectSourceReversalRules,
    custom_grouping_rules: CustomGroupingRules,
    damage_out_exclusion_rules: DamageOutExclusionRules,
    combat_names_rules: CombatNameRules,
}

/// The Analysis tab holds four independent rule sets. Stacking them made each
/// table compete for height inside one scroll area; as sub-tabs only one is on
/// screen at a time, so it can use the window's full height.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum AnalysisSection {
    #[default]
    CombatNames,
    SourceReversal,
    CustomGrouping,
    DamageExclusion,
}

#[derive(Default)]
struct IndirectSourceReversalRules {
    selected: Option<usize>,
}

#[derive(Default)]
struct CustomGroupingRules {
    selected_group: Option<usize>,
    selected_rule: Option<usize>,
}

#[derive(Default)]
struct DamageOutExclusionRules {
    selected: Option<usize>,
}

#[derive(Default)]
struct CombatNameRules {
    selected_group: Option<usize>,
    selected_rule: Option<usize>,
    selected_additional_info_group: Option<usize>,
    selected_additional_info_rule: Option<usize>,
}

struct GroupRulesTable<'a, T: BorrowMut<RulesGroup> + Default + Clone> {
    group_rules: &'a mut Vec<T>,
    title: &'a str,
    name_header: &'a str,
    selected_group: &'a mut Option<usize>,
    popup_extra_space: f32,
    /// Optional per-row warning: returns a tooltip when the row's rule should be
    /// flagged (e.g. it shadows an auto-detected map). Adds a ⚠ cell per row.
    row_warning: Option<&'a dyn Fn(&RulesGroup) -> Option<String>>,
    /// Explicit height cap; falls back to all available space.
    max_height: Option<f32>,
}

struct RulesTable<'a> {
    rules: &'a mut Vec<MatchRule>,
    title: &'a str,
    match_aspect_set: &'a [MatchAspect],
    selected_rule: &'a mut Option<usize>,
}

impl AnalysisTab {
    pub fn show(
        &mut self,
        modified_settings: &mut Settings,
        selected_combat: Option<&Combat>,
        ui: &mut Ui,
    ) {
        if ui
            .add_enabled(
                selected_combat.is_some(),
                Button::new("List Selected Combat Occurred Names"),
            )
            .clicked()
        {
            self.list_selected_combat_occurred_names = true;
        }

        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            use AnalysisSection::*;
            for (section, label) in [
                (CombatNames, "Combat Names"),
                (SourceReversal, "Source Reversal"),
                (CustomGrouping, "Custom Grouping"),
                (DamageExclusion, "Damage Exclusion"),
            ] {
                ui.selectable_value(&mut self.selected_section, section, label);
            }
        });
        ui.separator();

        match self.selected_section {
            AnalysisSection::CombatNames => self
                .combat_names_rules
                .show(&mut modified_settings.analysis, ui),
            AnalysisSection::SourceReversal => self
                .indirect_source_reversal_rules
                .show(&mut modified_settings.analysis, ui),
            AnalysisSection::CustomGrouping => {
                ui.push_id(line!(), |ui| {
                    self.custom_grouping_rules
                        .show(&mut modified_settings.analysis, ui);
                });
            }
            AnalysisSection::DamageExclusion => self
                .damage_out_exclusion_rules
                .show(&mut modified_settings.analysis, ui),
        }

        self.show_occurred_names_window(selected_combat, ui);
    }

    fn show_occurred_names_window(&mut self, selected_combat: Option<&Combat>, ui: &mut Ui) {
        let combat = unwrap_or_return!(selected_combat);
        if !self.list_selected_combat_occurred_names {
            return;
        }

        Window::new("Selected Combat Occurred Names")
            .collapsible(false)
            .open(&mut self.list_selected_combat_occurred_names)
            .scroll(true)
            .constrain(true)
            .show(ui.ctx(), |ui| {
                const SPACE: f32 = 40.0;

                ui.label("This window is intended to help with creating combat naming rules.");

                ui.horizontal(|ui| {
                    ui.label("Search");
                    ui.text_edit_singleline(&mut self.occurred_combat_names_search_term);
                });

                ui.add_space(SPACE);

                Self::show_occurred_names_table(
                    ui,
                    "Source or Target Name",
                    &self.occurred_combat_names_search_term,
                    combat.name_manager.source_targets(),
                );

                ui.add_space(SPACE);

                Self::show_occurred_names_table(
                    ui,
                    "Source or Target Unique Name",
                    &self.occurred_combat_names_search_term,
                    combat.name_manager.source_targets_unique(),
                );

                ui.add_space(SPACE);

                Self::show_occurred_names_table(
                    ui,
                    "Indirect Source Name",
                    &self.occurred_combat_names_search_term,
                    combat.name_manager.indirect_sources(),
                );

                ui.add_space(SPACE);

                Self::show_occurred_names_table(
                    ui,
                    "Indirect Source Unique Name",
                    &self.occurred_combat_names_search_term,
                    combat.name_manager.source_targets_unique(),
                );

                ui.add_space(SPACE);

                Self::show_occurred_names_table(
                    ui,
                    "Damage / Heal Name",
                    &self.occurred_combat_names_search_term,
                    combat.name_manager.values(),
                );
            });
    }

    fn show_occurred_names_table<'a>(
        ui: &mut Ui,
        title: &str,
        filter: &str,
        names: impl Iterator<Item = &'a str>,
    ) {
        ui.push_id(title, |ui| {
            Table::new(ui)
                .min_scroll_height(300.0)
                .max_scroll_height(300.0)
                .header(HEADER_HEIGHT, |r| {
                    r.cell(|ui| {
                        ui.label(title);
                    });
                })
                .body(ROW_HEIGHT, |b| {
                    for name in names.filter(|n| {
                        filter.is_empty() || n.to_lowercase().contains(&filter.to_lowercase())
                    }) {
                        b.row(|r| {
                            r.cell(|ui| {
                                ui.label(name);
                            });
                            r.cell(|ui| {
                                if ui.button("🗐").on_hover_text("Copy").clicked() {
                                    ui.ctx().copy_text(name.to_string());
                                }
                            });
                        });
                    }
                });
        });
    }
}

impl IndirectSourceReversalRules {
    fn show(&mut self, modified_settings: &mut AnalysisSettings, ui: &mut Ui) {
        RulesTable::new(
            &mut modified_settings.indirect_source_grouping_revers_rules,
            "Indirect Source Grouping Reversal Rules\n(e.g. pets, anomalies, certain traits etc.)",
            &[
                MatchAspect::DamageOrHealName,
                MatchAspect::IndirectSourceName,
                MatchAspect::IndirectUniqueSourceName,
            ],
            &mut self.selected,
        )
        .show(ui);
    }
}

impl DamageOutExclusionRules {
    fn show(&mut self, modified_settings: &mut AnalysisSettings, ui: &mut Ui) {
        RulesTable::new(
            &mut modified_settings.damage_out_exclusion_rules,
            "Damage Out Exclusion Rules",
            &[
                MatchAspect::DamageOrHealName,
                MatchAspect::IndirectSourceName,
                MatchAspect::IndirectUniqueSourceName,
                MatchAspect::SourceOrTargetName,
                MatchAspect::SourceOrTargetUniqueName,
            ],
            &mut self.selected,
        )
        .show(ui);
    }
}

impl CustomGroupingRules {
    fn show(&mut self, modified_settings: &mut AnalysisSettings, ui: &mut Ui) {
        GroupRulesTable::new(
            &mut modified_settings.custom_group_rules,
            "Custom Grouping Rules",
            "Group Name",
            &mut self.selected_group,
            100.0,
        )
        .show(ui, |r, ui| {
            RulesTable::new(
                &mut r.rules,
                &r.name,
                &[
                    MatchAspect::DamageOrHealName,
                    MatchAspect::IndirectSourceName,
                    MatchAspect::IndirectUniqueSourceName,
                ],
                &mut self.selected_rule,
            )
            .show(ui);
        });
    }
}

impl CombatNameRules {
    fn show(&mut self, modified_settings: &mut AnalysisSettings, ui: &mut Ui) {
        // Flag rules that shadow an auto-detected map, as a ⚠ on the rule's row.
        let identifiers = curated_map_identifiers();
        let row_warning = |group: &RulesGroup| {
            let maps = Self::overlapping_maps(group, &identifiers);
            (!maps.is_empty()).then(|| {
                format!(
                    "This rule overlaps the auto-detected map(s): {}. \
                     Your rule takes priority over the detected name.",
                    maps.join(", ")
                )
            })
        };

        {
            // The rules table and the auto-detected map list below it share the
            // window. Each may take up to half; whichever needs less than its
            // half gives the remainder to the other, so neither is squeezed while
            // the other shows empty space.
            let text_row = ui.text_style_height(&TextStyle::Body) + ui.spacing().item_spacing.y;
            let available = ui.available_height();
            let half = available / 2.0;
            // What each would use if unconstrained.
            let rules_need = HEADER_HEIGHT
                + ROW_HEIGHT * modified_settings.combat_name_rules.len() as f32
                + text_row * 2.0;
            let maps_need = text_row * (curated_map_names().len() as f32 + 4.0);
            let (rules_height, maps_height) = if rules_need <= half {
                (rules_need, available - rules_need)
            } else if maps_need <= half {
                (available - maps_need, maps_need)
            } else {
                (half, half)
            };
            GroupRulesTable::new(
                &mut modified_settings.combat_name_rules,
                "Combat Name Detection Rules",
                "Combat Name",
                &mut self.selected_group,
                200.0,
            )
            .with_max_height(rules_height)
            .with_row_warning(&row_warning)
            .show(ui, |r, ui| {
                RulesTable::new(
                    &mut r.name_rule.rules,
                    "combat name",
                    &[
                        MatchAspect::DamageOrHealName,
                        MatchAspect::IndirectSourceName,
                        MatchAspect::IndirectUniqueSourceName,
                        MatchAspect::SourceOrTargetName,
                        MatchAspect::SourceOrTargetUniqueName,
                    ],
                    &mut self.selected_rule,
                )
                .show(ui);

                ui.push_id("additional info rules", |ui| {
                    GroupRulesTable::new(
                        &mut r.additional_info_rules,
                        "additional infos rules (difficulty is detected automatically — don't add it here)",
                        "Info",
                        &mut self.selected_additional_info_group,
                        200.0,
                    )
                    .show(ui, |r, ui| {
                        RulesTable::new(
                            &mut r.rules,
                            &r.name,
                            &[
                                MatchAspect::DamageOrHealName,
                                MatchAspect::IndirectSourceName,
                                MatchAspect::IndirectUniqueSourceName,
                                MatchAspect::SourceOrTargetName,
                                MatchAspect::SourceOrTargetUniqueName,
                            ],
                            &mut self.selected_additional_info_rule,
                        )
                        .show(ui);
                    });
                });
            });

            Self::show_auto_detected(maps_height, ui);
        }
    }

    /// Read-only view of the maps the analyzer auto-detects. These act as a
    /// lower-priority layer below the rules above: a combat that no rule names
    /// falls back to its detected map (with difficulty). Individual rules that
    /// shadow a detected map are flagged with a ⚠ on their row (see
    /// `overlapping_maps`); this section additionally notes it for the selected
    /// combat and lists the detectable maps.
    fn show_auto_detected(max_height: f32, ui: &mut Ui) {
        // Legend for the per-row ⚠, directly under the rules frame above.
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.colored_label(theme::palette().warn, "⚠");
            ui.label(
                RichText::new(
                    "= this rule overlaps an auto-detected map (your rule takes priority \
                     over the detected name).",
                )
                .weak(),
            );
        });

        ui.add_space(10.0);
        ui.separator();
        ui.label(
            RichText::new("Auto-detected maps — used only when no rule above matches.").weak(),
        );

        ui.add_space(6.0);
        ui.label(RichText::new("Auto-detected maps:").weak());
        let row = ui.text_style_height(&TextStyle::Body) + ui.spacing().item_spacing.y;
        ScrollArea::vertical()
            .id_salt("auto detected maps")
            .max_height(ui.available_height().min(max_height).at_least(row * 4.0))
            .show(ui, |ui| {
                for map in curated_map_names() {
                    ui.label(RichText::new(map).weak());
                }
            });
    }

    /// The curated maps a single rule overlaps, either way:
    /// - **entity**: the rule matches the map's identifying NPC (unique name), or
    /// - **name**: the map's name *appears in* the rule's own name, ignoring the
    ///   `[TFO]`/`[Patrol]` category prefix on either side (e.g. a rule named
    ///   "Trouble Over Terrh" vs the "[Patrol] Trouble Over Terrh" map).
    ///
    /// The name check is containment rather than equality on purpose: users
    /// annotate their rules (e.g. "[Patrol] The Ninth Rule [M]"), and an exact
    /// comparison silently dropped the warning for every such rule. No curated
    /// map name is a substring of another, so containment adds no ambiguity.
    ///
    /// Sorted and deduped; empty when none or when the rule is disabled.
    fn overlapping_maps(group: &RulesGroup, identifiers: &[(String, String)]) -> Vec<String> {
        if !group.enabled {
            return Vec::new();
        }
        let rule_name = strip_category_prefix(&group.name).to_lowercase();
        let mut maps: Vec<String> = identifiers
            .iter()
            .filter(|(unique_name, map)| {
                group.matches_source_or_target_unique_names(std::iter::once(unique_name.as_str()))
                    || group
                        .matches_indirect_source_unique_names(std::iter::once(unique_name.as_str()))
                    || (!rule_name.is_empty()
                        && rule_name.contains(&strip_category_prefix(map).to_lowercase()))
            })
            .map(|(_, map)| map.clone())
            .collect();
        maps.sort();
        maps.dedup();
        maps
    }
}

/// Strip a leading `[category] ` prefix (e.g. `[TFO] `, `[Patrol] `) from a map
/// or rule name, so names can be compared regardless of the category prefix.
fn strip_category_prefix(name: &str) -> &str {
    name.strip_prefix('[')
        .and_then(|rest| rest.split_once(']'))
        .map(|(_, after)| after.trim_start())
        .unwrap_or(name)
}

impl<'a, T: BorrowMut<RulesGroup> + Default + Clone> GroupRulesTable<'a, T> {
    fn new(
        group_rules: &'a mut Vec<T>,
        title: &'a str,
        name_header: &'a str,
        selected_group: &'a mut Option<usize>,
        popup_extra_space: f32,
    ) -> Self {
        Self {
            group_rules,
            title,
            name_header,
            selected_group,
            popup_extra_space,
            row_warning: None,
            max_height: None,
        }
    }

    /// Show a ⚠ on the right of each row for which `warning` returns a tooltip.
    /// Cap the table at `height`. `None` lets it use whatever is available.
    fn with_max_height(mut self, height: f32) -> Self {
        self.max_height = Some(height);
        self
    }

    fn with_row_warning(mut self, warning: &'a dyn Fn(&RulesGroup) -> Option<String>) -> Self {
        self.row_warning = Some(warning);
        self
    }

    fn show(&mut self, ui: &mut Ui, mut edit: impl FnMut(&mut T, &mut Ui)) {
        let row_warning = self.row_warning;
        ui.horizontal(|ui| {
            ui.strong(self.title);
            if ui.button("Add ✚").clicked() {
                self.group_rules.push(Default::default());
            }

            show_move_up_down(self.selected_group, self.group_rules, ui);
        });
        // Fills whatever height the window offers, minus whatever the caller
        // reserved for the content below it. The Settings window scrolls as a
        // whole, so a short list still takes only the room it needs.
        let height = self
            .max_height
            .unwrap_or_else(|| ui.available_height())
            .at_least(ROW_HEIGHT * 4.0);
        Table::new(ui)
            .min_scroll_height(0.0)
            .max_scroll_height(height)
            .cell_spacing(10.0)
            .header(HEADER_HEIGHT, |r| {
                r.cell(|ui| {
                    ui.label("On");
                });
                r.cell(|ui| {
                    ui.label("Edit");
                });
                r.cell(|ui| {
                    ui.label("Clone");
                });
                r.cell(|ui| {
                    ui.label(self.name_header);
                });
            })
            .body(ROW_HEIGHT, |t| {
                let mut to_remove = Vec::new();
                // At most one row can be cloned per frame, so the index stays
                // valid: removals are applied first, then this is bounds-checked.
                let mut to_clone: Option<usize> = None;
                for (id, rule) in self.group_rules.iter_mut().enumerate() {
                    let row_response = t.selectable_row(*self.selected_group == Some(id), |r| {
                        r.cell(|ui| {
                            ui.checkbox(&mut rule.borrow_mut().enabled, "");
                        });

                        r.cell(|ui| {
                            PopupButton::new("✏").show(ui, |ui| {
                                edit(rule, ui);
                                // HACK: so that the popup does not close when clicking the in one of the combo boxes
                                ui.add_space(self.popup_extra_space);
                            });
                        });

                        r.cell(|ui| {
                            // A framed button, to match the ✏ next to it.
                            if ui.button("🗐").on_hover_text("Clone this rule").clicked() {
                                to_clone = Some(id);
                            }
                        });

                        r.cell(|ui| {
                            TextEdit::singleline(&mut rule.borrow_mut().name)
                                .clip_text(false)
                                .show(ui);
                        });

                        if let Some(row_warning) = row_warning {
                            r.cell(|ui| match row_warning(rule.borrow()) {
                                Some(tooltip) => {
                                    ui.colored_label(theme::palette().warn, "⚠")
                                        .on_hover_text(tooltip);
                                }
                                // Keep the column width constant whether or not a
                                // warning shows, so toggling rules doesn't shift the row.
                                None => {
                                    ui.colored_label(Color32::TRANSPARENT, "⚠");
                                }
                            });
                        }

                        r.cell(|ui| {
                            if ui.selectable_label(false, "🗑").clicked() {
                                to_remove.push(id);
                            }
                        });
                    });

                    if row_response.clicked() {
                        *self.selected_group = Some(id);
                    }
                }

                to_remove.into_iter().rev().for_each(|i| {
                    self.group_rules.remove(i);
                });

                // The clone goes to the end of the list and becomes the
                // selection, so it can be renamed straight away without the rows
                // around it shifting.
                if let Some(index) = to_clone.filter(|i| *i < self.group_rules.len()) {
                    let clone = self.group_rules[index].clone();
                    self.group_rules.push(clone);
                    *self.selected_group = Some(self.group_rules.len() - 1);
                }
            });
    }
}

impl<'a> RulesTable<'a> {
    fn new(
        rules: &'a mut Vec<MatchRule>,
        title: &'a str,
        match_aspect_set: &'a [MatchAspect],
        selected_rule: &'a mut Option<usize>,
    ) -> Self {
        Self {
            rules,
            title,
            match_aspect_set,
            selected_rule,
        }
    }

    fn show(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(self.title);
            if ui.button("Add ✚").clicked() {
                self.rules.push(Default::default());
            }

            show_move_up_down(self.selected_rule, self.rules, ui);
        });
        ui.push_id(self.title, |ui| {
            let height = ui.available_height().at_least(ROW_HEIGHT * 4.0);
            Table::new(ui)
                .min_scroll_height(0.0)
                .max_scroll_height(height)
                .cell_spacing(10.0)
                .header(HEADER_HEIGHT, |r| {
                    r.cell(|ui| {
                        ui.label("On");
                    });
                    r.cell(|ui| {
                        ui.label("Clone");
                    });
                    r.cell(|ui| {
                        ui.label("Aspect to match");
                    });
                    r.cell(|ui| {
                        ui.label("Match Method");
                    });
                    r.cell(|ui| {
                        ui.label("Text to match");
                    });
                })
                .body(ROW_HEIGHT, |t| {
                    let mut to_remove = Vec::new();
                    // One clone per frame; see GroupRulesTable for the reasoning.
                    let mut to_clone: Option<usize> = None;
                    for (id, rule) in self.rules.iter_mut().enumerate() {
                        let row_response = t.selectable_row(*self.selected_rule == Some(id), |r| {
                            r.cell(|ui| {
                                ui.checkbox(&mut rule.enabled, "");
                            });

                            r.cell(|ui| {
                                if ui
                                    .button("🗐")
                                    .on_hover_text("Clone this condition")
                                    .clicked()
                                {
                                    to_clone = Some(id);
                                }
                            });

                            r.cell(|ui| {
                                ComboBox::from_id_salt(id + 9387465)
                                    .selected_text(rule.aspect.display())
                                    .width(150.0)
                                    .show_ui(ui, |ui| {
                                        self.match_aspect_set.iter().for_each(|a| {
                                            ui.selectable_value(&mut rule.aspect, *a, a.display());
                                        });
                                    });
                            });

                            r.cell(|ui| {
                                ComboBox::from_id_salt(id + 394857)
                                    .selected_text(rule.method.display())
                                    .width(150.0)
                                    .show_ui(ui, |ui| {
                                        [
                                            MatchMethod::Equals,
                                            MatchMethod::StartsWith,
                                            MatchMethod::EndsWith,
                                            MatchMethod::Contains,
                                        ]
                                        .into_iter()
                                        .for_each(|m| {
                                            ui.selectable_value(&mut rule.method, m, m.display());
                                        });
                                    });
                            });

                            r.cell(|ui| {
                                TextEdit::singleline(&mut rule.expression)
                                    .clip_text(false)
                                    .show(ui);
                            });

                            r.cell(|ui| {
                                if ui.selectable_label(false, "🗑").clicked() {
                                    to_remove.push(id);
                                }
                            });
                        });

                        if row_response.clicked() {
                            *self.selected_rule = Some(id);
                        }
                    }

                    to_remove.into_iter().rev().for_each(|i| {
                        self.rules.remove(i);
                    });

                    if let Some(index) = to_clone.filter(|i| *i < self.rules.len()) {
                        let clone = self.rules[index].clone();
                        self.rules.push(clone);
                        *self.selected_rule = Some(self.rules.len() - 1);
                    }
                });
        });
    }
}

fn show_move_up_down<T>(selected: &mut Option<usize>, items: &mut [T], ui: &mut Ui) {
    if ui
        .add_enabled(
            selected.map(|s| s > 0 && s < items.len()).unwrap_or(false),
            Button::new("⬆"),
        )
        .clicked()
    {
        let index = selected.unwrap();
        items.swap(index, index - 1);
        *selected = Some(index - 1);
    }

    if ui
        .add_enabled(
            selected.map(|s| s < items.len() - 1).unwrap_or(false),
            Button::new("⬇"),
        )
        .clicked()
    {
        let index = selected.unwrap();
        items.swap(index, index + 1);
        *selected = Some(index + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_name_rule(name: &str, expression: &str) -> CombatNameRule {
        CombatNameRule {
            name_rule: RulesGroup {
                name: name.to_string(),
                enabled: true,
                rules: vec![MatchRule {
                    aspect: MatchAspect::SourceOrTargetUniqueName,
                    expression: expression.to_string(),
                    method: MatchMethod::Equals,
                    enabled: true,
                }],
            },
            additional_info_rules: Vec::new(),
        }
    }

    #[test]
    fn overlap_flags_rule_matching_a_curated_entity() {
        let identifiers = curated_map_identifiers();
        let hive = unique_name_rule("My Hive", "Space_Borg_Dreadnought_Hive_Intro");
        let unrelated = unique_name_rule("Unrelated", "Some_Random_Entity");

        assert_eq!(
            CombatNameRules::overlapping_maps(&hive.name_rule, &identifiers),
            vec!["[TFO] Hive Onslaught".to_string()]
        );
        assert!(CombatNameRules::overlapping_maps(&unrelated.name_rule, &identifiers).is_empty());
    }

    #[test]
    fn disabled_rule_is_not_flagged() {
        let identifiers = curated_map_identifiers();
        let mut rule = unique_name_rule("My Hive", "Space_Borg_Dreadnought_Hive_Intro");
        rule.name_rule.enabled = false;
        assert!(CombatNameRules::overlapping_maps(&rule.name_rule, &identifiers).is_empty());
    }

    #[test]
    fn strip_category_prefix_removes_only_a_leading_bracket_tag() {
        assert_eq!(
            strip_category_prefix("[Patrol] Trouble Over Terrh"),
            "Trouble Over Terrh"
        );
        assert_eq!(
            strip_category_prefix("[TFO] Azure Nebula Rescue"),
            "Azure Nebula Rescue"
        );
        assert_eq!(strip_category_prefix("Infected Space"), "Infected Space");
        // A bracket that is not a leading category tag is left alone.
        assert_eq!(strip_category_prefix("Nukara [x]"), "Nukara [x]");
    }

    #[test]
    fn rule_overlaps_a_curated_map_by_name_ignoring_prefix() {
        // A rule whose *name* matches a curated map (prefix aside) is flagged,
        // even though it matches on a display name the entity check cannot see.
        let identifiers = vec![(
            "Space_Elachi_Frigate".to_string(),
            "[Patrol] Trouble Over Terrh".to_string(),
        )];
        let rule = CombatNameRule {
            name_rule: RulesGroup {
                name: "Trouble Over Terrh".to_string(),
                enabled: true,
                rules: vec![MatchRule {
                    aspect: MatchAspect::SourceOrTargetName,
                    expression: "R.R.W. Lleiset".to_string(),
                    method: MatchMethod::Contains,
                    enabled: true,
                }],
            },
            additional_info_rules: Vec::new(),
        };
        assert_eq!(
            CombatNameRules::overlapping_maps(&rule.name_rule, &identifiers),
            vec!["[Patrol] Trouble Over Terrh".to_string()],
        );

        // A different name does not overlap.
        let mut other = rule;
        other.name_rule.name = "Something Else".to_string();
        assert!(CombatNameRules::overlapping_maps(&other.name_rule, &identifiers).is_empty());
    }

    /// Real user rules, verbatim: they carry the `[Patrol] ` prefix *and* match
    /// on a display name, so the prefix must be stripped on both sides and the
    /// entity check cannot be what flags them. The annotated `[M]` variant is
    /// the case an exact name comparison used to miss.
    #[test]
    fn prefixed_and_annotated_rule_names_overlap_the_curated_map() {
        let identifiers = curated_map_identifiers();
        let rule = |name: &str| RulesGroup {
            name: name.to_string(),
            enabled: true,
            rules: vec![MatchRule {
                aspect: MatchAspect::SourceOrTargetName,
                expression: "U.S.S. Birmingham".to_string(),
                method: MatchMethod::Contains,
                enabled: true,
            }],
        };
        let expected = vec!["[Patrol] The Ninth Rule".to_string()];

        for name in [
            "[Patrol] The Ninth Rule",
            "[Patrol] The Ninth Rule [M]",
            "The Ninth Rule [M]",
            "the ninth rule",
        ] {
            assert_eq!(
                CombatNameRules::overlapping_maps(&rule(name), &identifiers),
                expected,
                "rule named {name:?} must be flagged"
            );
        }

        // A rule that merely mentions an unrelated name is still not flagged.
        assert!(CombatNameRules::overlapping_maps(&rule("My Own Thing"), &identifiers).is_empty());
    }
}
