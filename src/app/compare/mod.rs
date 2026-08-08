//! Compare combats view.
//!
//! Lets the user pick combats (from the combats list, which already spans the
//! whole log directory via consolidation) and compare the outgoing damage
//! ability tree of a chosen player side by side, with +/- deltas against the
//! first (reference) combat — or averaged over the lot of them.
//!
//! Nothing caps the number picked: a session's worth of one map is a fair thing
//! to ask of it, and what gives way past a certain size (chart colours, memory)
//! is said in the picker rather than forbidden.

use std::sync::Arc;

use chrono::NaiveDateTime;
use eframe::{Frame, egui::*};
use serde::{Deserialize, Serialize};

use crate::{
    analyzer::{Combat, DamageGroup, Difficulty},
    app::{
        combat_filter::{CombatEntry, CombatFilter},
        settings::{CombatNotes, Settings},
        state::AppState,
        theme,
    },
};

mod compare_table;
mod date_range;

use compare_table::Comparison;
use date_range::DateRange;

/// Past this many combats the chart's line colours start over (the palette
/// holds eight), so the legend says which line is which but the colours no
/// longer do.
const COLORS_RUN_OUT_AT: usize = 8;

/// Past this many combats a comparison is worth a word of warning: it is a
/// column per combat per metric, and every combat is held in full while the
/// table is up. Measured on a real log, a 7-minute Elite run costs about 3.5 MB
/// of that, so the number is about how much table can be read at once rather
/// than about running out of memory.
const MANY_COMBATS: usize = 50;

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
    /// Whether the columns collapse into one averaged column per metric
    /// instead of one column per combat.
    #[serde(default)]
    pub show_averages: bool,
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
            show_averages: false,
        }
    }
}

#[derive(Default)]
pub struct CompareView {
    open: bool,
    selected: Vec<usize>,
    name_filter: String,
    /// The same environment/level/map pickers the main window uses, so both
    /// lists are filtered the same way and mean the same by each choice.
    filter: CombatFilter,
    /// When the combats were played — the compare view's own filter, since it
    /// is where a whole session gets picked out at once.
    range: DateRange,
    comparison: Option<Comparison>,
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
        self.comparison = Some(Comparison::new(combats, settings));
    }

    // Drawing context threaded through; a struct of the same fields would
    // only move the list somewhere else.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        state: &mut AppState,
        combats: &[String],
        difficulties: &[Option<Difficulty>],
        base_names: &[String],
        environments: &[Option<String>],
        start_times: &[NaiveDateTime],
        ui: &mut Ui,
        frame: &Frame,
    ) {
        match &mut self.comparison {
            Some(comparison) => {
                if ui.button("◀ Change selection").clicked() {
                    self.comparison = None;
                    // Going back to the picker is the other moment the list can
                    // be out of date — a run finished while the table was up.
                    state.analysis_handler.refresh_combats_list();
                    ui.separator();
                    self.show_selection(
                        state,
                        combats,
                        difficulties,
                        base_names,
                        environments,
                        start_times,
                        ui,
                    );
                } else {
                    ui.separator();
                    comparison.show(ui, &mut state.settings, frame);
                }
            }
            None => self.show_selection(
                state,
                combats,
                difficulties,
                base_names,
                environments,
                start_times,
                ui,
            ),
        }
    }

    // Drawing context threaded through; a struct of the same fields would
    // only move the list somewhere else.
    #[allow(clippy::too_many_arguments)]
    fn show_selection(
        &mut self,
        state: &mut AppState,
        combats: &[String],
        difficulties: &[Option<Difficulty>],
        base_names: &[String],
        environments: &[Option<String>],
        start_times: &[NaiveDateTime],
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
            self.range.show(
                start_times
                    .first()
                    .copied()
                    .zip(start_times.last().copied()),
                ui,
            );
            if (self.filter.is_active() || self.range.is_active())
                && ui.button("Clear filter").clicked()
            {
                self.filter.clear();
                self.range.clear();
            }
        });

        let notes: Vec<&str> = (0..combats.len())
            .map(|i| {
                start_times
                    .get(i)
                    .map(|&start| state.settings.combat_notes.get(&CombatNotes::key_at(start)))
                    .unwrap_or("")
            })
            .collect();
        let visible = self.visible_combats(combats, &notes, &entries, start_times);

        ui.horizontal_wrapped(|ui| {
            ui.label(format!("Selected {}", self.selected.len()));
            // Adds to the selection rather than replacing it, so a selection
            // can be built up from one filtered list after another. Only what
            // the filters actually left in — a ticked combat that is only on
            // screen because it is ticked is already in the selection anyway.
            let matching: Vec<usize> = visible
                .iter()
                .filter(|(_, matches)| *matches)
                .map(|(i, _)| *i)
                .collect();
            if !matching.is_empty()
                && ui
                    .button("Select all")
                    .on_hover_text("Add every combat the filters above leave in the list")
                    .clicked()
            {
                for i in matching {
                    self.toggle_selected(i, true);
                }
            }
            if !self.selected.is_empty() && ui.button("Clear selection").clicked() {
                self.selected.clear();
            }
            if let Some(hint) = selection_hint(self.selected.len()) {
                ui.label(RichText::new(hint).weak());
            }
        });

        // A line of its own: it is what the picker is for, and in the row above
        // it sat between the two buttons that only tick and untick.
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.selected.len() >= 2, Button::new("Compare selected 🆚"))
                .clicked()
            {
                let mut indices = self.selected.clone();
                indices.sort_unstable();
                state.analysis_handler.get_combats(indices);
            }
        });

        ui.separator();

        ScrollArea::vertical().show(ui, |ui| {
            for (i, matches) in visible {
                let note = notes[i];
                let mut checked = self.selected.contains(&i);
                let label = if note.is_empty() {
                    combats[i].clone()
                } else {
                    format!("{} — {note}", combats[i])
                };
                ui.horizontal(|ui| {
                    if ui.checkbox(&mut checked, label).clicked() {
                        self.toggle_selected(i, checked);
                    }
                    if !matches {
                        ui.colored_label(theme::palette().warn, "⚠").on_hover_text(
                            "Ticked, but it does not match the filters above — it is shown so a \
                             combat cannot go into a comparison out of sight. Untick it, or \
                             widen the filters to see it in place.",
                        );
                    }
                });
            }
        });
    }

    /// The combats the list shows, newest first, each with whether it matches
    /// the filters.
    ///
    /// A ticked combat is always shown, matching or not. It is going into the
    /// comparison either way, and a filter that hid it — switching the level
    /// from Any to Elite over a selection that holds an Advanced run — would
    /// leave the user comparing a run they cannot see and cannot untick.
    ///
    /// Newest first, like the combats dropdown in the main window: the analyzer
    /// hands the list over oldest first, so a plain walk buried the runs just
    /// played under a log's worth of older ones.
    fn visible_combats(
        &self,
        combats: &[String],
        notes: &[&str],
        entries: &[CombatEntry],
        start_times: &[NaiveDateTime],
    ) -> Vec<(usize, bool)> {
        (0..combats.len())
            .rev()
            .filter_map(|i| {
                let matches = self.matches_filters(
                    &combats[i],
                    notes[i],
                    entries[i],
                    start_times.get(i).copied(),
                );
                (matches || self.selected.contains(&i)).then_some((i, matches))
            })
            .collect()
    }

    fn toggle_selected(&mut self, index: usize, checked: bool) {
        if checked {
            if !self.selected.contains(&index) {
                self.selected.push(index);
            }
        } else {
            self.selected.retain(|&i| i != index);
        }
    }

    /// The search box reads the note as well as the identifier, so a run can be
    /// found by what the user called it.
    fn matches_filters(
        &self,
        identifier: &str,
        note: &str,
        entry: CombatEntry,
        start: Option<NaiveDateTime>,
    ) -> bool {
        let needle = self.name_filter.trim().to_lowercase();
        if !needle.is_empty()
            && !identifier.to_lowercase().contains(&needle)
            && !note.to_lowercase().contains(&needle)
        {
            return false;
        }

        // A combat the list has no start time for is left in: the window is
        // about picking a session out, not about hiding what it cannot place.
        if let Some(start) = start
            && !self.range.matches(start)
        {
            return false;
        }

        self.filter
            .matches(entry.environment, entry.difficulty, entry.base_name)
    }
}

/// What is worth saying about a selection this size, if anything. Nothing caps
/// the count any more, so the two things that do give way say so themselves.
fn selection_hint(selected: usize) -> Option<&'static str> {
    if selected > MANY_COMBATS {
        Some(
            "— a comparison this wide takes a moment to build, and reads best with the averages \
             turned on; the chart's line colours start over past eight",
        )
    } else if selected > COLORS_RUN_OUT_AT {
        Some("— past eight combats the chart's line colours start over")
    } else {
        None
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

    fn at(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").unwrap()
    }

    /// The search box matches the whole displayed identifier, so a date or a
    /// time narrows the list as well as a name does.
    #[test]
    fn the_search_box_matches_the_displayed_identifier() {
        let mut view = CompareView::default();
        let identifier = "Infected Space [Elite] | 2026-07-23 20:07:22 - 20:11:37";

        view.name_filter = "infected".to_string();
        assert!(view.matches_filters(identifier, "", entry(), None));

        view.name_filter = "20:07".to_string();
        assert!(view.matches_filters(identifier, "", entry(), None));

        view.name_filter = "hive".to_string();
        assert!(!view.matches_filters(identifier, "", entry(), None));
    }

    /// A combat can also be found by the note the user wrote for it, which is
    /// the point of writing one.
    #[test]
    fn the_search_box_matches_the_note() {
        let mut view = CompareView::default();
        let identifier = "Infected Space [Elite] | 2026-07-23 20:07:22 - 20:11:37";

        view.name_filter = "cheops".to_string();
        assert!(view.matches_filters(identifier, "Cheops build", entry(), None));
        assert!(!view.matches_filters(identifier, "FAW build", entry(), None));
    }

    /// Search and pickers narrow together, not one or the other.
    #[test]
    fn the_search_box_and_the_pickers_both_apply() {
        let mut view = CompareView {
            name_filter: "infected".to_string(),
            ..Default::default()
        };
        view.filter.environment = Some("Ground".to_string());
        assert!(!view.matches_filters("Infected Space | t", "", entry(), None));
    }

    /// The date window narrows alongside the rest, so "this evening's Infected
    /// runs" is one list rather than a search through a month of them.
    #[test]
    fn the_date_window_narrows_with_the_other_filters() {
        let mut view = CompareView::default();
        view.range.set("2026-07-23 20:00", "2026-07-23 23:00");

        assert!(view.matches_filters("Infected Space", "", entry(), Some(at("2026-07-23 21:30"))));
        assert!(!view.matches_filters("Infected Space", "", entry(), Some(at("2026-07-22 21:30"))));
        // A combat the list could not place is left in rather than hidden.
        assert!(view.matches_filters("Infected Space", "", entry(), None));
    }

    /// Nothing caps the selection any more: comparing a whole evening is the
    /// point of "Select all", and three columns would not hold one.
    #[test]
    fn any_number_of_combats_can_be_selected() {
        let mut view = CompareView::default();
        for i in 0..12 {
            view.toggle_selected(i, true);
        }
        assert_eq!(12, view.selected.len());

        // Selecting the same combat twice is still one combat.
        view.toggle_selected(5, true);
        assert_eq!(12, view.selected.len());

        view.toggle_selected(5, false);
        assert_eq!(11, view.selected.len());
        assert!(!view.selected.contains(&5));
    }

    fn list(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| n.to_string()).collect()
    }

    /// A ticked combat stays in the list whatever the filters say. Narrowing
    /// the level to Elite over a selection holding an Advanced run used to hide
    /// that run while it was still going into the comparison — invisible, and
    /// impossible to untick.
    #[test]
    fn a_ticked_combat_is_never_hidden_by_a_filter() {
        let combats = list(&["Infected Elite", "Infected Advanced"]);
        let notes = ["", ""];
        let entries = [
            CombatEntry {
                environment: Some("Space"),
                difficulty: Some(Difficulty::Elite),
                base_name: "Infected Space",
            },
            CombatEntry {
                environment: Some("Space"),
                difficulty: Some(Difficulty::Advanced),
                base_name: "Infected Space",
            },
        ];
        let start_times = [at("2026-07-23 20:00"), at("2026-07-23 21:00")];

        let mut view = CompareView::default();
        view.filter.difficulty = crate::app::combat_filter::DifficultyFilter::Elite;

        // Unticked, the Advanced run is simply filtered out.
        let visible = view.visible_combats(&combats, &notes, &entries, &start_times);
        assert_eq!(vec![(0, true)], visible);

        // Ticked, it stays — and is marked as not matching.
        view.toggle_selected(1, true);
        let visible = view.visible_combats(&combats, &notes, &entries, &start_times);
        assert_eq!(vec![(1, false), (0, true)], visible, "newest first");
    }

    /// The same for the date window, which is the other way a selected combat
    /// can fall out of the list.
    #[test]
    fn a_ticked_combat_survives_the_date_window_too() {
        let combats = list(&["old run"]);
        let notes = [""];
        let entries = [entry()];
        let start_times = [at("2026-07-23 20:00")];

        let mut view = CompareView::default();
        view.range.set("2026-08-01 00:00", "");
        assert!(
            view.visible_combats(&combats, &notes, &entries, &start_times)
                .is_empty()
        );

        view.toggle_selected(0, true);
        assert_eq!(
            vec![(0, false)],
            view.visible_combats(&combats, &notes, &entries, &start_times)
        );
    }

    /// The two things a big selection gives up are said out loud, and a small
    /// one is left alone.
    #[test]
    fn a_big_selection_says_what_it_gives_up() {
        assert_eq!(None, selection_hint(2));
        assert_eq!(None, selection_hint(COLORS_RUN_OUT_AT));
        assert!(selection_hint(COLORS_RUN_OUT_AT + 1).is_some());
        assert_ne!(
            selection_hint(COLORS_RUN_OUT_AT + 1),
            selection_hint(MANY_COMBATS + 1)
        );
    }
}
