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
#[derive(PartialEq, Eq, Clone, Copy, Default)]
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
#[derive(Default, Clone, PartialEq, Eq)]
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
        self.environment.is_some() || self.map.is_some() || self.difficulty != DifficultyFilter::Any
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
        if let Some(wanted) = &self.environment
            && environment != Some(wanted.as_str())
        {
            return false;
        }
        if let Some(wanted) = &self.map
            && base_name != wanted
        {
            return false;
        }
        self.difficulty.matches(difficulty)
    }

    /// The values a menu may offer: those present among the combats that pass
    /// the *other* two filters. Picking from a menu can then never produce an
    /// empty list, because every option still has at least one combat behind it.
    fn options(&self, combats: &[CombatEntry], dimension: Dimension) -> Options {
        let mut without = self.clone();
        match dimension {
            Dimension::Environment => without.environment = None,
            Dimension::Difficulty => without.difficulty = DifficultyFilter::Any,
            Dimension::Map => without.map = None,
        }
        let matching = combats
            .iter()
            .filter(|c| without.matches(c.environment, c.difficulty, c.base_name));

        let mut options = Options::default();
        for combat in matching {
            if let Some(environment) = combat.environment {
                options.environments.push(environment.to_string());
            }
            options.maps.push(combat.base_name.to_string());
            options.difficulties.push(combat.difficulty);
        }
        options.environments.sort_unstable();
        options.environments.dedup();
        options.maps.sort_unstable();
        options.maps.dedup();
        options
    }

    /// Drops a choice that the other filters have made impossible, so the list
    /// can never end up empty through a combination nothing matches.
    fn drop_impossible_choices(&mut self, combats: &[CombatEntry]) {
        if let Some(environment) = &self.environment
            && !self
                .options(combats, Dimension::Environment)
                .environments
                .iter()
                .any(|e| e == environment)
        {
            self.environment = None;
        }
        if let Some(map) = &self.map
            && !self
                .options(combats, Dimension::Map)
                .maps
                .iter()
                .any(|m| m == map)
        {
            self.map = None;
        }
        if self.difficulty != DifficultyFilter::Any
            && !self
                .options(combats, Dimension::Difficulty)
                .difficulties
                .iter()
                .any(|d| self.difficulty.matches(*d))
        {
            self.difficulty = DifficultyFilter::Any;
        }
    }

    /// Draws the three pickers inline. Each menu offers only what the other two
    /// leave reachable, so no combination can empty the list.
    pub fn show(&mut self, id: &str, combats: &[CombatEntry], ui: &mut Ui) {
        self.drop_impossible_choices(combats);
        let environments = self.options(combats, Dimension::Environment).environments;
        let maps = self.options(combats, Dimension::Map).maps;
        let difficulties = self.options(combats, Dimension::Difficulty).difficulties;
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
                other => DifficultyFilter::ALL
                    .iter()
                    .find(|(f, _)| *f == other)
                    .map(|(_, label)| *label)
                    .unwrap_or("Any level"),
            })
            .width(100.0)
            .show_ui(ui, |ui| {
                for &(filter, label) in DifficultyFilter::ALL {
                    let reachable = filter == DifficultyFilter::Any
                        || difficulties.iter().any(|d| filter.matches(*d));
                    if !reachable {
                        continue;
                    }
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

/// What the filter knows about one combat in the list.
#[derive(Clone, Copy)]
pub struct CombatEntry<'a> {
    pub environment: Option<&'a str>,
    pub difficulty: Option<Difficulty>,
    pub base_name: &'a str,
}

#[derive(Clone, Copy)]
enum Dimension {
    Environment,
    Difficulty,
    Map,
}

#[derive(Default)]
struct Options {
    environments: Vec<String>,
    maps: Vec<String>,
    difficulties: Vec<Option<Difficulty>>,
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
        let filter = CombatFilter {
            environment: Some("Ground".to_string()),
            ..Default::default()
        };
        assert!(filter.matches(Some("Ground"), None, "Bug Hunt"));
        assert!(!filter.matches(Some("Space"), None, "Bug Hunt"));
        // A combat whose map was never recognized has no environment at all.
        assert!(!filter.matches(None, None, "Combat"));

        let filter = CombatFilter {
            map: Some("Infected Space".to_string()),
            ..Default::default()
        };
        assert!(filter.matches(Some("Space"), None, "Infected Space"));
        assert!(!filter.matches(Some("Space"), None, "Hive Onslaught"));

        let filter = CombatFilter {
            difficulty: DifficultyFilter::Elite,
            ..Default::default()
        };
        assert!(filter.matches(None, Some(Difficulty::Elite), "x"));
        assert!(!filter.matches(None, Some(Difficulty::Normal), "x"));
    }

    fn entries() -> Vec<CombatEntry<'static>> {
        vec![
            CombatEntry {
                environment: Some("Space"),
                difficulty: Some(Difficulty::Elite),
                base_name: "Infected Space",
            },
            CombatEntry {
                environment: Some("Space"),
                difficulty: Some(Difficulty::Normal),
                base_name: "Azure Nebula",
            },
            CombatEntry {
                environment: Some("Ground"),
                difficulty: Some(Difficulty::Advanced),
                base_name: "Bug Hunt",
            },
            CombatEntry {
                environment: None,
                difficulty: None,
                base_name: "Combat",
            },
        ]
    }

    /// Choosing an environment must leave only the maps and levels that still
    /// have a combat behind them — otherwise the next pick empties the list.
    #[test]
    fn a_chosen_environment_narrows_the_other_menus() {
        let entries = entries();
        let filter = CombatFilter {
            environment: Some("Ground".to_string()),
            ..Default::default()
        };

        let maps = filter.options(&entries, Dimension::Map).maps;
        assert_eq!(vec!["Bug Hunt".to_string()], maps);

        let levels = filter.options(&entries, Dimension::Difficulty).difficulties;
        assert!(
            levels
                .iter()
                .any(|d| DifficultyFilter::Advanced.matches(*d))
        );
        assert!(!levels.iter().any(|d| DifficultyFilter::Elite.matches(*d)));
    }

    /// The menu being opened is not narrowed by its own current value, or it
    /// would offer nothing but what is already picked.
    #[test]
    fn a_menu_is_not_narrowed_by_its_own_choice() {
        let entries = entries();
        let filter = CombatFilter {
            environment: Some("Ground".to_string()),
            ..Default::default()
        };

        let environments = filter
            .options(&entries, Dimension::Environment)
            .environments;
        assert_eq!(
            vec!["Ground".to_string(), "Space".to_string()],
            environments
        );
    }

    /// The cascade stops a contradictory pair from being picked, but the list
    /// itself can change under a filter that is already set — a refresh, or a
    /// combat deleted. Whichever of the two is dropped then is arbitrary; what
    /// has to hold is that the list does not come back empty.
    #[test]
    fn an_impossible_pair_is_resolved_rather_than_left_empty() {
        let entries = entries();
        for (environment, map) in [("Ground", "Infected Space"), ("Space", "Bug Hunt")] {
            let mut filter = CombatFilter {
                environment: Some(environment.to_string()),
                map: Some(map.to_string()),
                ..Default::default()
            };
            assert!(
                !entries
                    .iter()
                    .any(|c| filter.matches(c.environment, c.difficulty, c.base_name)),
                "the pair really is contradictory to begin with"
            );

            filter.drop_impossible_choices(&entries);

            assert!(
                entries
                    .iter()
                    .any(|c| filter.matches(c.environment, c.difficulty, c.base_name)),
                "after resolving it, something matches again"
            );
            assert!(
                filter.environment.is_some() || filter.map.is_some(),
                "only the conflicting half is given up, not both"
            );
        }
    }
}
