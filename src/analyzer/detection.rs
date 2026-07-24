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
//! The detection is data-driven and independent of the user-editable Combat Name
//! Rules: it produces an authoritative `(map, difficulty)`. The difficulty drives
//! the Compare filter; the map name is used as a lower-priority naming fallback
//! (a matching user rule always wins — see `Combat::name`).
//!
//! Phases: existence (which curated entity is present), death counts (exact
//! per-entity deaths, low tier to high), and a hull-damage tie-break for maps
//! whose tiers share death counts (e.g. Hive Space).

use std::{collections::HashMap, path::PathBuf};

use lazy_static::lazy_static;
use log::{info, warn};
use rustc_hash::FxHashMap;
use serde::Deserialize;

/// Config-dir subfolder shared with the app settings/log.
const APP_CONFIG_DIR: &str = "STO_CombatLogAnalyzer";
/// Optional user override file; when present it fully replaces the bundled rules.
const OVERRIDE_FILE_NAME: &str = "detection_rules.json";

/// A map difficulty. `Any` means "known map, tier not distinguished" (either the
/// map has a single tier, or the tier could not be resolved).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
pub enum Difficulty {
    Any,
    Normal,
    Advanced,
    Elite,
}

impl Difficulty {
    /// Human label for display, or `None` when the tier should not be shown
    /// (`Any` — a known map whose tier was not resolved).
    pub fn label(self) -> Option<&'static str> {
        match self {
            Difficulty::Any => None,
            Difficulty::Normal => Some("Normal"),
            Difficulty::Advanced => Some("Advanced"),
            Difficulty::Elite => Some("Elite"),
        }
    }
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
    /// Total hull damage suffered, per distinct entity instance (by id). The
    /// median across instances approximates the entity's hull HP and is what
    /// tells the tiers apart on maps where death counts do not.
    pub hull_damage_per_instance: FxHashMap<u64, f64>,
}

impl CritterMeta {
    /// Median (50th percentile, linearly interpolated to match OSCR's
    /// `numpy.percentile`) of the per-instance hull damage suffered.
    fn median_hull_damage(&self) -> f64 {
        let mut values: Vec<f64> = self.hull_damage_per_instance.values().copied().collect();
        if values.is_empty() {
            return 0.0;
        }
        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let n = values.len();
        if n % 2 == 1 {
            values[n / 2]
        } else {
            (values[n / 2 - 1] + values[n / 2]) / 2.0
        }
    }
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
    /// Map -> difficulty -> {entity unique name -> hull-damage threshold}. Used
    /// to break ties where the tiers share death counts (e.g. Hive Space). An
    /// entity matches when its median hull damage exceeds `threshold * (1 - VAR)`.
    #[serde(default)]
    hull_counts: HashMap<String, HashMap<Difficulty, HashMap<String, f64>>>,
}

/// Variance tolerance for the hull-damage check (matches OSCR's `var`).
const HULL_VARIANCE: f64 = 0.20;

/// The detection result for a combat.
#[derive(Debug, Clone, Default)]
pub struct Detected {
    /// The identified map, or `None` when nothing matched ("Combat").
    pub map: Option<String>,
    /// The resolved difficulty, or `None` when the map is unknown.
    pub difficulty: Option<Difficulty>,
}

lazy_static! {
    /// The active rules, parsed once: a user override from the config dir if
    /// present and valid, otherwise the bundled default.
    pub static ref DETECTION_RULES: DetectionRules = load_rules();
}

/// Path to the optional override, e.g.
/// `~/.config/STO_CombatLogAnalyzer/detection_rules.json`.
fn override_path() -> Option<PathBuf> {
    Some(
        dirs::config_dir()?
            .join(APP_CONFIG_DIR)
            .join(OVERRIDE_FILE_NAME),
    )
}

/// Load the override file if it exists and parses; otherwise the embedded
/// default. A malformed override is ignored (logged), never fatal.
fn load_rules() -> DetectionRules {
    if let Some(path) = override_path() {
        if let Ok(text) = std::fs::read_to_string(&path) {
            match serde_json::from_str(&text) {
                Ok(rules) => {
                    info!("using detection rules override from {}", path.display());
                    return rules;
                }
                Err(e) => warn!(
                    "ignoring invalid detection rules override at {}: {e}",
                    path.display()
                ),
            }
        }
    }
    serde_json::from_str(include_str!("detection_rules.json"))
        .expect("bundled detection_rules.json must be valid")
}

/// The distinct map names the detection can produce, sorted, for showing the
/// curated (auto-detected) maps in the Combat Name Rules editor.
pub fn curated_map_names() -> Vec<String> {
    let mut names: Vec<String> = DETECTION_RULES
        .map_identifiers
        .values()
        .map(|m| m.map.clone())
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The `(entity unique name, map name)` pairs the detection keys on, so the
/// editor can flag user naming rules that match the same entities (and thus
/// shadow an auto-detected map).
pub fn curated_map_identifiers() -> Vec<(String, String)> {
    DETECTION_RULES
        .map_identifiers
        .iter()
        .map(|(unique_name, identifier)| (unique_name.clone(), identifier.map.clone()))
        .collect()
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
    // Death tables exist but none matched: the tier cannot be resolved. Skip the
    // hull tie-break (as OSCR does) and report `Any`.
    if had_tables && difficulty.is_none() {
        return Detected {
            map: Some(map.to_string()),
            difficulty: Some(Difficulty::Any),
        };
    }

    // Hull-damage tie-break: overrides the death-count result where the tiers
    // share death counts (e.g. Hive Space). Higher tiers still override lower.
    if let Some(tier_tables) = rules.hull_counts.get(map) {
        for tier in DIFFICULTY_ORDER {
            if let Some(table) = tier_tables.get(&tier) {
                if hull_damage_match(table, critters) {
                    difficulty = Some(tier);
                }
            }
        }
    }

    Detected {
        map: Some(map.to_string()),
        difficulty,
    }
}

/// A tier matches when every listed entity is present and its median hull damage
/// suffered exceeds `threshold * (1 - VAR)`.
fn hull_damage_match(table: &HashMap<String, f64>, critters: &FxHashMap<&str, &CritterMeta>) -> bool {
    for (unique_name, threshold) in table.iter() {
        match critters.get(unique_name.as_str()) {
            None => return false,
            Some(meta) => {
                let low = threshold * (1.0 - HULL_VARIANCE);
                if !(low < meta.median_hull_damage()) {
                    return false;
                }
            }
        }
    }
    true
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

    /// The bundled default rules, parsed directly so tests are independent of
    /// any user override in the config dir.
    fn bundled_rules() -> DetectionRules {
        serde_json::from_str(include_str!("detection_rules.json"))
            .expect("bundled detection_rules.json must be valid")
    }

    /// Guards the bundled rules file: it must parse.
    #[test]
    fn bundled_rules_parse() {
        let _ = bundled_rules();
    }

    fn critters(entries: &[(&'static str, u32)]) -> Vec<(&'static str, CritterMeta)> {
        entries
            .iter()
            .map(|(n, d)| {
                (
                    *n,
                    CritterMeta {
                        deaths: *d,
                        ..Default::default()
                    },
                )
            })
            .collect()
    }

    /// Build a critter with a single instance carrying the given hull damage.
    fn hull_critter(name: &'static str, hull_damage: f64) -> (&'static str, CritterMeta) {
        let mut meta = CritterMeta::default();
        meta.hull_damage_per_instance.insert(0, hull_damage);
        (name, meta)
    }

    fn view<'a>(owned: &'a [(&'static str, CritterMeta)]) -> FxHashMap<&'a str, &'a CritterMeta> {
        owned.iter().map(|(n, m)| (*n, m)).collect()
    }

    #[test]
    fn unknown_map_when_no_identifier_present() {
        let owned = critters(&[("Some_Random_Npc", 3)]);
        let result = detect(&bundled_rules(), &view(&owned));
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
        let result = detect(&bundled_rules(), &view(&owned));
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
        let result = detect(&bundled_rules(), &view(&owned));
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
        let result = detect(&bundled_rules(), &view(&owned));
        assert_eq!(result.map.as_deref(), Some("Infected Space"));
        assert_eq!(result.difficulty, Some(Difficulty::Any));
    }

    #[test]
    fn hive_space_tier_resolved_by_hull_damage() {
        // Hive Space shares death counts across tiers, so only the median hull
        // damage of the dreadnought distinguishes Advanced (~1.7M) from Elite
        // (~8M). Provide Advanced-level hull damage; expect Advanced.
        let deaths = [
            ("Mission_Space_Borg_Queen_Diamond", 1u32),
            ("Mission_Space_Borg_Battleship_Queen_2_0f_2", 1),
            ("Mission_Space_Borg_Battleship_Queen_1_0f_2", 1),
        ];
        let mut owned: Vec<(&str, CritterMeta)> = deaths
            .iter()
            .map(|(n, d)| {
                (
                    *n,
                    CritterMeta {
                        deaths: *d,
                        ..Default::default()
                    },
                )
            })
            .collect();
        owned.push(hull_critter("Space_Borg_Dreadnought_Hive_Intro", 1_707_034.0));
        owned.push(hull_critter("Space_Borg_Cruiser_Hive_Intro1", 461_582.0));
        owned.push(hull_critter("Space_Borg_Cruiser_Hive_Intro2", 461_582.0));
        owned.push(hull_critter("Space_Borg_Battleship_Hive_Intro", 576_977.0));

        let result = detect(&bundled_rules(), &view(&owned));
        assert_eq!(result.map.as_deref(), Some("Hive Space"));
        assert_eq!(result.difficulty, Some(Difficulty::Advanced));

        // Now Elite-level hull damage on the dreadnought: expect Elite.
        owned[3] = hull_critter("Space_Borg_Dreadnought_Hive_Intro", 8_007_542.0);
        owned[4] = hull_critter("Space_Borg_Cruiser_Hive_Intro1", 2_165_239.0);
        owned[5] = hull_critter("Space_Borg_Cruiser_Hive_Intro2", 2_165_239.0);
        owned[6] = hull_critter("Space_Borg_Battleship_Hive_Intro", 2_706_549.0);
        let result = detect(&bundled_rules(), &view(&owned));
        assert_eq!(result.difficulty, Some(Difficulty::Elite));
    }

    #[test]
    fn fixed_difficulty_map_reports_its_tier() {
        // Winter Invasion is pinned to Normal by its boss entity.
        let owned = critters(&[("Snowman_Q_Boss_Msn_Snowglobe", 1)]);
        let result = detect(&bundled_rules(), &view(&owned));
        assert_eq!(result.map.as_deref(), Some("Winter Invasion"));
        assert_eq!(result.difficulty, Some(Difficulty::Normal));
    }
}
