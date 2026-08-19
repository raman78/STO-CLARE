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

use eframe::{Frame, egui::*};
use serde::{Deserialize, Serialize};

use crate::{
    analyzer::{Combat, CombatSummary, DamageGroup},
    app::{
        combat_filter::{CombatEntry, CombatFilter, DifficultyFilter},
        combats_list::{
            ListCounts, SelectAll, dps_shown, footer_frame, show_combat_cells, show_header_cells,
            show_list_footer,
        },
        date_range::DateRange,
        effective_handle,
        settings::{CombatNotes, Settings},
        state::AppState,
        theme,
    },
    helpers::number_formatting::NumberFormatter,
};

mod compare_table;

use crate::custom_widgets::table::Table;
/// The height a row of the picker takes — the tables' own.
const ROW_HEIGHT: f32 = 25.0;
use crate::custom_widgets::tooltip::CloseTooltip;
use compare_table::Comparison;

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

    /// Starts the picker on the map and level of the run being compared
    /// against, because that is what a comparison is nearly always of — the
    /// same fight fought differently. Nothing is locked: the pickers are the
    /// ordinary ones and the reader can widen them the moment they want
    /// something else.
    ///
    /// Only ever set once per run, so a reader who has widened the filters does
    /// not find them narrowed again underneath them.
    pub fn suggest_filter(&mut self, map: Option<String>, difficulty: DifficultyFilter) {
        self.filter.map = map;
        self.filter.difficulty = difficulty;
        self.filter.environment = None;
    }

    /// Receive the combats fetched for comparison and build the table.
    pub fn set_combats(&mut self, combats: Vec<(usize, Arc<Combat>)>, settings: &Settings) {
        self.comparison = Some(Comparison::new(combats, settings));
    }

    pub fn show(
        &mut self,
        state: &mut AppState,
        combats: &[CombatSummary],
        // Whose log this is, as the log itself says. What the reader named
        // themselves in the settings wins over it; see `effective_handle`.
        detected_owner: Option<&str>,
        // A fight that is in the comparison whatever the reader picks — the run
        // fetched from the ladder, which is the reason the picker is open at
        // all. Ticked, and not for unticking.
        pinned: Option<String>,
        ui: &mut Ui,
        frame: &Frame,
    ) -> Option<Vec<usize>> {
        match &mut self.comparison {
            Some(comparison) => {
                if ui.button("◀ Change selection").clicked() {
                    self.comparison = None;
                    // Going back to the picker is the other moment the list can
                    // be out of date — a run finished while the table was up.
                    state.analysis_handler.refresh_combats_list();
                    ui.separator();
                    self.show_selection(state, combats, detected_owner, pinned, ui)
                } else {
                    ui.separator();
                    comparison.show(ui, &mut state.settings, frame);
                    None
                }
            }
            None => self.show_selection(state, combats, detected_owner, pinned, ui),
        }
    }

    fn show_selection(
        &mut self,
        state: &mut AppState,
        combats: &[CombatSummary],
        detected_owner: Option<&str>,
        pinned: Option<String>,
        ui: &mut Ui,
    ) -> Option<Vec<usize>> {
        // The same rimmed section the combats panel is drawn in, so the two
        // lists of fights read as the same thing in two places.
        theme::section(ui, "Combats to compare", |ui| {
            self.show_picker(state, combats, detected_owner, pinned, ui)
        })
    }

    fn show_picker(
        &mut self,
        state: &mut AppState,
        combats: &[CombatSummary],
        detected_owner: Option<&str>,
        pinned: Option<String>,
        ui: &mut Ui,
    ) -> Option<Vec<usize>> {
        let mut picked = None;
        let entries: Vec<CombatEntry> = combats
            .iter()
            .map(|combat| CombatEntry {
                environment: combat.environment.as_deref(),
                difficulty: combat.difficulty,
                base_name: combat.base_name.as_str(),
                solo: combat.solo,
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
                "compare",
                combats
                    .first()
                    .map(|combat| combat.start)
                    .zip(combats.last().map(|combat| combat.start)),
                ui,
            );
            if (self.filter.is_active() || self.range.is_active())
                && ui.button("Clear filter").clicked()
            {
                self.filter.clear();
                self.range.clear();
            }
        });

        let notes: Vec<&str> = combats
            .iter()
            .map(|combat| {
                state
                    .settings
                    .combat_notes
                    .get(&CombatNotes::key_at(combat.start))
            })
            .collect();
        let visible = self.visible_combats(combats, &notes, &entries);
        // Which run the reader ticked this pass, applied once the table has
        // finished drawing — the rows borrow the selection while they draw.
        let mut toggled: Option<(usize, bool)> = None;

        // A line of its own, above the list: the run the comparison is *of*,
        // which is not something the reader picks or unpicks.
        if let Some(pinned) = &pinned {
            ui.horizontal(|ui| {
                let mut always = true;
                ui.add_enabled_ui(false, |ui| {
                    ui.checkbox(&mut always, pinned).disabled_hover(
                        "The run you opened from the ladder. It is what the \
                             comparison is of, so it cannot be taken out of it.",
                    );
                });
            });
        }

        // The strip that closes the picker off, the same one the combats panel
        // has: what is picked out of what there is, the two buttons that pick
        // and unpick, and the one button this screen is for.
        //
        // Claimed before the table, so it sits at the bottom of the section
        // whatever the table above it does.
        // The pinned run counts: it is in the comparison whatever else is
        // picked, and a count that left it out read as one short.
        let selected = self.selected.len() + usize::from(pinned.is_some());
        let counts = ListCounts {
            total: combats.len(),
            shown: visible.len(),
            selected: Some(selected),
        };
        // Only what the filters actually left in — a ticked combat that is only
        // on screen because it is ticked is already in the selection anyway.
        let matching: Vec<usize> = visible
            .iter()
            .filter(|(_, matches)| *matches)
            .map(|(i, _)| *i)
            .collect();
        let hint = selection_hint(self.selected.len());
        let mut select = None;
        Panel::bottom("compare footer")
            .frame(footer_frame(ui))
            .show_inside(ui, |ui| {
                select = show_list_footer(ui, counts, hint, |ui| {
                    // With a run pinned, one of the reader's own fights is
                    // enough to have something to compare it against.
                    let enough = if pinned.is_some() { 1 } else { 2 };
                    // The one button on this screen that does anything:
                    // everything around it only ticks, unticks or narrows the
                    // list. It carries the theme's accent rim so it is not read
                    // as one more of them.
                    ui.scope(|ui| {
                        theme::accent_rim(ui);
                        if ui
                            .add_enabled(
                                self.selected.len() >= enough,
                                Button::new("Compare selected 🆚"),
                            )
                            .clicked()
                        {
                            let mut indices = self.selected.clone();
                            indices.sort_unstable();
                            // With a run pinned the comparison cannot be built
                            // from this log at all — the fights live in two
                            // different ones, and the caller has to write them
                            // into a third first.
                            if pinned.is_some() {
                                picked = Some(indices);
                            } else {
                                state.analysis_handler.get_combats(indices);
                            }
                        }
                    });
                });
            });
        match select {
            // Adds to the selection rather than replacing it, so a selection
            // can be built up from one filtered list after another.
            Some(SelectAll::All) => {
                for i in matching {
                    self.toggle_selected(i, true);
                }
            }
            Some(SelectAll::None) => self.selected.clear(),
            None => (),
        }

        // The same table the combats panel shows, with a tick box in front of
        // it: a fight reads the same wherever the program offers a choice of
        // one. Striping, the highlight under the pointer and the bar at the
        // edge of the panel all come with it.
        let my_handle =
            effective_handle(state.settings.general.my_handle.as_deref(), detected_owner);
        let mut formatter = NumberFormatter::new();
        Table::new(ui)
            .header(ROW_HEIGHT)
            .body(ROW_HEIGHT, |t| {
                for (i, matches) in visible {
                    let combat = &combats[i];
                    let mut checked = self.selected.contains(&i);
                    let row = t.selectable_row(checked, |r| {
                        r.cell(|ui| {
                            ui.horizontal(|ui| {
                                if ui.checkbox(&mut checked, "").clicked() {
                                    toggled = Some((i, checked));
                                }
                                // Beside the tick it is about, rather than in a
                                // column of its own past the note — that column
                                // was empty on every row but the odd one, and
                                // hung off the end of the table.
                                if !matches {
                                    ui.colored_label(theme::palette().warn, "⚠").hover(
                                        "Ticked, but it does not match the filters above — it is \
                                         shown so a combat cannot go into a comparison out of \
                                         sight. Untick it, or widen the filters to see it in \
                                         place.",
                                    );
                                }
                            });
                        });
                        show_combat_cells(
                            r,
                            combat,
                            notes[i],
                            dps_shown(combat, my_handle),
                            None,
                            &mut formatter,
                        );
                    });
                    let row = row.on_hover_text(&combat.identifier);
                    // The whole row picks the run, not the tick box alone: the
                    // row is what lights up under the pointer, so it is what a
                    // click lands on.
                    if row.clicked() {
                        toggled = Some((i, !checked));
                    }
                }
            })
            .header_row(|r| {
                // The tick column's heading stays empty; the table ends at the
                // note, like the list in the panel.
                r.cell(|_| {});
                show_header_cells(r, my_handle, None);
            });
        if let Some((i, checked)) = toggled {
            self.toggle_selected(i, checked);
        }
        picked
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
        combats: &[CombatSummary],
        notes: &[&str],
        entries: &[CombatEntry],
    ) -> Vec<(usize, bool)> {
        (0..combats.len())
            .rev()
            .filter_map(|i| {
                let matches = self.matches_filters(&combats[i], notes[i], entries[i]);
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
    fn matches_filters(&self, combat: &CombatSummary, note: &str, entry: CombatEntry) -> bool {
        let needle = self.name_filter.trim().to_lowercase();
        if !needle.is_empty()
            && !combat.identifier.to_lowercase().contains(&needle)
            && !note.to_lowercase().contains(&needle)
        {
            return false;
        }

        if !self.range.matches(combat.start) {
            return false;
        }

        self.filter.matches(
            entry.environment,
            entry.difficulty,
            entry.base_name,
            entry.solo,
        )
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

    use crate::analyzer::Difficulty;
    use chrono::NaiveDateTime;

    fn entry() -> CombatEntry<'static> {
        CombatEntry {
            environment: Some("Space"),
            difficulty: Some(Difficulty::Elite),
            base_name: "Infected Space",
            solo: false,
        }
    }

    fn at(text: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").unwrap()
    }

    /// A combat as the picker sees it: only the identifier, the start and what
    /// the filters read are used here.
    fn combat(identifier: &str, start: &str) -> CombatSummary {
        CombatSummary {
            name: identifier.to_owned(),
            identifier: identifier.to_owned(),
            base_name: "Infected Space".to_owned(),
            category: None,
            environment: Some("Space".to_owned()),
            difficulty: Some(Difficulty::Elite),
            solo: false,
            start: at(start),
            duration: chrono::Duration::seconds(252),
            players: Vec::new(),
        }
    }

    /// The search box matches the whole displayed identifier, so a date or a
    /// time narrows the list as well as a name does.
    #[test]
    fn the_search_box_matches_the_displayed_identifier() {
        let mut view = CompareView::default();
        let combat = combat(
            "Infected Space [Elite] | 2026-07-23 20:07:22 - 20:11:37",
            "2026-07-23 20:07",
        );

        view.name_filter = "infected".to_string();
        assert!(view.matches_filters(&combat, "", entry()));

        view.name_filter = "20:07".to_string();
        assert!(view.matches_filters(&combat, "", entry()));

        view.name_filter = "hive".to_string();
        assert!(!view.matches_filters(&combat, "", entry()));
    }

    /// A combat can also be found by the note the user wrote for it, which is
    /// the point of writing one.
    #[test]
    fn the_search_box_matches_the_note() {
        let mut view = CompareView::default();
        let combat = combat(
            "Infected Space [Elite] | 2026-07-23 20:07:22 - 20:11:37",
            "2026-07-23 20:07",
        );

        view.name_filter = "cheops".to_string();
        assert!(view.matches_filters(&combat, "Cheops build", entry()));
        assert!(!view.matches_filters(&combat, "FAW build", entry()));
    }

    /// Search and pickers narrow together, not one or the other.
    #[test]
    fn the_search_box_and_the_pickers_both_apply() {
        let mut view = CompareView {
            name_filter: "infected".to_string(),
            ..Default::default()
        };
        view.filter.environment = Some("Ground".to_string());
        assert!(!view.matches_filters(
            &combat("Infected Space | t", "2026-07-23 20:00"),
            "",
            entry()
        ));
    }

    /// The date window narrows alongside the rest, so "this evening's Infected
    /// runs" is one list rather than a search through a month of them.
    #[test]
    fn the_date_window_narrows_with_the_other_filters() {
        let mut view = CompareView::default();
        view.range.set("2026-07-23 20:00", "2026-07-23 23:00");

        assert!(view.matches_filters(&combat("Infected Space", "2026-07-23 21:30"), "", entry()));
        assert!(!view.matches_filters(&combat("Infected Space", "2026-07-22 21:30"), "", entry()));
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

    /// A ticked combat stays in the list whatever the filters say. Narrowing
    /// the level to Elite over a selection holding an Advanced run used to hide
    /// that run while it was still going into the comparison — invisible, and
    /// impossible to untick.
    #[test]
    fn a_ticked_combat_is_never_hidden_by_a_filter() {
        let combats = [
            combat("Infected Elite", "2026-07-23 20:00"),
            combat("Infected Advanced", "2026-07-23 21:00"),
        ];
        let notes = ["", ""];
        let entries = [
            CombatEntry {
                environment: Some("Space"),
                difficulty: Some(Difficulty::Elite),
                base_name: "Infected Space",
                solo: false,
            },
            CombatEntry {
                environment: Some("Space"),
                difficulty: Some(Difficulty::Advanced),
                base_name: "Infected Space",
                solo: false,
            },
        ];

        let mut view = CompareView::default();
        view.filter.difficulty = crate::app::combat_filter::DifficultyFilter::Elite;

        // Unticked, the Advanced run is simply filtered out.
        let visible = view.visible_combats(&combats, &notes, &entries);
        assert_eq!(vec![(0, true)], visible);

        // Ticked, it stays — and is marked as not matching.
        view.toggle_selected(1, true);
        let visible = view.visible_combats(&combats, &notes, &entries);
        assert_eq!(vec![(1, false), (0, true)], visible, "newest first");
    }

    /// The same for the date window, which is the other way a selected combat
    /// can fall out of the list.
    #[test]
    fn a_ticked_combat_survives_the_date_window_too() {
        let combats = [combat("old run", "2026-07-23 20:00")];
        let notes = [""];
        let entries = [entry()];

        let mut view = CompareView::default();
        view.range.set("2026-08-01 00:00", "");
        assert!(view.visible_combats(&combats, &notes, &entries).is_empty());

        view.toggle_selected(0, true);
        assert_eq!(
            vec![(0, false)],
            view.visible_combats(&combats, &notes, &entries)
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
