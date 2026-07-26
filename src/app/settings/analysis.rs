use std::borrow::BorrowMut;

use eframe::egui::*;

use super::Settings;
use crate::analyzer::{Combat, curated_map_identifiers, curated_map_names};
use crate::custom_widgets::table::Table;
use crate::unwrap_or_return;
use crate::{analyzer::settings::*, custom_widgets::popup_button::PopupButton};

const HEADER_HEIGHT: f32 = 15.0;
const ROW_HEIGHT: f32 = 25.0;
/// Amber used for shadow/overlap warnings.
const WARN_COLOR: Color32 = Color32::from_rgb(0xd9, 0x95, 0x00);

#[derive(Default)]
pub struct AnalysisTab {
    list_selected_combat_occurred_names: bool,
    occurred_combat_names_search_term: String,
    indirect_source_reversal_rules: IndirectSourceReversalRules,
    custom_grouping_rules: CustomGroupingRules,
    damage_out_exclusion_rules: DamageOutExclusionRules,
    combat_names_rules: CombatNameRules,
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

struct GroupRulesTable<'a, T: BorrowMut<RulesGroup> + Default> {
    group_rules: &'a mut Vec<T>,
    title: &'a str,
    name_header: &'a str,
    selected_group: &'a mut Option<usize>,
    popup_extra_space: f32,
    /// Optional per-row warning: returns a tooltip when the row's rule should be
    /// flagged (e.g. it shadows an auto-detected map). Adds a ⚠ cell per row.
    row_warning: Option<&'a dyn Fn(&RulesGroup) -> Option<String>>,
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

        self.indirect_source_reversal_rules
            .show(&mut modified_settings.analysis, ui);
        ui.add_space(20.0);

        ui.separator();
        ui.push_id(line!(), |ui| {
            self.custom_grouping_rules
                .show(&mut modified_settings.analysis, ui);
        });
        ui.add_space(20.0);

        ui.separator();
        self.damage_out_exclusion_rules
            .show(&mut modified_settings.analysis, ui);
        ui.add_space(20.0);

        ui.separator();
        self.combat_names_rules
            .show(&mut modified_settings.analysis, selected_combat, ui);

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
                        filter.len() == 0 || n.to_lowercase().contains(&filter.to_lowercase())
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
    fn show(
        &mut self,
        modified_settings: &mut AnalysisSettings,
        selected_combat: Option<&Combat>,
        ui: &mut Ui,
    ) {
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

        CollapsingHeader::new("Combat Name Detection Rules").show_unindented(ui, |ui| {
            GroupRulesTable::new(
                &mut modified_settings.combat_name_rules,
                "",
                "Combat Name",
                &mut self.selected_group,
                200.0,
            )
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

            Self::show_auto_detected(selected_combat, ui);
        });
    }

    /// Read-only view of the maps the analyzer auto-detects. These act as a
    /// lower-priority layer below the rules above: a combat that no rule names
    /// falls back to its detected map (with difficulty). Individual rules that
    /// shadow a detected map are flagged with a ⚠ on their row (see
    /// `overlapping_maps`); this section additionally notes it for the selected
    /// combat and lists the detectable maps.
    fn show_auto_detected(selected_combat: Option<&Combat>, ui: &mut Ui) {
        // Legend for the per-row ⚠, directly under the rules frame above.
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.colored_label(WARN_COLOR, "⚠");
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

        // Collision note for the currently selected combat, if a rule renamed it.
        if let Some(combat) = selected_combat {
            if let Some(detected) = combat.detected_name() {
                if !combat.combat_names.is_empty() {
                    ui.add_space(6.0);
                    ui.colored_label(
                        WARN_COLOR,
                        format!(
                            "⚠ Your rules name the selected combat \"{}\", shadowing the \
                             detected \"{}\".",
                            combat.name(),
                            detected
                        ),
                    );
                }
            }
        }

        ui.add_space(6.0);
        ui.label(RichText::new("Auto-detected maps:").weak());
        ScrollArea::vertical()
            .id_salt("auto detected maps")
            .max_height(150.0)
            .show(ui, |ui| {
                for map in curated_map_names() {
                    ui.label(RichText::new(map).weak());
                }
            });
    }

    /// The curated maps a single rule overlaps, either way:
    /// - **entity**: the rule matches the map's identifying NPC (unique name), or
    /// - **name**: the rule's own name equals the map's name, ignoring the
    ///   `[TFO]`/`[Patrol]` category prefix on either side (e.g. a rule named
    ///   "Trouble Over Terrh" vs the "[Patrol] Trouble Over Terrh" map).
    ///
    /// Sorted and deduped; empty when none or when the rule is disabled.
    fn overlapping_maps(group: &RulesGroup, identifiers: &[(String, String)]) -> Vec<String> {
        if !group.enabled {
            return Vec::new();
        }
        let rule_name = strip_category_prefix(&group.name);
        let mut maps: Vec<String> = identifiers
            .iter()
            .filter(|(unique_name, map)| {
                group.matches_source_or_target_unique_names(std::iter::once(unique_name.as_str()))
                    || group
                        .matches_indirect_source_unique_names(std::iter::once(unique_name.as_str()))
                    || (!rule_name.is_empty()
                        && strip_category_prefix(map).eq_ignore_ascii_case(rule_name))
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

impl<'a, T: BorrowMut<RulesGroup> + Default> GroupRulesTable<'a, T> {
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
        }
    }

    /// Show a ⚠ on the right of each row for which `warning` returns a tooltip.
    fn with_row_warning(mut self, warning: &'a dyn Fn(&RulesGroup) -> Option<String>) -> Self {
        self.row_warning = Some(warning);
        self
    }

    fn show(&mut self, ui: &mut Ui, mut edit: impl FnMut(&mut T, &mut Ui)) {
        let row_warning = self.row_warning;
        ui.horizontal(|ui| {
            ui.label(self.title);
            if ui.button("Add ✚").clicked() {
                self.group_rules.push(Default::default());
            }

            show_move_up_down(self.selected_group, self.group_rules, ui);
        });
        Table::new(ui)
            .min_scroll_height(200.0)
            .max_scroll_height(200.0)
            .cell_spacing(10.0)
            .header(HEADER_HEIGHT, |r| {
                r.cell(|ui| {
                    ui.label("On");
                });
                r.cell(|ui| {
                    ui.label("Edit");
                });
                r.cell(|ui| {
                    ui.label(self.name_header);
                });
            })
            .body(ROW_HEIGHT, |t| {
                let mut to_remove = Vec::new();
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
                            TextEdit::singleline(&mut rule.borrow_mut().name)
                                .clip_text(false)
                                .show(ui);
                        });

                        if let Some(row_warning) = row_warning {
                            r.cell(|ui| match row_warning(rule.borrow()) {
                                Some(tooltip) => {
                                    ui.colored_label(WARN_COLOR, "⚠").on_hover_text(tooltip);
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
            Table::new(ui)
                .min_scroll_height(100.0)
                .max_scroll_height(200.0)
                .cell_spacing(10.0)
                .header(HEADER_HEIGHT, |r| {
                    r.cell(|ui| {
                        ui.label("On");
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
                    for (id, rule) in self.rules.iter_mut().enumerate() {
                        let row_response = t.selectable_row(*self.selected_rule == Some(id), |r| {
                            r.cell(|ui| {
                                ui.checkbox(&mut rule.enabled, "");
                            });

                            r.cell(|ui| {
                                ComboBox::from_id_salt(id + 9387465)
                                    .selected_text(rule.aspect.display())
                                    .width(150.0)
                                    .show_ui(ui, |ui| {
                                        self.match_aspect_set.into_iter().for_each(|a| {
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
                });
        });
    }
}

fn show_move_up_down<T>(selected: &mut Option<usize>, items: &mut Vec<T>, ui: &mut Ui) {
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
        assert_eq!(strip_category_prefix("[Patrol] Trouble Over Terrh"), "Trouble Over Terrh");
        assert_eq!(strip_category_prefix("[TFO] Azure Nebula Rescue"), "Azure Nebula Rescue");
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
}
