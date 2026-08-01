//! Compare combats view.
//!
//! Lets the user pick up to a few combats (from the combats list, which already
//! spans the whole log directory via consolidation) and compare the outgoing
//! damage ability tree of a chosen player side by side, with +/- deltas against
//! the first (reference) combat.

use std::sync::Arc;

use eframe::egui::*;
use serde::{Deserialize, Serialize};

use crate::{
    analyzer::{Combat, DamageGroup, Difficulty},
    app::{settings::Settings, state::AppState},
};

mod compare_table;

use compare_table::Comparison;

/// Maximum number of combats that can be compared at once.
const MAX_COMBATS: usize = 3;

/// A metric that can be shown as a compare column. Serialized in the settings so
/// the user's chosen columns persist across restarts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompareMetric {
    Dps,
    TotalDamage,
    DamagePercentage,
    Resistance,
    Critical,
    Flanking,
    Accuracy,
    MaxOneHit,
    AverageHit,
    Hits,
    HitsPerSecond,
    BaseDps,
}

impl CompareMetric {
    /// Every metric that can be picked as a column, in menu order.
    pub const ALL: &'static [CompareMetric] = &[
        CompareMetric::Dps,
        CompareMetric::TotalDamage,
        CompareMetric::DamagePercentage,
        CompareMetric::Resistance,
        CompareMetric::Critical,
        CompareMetric::Flanking,
        CompareMetric::Accuracy,
        CompareMetric::MaxOneHit,
        CompareMetric::AverageHit,
        CompareMetric::Hits,
        CompareMetric::HitsPerSecond,
        CompareMetric::BaseDps,
    ];

    pub fn label(self) -> &'static str {
        match self {
            CompareMetric::Dps => "DPS",
            CompareMetric::TotalDamage => "Total Damage",
            CompareMetric::DamagePercentage => "Damage %",
            CompareMetric::Resistance => "Resistance %",
            CompareMetric::Critical => "Critical %",
            CompareMetric::Flanking => "Flanking %",
            CompareMetric::Accuracy => "Accuracy %",
            CompareMetric::MaxOneHit => "Max One-Hit",
            CompareMetric::AverageHit => "Average Hit",
            CompareMetric::Hits => "Hits",
            CompareMetric::HitsPerSecond => "Hits/s",
            CompareMetric::BaseDps => "Base DPS",
        }
    }

    pub fn precision(self) -> usize {
        match self {
            CompareMetric::DamagePercentage
            | CompareMetric::Resistance
            | CompareMetric::Critical
            | CompareMetric::Flanking
            | CompareMetric::Accuracy => 2,
            CompareMetric::HitsPerSecond => 1,
            _ => 0,
        }
    }

    /// Whether a higher value is an improvement (drives the delta color). For
    /// resistance the damage faced, lower is better (matches the single-combat
    /// view sorting resistance ascending).
    pub fn higher_is_better(self) -> bool {
        !matches!(self, CompareMetric::Resistance)
    }

    /// Pull this metric out of a damage group; `None` when it does not apply.
    pub fn extract(self, group: &DamageGroup) -> Option<f64> {
        match self {
            CompareMetric::Dps => Some(group.dps.all),
            CompareMetric::TotalDamage => Some(group.total_damage.all),
            CompareMetric::DamagePercentage => group.damage_percentage.all,
            CompareMetric::Resistance => group.damage_resistance_percentage,
            CompareMetric::Critical => group.critical_percentage,
            CompareMetric::Flanking => group.flanking,
            CompareMetric::Accuracy => group.accuracy_percentage,
            CompareMetric::MaxOneHit => Some(group.max_one_hit.damage),
            CompareMetric::AverageHit => group.average_hit.all,
            CompareMetric::Hits => Some(group.damage_metrics.hits.all as f64),
            CompareMetric::HitsPerSecond => Some(group.hits_per_second.all),
            CompareMetric::BaseDps => Some(group.base_dps),
        }
    }
}

/// Persisted compare settings (the chosen columns).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CompareSettings {
    pub columns: Vec<CompareMetric>,
}

impl Default for CompareSettings {
    fn default() -> Self {
        Self {
            columns: vec![
                CompareMetric::Dps,
                CompareMetric::Resistance,
                CompareMetric::Critical,
                CompareMetric::Accuracy,
            ],
        }
    }
}

/// The difficulty picker above the combat list. `Any` matches everything;
/// `Unknown` catches combats whose tier could not be worked out, which would
/// otherwise be invisible under every other setting.
#[derive(PartialEq, Clone, Copy)]
enum DifficultyFilter {
    Any,
    Normal,
    Advanced,
    Elite,
    Unknown,
}

impl DifficultyFilter {
    const ALL: &'static [(DifficultyFilter, &'static str)] = &[
        (DifficultyFilter::Any, "Any"),
        (DifficultyFilter::Normal, "Normal"),
        (DifficultyFilter::Advanced, "Advanced"),
        (DifficultyFilter::Elite, "Elite"),
        (DifficultyFilter::Unknown, "Unknown"),
    ];

    fn matches(self, difficulty: Option<Difficulty>) -> bool {
        match self {
            DifficultyFilter::Any => true,
            DifficultyFilter::Normal => difficulty == Some(Difficulty::Normal),
            DifficultyFilter::Advanced => difficulty == Some(Difficulty::Advanced),
            DifficultyFilter::Elite => difficulty == Some(Difficulty::Elite),
            // `Difficulty::Any` means a known map whose tier was not resolved,
            // so it reads as unknown here just like a missing value.
            DifficultyFilter::Unknown => {
                difficulty.is_none() || difficulty == Some(Difficulty::Any)
            }
        }
    }
}

pub struct CompareView {
    open: bool,
    selected: Vec<usize>,
    name_filter: String,
    type_filter: Option<String>,
    difficulty_filter: DifficultyFilter,
    comparison: Option<Comparison>,
}

impl Default for CompareView {
    fn default() -> Self {
        Self {
            open: false,
            selected: Vec::new(),
            name_filter: String::new(),
            type_filter: None,
            difficulty_filter: DifficultyFilter::Any,
            comparison: None,
        }
    }
}

impl CompareView {
    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
    }

    /// Receive the combats fetched for comparison and build the table.
    pub fn set_combats(&mut self, combats: Vec<(usize, Arc<Combat>)>, settings: &Settings) {
        self.comparison = Some(Comparison::new(combats, &settings.compare.columns));
    }

    pub fn show(
        &mut self,
        state: &mut AppState,
        combats: &[String],
        difficulties: &[Option<Difficulty>],
        base_names: &[String],
        ui: &mut Ui,
    ) {
        match &mut self.comparison {
            Some(comparison) => {
                if ui.button("◀ Change selection").clicked() {
                    self.comparison = None;
                    ui.separator();
                    self.show_selection(state, combats, difficulties, base_names, ui);
                } else {
                    ui.separator();
                    comparison.show(ui, &mut state.settings);
                }
            }
            None => self.show_selection(state, combats, difficulties, base_names, ui),
        }
    }

    fn show_selection(
        &mut self,
        state: &mut AppState,
        combats: &[String],
        difficulties: &[Option<Difficulty>],
        base_names: &[String],
        ui: &mut Ui,
    ) {
        ui.horizontal_wrapped(|ui| {
            ui.label("Search:");
            ui.text_edit_singleline(&mut self.name_filter);

            let selected_type = self.type_filter.clone().unwrap_or_else(|| "All".to_string());
            ComboBox::new("compare type filter", "Type")
                .selected_text(selected_type)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.type_filter, None, "All");
                    for combat_type in combat_types(base_names) {
                        ui.selectable_value(
                            &mut self.type_filter,
                            Some(combat_type.clone()),
                            combat_type,
                        );
                    }
                });

            ui.label("Difficulty:");
            for &(filter, label) in DifficultyFilter::ALL {
                ui.selectable_value(&mut self.difficulty_filter, filter, label);
            }
        });

        ui.horizontal(|ui| {
            ui.label(format!("Selected {}/{}", self.selected.len(), MAX_COMBATS));
            if ui
                .add_enabled(self.selected.len() >= 2, Button::new("Compare selected 🆚"))
                .clicked()
            {
                let mut indices = self.selected.clone();
                indices.sort_unstable();
                state.analysis_handler.get_combats(indices);
            }
            if !self.selected.is_empty() && ui.button("Clear selection").clicked() {
                self.selected.clear();
            }
        });

        ui.separator();

        ScrollArea::vertical().show(ui, |ui| {
            for (i, identifier) in combats.iter().enumerate() {
                let difficulty = difficulties.get(i).copied().flatten();
                let base_name = base_names.get(i).map(String::as_str).unwrap_or("");
                if !self.matches_filters(identifier, base_name, difficulty) {
                    continue;
                }
                let mut checked = self.selected.contains(&i);
                if ui.checkbox(&mut checked, identifier).clicked() {
                    self.toggle_selected(i, checked);
                }
            }
        });
    }

    fn toggle_selected(&mut self, index: usize, checked: bool) {
        if checked {
            if self.selected.len() < MAX_COMBATS && !self.selected.contains(&index) {
                self.selected.push(index);
            }
        } else {
            self.selected.retain(|&i| i != index);
        }
    }

    fn matches_filters(
        &self,
        identifier: &str,
        base_name: &str,
        difficulty: Option<Difficulty>,
    ) -> bool {
        if !self.name_filter.trim().is_empty()
            && !identifier
                .to_lowercase()
                .contains(&self.name_filter.trim().to_lowercase())
        {
            return false;
        }

        if let Some(type_filter) = &self.type_filter {
            if base_name != type_filter {
                return false;
            }
        }

        self.difficulty_filter.matches(difficulty)
    }
}

/// Distinct combat types across the list, sorted, for the type filter dropdown.
fn combat_types(base_names: &[String]) -> Vec<String> {
    let mut types: Vec<String> = base_names.to_vec();
    types.sort_unstable();
    types.dedup();
    types
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combat_types_are_sorted_and_unique() {
        let base_names = vec![
            "Trouble Over Terrh".to_string(),
            "Combat".to_string(),
            "Trouble Over Terrh".to_string(),
        ];
        assert_eq!(combat_types(&base_names), vec!["Combat", "Trouble Over Terrh"]);
    }

    /// A rule whose own name carries a bracket used to be cut in half by the
    /// string surgery that reconstructed the type from the display name.
    #[test]
    fn a_bracket_in_the_name_no_longer_splits_the_type() {
        let base_names = vec!["Bug Hunt (Ground) practice".to_string()];
        assert_eq!(combat_types(&base_names), vec!["Bug Hunt (Ground) practice"]);
    }

    #[test]
    fn unknown_matches_both_a_missing_tier_and_an_unresolved_one() {
        assert!(DifficultyFilter::Unknown.matches(None));
        assert!(DifficultyFilter::Unknown.matches(Some(Difficulty::Any)));
        assert!(!DifficultyFilter::Unknown.matches(Some(Difficulty::Normal)));
        assert!(DifficultyFilter::Normal.matches(Some(Difficulty::Normal)));
        assert!(DifficultyFilter::Any.matches(None));
    }
}
