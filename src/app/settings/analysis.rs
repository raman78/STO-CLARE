use crate::custom_widgets::dialog::escape_closes;
use rustc_hash::FxHashMap;
use std::borrow::BorrowMut;

use eframe::{Frame, egui::*};
use rfd::FileDialog;

use super::Settings;
use crate::analyzer::settings::*;
use crate::analyzer::{Combat, curated_map_identifiers, curated_map_names};
use crate::app::theme;
use crate::custom_widgets::table::{SortState, Table, sort_marker_width};
use crate::custom_widgets::toggle::Toggle;
use crate::custom_widgets::tooltip::CloseTooltip;
use crate::unwrap_or_return;

/// Per-row warning: returns a tooltip when the row's rule should be flagged.
type RowWarning<'a> = &'a dyn Fn(&RulesGroup) -> Option<String>;

const HEADER_HEIGHT: f32 = 15.0;
const ROW_HEIGHT: f32 = 25.0;

/// Every way a condition can compare its text to a name, in the order they are
/// offered. One list, so the picker in the rules table and anything else that
/// has to enumerate them cannot fall out of step with the enum.
const MATCH_METHODS: [MatchMethod; 4] = [
    MatchMethod::Equals,
    MatchMethod::StartsWith,
    MatchMethod::EndsWith,
    MatchMethod::Contains,
];

/// How wide the list of live examples is drawn beside a set of conditions.
/// Enough for an ability name of the length the game actually produces
/// ("Quad Disruptor Cannons - Rapid Fire III"), and no wider: the width past
/// that belongs to the conditions, which is where the typing happens.
const MATCHES_PANE_WIDTH: f32 = 300.0;

/// How wide the rule-editing dialog opens.
///
/// Enough for both halves side by side: a table of conditions settles at 751
/// points — two 150-point pickers, a 260-point pattern field, three narrow
/// button columns and the spacing between them — and the pane beside it takes
/// [`MATCHES_PANE_WIDTH`]. Narrower than this and the conditions could only be
/// worked by dragging them sideways. A screen too small for it caps the dialog
/// instead, and then the table does scroll.
const EDIT_DIALOG_WIDTH: f32 = 1100.0;

/// Which column of a group-rules table holds the name: On, Edit, Clone, name.
/// The one that takes whatever width is left over.
const NAME_COLUMN: usize = 3;

/// Which column of a conditions table holds the text to match: On, Clone,
/// Aspect, Method, text.
const EXPRESSION_COLUMN: usize = 4;

/// How narrow a name or pattern field may be squeezed when the window is
/// dragged in. Below this it stops giving ground and the table scrolls
/// sideways instead — a field of two characters is not one anybody can work in,
/// and by then the window is too small for the tab whatever is done.
const NAME_COLUMN_MIN_WIDTH: f32 = 90.0;

/// The width a rule's name field starts at, before its own text has widened the
/// column around it.
const NAME_COLUMN_WIDTH: f32 = 260.0;

/// How tall the rule-editing dialog opens: the name row, a table of conditions
/// deep enough to hold a rule's worth of them, and the Close button.
const EDIT_DIALOG_HEIGHT: f32 = 420.0;

/// How much of the screen is left around the dialog, so it always reads as a
/// window standing over the program rather than covering it.
const DIALOG_SCREEN_MARGIN: f32 = 80.0;

#[derive(Default)]
pub struct AnalysisTab {
    list_selected_combat_occurred_names: bool,
    occurred_combat_names_search_term: String,
    /// What the last Export or Import did, or why it could not. Shown until it
    /// is dismissed: a file dialog that closes with nothing else happening is
    /// indistinguishable from one whose work quietly failed.
    transfer_report: Option<String>,
    /// What the clash bar has to say, worked out while the tab is drawn and
    /// shown *outside* the window's scroll area — see
    /// [`AnalysisTab::show_footer`]. `None` on a section that has nothing to
    /// report there.
    footer: Option<ClashFooter>,
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
    editing_group: Option<usize>,
    order: SortState<RuleColumn>,
}

#[derive(Default)]
struct DamageOutExclusionRules {
    selected: Option<usize>,
}

#[derive(Default)]
struct CombatNameRules {
    selected_group: Option<usize>,
    selected_rule: Option<usize>,
    editing_group: Option<usize>,
    order: SortState<RuleColumn>,
    selected_additional_info_group: Option<usize>,
    selected_additional_info_rule: Option<usize>,
    editing_additional_info_group: Option<usize>,
    additional_info_order: SortState<RuleColumn>,
}

/// The columns of a rules table a reader can order the list by.
///
/// Only two are worth ordering by, and both answer a question about a long
/// list: "where is the rule called X" and "which of these are switched off".
/// The rest hold a button apiece.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RuleColumn {
    /// Whether the rule is switched on. Off first when the order runs the
    /// column's own way: a list is scanned for what is *not* doing its job.
    Enabled,
    Name,
}

struct GroupRulesTable<'a, T: BorrowMut<RulesGroup> + Default + Clone> {
    group_rules: &'a mut Vec<T>,
    title: &'a str,
    name_header: &'a str,
    selected_group: &'a mut Option<usize>,
    /// Which row's ✏ is open, if any. See [`GroupRulesTable::show_edit_dialog`].
    editing: &'a mut Option<usize>,
    /// Which column the list is ordered by, and which way round.
    order: &'a mut SortState<RuleColumn>,
    /// Optional per-row warning: returns a tooltip when the row's rule should be
    /// flagged (e.g. it shadows an auto-detected map). Adds a ⚠ cell per row.
    row_warning: Option<RowWarning<'a>>,
    /// Explicit height cap; falls back to all available space.
    max_height: Option<f32>,
}

struct RulesTable<'a> {
    rules: &'a mut Vec<MatchRule>,
    title: &'a str,
    match_aspect_set: &'a [MatchAspect],
    selected_rule: &'a mut Option<usize>,
    /// The combat the live examples are taken from, when one is selected. See
    /// [`show_matches_pane`].
    combat: Option<&'a Combat>,
    /// Explicit height cap; falls back to all available space.
    max_height: Option<f32>,
}

impl AnalysisTab {
    pub fn show(
        &mut self,
        modified_settings: &mut Settings,
        selected_combat: Option<&Combat>,
        ui: &mut Ui,
        frame: &Frame,
    ) {
        // Said here, where the rules are, and said every time the tab is opened
        // rather than once at start-up: the rules on screen are not the ones in
        // the file, and editing them without knowing that would be working on
        // the wrong copy. Nothing is written over that file while this stands.
        if let Some(problem) = modified_settings.rules_file_problem() {
            ui.colored_label(
                theme::palette().warn,
                format!(
                    "⚠ Your rules file could not be read, so the rules below are the ones \
                     from before it was split out, and the file is being left alone.\n{problem}"
                ),
            );
            ui.separator();
        }

        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    selected_combat.is_some(),
                    Button::new("List Selected Combat Occurred Names"),
                )
                .clicked()
            {
                self.list_selected_combat_occurred_names = true;
            }

            ui.separator();
            self.show_transfer_buttons(
                &mut modified_settings.analysis,
                None,
                "all four rule sets",
                ui,
                frame,
            );
        });

        ui.add_space(8.0);
        ui.horizontal_wrapped(|ui| {
            use AnalysisSection::*;
            for section in [CombatNames, SourceReversal, CustomGrouping, DamageExclusion] {
                ui.steady_toggle_value(&mut self.selected_section, section, section.label());
            }
        });
        ui.separator();

        ui.horizontal_wrapped(|ui| {
            let section = self.selected_section;
            self.show_transfer_buttons(
                &mut modified_settings.analysis,
                Some(section),
                "this section",
                ui,
                frame,
            );
        });
        ui.add_space(4.0);

        // Only Custom Grouping has anything for the bar. Cleared here so a
        // switch of section takes the last one's bar with it.
        self.footer = None;
        match self.selected_section {
            AnalysisSection::CombatNames => {
                self.combat_names_rules
                    .show(&mut modified_settings.analysis, selected_combat, ui)
            }
            AnalysisSection::SourceReversal => self.indirect_source_reversal_rules.show(
                &mut modified_settings.analysis,
                selected_combat,
                ui,
            ),
            AnalysisSection::CustomGrouping => {
                self.footer = ui
                    .push_id(line!(), |ui| {
                        self.custom_grouping_rules.show(
                            &mut modified_settings.analysis,
                            selected_combat,
                            ui,
                        )
                    })
                    .inner
                    .into();
            }
            AnalysisSection::DamageExclusion => self.damage_out_exclusion_rules.show(
                &mut modified_settings.analysis,
                selected_combat,
                ui,
            ),
        }

        self.show_occurred_names_window(selected_combat, ui);
        self.show_transfer_report(ui);
    }

    /// How much room the standing bar needs, so the caller can keep it before
    /// drawing the tab.
    ///
    /// Read from what the bar said on the previous frame: it is worked out
    /// while the tab is drawn, and the room for it has to be claimed first. One
    /// frame behind on the frame a rule is picked, which nothing can tell.
    pub fn footer_height(&self, ui: &Ui) -> f32 {
        let Some(footer) = &self.footer else {
            return 0.0;
        };
        let row = ui.text_style_height(&TextStyle::Body) + ui.spacing().item_spacing.y;
        // The rule, the summary, and up to CLASH_DETAIL_ROWS of the detail.
        let detail = footer
            .detail
            .as_ref()
            .map_or(0.0, |_| row * CLASH_DETAIL_ROWS);
        row + detail + ui.spacing().item_spacing.y * 2.0
    }

    /// The standing bar, drawn **outside** the Settings window's scroll area.
    ///
    /// Inside it, the bar scrolled away with the table it is about — which for
    /// a list of fifty rules means it is off screen exactly when it is being
    /// read. See [`clash_footer`] for what it says.
    pub fn show_footer(&mut self, ui: &mut Ui) {
        let Some(footer) = self.footer.clone() else {
            return;
        };
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if footer.warn {
                ui.colored_label(theme::palette().warn, "⚠");
            }
            ui.label(RichText::new(&footer.summary).weak());
        });
        if let Some(detail) = &footer.detail {
            let row = ui.text_style_height(&TextStyle::Body) + ui.spacing().item_spacing.y;
            ScrollArea::vertical()
                .id_salt("clash detail")
                .auto_shrink([false, true])
                .max_height(row * CLASH_DETAIL_ROWS)
                .show(ui, |ui| {
                    ui.label(RichText::new(detail).weak());
                });
        }
    }

    /// Export and Import, for one section or for all four.
    ///
    /// `section` of `None` means the whole tab. Both write the same shape of
    /// file, so a section's file can be imported into the tab and the other way
    /// round — the reader is spared having to know that there are two kinds.
    fn show_transfer_buttons(
        &mut self,
        settings: &mut AnalysisSettings,
        section: Option<AnalysisSection>,
        what: &str,
        ui: &mut Ui,
        frame: &Frame,
    ) {
        let (sets, sections, file_name) = match section {
            Some(section) => (
                section.taken_from(settings),
                vec![section],
                section.file_name(),
            ),
            None => (
                settings.rule_sets(),
                vec![
                    AnalysisSection::CombatNames,
                    AnalysisSection::SourceReversal,
                    AnalysisSection::CustomGrouping,
                    AnalysisSection::DamageExclusion,
                ],
                crate::helpers::paths::RULES_FILE_NAME.to_string(),
            ),
        };

        if ui
            .button("Export…")
            .hover(format!("Write {what} to a file you can keep or pass on"))
            .clicked()
            && let Some(path) = FileDialog::new()
                .set_title("Export rules")
                .add_filter("rules", &["toml"])
                .set_file_name(&file_name)
                .set_parent(frame)
                .save_file()
        {
            self.transfer_report = Some(match sets.write(&path) {
                Ok(()) => format!("Exported {} to\n{}", sets.summary(), path.display()),
                Err(e) => format!("Nothing was exported.\n\n{e}"),
            });
        }

        if ui
            .button("Import…")
            .hover(format!(
                "Add rules from a file to {what}. Nothing of yours is removed."
            ))
            .clicked()
            && let Some(path) = FileDialog::new()
                .set_title("Import rules")
                .add_filter("rules", &["toml"])
                .set_parent(frame)
                .pick_file()
        {
            self.transfer_report = Some(match RuleSets::read(&path) {
                Ok(incoming) => import_rules(settings, incoming, &sections),
                Err(e) => format!("Nothing was imported.\n\n{}\n\n{e}", path.display()),
            });
        }
    }

    /// What the last Export or Import did. Said rather than left to be inferred
    /// from a list that may look unchanged — an import of rules already held
    /// adds nothing at all, and that is a result, not a failure.
    fn show_transfer_report(&mut self, ui: &mut Ui) {
        let Some(report) = self.transfer_report.clone() else {
            return;
        };
        let response = Modal::new(ui.id().with("rule transfer report")).show(ui.ctx(), |ui| {
            ui.set_width(EDIT_DIALOG_WIDTH.min(ui.ctx().content_rect().width() - 40.0) * 0.6);
            ui.label(report);
            ui.separator();
            let closed = ui.horizontal(|ui| ui.button("Close").clicked()).inner;
            closed || escape_closes(ui.ctx())
        });
        if response.inner || response.backdrop_response.clicked() {
            self.transfer_report = None;
        }
    }

    fn show_occurred_names_window(&mut self, selected_combat: Option<&Combat>, ui: &mut Ui) {
        let combat = unwrap_or_return!(selected_combat);
        if !self.list_selected_combat_occurred_names {
            return;
        }

        let mut close = false;
        Window::new("Selected Combat Occurred Names")
            .collapsible(false)
            .open(&mut self.list_selected_combat_occurred_names)
            .scroll(true)
            .constrain(true)
            .show(ui.ctx(), |ui| {
                if escape_closes(ui.ctx()) {
                    close = true;
                }
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
        if close {
            self.list_selected_combat_occurred_names = false;
        }
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
                .header(HEADER_HEIGHT)
                .body(ROW_HEIGHT, |b| {
                    for name in names.filter(|n| {
                        filter.is_empty() || n.to_lowercase().contains(&filter.to_lowercase())
                    }) {
                        b.row(|r| {
                            r.cell(|ui| {
                                ui.label(name);
                            });
                            r.cell(|ui| {
                                if ui.button("🗐").hover("Copy").clicked() {
                                    ui.ctx().copy_text(name.to_string());
                                }
                            });
                        });
                    }
                })
                .header_row(|r| {
                    r.cell(|ui| {
                        ui.label(title);
                    });
                });
        });
    }
}

impl IndirectSourceReversalRules {
    fn show(
        &mut self,
        modified_settings: &mut AnalysisSettings,
        selected_combat: Option<&Combat>,
        ui: &mut Ui,
    ) {
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
        .with_combat(selected_combat)
        .show(ui);
    }
}

impl DamageOutExclusionRules {
    fn show(
        &mut self,
        modified_settings: &mut AnalysisSettings,
        selected_combat: Option<&Combat>,
        ui: &mut Ui,
    ) {
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
        .with_combat(selected_combat)
        .show(ui);
    }
}

impl CustomGroupingRules {
    fn show(
        &mut self,
        modified_settings: &mut AnalysisSettings,
        selected_combat: Option<&Combat>,
        ui: &mut Ui,
    ) -> ClashFooter {
        const ASPECTS: [MatchAspect; 3] = [
            MatchAspect::DamageOrHealName,
            MatchAspect::IndirectSourceName,
            MatchAspect::IndirectUniqueSourceName,
        ];
        // Which rules quietly share an effect with another rule, worked out
        // once for the whole table rather than per row.
        let clashes = selected_combat
            .map(|combat| clashing_rules(&modified_settings.custom_group_rules, &ASPECTS, combat))
            .unwrap_or_default();
        let row_warning = |group: &RulesGroup| clashes.get(&group.name).cloned();

        GroupRulesTable::new(
            &mut modified_settings.custom_group_rules,
            "Custom Grouping Rules",
            "Group Name",
            &mut self.selected_group,
            &mut self.editing_group,
            &mut self.order,
        )
        .with_row_warning(&row_warning)
        .show(ui, |r, ui| {
            RulesTable::new(
                &mut r.rules,
                "Match when any of these is true:",
                &[
                    MatchAspect::DamageOrHealName,
                    MatchAspect::IndirectSourceName,
                    MatchAspect::IndirectUniqueSourceName,
                ],
                &mut self.selected_rule,
            )
            // No height given: the dialog sets one, and the conditions take
            // whatever it left them.
            .with_combat(selected_combat)
            .show(ui);
        });

        // Read after the table, which is where the selection may have just
        // changed and where the list may have just been re-sorted.
        let picked = self
            .selected_group
            .and_then(|index| modified_settings.custom_group_rules.get(index))
            .map(|rule| rule.name.clone());
        clash_footer(&clashes, picked.as_deref(), selected_combat)
    }
}

/// What the standing bar under the rules table has to say.
///
/// Worked out here and drawn by [`AnalysisTab::show_footer`], outside the
/// Settings window's scroll area, so the bar stays put instead of scrolling
/// away with the table it is about.
///
/// A tooltip answers a question the reader has already thought to ask. This is
/// for the one they have not: two rules quietly fitting the same effect is
/// something to notice while scrolling a list of fifty, and a mark that says
/// nothing until the pointer rests on it is a mark most readers never read.
///
/// It names the picked rule's clash when there is one, so clicking down the
/// list reads out each rule's own case, and otherwise sums up the tab.
fn clash_footer(
    clashes: &FxHashMap<String, String>,
    picked: Option<&str>,
    selected_combat: Option<&Combat>,
) -> ClashFooter {
    // Said rather than left blank: a column with no marks in it reads as
    // "nothing clashes", and here it means "not checked".
    if selected_combat.is_none() {
        return ClashFooter {
            summary: "⚠ marks two rules catching the same effect. Select a combat to have \
                      your rules checked against it."
                .to_string(),
            warn: false,
            detail: None,
        };
    }

    if clashes.is_empty() {
        return ClashFooter {
            summary: "No two rules catch the same effect in this combat.".to_string(),
            warn: false,
            detail: None,
        };
    }

    ClashFooter {
        summary: format!(
            "{} of your rules share an effect with another rule in this combat. \
             The more precise one takes each.",
            clashes.len()
        ),
        warn: true,
        detail: Some(match picked.and_then(|name| clashes.get(name)) {
            Some(detail) => detail.clone(),
            None => "Pick a marked rule to see which effects, and where they go.".to_string(),
        }),
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
            const ALL_ASPECTS: [MatchAspect; 5] = [
                MatchAspect::DamageOrHealName,
                MatchAspect::IndirectSourceName,
                MatchAspect::IndirectUniqueSourceName,
                MatchAspect::SourceOrTargetName,
                MatchAspect::SourceOrTargetUniqueName,
            ];
            GroupRulesTable::new(
                &mut modified_settings.combat_name_rules,
                "Combat Name Detection Rules",
                "Combat Name",
                &mut self.selected_group,
                &mut self.editing_group,
                &mut self.order,
            )
            .with_max_height(rules_height)
            .with_row_warning(&row_warning)
            .show(ui, |r, ui| {
                // This dialog holds two tables rather than one, so they split
                // the room it was given instead of each asking for all of it.
                const GAP: f32 = 8.0;
                let each = ((ui.available_height() - GAP) / 2.0).at_least(ROW_HEIGHT * 4.0);

                RulesTable::new(
                    &mut r.name_rule.rules,
                    "Name the combat when any of these is true:",
                    &ALL_ASPECTS,
                    &mut self.selected_rule,
                )
                .with_combat(selected_combat)
                .with_max_height(each)
                .show(ui);

                ui.add_space(GAP);
                ui.push_id("additional info rules", |ui| {
                    GroupRulesTable::new(
                        &mut r.additional_info_rules,
                        "Additional info (difficulty is detected automatically — don't add it here)",
                        "Info",
                        &mut self.selected_additional_info_group,
                        &mut self.editing_additional_info_group,
                        &mut self.additional_info_order,
                    )
                    .with_max_height(each)
                    .show(ui, |r, ui| {
                        RulesTable::new(
                            &mut r.rules,
                            "Add this info when any of these is true:",
                            &ALL_ASPECTS,
                            &mut self.selected_additional_info_rule,
                        )
                        .with_combat(selected_combat)
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
            // The bar goes at the edge of the panel rather than against the
            // longest map name.
            .auto_shrink([false, true])
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
        editing: &'a mut Option<usize>,
        order: &'a mut SortState<RuleColumn>,
    ) -> Self {
        Self {
            group_rules,
            title,
            name_header,
            selected_group,
            editing,
            order,
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

    /// Put the rules in alphabetical order, and carry the selection and the open
    /// dialog across to wherever their rules ended up.
    ///
    /// The order is for reading and nothing else: which rule claims a shot is
    /// decided by how precisely it fits, never by where it sits — see
    /// `analyzer::settings::most_specific_match`. That is what makes sorting
    /// safe to do at all, and it is what makes a rule imported from someone
    /// else's file behave the same at either end of the list.
    ///
    /// Not while anything is being typed into, and not while a dialog is open:
    /// a list that re-sorted on every keystroke would take the row out from
    /// under the cursor halfway through naming it. A rule with no name yet goes
    /// to the end rather than to the top, so one just added by ✚ stays where it
    /// was put until it is called something.
    fn sort_by_name(&mut self, ui: &Ui) {
        if ui.memory(|m| m.focused()).is_some() || self.editing.is_some() {
            return;
        }

        // A rule with no name yet sorts to the end however the list is ordered:
        // one just added by ✚ stays where it was put until it is called
        // something, rather than jumping to whichever end is currently first.
        let unnamed = |rule: &T| rule.borrow().name.is_empty();
        let name = |rule: &T| rule.borrow().name.to_lowercase();
        let enabled = |rule: &T| rule.borrow().enabled;
        let ordering = *self.order;
        let mut order: Vec<usize> = (0..self.group_rules.len()).collect();
        order.sort_by(|a, b| {
            let (a, b) = (&self.group_rules[*a], &self.group_rules[*b]);
            unnamed(a).cmp(&unnamed(b)).then_with(|| {
                let by = match ordering.column.unwrap_or(RuleColumn::Name) {
                    // Off first the natural way round: a long list is scanned
                    // for the rules that are *not* doing anything.
                    RuleColumn::Enabled => enabled(a).cmp(&enabled(b)),
                    RuleColumn::Name => name(a).cmp(&name(b)),
                };
                let by = if ordering.natural { by } else { by.reverse() };
                // Names settle every tie, so the order is total and the list
                // does not shuffle among equals from frame to frame.
                by.then_with(|| name(a).cmp(&name(b)))
            })
        });
        if order.iter().enumerate().all(|(to, from)| to == *from) {
            return;
        }

        let mut moved_to = vec![0usize; order.len()];
        for (to, from) in order.iter().enumerate() {
            moved_to[*from] = to;
        }
        let follow = |index: &mut Option<usize>| {
            *index = index.and_then(|i| moved_to.get(i).copied());
        };
        follow(self.selected_group);
        follow(self.editing);

        let mut taken: Vec<Option<T>> = std::mem::take(self.group_rules)
            .into_iter()
            .map(Some)
            .collect();
        *self.group_rules = order
            .into_iter()
            .map(|from| taken[from].take().expect("each rule is moved once"))
            .collect();
    }

    fn show(&mut self, ui: &mut Ui, edit: impl FnMut(&mut T, &mut Ui)) {
        self.sort_by_name(ui);
        let row_warning = self.row_warning;
        let ordering = *self.order;
        let mut reorder: Option<RuleColumn> = None;
        ui.horizontal(|ui| {
            ui.strong(self.title);
            // The new rule becomes the selection, so it is obvious which of the
            // rows is the one just added — it has no name to find it by, and it
            // is at the end of a list ordered by names.
            if ui.button("Add ✚").clicked() {
                self.group_rules.push(Default::default());
                *self.selected_group = Some(self.group_rules.len() - 1);
            }
            ui.label(RichText::new("Click a heading to order the list").weak())
                .hover(
                    "The order is for finding a rule, never for deciding which one \
                     applies: where two rules fit the same effect, the more precise \
                     one wins wherever either of them sits.",
                );
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
            // The name takes whatever the window has left over, so the table
            // fills it instead of ending in a narrow box beside an expanse of
            // nothing — and a long name is read without scrolling inside its
            // own field. Fourth column: On, Edit, Clone, then the name.
            .stretch_column(NAME_COLUMN, NAME_COLUMN_MIN_WIDTH)
            .cell_spacing(10.0)
            .header(HEADER_HEIGHT)
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
                            let open = *self.editing == Some(id);
                            if ui
                                .add(Button::new("✏").selected(open))
                                .hover("Edit this rule")
                                .clicked()
                            {
                                *self.editing = if open { None } else { Some(id) };
                            }
                        });

                        r.cell(|ui| {
                            // A framed button, to match the ✏ next to it.
                            if ui.button("🗐").hover("Clone this rule").clicked() {
                                to_clone = Some(id);
                            }
                        });

                        // A floor under the column's width, not a cap: a longer
                        // name still widens it. The claim is made to the column
                        // rather than to the field, because a `TextEdit` is
                        // never wider than the room it is given — asked for a
                        // width inside a cell measured from a rule with no name
                        // yet, it got 73 points and every new rule had to be
                        // widened by hand before it could be read.
                        r.measured_cell(|ui| {
                            // Fills its cell, and the cell fills the window —
                            // see `Table::stretch_column`. The returned width is
                            // a floor for the first frame, before the column has
                            // been measured and before there is any name in it
                            // to measure: without it a new rule opened as a box
                            // 73 points wide.
                            TextEdit::singleline(&mut rule.borrow_mut().name)
                                .desired_width(f32::MAX)
                                .show(ui);
                            NAME_COLUMN_WIDTH
                        });

                        if let Some(row_warning) = row_warning {
                            r.cell(|ui| match row_warning(rule.borrow()) {
                                Some(tooltip) => {
                                    ui.colored_label(theme::palette().warn, "⚠").hover(tooltip);
                                }
                                // Keep the column width constant whether or not a
                                // warning shows, so toggling rules doesn't shift the row.
                                None => {
                                    ui.colored_label(Color32::TRANSPARENT, "⚠");
                                }
                            });
                        }

                        r.cell(|ui| {
                            if ui.steady_toggle(false, "🗑").clicked() {
                                to_remove.push(id);
                            }
                        });
                    });

                    if row_response.clicked() {
                        *self.selected_group = Some(id);
                    }
                }

                // A removal shifts every row after it, so an open dialog would
                // go on editing whatever slid into that index. It is closed
                // instead: silently editing a different rule than the one the ✏
                // was pressed on is worse than having to press it again.
                if let Some(open) = *self.editing
                    && to_remove.iter().any(|removed| *removed <= open)
                {
                    *self.editing = None;
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
            })
            .header_row(|r| {
                // The two columns worth ordering a long list by: where a rule
                // is, and which of them are switched off. The rest hold a
                // button apiece and there is nothing to order them by.
                r.cell(|ui| {
                    if sortable(ui, ordering, RuleColumn::Enabled, "On").clicked() {
                        reorder = Some(RuleColumn::Enabled);
                    }
                });
                r.cell(|ui| {
                    ui.label("Edit");
                });
                r.cell(|ui| {
                    ui.label("Clone");
                });
                r.cell(|ui| {
                    if sortable(ui, ordering, RuleColumn::Name, self.name_header).clicked() {
                        reorder = Some(RuleColumn::Name);
                    }
                });
                // The last two columns had no headings at all, so the reader
                // was left to work out what the mark and the bin were from the
                // rows alone.
                if row_warning.is_some() {
                    r.cell(|ui| {
                        ui.label("⚠");
                    });
                }
                r.cell(|ui| {
                    ui.label("Delete");
                });
            });

        if let Some(column) = reorder {
            self.order.clicked(column);
        }

        self.show_edit_dialog(ui, edit);
    }

    /// The dialog behind the ✏ on a row: the rule's name, and whatever the
    /// caller draws for its conditions.
    ///
    /// A centred modal rather than a window under the button. The window it
    /// replaced sized itself to its contents, which put a table of conditions
    /// wherever there happened to be room, and it closed on any click outside
    /// itself — including a click on one of its own combo boxes, which is what
    /// the space padded onto the bottom of it used to work around. A modal
    /// closes on the backdrop, on Escape and on its own button, and on nothing
    /// else.
    ///
    /// Escape is asked through [`escape_closes`] rather than through egui's own
    /// `ModalResponse::should_close`, which takes the key without caring what
    /// has the keyboard: pressing Escape to get out of a half-written rule name
    /// would then shut the dialog on the way. Asked here — inside the modal's
    /// own contents — it is taken before the Settings window offers it to
    /// Cancel, so the dialog goes and the window it stands in stays.
    fn show_edit_dialog(&mut self, ui: &mut Ui, mut edit: impl FnMut(&mut T, &mut Ui)) {
        let Some(index) = (*self.editing).filter(|i| *i < self.group_rules.len()) else {
            // A row edited and then deleted leaves the index dangling.
            *self.editing = None;
            return;
        };
        let rule = &mut self.group_rules[index];
        let name_header = self.name_header;

        let response = Modal::new(ui.id().with(("rule editor", self.title))).show(ui.ctx(), |ui| {
            // Both dimensions are pinned, and that is not decoration. A modal is
            // an area sized by what it holds, so a table inside it that asks for
            // a fixed height gets it, the modal grows to match, and next frame
            // there is that much more room to ask for — the dialog grew by one
            // row every frame without end, and on the frame after it opened the
            // budget collapsed to a few dozen points and the conditions table
            // had no room to draw a single row. Given a size of its own, the
            // contents divide a fixed budget instead of bidding against it.
            let room = ui.ctx().content_rect().size() - Vec2::splat(DIALOG_SCREEN_MARGIN);
            ui.set_width(EDIT_DIALOG_WIDTH.min(room.x).at_least(MATCHES_PANE_WIDTH));
            ui.set_height(EDIT_DIALOG_HEIGHT.min(room.y).at_least(ROW_HEIGHT * 6.0));

            ui.horizontal(|ui| {
                ui.strong(name_header);
                ui.add(
                    TextEdit::singleline(&mut rule.borrow_mut().name)
                        .desired_width(NAME_COLUMN_WIDTH),
                );
                ui.checkbox(&mut rule.borrow_mut().enabled, "Enabled");
            });
            if !rule.borrow().enabled {
                ui.label(
                    RichText::new(
                        "This rule is switched off, so it changes nothing yet. \
                         The matches below are what it would pick out.",
                    )
                    .weak(),
                );
            }
            ui.separator();

            // Keep the room the separator and the Close button below need, then
            // hand the rest to the conditions. They read it as
            // `ui.available_height()`, so neither has to be told a number that
            // would stop matching the window the moment either changed.
            let bottom_bar = ui.spacing().interact_size.y + ui.spacing().item_spacing.y * 3.0;
            let room = (ui.available_height() - bottom_bar).at_least(ROW_HEIGHT * 4.0);
            ui.allocate_ui(vec2(ui.available_width(), room), |ui| {
                edit(rule, ui);
            });

            ui.separator();
            let closed = ui.horizontal(|ui| ui.button("Close").clicked()).inner;
            closed || escape_closes(ui.ctx())
        });

        if response.inner || response.backdrop_response.clicked() {
            *self.editing = None;
        }
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
            combat: None,
            max_height: None,
        }
    }

    /// Show, beside the conditions, the names in `combat` they currently pick
    /// out. `None` for a caller with no combat to draw on.
    fn with_combat(mut self, combat: Option<&'a Combat>) -> Self {
        self.combat = combat;
        self
    }

    fn with_max_height(mut self, height: f32) -> Self {
        self.max_height = Some(height);
        self
    }

    /// The conditions, and — when a combat is selected — what they match in it,
    /// side by side. Seeing both at once is the point: a pattern is typed on the
    /// left and the list on the right answers on the same keystroke, so a rule
    /// is finished by looking rather than by guessing and re-running the log.
    fn show(&mut self, ui: &mut Ui) {
        let height = self
            .max_height
            .unwrap_or_else(|| ui.available_height())
            .at_least(ROW_HEIGHT * 5.0);
        let Some(combat) = self.combat else {
            self.show_conditions(height, ui);
            return;
        };

        // Each half is given a top-down layout of its own. `allocate_ui` hands
        // the child whatever layout its parent is in, which here is the
        // left-to-right one that puts the two halves side by side — so the
        // conditions table was laid out *beside* its own heading rather than
        // under it, on whatever width the heading had left, and its rows fell
        // outside the half and were never drawn.
        let column = Layout::top_down(Align::Min);
        ui.horizontal_top(|ui| {
            // The pane keeps a fixed slice of the width and the conditions take
            // the rest, so widening the window widens the part with the text
            // fields in it rather than the list of names beside it.
            let pane = MATCHES_PANE_WIDTH.min(ui.available_width() / 2.0);
            let conditions = (ui.available_width() - pane - ui.spacing().item_spacing.x * 3.0)
                .at_least(MATCHES_PANE_WIDTH);
            ui.allocate_ui_with_layout(vec2(conditions, height), column, |ui| {
                self.show_conditions(height, ui);
            });
            ui.separator();
            // Read after the conditions were drawn, so a character typed into a
            // pattern is already in them: gathered beforehand, the list beside
            // the field would always be one keystroke behind it.
            let aspects = self.match_aspect_set;
            ui.allocate_ui_with_layout(vec2(pane, height), column, |ui| {
                show_matches_pane(self.rules, aspects, combat, height, ui);
            });
        });
    }

    fn show_conditions(&mut self, height: f32, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(self.title);
            if ui.button("Add ✚").clicked() {
                self.rules.push(Default::default());
            }

            show_move_up_down(self.selected_rule, self.rules, ui);
        });
        ui.push_id(self.title, |ui| {
            Table::new(ui)
                .min_scroll_height(0.0)
                .max_scroll_height(height)
                // The pattern is what is typed and read here, so it takes the
                // width the pickers beside it do not need.
                .stretch_column(EXPRESSION_COLUMN, NAME_COLUMN_MIN_WIDTH)
                .cell_spacing(10.0)
                .header(HEADER_HEIGHT)
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
                                if ui.button("🗐").hover("Clone this condition").clicked() {
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
                                        MATCH_METHODS.iter().for_each(|m| {
                                            ui.selectable_value(&mut rule.method, *m, m.display())
                                                .hover(m.explanation());
                                        });
                                    })
                                    .response
                                    .hover(rule.method.explanation());
                            });

                            // Same floor as the rule's own name, and for the
                            // same reason: a condition added by ✚ has nothing
                            // in it yet to measure the column from.
                            r.measured_cell(|ui| {
                                TextEdit::singleline(&mut rule.expression)
                                    .desired_width(f32::MAX)
                                    // Wildcards work under every method, and
                                    // there is nothing on the row to say so —
                                    // an empty field is where a reader looks.
                                    .hint_text("text, or Quad*Cannons")
                                    .show(ui);
                                NAME_COLUMN_WIDTH
                            });

                            r.cell(|ui| {
                                if ui.steady_toggle(false, "🗑").clicked() {
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
                })
                .header_row(|r| {
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
                });
        });
    }
}

/// Every name of one kind that occurred in a combat.
///
/// Boxed because the five iterators are five different types; the lists are
/// hundreds of names at most, so the indirection costs nothing that shows.
fn names_of_aspect<'a>(
    aspect: MatchAspect,
    combat: &'a Combat,
) -> Box<dyn Iterator<Item = &'a str> + 'a> {
    let names = &combat.name_manager;
    match aspect {
        MatchAspect::SourceOrTargetName => Box::new(names.source_targets()),
        MatchAspect::SourceOrTargetUniqueName => Box::new(names.source_targets_unique()),
        MatchAspect::IndirectSourceName => Box::new(names.indirect_sources()),
        MatchAspect::IndirectUniqueSourceName => Box::new(names.indirect_sources_unique()),
        MatchAspect::DamageOrHealName => Box::new(names.values()),
    }
}

/// Whether one condition picks out `name` read as `aspect`. Each of these also
/// checks that the condition is switched on and is about that aspect in the
/// first place, so a condition about ability names says no to an entity name.
fn rule_matches_name(rule: &MatchRule, aspect: MatchAspect, name: &str) -> bool {
    match aspect {
        MatchAspect::SourceOrTargetName => rule.matches_source_or_target_name(name),
        MatchAspect::SourceOrTargetUniqueName => rule.matches_source_or_target_unique_name(name),
        MatchAspect::IndirectSourceName => rule.matches_indirect_source_name(name),
        MatchAspect::IndirectUniqueSourceName => rule.matches_indirect_source_unique_name(name),
        MatchAspect::DamageOrHealName => rule.matches_damage_or_heal_name(name),
    }
}

/// The names in `combat` that `rules` currently pick out, and how many were
/// looked at, one list per aspect the rule set can ask about.
///
/// Every aspect is reported even when it matches nothing: a rule that catches
/// nothing looks exactly like a rule that was never evaluated, and the count
/// beside the heading is what tells the two apart.
fn matches_by_aspect<'a>(
    rules: &[MatchRule],
    aspects: &[MatchAspect],
    combat: &'a Combat,
) -> Vec<(MatchAspect, Vec<&'a str>, usize)> {
    aspects
        .iter()
        .map(|aspect| {
            let mut considered = 0usize;
            let mut matched: Vec<&str> = names_of_aspect(*aspect, combat)
                .inspect(|_| considered += 1)
                .filter(|name| {
                    rules
                        .iter()
                        .any(|rule| rule_matches_name(rule, *aspect, name))
                })
                .collect();
            matched.sort_unstable();
            matched.dedup();
            (*aspect, matched, considered)
        })
        .collect()
}

/// What the standing bar under the rules table says.
///
/// Held from one frame to the next because the bar is drawn outside the
/// Settings window's scroll area, and the room for it has to be kept *before*
/// the tab is drawn — so the height is taken from what the bar said last frame.
/// One frame behind on the frame a rule is picked, which nothing can tell.
#[derive(Default, Clone, PartialEq)]
struct ClashFooter {
    /// The one-line summary: how many rules share an effect, or that none do.
    summary: String,
    /// Whether that summary is a warning or a plain statement.
    warn: bool,
    /// The picked rule's own case, spelled out.
    detail: Option<String>,
}

/// How many lines of the picked rule's case the standing bar shows before it
/// scrolls. Enough for a heading and the [`CLASHES_LISTED`] effects under it.
const CLASH_DETAIL_ROWS: f32 = 5.0;

/// How many clashing effects a warning names before it says "and N more".
const CLASHES_LISTED: usize = 3;

impl AnalysisSection {
    /// What the section is called where a reader is asked about it.
    const fn label(self) -> &'static str {
        match self {
            Self::CombatNames => "Combat Names",
            Self::SourceReversal => "Source Reversal",
            Self::CustomGrouping => "Custom Grouping",
            Self::DamageExclusion => "Damage Exclusion",
        }
    }

    /// The suggested file name for exporting this section on its own.
    fn file_name(self) -> String {
        format!("STO-CLARE_{}_Rules.toml", self.label().replace(' ', "-"))
    }

    /// Just this section's rules, as a file of the same shape as the whole one.
    /// One shape for both, so a file exported from a section can be imported
    /// into the whole tab and the other way round.
    fn taken_from(self, settings: &AnalysisSettings) -> RuleSets {
        let all = settings.rule_sets();
        let mut one = RuleSets::default();
        match self {
            Self::CombatNames => one.combat_name_rules = all.combat_name_rules,
            Self::SourceReversal => {
                one.indirect_source_grouping_revers_rules =
                    all.indirect_source_grouping_revers_rules
            }
            Self::CustomGrouping => one.custom_group_rules = all.custom_group_rules,
            Self::DamageExclusion => {
                one.damage_out_exclusion_rules = all.damage_out_exclusion_rules
            }
        }
        one
    }
}

/// Add `incoming` to what `settings` already holds, and say what happened.
///
/// **Added to, not put in place of.** A file of rules is something a player
/// fetches to have *as well as* their own, and an import that emptied four
/// lists would be the one action in the program capable of destroying an
/// evening's work in a click. (Cancel in Settings still undoes it either way —
/// nothing here is on disk until Ok.)
///
/// A rule identical to one already held is skipped rather than duplicated, and
/// the count of those is reported: silently doubling every rule on a second
/// import would leave a list nobody can read, and silently dropping them would
/// leave the reader wondering whether the file was read at all.
///
/// `sections` says which of the four to take, so the same routine serves the
/// per-section buttons and the whole-tab one.
fn import_rules(
    settings: &mut AnalysisSettings,
    incoming: RuleSets,
    sections: &[AnalysisSection],
) -> String {
    fn merge<T: PartialEq>(into: &mut Vec<T>, from: Vec<T>, added: &mut usize, same: &mut usize) {
        for rule in from {
            if into.contains(&rule) {
                *same += 1;
            } else {
                into.push(rule);
                *added += 1;
            }
        }
    }

    let mut lines = Vec::new();
    for section in sections {
        let (mut added, mut same) = (0usize, 0usize);
        match section {
            AnalysisSection::CombatNames => merge(
                &mut settings.combat_name_rules,
                incoming.combat_name_rules.clone(),
                &mut added,
                &mut same,
            ),
            AnalysisSection::SourceReversal => merge(
                &mut settings.indirect_source_grouping_revers_rules,
                incoming.indirect_source_grouping_revers_rules.clone(),
                &mut added,
                &mut same,
            ),
            AnalysisSection::CustomGrouping => merge(
                &mut settings.custom_group_rules,
                incoming.custom_group_rules.clone(),
                &mut added,
                &mut same,
            ),
            AnalysisSection::DamageExclusion => merge(
                &mut settings.damage_out_exclusion_rules,
                incoming.damage_out_exclusion_rules.clone(),
                &mut added,
                &mut same,
            ),
        }
        if added == 0 && same == 0 {
            continue;
        }
        let skipped = match same {
            0 => String::new(),
            n => format!(", {n} already there and skipped"),
        };
        lines.push(format!("{}: {added} added{skipped}", section.label()));
    }

    if lines.is_empty() {
        return "That file holds no rules for this section, so nothing was added.".to_string();
    }
    format!(
        "Added to your rules. Nothing is written until you press Ok — Cancel puts \
         it all back.\n\n{}",
        lines.join("\n")
    )
}

/// Which rules claim an effect that another rule also claims, and who takes it.
///
/// Keyed by rule name rather than by position, because the list is sorted and
/// positions move; two rules sharing a name are not a clash at all, since both
/// file the record under that same name whichever of them wins.
///
/// Only a clash between rules of *different* names matters, and only one the
/// selected combat can actually produce: a warning about two patterns that
/// could theoretically overlap, on effects no log contains, is noise.
///
/// Reported to both sides. Knowing a rule of yours quietly loses an effect to
/// another is the whole point — it is the case that used to be settled by
/// whichever rule happened to sit higher in the list, with nothing on screen
/// saying so.
fn clashing_rules(
    groups: &[RulesGroup],
    aspects: &[MatchAspect],
    combat: &Combat,
) -> FxHashMap<String, String> {
    // rule name -> effect names it shares with another rule, and who takes each
    let mut shared: FxHashMap<&str, Vec<(String, String)>> = FxHashMap::default();

    for aspect in aspects {
        for name in names_of_aspect(*aspect, combat) {
            let mut claiming: Vec<(Specificity, &RulesGroup)> = groups
                .iter()
                .filter_map(|group| Some((group.specificity_for(*aspect, name)?, group)))
                .collect();
            if claiming.len() < 2 {
                continue;
            }
            // The winner, by the same rule the analyzer uses: best fit, and a
            // tie settled by name.
            claiming.sort_by(|(left, left_group), (right, right_group)| {
                right
                    .cmp(left)
                    .then_with(|| left_group.name.cmp(&right_group.name))
            });
            let winner = claiming[0].1.name.as_str();
            if claiming.iter().all(|(_, group)| group.name == winner) {
                // All of them file it under the same name, so nothing is lost.
                continue;
            }
            for (_, group) in &claiming {
                shared
                    .entry(group.name.as_str())
                    .or_default()
                    .push((name.to_string(), winner.to_string()));
            }
        }
    }

    shared
        .into_iter()
        .map(|(rule, mut effects)| {
            effects.sort_unstable();
            effects.dedup();
            let listed: Vec<String> = effects
                .iter()
                .take(CLASHES_LISTED)
                .map(|(effect, winner)| {
                    if winner == rule {
                        format!("• {effect} — this rule takes it")
                    } else {
                        format!("• {effect} — goes to “{winner}”")
                    }
                })
                .collect();
            let more = effects.len().saturating_sub(CLASHES_LISTED);
            let tail = if more > 0 {
                format!("\n…and {more} more")
            } else {
                String::new()
            };
            (
                rule.to_string(),
                format!(
                    "Another rule catches some of the same effects in this combat. \
                     The more precisely fitting rule takes each one:\n{}{tail}",
                    listed.join("\n")
                ),
            )
        })
        .collect()
}

/// What a set of conditions picks out of the selected combat, right now.
///
/// This is the answer to the question a rule is written to ask, and it is here
/// so that it can be asked while the rule is being typed rather than after the
/// log has been re-read. A name is clickable and copies itself, so an exact
/// match can be built from what is on screen instead of from memory.
fn show_matches_pane(
    rules: &[MatchRule],
    aspects: &[MatchAspect],
    combat: &Combat,
    height: f32,
    ui: &mut Ui,
) {
    ui.strong("Matches in the selected combat");
    let found = matches_by_aspect(rules, aspects, combat);
    let total: usize = found.iter().map(|(_, matched, _)| matched.len()).sum();
    if total == 0 {
        ui.label(
            RichText::new(
                "Nothing yet. Add a condition, or widen the text it matches — \
                 the list fills in as you type.",
            )
            .weak(),
        );
    }

    ScrollArea::vertical()
        .id_salt("rule matches")
        .auto_shrink([false, true])
        .max_height((height - ROW_HEIGHT * 2.0).at_least(ROW_HEIGHT * 3.0))
        .show(ui, |ui| {
            for (aspect, matched, considered) in found {
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "{} — {} of {}",
                        aspect.display(),
                        matched.len(),
                        considered
                    ))
                    .weak(),
                );
                for name in matched {
                    if ui
                        .selectable_label(false, name)
                        .hover("Click to copy this name")
                        .clicked()
                    {
                        ui.ctx().copy_text(name.to_string());
                    }
                }
            }
        });
}

/// A column heading that orders the rows by its column.
///
/// Reads as a heading, not as a button — a row of buttons across the top of a
/// table reads as a toolbar — but it rims under the pointer so a reader finds
/// out it can be clicked by pointing at it, and it carries the mark saying
/// which way the order runs. The mark's room is kept whether or not it is
/// showing, so taking charge of the order does not shift the columns.
fn sortable(
    ui: &mut Ui,
    order: SortState<RuleColumn>,
    column: RuleColumn,
    label: &str,
) -> Response {
    let response = ui
        .selectable_label(order.is_sorted_by(column), label)
        .hover("Click to order the list by this column, again to turn it round");
    let marker = order.marker(column);
    if marker.is_empty() {
        ui.add_space(sort_marker_width(ui));
    } else {
        ui.label(marker);
    }
    response
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
mod editor_tests {
    use super::*;
    use crate::analyzer::Analyzer;

    /// Every piece of text the frame drew, so a test can ask what the reader
    /// would have on screen without a window server.
    fn drawn_text(shapes: &[epaint::ClippedShape]) -> Vec<String> {
        fn walk(shape: &Shape, found: &mut Vec<String>) {
            match shape {
                Shape::Text(text) => found.push(text.galley.text().to_string()),
                Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, found)),
                _ => (),
            }
        }

        let mut found = Vec::new();
        for clipped in shapes {
            walk(&clipped.shape, &mut found);
        }
        found
    }

    /// A screen big enough for the dialog to open on. Without one the context
    /// has no content rectangle, and a modal centred in nothing draws nothing.
    fn a_screen() -> RawInput {
        a_screen_of(1600.0)
    }

    fn a_screen_of(width: f32) -> RawInput {
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, 1000.0))),
            ..Default::default()
        }
    }

    fn escape() -> RawInput {
        let mut input = a_screen();
        input.events.push(Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        input
    }

    /// A fight with a player firing three differently named weapons, two of
    /// which are Quad Cannons. Parsed by the real analyzer, so the names the
    /// pane reads are the names the program would actually have.
    fn a_combat(dir: &str) -> Combat {
        let dir = std::env::temp_dir().join(dir);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("combatlog.log");

        let mut records = String::new();
        for (second, ability) in [
            (17, "Quad Disruptor Cannons"),
            (18, "Quad Phaser Cannons"),
            (19, "Terran Task Force Phaser Beam Array"),
        ] {
            records.push_str(&format!(
                "26:07:28:22:20:{second}.5::Kestrel,P[1@2 Kestrel@handle],,*,\
                 Talon Battleship,C[11759 Space_Nausicaan_Battleship],{ability},Pn.Cedjls,\
                 Phaser,,100.0,100.0\n"
            ));
        }
        std::fs::write(&log, &records).unwrap();

        let mut analyzer = Analyzer::new(AnalysisSettings {
            combatlog_file: log.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .unwrap();
        analyzer.update();
        let combat = analyzer.result().first().expect("one combat").clone();
        let _ = std::fs::remove_dir_all(&dir);
        combat
    }

    fn wildcard(expression: &str) -> MatchRule {
        MatchRule {
            aspect: MatchAspect::DamageOrHealName,
            expression: expression.to_string(),
            method: MatchMethod::StartsWith,
            enabled: true,
        }
    }

    /// The pane answers the question the rule is being written to ask: which of
    /// the names in this fight does it pick out.
    #[test]
    fn the_pane_lists_the_names_the_conditions_pick_out() {
        let combat = a_combat("cla-rule-editor-matches");
        let found = matches_by_aspect(
            &[wildcard("Quad*Cannons")],
            &[MatchAspect::DamageOrHealName],
            &combat,
        );
        let (aspect, matched, considered) = &found[0];

        assert_eq!(MatchAspect::DamageOrHealName, *aspect);
        assert_eq!(
            vec!["Quad Disruptor Cannons", "Quad Phaser Cannons"],
            *matched
        );
        assert!(
            *considered >= 3,
            "all three ability names should have been looked at, not {considered}"
        );
    }

    /// A rule that catches nothing must be distinguishable from one that was
    /// never asked, so the aspect is reported with a count of zero rather than
    /// left out of the list.
    #[test]
    fn an_aspect_that_matches_nothing_is_still_reported() {
        let combat = a_combat("cla-rule-editor-no-matches");
        let found = matches_by_aspect(
            &[wildcard("*Torpedo*")],
            &[
                MatchAspect::DamageOrHealName,
                MatchAspect::IndirectSourceName,
            ],
            &combat,
        );

        assert_eq!(2, found.len(), "both aspects are accounted for");
        assert!(found.iter().all(|(_, matched, _)| matched.is_empty()));
        assert!(
            found.iter().any(
                |(aspect, _, considered)| *aspect == MatchAspect::DamageOrHealName
                    && *considered > 0
            ),
            "the ability names were read, they just did not match"
        );
    }

    /// A condition about ability names must not be answered with entity names:
    /// the pane would then show a rule catching things it will never catch.
    #[test]
    fn a_condition_is_only_asked_about_its_own_aspect() {
        let combat = a_combat("cla-rule-editor-aspects");
        let found = matches_by_aspect(
            &[wildcard("*Talon*")],
            &[
                MatchAspect::DamageOrHealName,
                MatchAspect::SourceOrTargetName,
            ],
            &combat,
        );

        let matched = |wanted: MatchAspect| {
            found
                .iter()
                .find(|(aspect, _, _)| *aspect == wanted)
                .map(|(_, matched, _)| matched.clone())
                .unwrap()
        };
        assert!(
            matched(MatchAspect::DamageOrHealName).is_empty(),
            "the condition is about ability names, and no ability is called Talon"
        );
        assert!(
            matched(MatchAspect::SourceOrTargetName).is_empty(),
            "and it must not be answered with the entity that is"
        );
    }

    /// One frame of a rules table with a row's ✏ open.
    fn a_frame_with_the_dialog_open(
        ctx: &Context,
        input: RawInput,
        groups: &mut Vec<RulesGroup>,
        editing: &mut Option<usize>,
    ) -> Vec<String> {
        a_frame(ctx, input, groups, editing, None)
    }

    fn a_frame(
        ctx: &Context,
        input: RawInput,
        groups: &mut Vec<RulesGroup>,
        editing: &mut Option<usize>,
        combat: Option<&Combat>,
    ) -> Vec<String> {
        let mut selected = None;
        let mut selected_rule = None;
        let output = ctx.run_ui(input, |ui| {
            GroupRulesTable::new(
                groups,
                "Custom Grouping Rules",
                "Group Name",
                &mut selected,
                editing,
                &mut SortState::default(),
            )
            .show(ui, |group, ui| {
                RulesTable::new(
                    &mut group.rules,
                    "Match when any of these is true:",
                    &[MatchAspect::DamageOrHealName],
                    &mut selected_rule,
                )
                .with_combat(combat)
                .show(ui);
            });
        });
        drawn_text(&output.shapes)
    }

    fn a_group() -> Vec<RulesGroup> {
        vec![RulesGroup {
            name: "Quad Cannons".to_string(),
            rules: vec![wildcard("Quad*Cannons")],
            enabled: true,
        }]
    }

    /// The dialog draws the rule it was opened on, conditions and all.
    ///
    /// Two frames: egui measures a newly opened area on the first pass and
    /// paints nothing, so a single frame proves only that the dialog was asked
    /// for, not that anything reached the screen.
    #[test]
    fn the_dialog_shows_the_rule_it_was_opened_on() {
        let ctx = Context::default();
        let mut groups = a_group();
        let mut editing = Some(0);
        a_frame_with_the_dialog_open(&ctx, a_screen(), &mut groups, &mut editing);
        let text = a_frame_with_the_dialog_open(&ctx, a_screen(), &mut groups, &mut editing);

        assert!(
            text.iter().any(|t| t == "Match when any of these is true:"),
            "the conditions were not drawn; on screen were {text:?}"
        );
        assert!(
            text.iter().any(|t| t == "Close"),
            "the dialog had no way out of it"
        );
        assert_eq!(Some(0), editing, "and it is still open");
    }

    /// Escape puts the dialog away, and the row it belongs to is forgotten.
    #[test]
    fn escape_closes_the_dialog() {
        let ctx = Context::default();
        let mut groups = a_group();
        let mut editing = Some(0);
        // A pass with nothing focused first, so the key is not spent leaving a
        // text field — see `custom_widgets::dialog`.
        a_frame_with_the_dialog_open(&ctx, a_screen(), &mut groups, &mut editing);
        a_frame_with_the_dialog_open(&ctx, escape(), &mut groups, &mut editing);

        assert_eq!(None, editing);
    }

    /// The whole point of the dialog, end to end: with a combat selected, the
    /// names it picks out of that combat are on screen beside the conditions.
    #[test]
    fn the_dialog_shows_the_matches_beside_the_conditions() {
        let ctx = Context::default();
        let combat = a_combat("cla-rule-editor-dialog-matches");
        let mut groups = a_group();
        let mut editing = Some(0);
        a_frame(&ctx, a_screen(), &mut groups, &mut editing, Some(&combat));
        let text = a_frame(&ctx, a_screen(), &mut groups, &mut editing, Some(&combat));

        assert!(
            text.iter().any(|t| t == "Matches in the selected combat"),
            "the pane was not drawn; on screen were {text:?}"
        );
        assert!(
            text.iter().any(|t| t == "Quad Disruptor Cannons"),
            "the name the rule picks out was not listed; on screen were {text:?}"
        );
        assert!(
            !text
                .iter()
                .any(|t| t == "Terran Task Force Phaser Beam Array"),
            "a name the rule does not pick out was listed anyway"
        );
        assert!(
            text.iter().any(|t| t.contains("2 of ")),
            "the count of what was looked at is missing; on screen were {text:?}"
        );
    }

    /// A condition's own row has to be drawn beside the pane, not only its
    /// column headings.
    ///
    /// `allocate_ui` hands a child the layout its parent is in, and the two
    /// halves sit in a left-to-right one — so the conditions table was laid out
    /// *beside* its own heading, on the width the heading had left over, and
    /// every row fell outside the half and was clipped away. Column headings
    /// are drawn outside the table's scroll area and kept appearing, which is
    /// what made the dialog look merely empty rather than broken.
    #[test]
    fn a_conditions_row_is_drawn_beside_the_pane() {
        let ctx = Context::default();
        let combat = a_combat("cla-rule-editor-row-beside-pane");
        let mut groups = a_group();
        let mut editing = Some(0);
        a_frame(&ctx, a_screen(), &mut groups, &mut editing, Some(&combat));
        let text = a_frame(&ctx, a_screen(), &mut groups, &mut editing, Some(&combat));

        for wanted in ["Starts with", "Quad*Cannons"] {
            assert!(
                text.iter().any(|t| t == wanted),
                "the condition's row is missing {wanted:?}; on screen were {text:?}"
            );
        }
    }

    /// The dialog opens at one size and keeps it.
    ///
    /// A modal is an area sized by what it holds. A table inside one that asks
    /// for a fixed height gets it, the modal grows to match, and the next frame
    /// there is that much more room to ask for: the dialog grew by one row
    /// every frame without end, and on the frame right after it opened the
    /// budget collapsed far enough that the conditions table could not draw a
    /// single row.
    #[test]
    fn the_dialog_holds_its_size_instead_of_growing_every_frame() {
        /// The box around everything the frame wrote, which is what grows when
        /// the dialog does — the backdrop covers the screen either way.
        fn text_bounds(shapes: &[epaint::ClippedShape]) -> Rect {
            fn walk(shape: &Shape, found: &mut Rect) {
                match shape {
                    Shape::Text(text) => *found = found.union(text.visual_bounding_rect()),
                    Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, found)),
                    _ => (),
                }
            }
            let mut found = Rect::NOTHING;
            for clipped in shapes {
                walk(&clipped.shape, &mut found);
            }
            found
        }

        let ctx = Context::default();
        let combat = a_combat("cla-rule-editor-steady-size");
        let mut groups = a_group();
        let mut selected = None;
        let mut selected_rule = None;
        let mut editing = Some(0);

        let mut sizes = Vec::new();
        for _ in 0..12 {
            let output = ctx.run_ui(a_screen(), |ui| {
                GroupRulesTable::new(
                    &mut groups,
                    "Custom Grouping Rules",
                    "Group Name",
                    &mut selected,
                    &mut editing,
                    &mut SortState::default(),
                )
                .show(ui, |group, ui| {
                    RulesTable::new(
                        &mut group.rules,
                        "Match when any of these is true:",
                        &[MatchAspect::DamageOrHealName],
                        &mut selected_rule,
                    )
                    .with_combat(Some(&combat))
                    .show(ui);
                });
            });
            sizes.push(text_bounds(&output.shapes));
        }

        // From the third frame on: egui measures a new area on the first and
        // settles the table's column widths on the second, and both land on
        // whole pixels a point either way.
        let settled = sizes[2];
        for (frame, size) in sizes.iter().enumerate().skip(2) {
            assert!(
                (size.height() - settled.height()).abs() < 1.0,
                "frame {frame} came to {:.0} points tall against {:.0} on frame 2: the dialog is \
                 still growing",
                size.height(),
                settled.height()
            );
        }

        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1600.0, 1000.0));
        assert!(
            screen.contains_rect(settled),
            "the dialog reaches {settled:?}, outside the {screen:?} screen"
        );
    }

    /// The name column takes whatever width the window has left over.
    ///
    /// A table column is measured from its contents, so a rule just added by
    /// the ✚ button — which has no name yet — opened as a field a few
    /// characters wide beside an expanse of empty window, and every rule had to
    /// be widened by hand before it could be read. Measured before the fix: 73
    /// points.
    ///
    /// A name too long even for the filled column scrolls inside its own field,
    /// as text in a text field does; the table does not grow past the window,
    /// which would put the buttons on the row out of reach.
    #[test]
    fn the_name_column_takes_the_width_the_window_has_over() {
        use crate::custom_widgets::table::{table_column_widths, table_id};

        fn name_column(ctx: &Context, groups: &mut Vec<RulesGroup>) -> f32 {
            let mut selected = None;
            let mut editing = None;
            let mut widths = Vec::new();
            for _ in 0..4 {
                let _ = ctx.run_ui(a_screen(), |ui| {
                    let id = table_id(ui);
                    GroupRulesTable::new(
                        groups,
                        "Custom Grouping Rules",
                        "Group Name",
                        &mut selected,
                        &mut editing,
                        &mut SortState::default(),
                    )
                    .show(ui, |_, _| {});
                    widths = table_column_widths(ui, id);
                });
            }
            widths[NAME_COLUMN]
        }

        // The screen the test frame is given.
        const VIEW: f32 = 1600.0;

        let fresh = name_column(&Context::default(), &mut vec![RulesGroup::default()]);
        assert!(
            fresh >= NAME_COLUMN_WIDTH,
            "a rule with no name yet opens {fresh:.0} points wide, under the \
             {NAME_COLUMN_WIDTH:.0} it should start at"
        );
        assert!(
            fresh > VIEW * 0.6,
            "the name column came to {fresh:.0} of a {VIEW:.0}-point view — it is \
             not taking the width the buttons beside it do not need"
        );

        // A name far longer than the window does not widen the table: the
        // field fills its cell and the text scrolls inside it, the way a text
        // field always behaves. A table wider than the window it sits in would
        // have to be dragged sideways to reach the buttons on the row.
        let long = name_column(
            &Context::default(),
            &mut vec![RulesGroup {
                name: "Quad Disruptor Cannons - Rapid Fire III".repeat(6),
                ..Default::default()
            }],
        );
        assert_eq!(
            fresh.round(),
            long.round(),
            "a long name must not push the table out past the window"
        );
    }

    /// One frame of a rules table, reporting the order its rules ended up in.
    fn order_after_a_frame(
        ctx: &Context,
        groups: &mut Vec<RulesGroup>,
        selected: &mut Option<usize>,
        editing: &mut Option<usize>,
    ) -> Vec<String> {
        let _ = ctx.run_ui(a_screen(), |ui| {
            GroupRulesTable::new(
                groups,
                "Custom Grouping Rules",
                "Group Name",
                selected,
                editing,
                &mut SortState::default(),
            )
            .show(ui, |_, _| {});
        });
        groups.iter().map(|g| g.name.clone()).collect()
    }

    fn named(names: &[&str]) -> Vec<RulesGroup> {
        names
            .iter()
            .map(|name| RulesGroup {
                name: name.to_string(),
                ..Default::default()
            })
            .collect()
    }

    #[test]
    fn the_rules_are_listed_alphabetically() {
        let ctx = Context::default();
        let mut groups = named(&["Torpedoes", "beams", "Cannons"]);
        let order = order_after_a_frame(&ctx, &mut groups, &mut None, &mut None);

        assert_eq!(
            vec!["beams", "Cannons", "Torpedoes"],
            order,
            "case is ignored"
        );
    }

    /// A rule just added by ✚ has no name yet. Sorted as an empty string it
    /// would jump to the top of the list, away from the button that made it;
    /// it stays at the end until it is called something.
    #[test]
    fn a_rule_with_no_name_yet_stays_at_the_end() {
        let ctx = Context::default();
        let mut groups = named(&["Torpedoes", "", "Cannons"]);
        let order = order_after_a_frame(&ctx, &mut groups, &mut None, &mut None);

        assert_eq!(vec!["Cannons", "Torpedoes", ""], order);
    }

    /// Sorting moves rules; the selection has to move with them, or the ✏ and
    /// the arrows would act on whichever rule slid into that position.
    #[test]
    fn the_selection_follows_the_rule_it_was_on() {
        let ctx = Context::default();
        let mut groups = named(&["Torpedoes", "Cannons"]);
        let mut selected = Some(0); // Torpedoes
        order_after_a_frame(&ctx, &mut groups, &mut selected, &mut None);

        assert_eq!(Some(1), selected, "Torpedoes is now the second row");
        assert_eq!("Torpedoes", groups[selected.unwrap()].name);
    }

    /// Not while a name is being typed: a list that re-sorted on every
    /// keystroke would take the row out from under the cursor.
    #[test]
    fn nothing_is_reordered_while_a_field_is_being_typed_in() {
        let ctx = Context::default();
        let mut groups = named(&["Torpedoes", "Cannons"]);
        let mut selected = None;
        let mut editing = None;

        let order = {
            let _ = ctx.run_ui(a_screen(), |ui| {
                let mut text = String::new();
                ui.add(TextEdit::singleline(&mut text)).request_focus();
                GroupRulesTable::new(
                    &mut groups,
                    "Custom Grouping Rules",
                    "Group Name",
                    &mut selected,
                    &mut editing,
                    &mut SortState::default(),
                )
                .show(ui, |_, _| {});
            });
            groups.iter().map(|g| g.name.clone()).collect::<Vec<_>>()
        };

        assert_eq!(vec!["Torpedoes", "Cannons"], order);
    }

    /// Clicking a heading orders the list by that column; clicking it again
    /// turns the order round. Two columns are worth it — where a rule is, and
    /// which rules are switched off — and the rest hold a button apiece.
    #[test]
    fn a_heading_orders_the_list_by_its_column() {
        fn order(groups: &mut Vec<RulesGroup>, state: &mut SortState<RuleColumn>) -> Vec<String> {
            let ctx = Context::default();
            let mut selected = None;
            let mut editing = None;
            for _ in 0..3 {
                let _ = ctx.run_ui(a_screen(), |ui| {
                    GroupRulesTable::new(
                        groups,
                        "Custom Grouping Rules",
                        "Group Name",
                        &mut selected,
                        &mut editing,
                        state,
                    )
                    .show(ui, |_, _| {});
                });
            }
            groups.iter().map(|g| g.name.clone()).collect()
        }

        let off = |name: &str| RulesGroup {
            name: name.to_string(),
            enabled: false,
            ..Default::default()
        };
        let on = |name: &str| RulesGroup {
            name: name.to_string(),
            enabled: true,
            ..Default::default()
        };

        let mut groups = vec![on("Cannons"), off("Beams"), on("Torpedoes")];
        let mut state = SortState::default();

        assert_eq!(
            vec!["Beams", "Cannons", "Torpedoes"],
            order(&mut groups, &mut state),
            "by name to begin with"
        );

        state.clicked(RuleColumn::Name);
        assert_eq!(
            vec!["Beams", "Cannons", "Torpedoes"],
            order(&mut groups, &mut state),
            "picking the name column keeps that order"
        );

        state.clicked(RuleColumn::Name);
        assert_eq!(
            vec!["Torpedoes", "Cannons", "Beams"],
            order(&mut groups, &mut state),
            "clicking it again turns it round"
        );

        state.clicked(RuleColumn::Enabled);
        assert_eq!(
            vec!["Beams", "Cannons", "Torpedoes"],
            order(&mut groups, &mut state),
            "ordering by On puts the switched-off rules first — a long list is \
             scanned for what is not doing its job"
        );
    }

    /// Two rules claiming the same effect are flagged on both rows, saying
    /// which one takes it. Before the more-precise rule decided that, whichever
    /// sat higher in the list won and nothing on screen said so.
    #[test]
    fn two_rules_claiming_one_effect_are_both_flagged() {
        let combat = a_combat("cla-rule-editor-clash");
        let group = |name: &str, method: MatchMethod, expression: &str| RulesGroup {
            name: name.to_string(),
            enabled: true,
            rules: vec![MatchRule {
                aspect: MatchAspect::DamageOrHealName,
                expression: expression.to_string(),
                method,
                enabled: true,
            }],
        };
        let groups = vec![
            group("Everything Quad", MatchMethod::Contains, "Quad"),
            group(
                "Disruptor Cannons",
                MatchMethod::StartsWith,
                "Quad Disruptor Cannons",
            ),
        ];

        let clashes = clashing_rules(&groups, &[MatchAspect::DamageOrHealName], &combat);
        let broad = clashes
            .get("Everything Quad")
            .expect("the broad rule is flagged");
        let narrow = clashes
            .get("Disruptor Cannons")
            .expect("the narrow rule is flagged too");

        assert!(
            broad.contains("Quad Disruptor Cannons")
                && broad.contains("goes to “Disruptor Cannons”"),
            "the broad rule should be told it loses that effect: {broad}"
        );
        assert!(
            narrow.contains("this rule takes it"),
            "the narrow rule should be told it wins it: {narrow}"
        );
        assert!(
            !broad.contains("Quad Phaser Cannons"),
            "the effect only the broad rule catches is not a clash: {broad}"
        );
    }

    /// Two rules of the same name file a record under that name either way, so
    /// there is nothing to warn about. Both of the clashes in the maintainer's
    /// own 48 rules are of this kind.
    #[test]
    fn rules_sharing_a_name_are_not_a_clash() {
        let combat = a_combat("cla-rule-editor-same-name");
        let group = |method: MatchMethod, expression: &str| RulesGroup {
            name: "Quad Cannons".to_string(),
            enabled: true,
            rules: vec![MatchRule {
                aspect: MatchAspect::DamageOrHealName,
                expression: expression.to_string(),
                method,
                enabled: true,
            }],
        };
        let groups = vec![
            group(MatchMethod::Contains, "Quad"),
            group(MatchMethod::StartsWith, "Quad Disruptor"),
        ];

        assert!(
            clashing_rules(&groups, &[MatchAspect::DamageOrHealName], &combat).is_empty(),
            "two rules of one name cannot disagree about where a record goes"
        );
    }

    fn a_named_group(name: &str) -> RulesGroup {
        RulesGroup {
            name: name.to_string(),
            enabled: true,
            rules: vec![wildcard("Quad*Cannons")],
        }
    }

    /// An import adds to what is there. It is not a replacement: a file of
    /// rules is something a player fetches to have *as well as* their own, and
    /// an import that emptied four lists would be the one action in the program
    /// able to destroy an evening's work in a click.
    #[test]
    fn an_import_adds_rules_and_removes_none() {
        let mut settings = AnalysisSettings {
            custom_group_rules: vec![a_named_group("Mine")],
            ..Default::default()
        };
        let incoming = RuleSets {
            custom_group_rules: vec![a_named_group("Theirs")],
            ..Default::default()
        };

        let report = import_rules(&mut settings, incoming, &[AnalysisSection::CustomGrouping]);

        assert_eq!(
            vec!["Mine", "Theirs"],
            settings
                .custom_group_rules
                .iter()
                .map(|r| r.name.as_str())
                .collect::<Vec<_>>()
        );
        assert!(report.contains("Custom Grouping: 1 added"), "{report}");
    }

    /// A rule identical to one already held is skipped, and the count is said.
    /// Doubling every rule on a second import leaves a list nobody can read;
    /// dropping them without a word leaves the reader wondering whether the
    /// file was read at all.
    #[test]
    fn importing_the_same_file_twice_adds_nothing_and_says_so() {
        let mut settings = AnalysisSettings {
            custom_group_rules: vec![a_named_group("Shared")],
            ..Default::default()
        };
        let incoming = RuleSets {
            custom_group_rules: vec![a_named_group("Shared")],
            ..Default::default()
        };

        let report = import_rules(&mut settings, incoming, &[AnalysisSection::CustomGrouping]);

        assert_eq!(1, settings.custom_group_rules.len(), "nothing was doubled");
        assert!(
            report.contains("0 added") && report.contains("1 already there and skipped"),
            "{report}"
        );
    }

    /// Importing into one section leaves the other three alone, so a file of
    /// combat names dropped on the Custom Grouping tab does not quietly fill
    /// a list the reader was not looking at.
    #[test]
    fn an_import_touches_only_the_sections_asked_for() {
        let mut settings = AnalysisSettings::default();
        let incoming = RuleSets {
            custom_group_rules: vec![a_named_group("Grouping")],
            damage_out_exclusion_rules: vec![wildcard("*Torpedo*")],
            ..Default::default()
        };

        let report = import_rules(&mut settings, incoming, &[AnalysisSection::CustomGrouping]);

        assert_eq!(1, settings.custom_group_rules.len());
        assert!(
            settings.damage_out_exclusion_rules.is_empty(),
            "a section that was not asked for must not be touched"
        );
        assert!(!report.contains("Damage Exclusion"), "{report}");
    }

    /// A file with nothing for this section is a result, not a silence.
    #[test]
    fn an_import_that_brings_nothing_says_that_too() {
        let mut settings = AnalysisSettings::default();
        let report = import_rules(
            &mut settings,
            RuleSets::default(),
            &[AnalysisSection::CustomGrouping],
        );
        assert!(report.contains("no rules for this section"), "{report}");
    }

    /// A section exports the same shape of file as the whole tab, so a file
    /// from either can be imported into either.
    #[test]
    fn a_section_exports_only_itself_in_the_shared_shape() {
        let settings = AnalysisSettings {
            custom_group_rules: vec![a_named_group("Grouping")],
            damage_out_exclusion_rules: vec![wildcard("*Torpedo*")],
            ..Default::default()
        };
        let one = AnalysisSection::CustomGrouping.taken_from(&settings);

        assert_eq!(1, one.custom_group_rules.len());
        assert!(
            one.damage_out_exclusion_rules.is_empty(),
            "a section's file holds that section and nothing else"
        );
        assert_eq!(RULES_FILE_VERSION, one.version);
    }

    /// Dragged narrower, the table gives the width back out of the name column
    /// rather than standing its ground and pushing its own buttons off the
    /// edge. The reader can still reach ✏, 🗐 and 🗑 on every row.
    #[test]
    fn a_narrower_window_takes_the_width_out_of_the_name_column() {
        use crate::custom_widgets::table::{table_column_widths, table_id};

        /// Every column's width, after the table has settled, on a screen of
        /// this width.
        fn columns(screen: f32) -> Vec<f32> {
            let ctx = Context::default();
            let mut groups = named(&["Quad Disruptor Cannons"]);
            let mut selected = None;
            let mut editing = None;
            let mut widths = Vec::new();
            for _ in 0..4 {
                let _ = ctx.run_ui(a_screen_of(screen), |ui| {
                    let id = table_id(ui);
                    GroupRulesTable::new(
                        &mut groups,
                        "Custom Grouping Rules",
                        "Group Name",
                        &mut selected,
                        &mut editing,
                        &mut SortState::default(),
                    )
                    .show(ui, |_, _| {});
                    widths = table_column_widths(ui, id);
                });
            }
            widths
        }

        let wide = columns(1600.0);
        // Narrow enough that the columns cannot all have what they claim: the
        // stretched one has to give ground, or the buttons to its right are
        // pushed off the edge and can only be reached by dragging sideways.
        let narrow = columns(400.0);

        assert!(
            narrow[NAME_COLUMN] < wide[NAME_COLUMN],
            "the name column held {:.0} points on a 400-point screen against {:.0} on a \
             1600-point one",
            narrow[NAME_COLUMN],
            wide[NAME_COLUMN]
        );
        assert!(
            narrow[NAME_COLUMN] < NAME_COLUMN_WIDTH,
            "the name column stopped at its own claim of {NAME_COLUMN_WIDTH:.0} instead of \
             giving the width back — it came to {:.0} in a 400-point window",
            narrow[NAME_COLUMN]
        );
        assert!(
            narrow[NAME_COLUMN] >= NAME_COLUMN_MIN_WIDTH,
            "and it must not be squeezed past {NAME_COLUMN_MIN_WIDTH:.0}, but came to {:.0}",
            narrow[NAME_COLUMN]
        );

        // Every column but the name one keeps what it needs, so the buttons to
        // the right of the name are still on screen.
        for (index, (wide, narrow)) in wide.iter().zip(narrow.iter()).enumerate() {
            if index == NAME_COLUMN {
                continue;
            }
            assert_eq!(
                wide.round(),
                narrow.round(),
                "column {index} changed width when the window did; only the \
                 stretched one should"
            );
        }
    }

    /// The bar is drawn outside the window's scroll area, so the room for it
    /// has to be claimed before the tab is drawn. A height of zero would let
    /// the scroll area take that room and push the bar off the bottom.
    #[test]
    fn the_bar_asks_for_room_only_when_it_has_something_to_say() {
        let ctx = Context::default();
        let mut heights = Vec::new();
        let _ = ctx.run_ui(a_screen(), |ui| {
            let quiet = AnalysisTab::default();
            let speaking = AnalysisTab {
                footer: Some(ClashFooter {
                    summary: "2 of your rules share an effect".to_string(),
                    warn: true,
                    detail: Some("• Quad Disruptor Cannons — this rule takes it".to_string()),
                }),
                ..Default::default()
            };
            heights.push(quiet.footer_height(ui));
            heights.push(speaking.footer_height(ui));
        });

        assert_eq!(
            0.0, heights[0],
            "a tab with nothing to report keeps no room"
        );
        assert!(
            heights[1] > 0.0,
            "a tab with something to report has to claim room for it"
        );
    }

    /// The clash bar stands under the table and says what it has to say without
    /// being pointed at. A tooltip answers a question the reader thought to
    /// ask; two rules quietly sharing an effect is the case they did not.
    #[test]
    fn the_clash_bar_says_its_piece_without_being_pointed_at() {
        fn bar_text(
            clashes: &FxHashMap<String, String>,
            selected: Option<&str>,
            combat: Option<&Combat>,
        ) -> Vec<String> {
            let mut tab = AnalysisTab {
                footer: Some(clash_footer(clashes, selected, combat)),
                ..Default::default()
            };
            let ctx = Context::default();
            let mut text = Vec::new();
            for _ in 0..2 {
                let output = ctx.run_ui(a_screen(), |ui| tab.show_footer(ui));
                text = drawn_text(&output.shapes);
            }
            text
        }
        let joined = |text: Vec<String>| text.join(" | ");

        let combat = a_combat("cla-rule-editor-clash-bar");
        let mut clashes = FxHashMap::default();
        clashes.insert(
            "Everything Quad".to_string(),
            "Another rule catches some of the same effects".to_string(),
        );

        // Nothing to check against is not the same as nothing to report.
        let unchecked = joined(bar_text(&FxHashMap::default(), None, None));
        assert!(unchecked.contains("Select a combat"), "{unchecked}");

        let clean = joined(bar_text(&FxHashMap::default(), None, Some(&combat)));
        assert!(
            clean.contains("No two rules catch the same effect"),
            "a clean check has to say so, or it reads as unchecked: {clean}"
        );

        let flagged = joined(bar_text(&clashes, None, Some(&combat)));
        assert!(
            flagged.contains("1 of your rules share an effect"),
            "the count belongs on the bar, not only on a tooltip: {flagged}"
        );

        let picked = joined(bar_text(&clashes, Some("Everything Quad"), Some(&combat)));
        assert!(
            picked.contains("Another rule catches some of the same effects"),
            "picking the marked rule should spell out its own case: {picked}"
        );
    }

    /// A rule deleted while its dialog is open leaves an index pointing past the
    /// end of the list. Indexing with it would panic.
    #[test]
    fn a_dialog_whose_rule_was_deleted_closes_itself() {
        let ctx = Context::default();
        let mut groups = Vec::new();
        let mut editing = Some(0);
        a_frame_with_the_dialog_open(&ctx, a_screen(), &mut groups, &mut editing);

        assert_eq!(None, editing);
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
