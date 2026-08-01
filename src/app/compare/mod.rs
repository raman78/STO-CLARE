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
    app::{
        combat_filter::{CombatEntry, CombatFilter},
        settings::Settings,
        state::AppState,
    },
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
    /// Whether to split each DPS difference into the part that came from
    /// firing more often and the part that came from each hit landing harder.
    #[serde(default)]
    pub show_dps_breakdown: bool,
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
            show_dps_breakdown: false,
        }
    }
}

pub struct CompareView {
    open: bool,
    selected: Vec<usize>,
    name_filter: String,
    /// The same environment/level/map pickers the main window uses, so both
    /// lists are filtered the same way and mean the same by each choice.
    filter: CombatFilter,
    comparison: Option<Comparison>,
}

impl Default for CompareView {
    fn default() -> Self {
        Self {
            open: false,
            selected: Vec::new(),
            name_filter: String::new(),
            filter: CombatFilter::default(),
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
        environments: &[Option<String>],
        ui: &mut Ui,
    ) {
        match &mut self.comparison {
            Some(comparison) => {
                if ui.button("◀ Change selection").clicked() {
                    self.comparison = None;
                    ui.separator();
                    self.show_selection(state, combats, difficulties, base_names, environments, ui);
                } else {
                    ui.separator();
                    comparison.show(ui, &mut state.settings);
                }
            }
            None => {
                self.show_selection(state, combats, difficulties, base_names, environments, ui)
            }
        }
    }

    fn show_selection(
        &mut self,
        state: &mut AppState,
        combats: &[String],
        difficulties: &[Option<Difficulty>],
        base_names: &[String],
        environments: &[Option<String>],
        ui: &mut Ui,
    ) {
        let entries: Vec<CombatEntry> = (0..combats.len())
            .map(|i| CombatEntry {
                environment: environments.get(i).and_then(|e| e.as_deref()),
                difficulty: difficulties.get(i).copied().flatten(),
                base_name: base_names.get(i).map(String::as_str).unwrap_or(""),
            })
            .collect();

        ui.horizontal_wrapped(|ui| {
            ui.label("Search:");
            ui.text_edit_singleline(&mut self.name_filter);
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Show only:");
            self.filter.show("compare", &entries, ui);
            if self.filter.is_active() && ui.button("Clear filter").clicked() {
                self.filter.clear();
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
                if !self.matches_filters(identifier, entries[i]) {
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

    fn matches_filters(&self, identifier: &str, entry: CombatEntry) -> bool {
        if !self.name_filter.trim().is_empty()
            && !identifier
                .to_lowercase()
                .contains(&self.name_filter.trim().to_lowercase())
        {
            return false;
        }

        self.filter
            .matches(entry.environment, entry.difficulty, entry.base_name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> CombatEntry<'static> {
        CombatEntry {
            environment: Some("Space"),
            difficulty: Some(Difficulty::Elite),
            base_name: "Infected Space",
        }
    }

    /// The search box matches the whole displayed identifier, so a date or a
    /// time narrows the list as well as a name does.
    #[test]
    fn the_search_box_matches_the_displayed_identifier() {
        let mut view = CompareView::default();
        let identifier = "Infected Space [Elite] | 2026-07-23 20:07:22 - 20:11:37";

        view.name_filter = "infected".to_string();
        assert!(view.matches_filters(identifier, entry()));

        view.name_filter = "20:07".to_string();
        assert!(view.matches_filters(identifier, entry()));

        view.name_filter = "hive".to_string();
        assert!(!view.matches_filters(identifier, entry()));
    }

    /// Search and pickers narrow together, not one or the other.
    #[test]
    fn the_search_box_and_the_pickers_both_apply() {
        let mut view = CompareView::default();
        view.name_filter = "infected".to_string();
        view.filter.environment = Some("Ground".to_string());
        assert!(!view.matches_filters("Infected Space | t", entry()));
    }
}
