//! Filtering a combats list by what the analyzer worked out about each fight:
//! its environment, its difficulty and which map it was.
//!
//! Shared by the combat picker on the main screen and the selection list in the
//! compare view, so the two offer the same choices and mean the same thing by
//! them.

use eframe::egui::*;

use crate::analyzer::Difficulty;

/// The difficulty picker. `Any` matches everything; `Unknown` catches combats
/// whose tier could not be worked out, which would otherwise be invisible under
/// every other setting.
#[derive(PartialEq, Clone, Copy, Default)]
pub enum DifficultyFilter {
    #[default]
    Any,
    Normal,
    Advanced,
    Elite,
    Unknown,
}

impl DifficultyFilter {
    pub const ALL: &'static [(DifficultyFilter, &'static str)] = &[
        (DifficultyFilter::Any, "Any"),
        (DifficultyFilter::Normal, "Normal"),
        (DifficultyFilter::Advanced, "Advanced"),
        (DifficultyFilter::Elite, "Elite"),
        (DifficultyFilter::Unknown, "Unknown"),
    ];

    pub fn matches(self, difficulty: Option<Difficulty>) -> bool {
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

/// What a combat has to match to stay in the list. Every part defaults to "all".
#[derive(Default)]
pub struct CombatFilter {
    /// "Space", "Ground", … — the curated environment of the detected map.
    /// `None` means any.
    pub environment: Option<String>,
    pub difficulty: DifficultyFilter,
    /// The combat's base name, i.e. which map it was. `None` means any.
    pub map: Option<String>,
}

impl CombatFilter {
    pub fn is_active(&self) -> bool {
        self.environment.is_some()
            || self.map.is_some()
            || self.difficulty != DifficultyFilter::Any
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn matches(
        &self,
        environment: Option<&str>,
        difficulty: Option<Difficulty>,
        base_name: &str,
    ) -> bool {
        if let Some(wanted) = &self.environment {
            if environment != Some(wanted.as_str()) {
                return false;
            }
        }
        if let Some(wanted) = &self.map {
            if base_name != wanted {
                return false;
            }
        }
        self.difficulty.matches(difficulty)
    }

    /// Draws the three pickers inline. `environments` and `maps` are the values
    /// actually present in the list, so the menus never offer a choice that
    /// would empty it.
    pub fn show(&mut self, id: &str, environments: &[String], maps: &[String], ui: &mut Ui) {
        ComboBox::new((id, "environment"), "")
            .selected_text(self.environment.as_deref().unwrap_or("Any type"))
            .width(90.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.environment, None, "Any type");
                for environment in environments {
                    ui.selectable_value(
                        &mut self.environment,
                        Some(environment.clone()),
                        environment,
                    );
                }
            });

        ComboBox::new((id, "difficulty"), "")
            .selected_text(match self.difficulty {
                DifficultyFilter::Any => "Any level",
                other => {
                    DifficultyFilter::ALL
                        .iter()
                        .find(|(f, _)| *f == other)
                        .map(|(_, label)| *label)
                        .unwrap_or("Any level")
                }
            })
            .width(100.0)
            .show_ui(ui, |ui| {
                for &(filter, label) in DifficultyFilter::ALL {
                    let label = if filter == DifficultyFilter::Any {
                        "Any level"
                    } else {
                        label
                    };
                    ui.selectable_value(&mut self.difficulty, filter, label);
                }
            });

        ComboBox::new((id, "map"), "")
            .selected_text(self.map.as_deref().unwrap_or("Any map"))
            .width(220.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.map, None, "Any map");
                for map in maps {
                    ui.selectable_value(&mut self.map, Some(map.clone()), map);
                }
            });
    }
}

/// The distinct values present in a list, sorted, for a filter menu.
pub fn distinct_sorted<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut values: Vec<String> = values.filter(|v| !v.is_empty()).map(String::from).collect();
    values.sort_unstable();
    values.dedup();
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_filter_matches_everything() {
        let filter = CombatFilter::default();
        assert!(!filter.is_active());
        assert!(filter.matches(Some("Space"), Some(Difficulty::Elite), "Infected Space"));
        assert!(filter.matches(None, None, "Combat"));
    }

    #[test]
    fn each_part_narrows_on_its_own() {
        let mut filter = CombatFilter::default();
        filter.environment = Some("Ground".to_string());
        assert!(filter.matches(Some("Ground"), None, "Bug Hunt"));
        assert!(!filter.matches(Some("Space"), None, "Bug Hunt"));
        // A combat whose map was never recognized has no environment at all.
        assert!(!filter.matches(None, None, "Combat"));

        let mut filter = CombatFilter::default();
        filter.map = Some("Infected Space".to_string());
        assert!(filter.matches(Some("Space"), None, "Infected Space"));
        assert!(!filter.matches(Some("Space"), None, "Hive Onslaught"));

        let mut filter = CombatFilter::default();
        filter.difficulty = DifficultyFilter::Elite;
        assert!(filter.matches(None, Some(Difficulty::Elite), "x"));
        assert!(!filter.matches(None, Some(Difficulty::Normal), "x"));
    }

    #[test]
    fn distinct_sorted_drops_blanks_and_duplicates() {
        let values = ["Space", "", "Ground", "Space"];
        assert_eq!(
            vec!["Ground".to_string(), "Space".to_string()],
            distinct_sorted(values.into_iter())
        );
    }
}
