//! Map and difficulty detection, ported from the OSCR parser
//! (`STOCD/OSCR`, `detection.py` + `combat.py::detect_map`).
//!
//! STO combat logs carry no explicit map or difficulty. OSCR — the official
//! parser feeding the DPS ladders — derives them purely from the entities that
//! appear in the log: which curated boss/NPC internal names exist, and, to tell
//! Advanced from Elite, how many times specific entities died (and, for a few
//! maps, how much hull damage they suffered). We mirror that here, but keep the
//! rule tables in a JSON file (`detection_rules.json`) instead of hard-coding
//! them, so they can be refreshed when OSCR updates.
//!
//! The detection is data-driven and completely separate from the user-editable
//! Combat Name Rules: those produce a display label, this produces an
//! authoritative `(map, difficulty)`.
//!
//! This module implements the existence + death-count phases. The hull-damage
//! tie-break (needed only where death counts do not distinguish the tiers, e.g.
//! Hive Space) is added on top of the same `CritterMeta`.

use std::collections::HashMap;

use lazy_static::lazy_static;
use rustc_hash::FxHashMap;
use serde::Deserialize;

/// A map difficulty. `Any` means "known map, tier not distinguished" (either the
/// map has a single tier, or the tier could not be resolved).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum Difficulty {
    Any,
    Normal,
    Advanced,
    Elite,
}

/// Difficulties tried during the death-count phase, ordered low to high. A
/// higher tier that also matches overrides a lower one (mirrors OSCR, where the
/// tables are ordered so Elite is checked after Advanced).
const DIFFICULTY_ORDER: [Difficulty; 2] = [Difficulty::Advanced, Difficulty::Elite];

/// Per-NPC facts gathered from a combat, keyed elsewhere by the entity's
/// internal unique name. Enough to run OSCR's detection.
#[derive(Debug, Clone, Default)]
pub struct CritterMeta {
    /// How many times an entity with this unique name died in the combat.
    pub deaths: u32,
}

#[derive(Debug, Deserialize)]
struct MapIdentifier {
    map: String,
    difficulty: Difficulty,
}

/// The rule tables, deserialized from `detection_rules.json`.
#[derive(Debug, Deserialize)]
pub struct DetectionRules {
    /// Internal entity name -> the map it identifies (and a fixed difficulty,
    /// or `Any` when the tier must be resolved from the counts).
    map_identifiers: HashMap<String, MapIdentifier>,
    /// Map -> difficulty -> {entity unique name -> required death count}. A
    /// required count of 0 means "must be present, any number of deaths"
    /// (used when only the entity's existence distinguishes the tier).
    death_counts: HashMap<String, HashMap<Difficulty, HashMap<String, u32>>>,
}

/// The detection result for a combat.
#[derive(Debug, Clone, Default)]
pub struct Detected {
    /// The identified map, or `None` when nothing matched ("Combat").
    pub map: Option<String>,
    /// The resolved difficulty, or `None` when the map is unknown.
    pub difficulty: Option<Difficulty>,
}

lazy_static! {
    /// The bundled default rules, parsed once. Panics only if the shipped JSON
    /// is malformed, which a test guards against.
    pub static ref DETECTION_RULES: DetectionRules =
        serde_json::from_str(include_str!("detection_rules.json"))
            .expect("bundled detection_rules.json must be valid");
}

/// Detect `(map, difficulty)` from the combat's critters, given a view keyed by
/// each entity's internal unique name. Mirrors OSCR's `detect_map`: identify the
/// map by a present curated entity, then resolve the tier from death counts.
pub fn detect(rules: &DetectionRules, critters: &FxHashMap<&str, &CritterMeta>) -> Detected {
    // Existence phase: find a curated entity that is present. Some entities pin
    // a fixed difficulty outright; otherwise we only learn the map here.
    let mut map: Option<&str> = None;
    for (unique_name, identifier) in rules.map_identifiers.iter() {
        if critters.contains_key(unique_name.as_str()) {
            map = Some(&identifier.map);
            if identifier.difficulty != Difficulty::Any {
                return Detected {
                    map: Some(identifier.map.clone()),
                    difficulty: Some(identifier.difficulty),
                };
            }
        }
    }
    let Some(map) = map else {
        return Detected::default();
    };

    // Death-count phase: check each tier (low to high); a higher matching tier
    // overrides. If the map has tables but none match, the tier stays `Any`.
    let mut difficulty = None;
    let mut had_tables = false;
    if let Some(tier_tables) = rules.death_counts.get(map) {
        for tier in DIFFICULTY_ORDER {
            if let Some(table) = tier_tables.get(&tier) {
                had_tables = true;
                if death_counts_match(table, critters) {
                    difficulty = Some(tier);
                }
            }
        }
    }
    if had_tables && difficulty.is_none() {
        difficulty = Some(Difficulty::Any);
    }

    Detected {
        map: Some(map.to_string()),
        difficulty,
    }
}

/// A tier matches when every listed entity is present and, for entries with a
/// required count > 0, died exactly that many times.
fn death_counts_match(table: &HashMap<String, u32>, critters: &FxHashMap<&str, &CritterMeta>) -> bool {
    for (unique_name, required) in table.iter() {
        match critters.get(unique_name.as_str()) {
            None => return false,
            Some(meta) => {
                if *required > 0 && meta.deaths != *required {
                    return false;
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guards the bundled rules file: it must parse.
    #[test]
    fn bundled_rules_parse() {
        let _ = &*DETECTION_RULES;
    }

    fn critters(entries: &[(&'static str, u32)]) -> Vec<(&'static str, CritterMeta)> {
        entries
            .iter()
            .map(|(n, d)| (*n, CritterMeta { deaths: *d }))
            .collect()
    }

    fn view<'a>(owned: &'a [(&'static str, CritterMeta)]) -> FxHashMap<&'a str, &'a CritterMeta> {
        owned.iter().map(|(n, m)| (*n, m)).collect()
    }

    #[test]
    fn unknown_map_when_no_identifier_present() {
        let owned = critters(&[("Some_Random_Npc", 3)]);
        let result = detect(&DETECTION_RULES, &view(&owned));
        assert!(result.map.is_none());
        assert!(result.difficulty.is_none());
    }

    #[test]
    fn infected_space_advanced_by_death_counts() {
        // The Advanced death counts for Infected Space, exactly.
        let owned = critters(&[
            ("Space_Borg_Dreadnought_Raidisode_Sibrian_Final_Boss", 1),
            ("Space_Borg_Battleship_Raidisode", 5),
            ("Space_Borg_Cruiser_Raidisode", 6),
            ("Mission_Borgraid1_Transwarp_02", 1),
        ]);
        let result = detect(&DETECTION_RULES, &view(&owned));
        assert_eq!(result.map.as_deref(), Some("Infected Space"));
        assert_eq!(result.difficulty, Some(Difficulty::Advanced));
    }

    #[test]
    fn infected_space_elite_overrides_when_elite_entities_present() {
        // Elite counts include an Elite-only entity; the Elite tier must win.
        let owned = critters(&[
            ("Space_Borg_Battleship_Raidisode_Sibrian_Elite_Initial", 2),
            ("Space_Borg_Dreadnought_Raidisode_Sibrian_Initial_Boss", 1),
            ("Space_Borg_Cruiser_Raidisode_Sibrian_Elite_Initial", 4),
            ("Space_Borg_Battleship_Raidisode", 2),
            ("Mission_Borgraid1_Transwarp_02", 1),
            ("Space_Borg_Dreadnought_Raidisode_Sibrian_Final_Boss", 1),
        ]);
        let result = detect(&DETECTION_RULES, &view(&owned));
        assert_eq!(result.map.as_deref(), Some("Infected Space"));
        assert_eq!(result.difficulty, Some(Difficulty::Elite));
    }

    #[test]
    fn known_map_but_wrong_counts_stays_any() {
        // The map identifier is present, but no tier's counts match.
        let owned = critters(&[(
            "Space_Borg_Dreadnought_Raidisode_Sibrian_Final_Boss",
            1,
        )]);
        let result = detect(&DETECTION_RULES, &view(&owned));
        assert_eq!(result.map.as_deref(), Some("Infected Space"));
        assert_eq!(result.difficulty, Some(Difficulty::Any));
    }

    #[test]
    fn fixed_difficulty_map_reports_its_tier() {
        // Winter Invasion is pinned to Normal by its boss entity.
        let owned = critters(&[("Snowman_Q_Boss_Msn_Snowglobe", 1)]);
        let result = detect(&DETECTION_RULES, &view(&owned));
        assert_eq!(result.map.as_deref(), Some("Winter Invasion"));
        assert_eq!(result.difficulty, Some(Difficulty::Normal));
    }
}
