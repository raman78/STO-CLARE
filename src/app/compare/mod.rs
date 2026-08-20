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
    analyzer::{Combat, DamageGroup},
    app::{settings::Settings, state::AppState, theme},
};

mod compare_table;

use compare_table::Comparison;
pub use compare_table::ComparisonSlot;

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

/// The comparison on screen, and whether the window is in that mode at all.
///
/// It used to hold a picker of its own — a second list of the same fights, with
/// its own search, its own filters and its own ticks. The fights are picked in
/// the combats list down the side of the window now, so what is left here is
/// the table itself.
#[derive(Default)]
pub struct CompareView {
    open: bool,
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

    /// Gives up whatever comparison is on screen — the fights it was of are no
    /// longer the fights the list holds.
    pub fn forget(&mut self) {
        self.comparison = None;
    }

    /// The runs this comparison is of, as the combats list draws them: their
    /// numbers, their colours, whose figures are being read and whether each is
    /// still in it.
    pub fn slots(&self) -> Vec<ComparisonSlot> {
        self.comparison
            .as_ref()
            .map(Comparison::list_slots)
            .unwrap_or_default()
    }

    /// Reads a run's figures for another of its players, by when that fight
    /// started.
    pub fn set_player(&mut self, start: NaiveDateTime, handle: &str) {
        if let Some(comparison) = self.comparison.as_mut() {
            comparison.set_player(start, handle);
        }
    }

    /// The comparison itself, once there is one.
    ///
    /// There is no picker here any more: the fights are ticked in the combats
    /// list down the side of the window, which is the same list they are
    /// browsed and cleared in. Changing the selection is ticking another row
    /// there, so there is nothing here to go "back" to either.
    pub fn show(&mut self, state: &mut AppState, ui: &mut Ui, frame: &Frame) {
        match &mut self.comparison {
            Some(comparison) => comparison.show(ui, &mut state.settings, frame),
            None => {
                theme::section(ui, "Compare Combats", |ui| {
                    ui.label(
                        "Tick two or more fights in the Combats list — the panel on the left, \
                         under the ☰ Combats button — and they appear here, side by side.",
                    );
                    ui.label(
                        RichText::new(
                            "A run fetched from the ladder is one of them: the magnifier in the \
                             Ladder window puts it at the top of that list, to be ticked like \
                             any other fight.",
                        )
                        .weak(),
                    );
                });
            }
        }
    }
}

/// What is worth saying about a selection this size, if anything. Nothing caps
/// the count any more, so the two things that do give way say so themselves.
///
/// Said in the combats list's footer, where the fights are ticked.
pub fn selection_hint(selected: usize) -> Option<&'static str> {
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
