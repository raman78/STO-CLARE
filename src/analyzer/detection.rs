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

use std::sync::LazyLock;
use std::{collections::HashMap, path::PathBuf};

use log::{info, warn};
use rustc_hash::FxHashMap;
use serde::Deserialize;

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

/// Difficulties tried during the death-count and hull phases, ordered low to
/// high. A higher tier that also matches overrides a lower one (mirrors OSCR,
/// where the tables are ordered so Elite is checked after Advanced).
///
/// `Normal` is included so a map whose queue offers Normal and Advanced but no
/// Elite — Khitomer in Stasis, for one — can carry per-map bands for it. Maps
/// without a Normal table are unaffected: the lookup simply finds nothing.
const DIFFICULTY_ORDER: [Difficulty; 3] =
    [Difficulty::Normal, Difficulty::Advanced, Difficulty::Elite];

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

/// One curated map: how to recognize it and (optionally) how to tell its tiers
/// apart. Keyed by map name in `DetectionRules::maps`.
#[derive(Debug, Deserialize)]
struct MapDef {
    /// Content type (e.g. "TFO", "Patrol"). Shown as a bracketed prefix on the
    /// displayed name (`[TFO] Azure Nebula Rescue`). Curated metadata — STO logs
    /// carry no content type.
    #[serde(default)]
    category: Option<String>,
    /// Combat environment ("Space" / "Ground" / "Shuttle"), from the STO wiki.
    /// Curated metadata — the log itself carries no such marker. Shown in
    /// parentheses on the combat's name (e.g. `[TFO] Into the Hive (Ground)`).
    ///
    /// Kept out of `display_name` on purpose: that string is also what the
    /// settings editor matches naming rules against, and appending the
    /// environment there would stop a rule named after the map from matching it.
    #[serde(default)]
    combat_type: Option<String>,
    /// A difficulty the map pins outright (e.g. Winter Invasion ⇒ Normal); `Any`
    /// when the tier is resolved from the tables below (or cannot be resolved).
    #[serde(default = "any_difficulty")]
    difficulty: Difficulty,
    /// Internal unique names whose presence identifies this map — **any-of** (one
    /// present is enough). Empty for catalog-only entries (a known map we cannot
    /// detect yet).
    ///
    /// NEVER use a `Device_*` entity here (or in the tier tables): those are
    /// player-carried devices (e.g. Kobayashi Maru Resupply, team buffs), present
    /// only when a player happens to equip one — random, not a property of the
    /// map. Anchor on fixed mission NPCs/objects/allies instead.
    #[serde(default)]
    identifiers: Vec<String>,
    /// Like `identifiers` but **all-of**: the map matches only when *every* listed
    /// entity is present together. For anchoring on generic non-dying
    /// "friend"/objective ships that recur individually but not as a set. NOTE:
    /// fragile to combat splitting — if the fight is split (short
    /// `combat_separation_time_seconds`), a fragment may carry only part of the set
    /// and go unrecognized; prefer any-of `identifiers` on a *named*/specific entity
    /// when one exists.
    #[serde(default)]
    identifiers_all: Vec<String>,
    /// difficulty -> {entity unique name -> required death count}. A required
    /// count of 0 means "must be present, any number of deaths".
    #[serde(default)]
    death_counts: HashMap<Difficulty, HashMap<String, u32>>,
    /// difficulty -> {entity unique name -> hull-damage threshold}, breaking ties
    /// where the tiers share death counts. **All** listed entities must be present
    /// and each exceed `threshold * (1 - VAR)`.
    #[serde(default)]
    hull_counts: HashMap<Difficulty, HashMap<String, f64>>,
    /// Like `hull_counts` but **any-of**: the tier matches when *at least one*
    /// listed entity is present and exceeds its threshold. For maps whose enemy
    /// faction is randomized (e.g. Unwanted Guests: regular / Mirror / Control
    /// Borg), where the tier signal is one entity per faction — list every
    /// variant with the same band; only whichever spawned is checked.
    #[serde(default)]
    hull_any: HashMap<Difficulty, HashMap<String, f64>>,
}

fn any_difficulty() -> Difficulty {
    Difficulty::Any
}

impl MapDef {
    /// The display name for `map_name`: the category prefix (if any) plus the
    /// bare name (which stays the map's key).
    fn display_name(&self, map_name: &str) -> String {
        match self.category.as_deref().map(str::trim) {
            Some(category) if !category.is_empty() => format!("[{category}] {map_name}"),
            _ => map_name.to_string(),
        }
    }

    /// Whether this map's identifying entities are present: any one of the
    /// any-of `identifiers`, or the full `identifiers_all` set together.
    fn is_present(&self, critters: &FxHashMap<&str, &CritterMeta>) -> bool {
        self.identifiers
            .iter()
            .any(|e| critters.contains_key(e.as_str()))
            || (!self.identifiers_all.is_empty()
                && self
                    .identifiers_all
                    .iter()
                    .all(|e| critters.contains_key(e.as_str())))
    }

    /// Every entity this map keys on (any-of + all-of), for the editor overlap
    /// warning and the "detectable" filter.
    fn all_identifiers(&self) -> impl Iterator<Item = &String> {
        self.identifiers.iter().chain(self.identifiers_all.iter())
    }
}

/// One ship class and the hull-damage threshold that splits Advanced from Elite
/// for it. Matched as a case-insensitive substring of the entity's unique name.
#[derive(Debug, Deserialize)]
struct ShipClassBand {
    /// Substring identifying the class, e.g. `Dreadnought`.
    #[serde(rename = "match")]
    class: String,
    /// Median hull damage above which the entity is Elite.
    threshold: f64,
    /// Median hull damage at or below which the entity is Normal. Absent means
    /// the class never votes Normal — the Normal bands rest on far less data
    /// than the Advanced/Elite ones (see `DETECTION_SAMPLES.md`).
    #[serde(default)]
    normal_threshold: Option<f64>,
}

/// Map-independent Advanced/Elite split.
///
/// Space HP turned out to be a property of the **ship class**, not of the map or
/// the faction: across nine mapped TFOs and patrols, the same entity has ~4.4x
/// the hull on Elite (median 4.44x over 45 entity pairs), and each class sits in
/// the same band whichever map it appears on. That makes a single global table
/// enough to tier a map we have never sampled — which is most of the catalog.
///
/// Validated leave-one-map-out (thresholds recomputed without the map under
/// test): 217/220 entities and 44/44 combats classified correctly.
///
/// Not applicable to ground maps (they scale ~1.54x, not ~4.4x), to bosses
/// (own scale), or to player-summoned allies — all excluded by name.
#[derive(Debug, Deserialize)]
struct GlobalTier {
    /// Entity name substrings that disqualify an entity from voting.
    #[serde(default)]
    exclude: Vec<String>,
    /// Class bands, tried **in order** — `Battlecruiser` must precede `Cruiser`,
    /// which is a substring of it.
    #[serde(default)]
    classes: Vec<ShipClassBand>,
}

impl GlobalTier {
    /// Tier this combat by majority vote of the entities that died. Returns
    /// `None` when nothing could vote, or when the vote is tied (ambiguous —
    /// better to fall back to the map's own tables than to guess).
    fn classify(&self, critters: &FxHashMap<&str, &CritterMeta>) -> Option<Difficulty> {
        let (mut normal, mut advanced, mut elite) = (0usize, 0usize, 0usize);
        for (name, meta) in critters.iter() {
            // Only entities that died: for the others the hull figure is damage
            // we happened to deal, not the entity's HP.
            if meta.deaths == 0 {
                continue;
            }
            let lower = name.to_lowercase();
            if self
                .exclude
                .iter()
                .any(|e| lower.contains(&e.to_lowercase()))
            {
                continue;
            }
            let Some(band) = self
                .classes
                .iter()
                .find(|c| lower.contains(&c.class.to_lowercase()))
            else {
                continue;
            };
            let median = meta.median_hull_damage();
            if median <= 0.0 {
                continue;
            }
            if median > band.threshold {
                elite += 1;
            } else if band.normal_threshold.is_some_and(|n| median <= n) {
                normal += 1;
            } else {
                advanced += 1;
            }
        }
        // Strict majority wins; a tie is ambiguous, so decline and let the map's
        // own tables decide.
        let winner = [
            (elite, Difficulty::Elite),
            (advanced, Difficulty::Advanced),
            (normal, Difficulty::Normal),
        ]
        .into_iter()
        .max_by_key(|(votes, _)| *votes)?;
        let tied = [elite, advanced, normal]
            .iter()
            .filter(|v| **v == winner.0)
            .count()
            > 1;
        (winner.0 > 0 && !tied).then_some(winner.1)
    }
}

/// The rule tables, deserialized from `detection_rules.json`: curated maps keyed
/// by name, each carrying how to recognize it and how to tell its tiers apart,
/// plus the map-independent tier table.
#[derive(Debug, Deserialize)]
pub struct DetectionRules {
    maps: HashMap<String, MapDef>,
    #[serde(default)]
    global_tier: Option<GlobalTier>,
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
    /// The map's environment ("Space" / "Ground" / "Shuttle"), when curated.
    pub combat_type: Option<String>,
}

/// The active rules, parsed once: a user override from the config dir if
/// present and valid, otherwise the bundled default.
pub static DETECTION_RULES: LazyLock<DetectionRules> = LazyLock::new(load_rules);

/// Path to the optional override, e.g. `~/.config/STO-CLARE/detection_rules.json`.
fn override_path() -> Option<PathBuf> {
    Some(crate::helpers::paths::config_dir()?.join(OVERRIDE_FILE_NAME))
}

/// Load the override file if it exists and parses; otherwise the embedded
/// default. A malformed override is ignored (logged), never fatal.
fn load_rules() -> DetectionRules {
    if let Some(path) = override_path()
        && let Ok(text) = std::fs::read_to_string(&path)
    {
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
    serde_json::from_str(include_str!("detection_rules.json"))
        .expect("bundled detection_rules.json must be valid")
}

/// The distinct map names the detection can produce, sorted, for showing the
/// curated (auto-detected) maps in the Combat Name Rules editor.
pub fn curated_map_names() -> Vec<String> {
    let mut names: Vec<String> = DETECTION_RULES
        .maps
        .iter()
        .filter(|(_, def)| def.all_identifiers().next().is_some())
        .map(|(name, def)| def.display_name(name))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The `(entity unique name, map name)` pairs the detection keys on, so the
/// editor can flag user naming rules that match the same entities (and thus
/// shadow an auto-detected map). Catalog-only maps (no identifiers) contribute
/// nothing.
pub fn curated_map_identifiers() -> Vec<(String, String)> {
    DETECTION_RULES
        .maps
        .iter()
        .flat_map(|(name, def)| {
            let display = def.display_name(name);
            def.all_identifiers()
                .map(move |unique| (unique.clone(), display.clone()))
        })
        .collect()
}

/// Detect `(map, difficulty)` from the combat's critters, given a view keyed by
/// each entity's internal unique name. Mirrors OSCR's `detect_map`: identify the
/// map by a present curated entity, then resolve the tier from death counts.
pub fn detect(rules: &DetectionRules, critters: &FxHashMap<&str, &CritterMeta>) -> Detected {
    // Existence phase: find a map whose identifying entity is present. A map that
    // pins a fixed difficulty returns immediately.
    let mut matched: Option<(&str, &MapDef)> = None;
    for (name, def) in rules.maps.iter() {
        if def.is_present(critters) {
            matched = Some((name.as_str(), def));
            if def.difficulty != Difficulty::Any {
                return Detected {
                    map: Some(def.display_name(name)),
                    difficulty: Some(def.difficulty),
                    combat_type: def.combat_type.clone(),
                };
            }
        }
    }
    let Some((name, def)) = matched else {
        return Detected::default();
    };

    // Death-count phase: check each tier (low to high); a higher matching tier
    // overrides. If the map has tables but none match, the tier stays `Any`.
    let mut difficulty = None;
    let mut had_tables = false;
    for tier in DIFFICULTY_ORDER {
        if let Some(table) = def.death_counts.get(&tier) {
            had_tables = true;
            if death_counts_match(table, critters) {
                difficulty = Some(tier);
            }
        }
    }
    // Map-independent tier, from the ship classes present. Computed up front so
    // it can also rescue a map whose death tables did not match.
    let global = rules
        .global_tier
        .as_ref()
        .and_then(|global| global.classify(critters));

    // Death tables exist but none matched: the tier cannot be resolved *by deaths*.
    // Report `Any` and skip the hull tie-break (as OSCR does) — unless the map also
    // has hull tables, in which case a death marker is only one signal (e.g. Bug
    // Hunt: an Elite-only entity plus hull bands) and we fall through to hull.
    if had_tables && difficulty.is_none() && def.hull_counts.is_empty() && def.hull_any.is_empty() {
        return Detected {
            map: Some(def.display_name(name)),
            difficulty: Some(global.unwrap_or(Difficulty::Any)),
            combat_type: def.combat_type.clone(),
        };
    }

    // Hull-damage tie-break: overrides the death-count result where the tiers
    // share death counts (e.g. Hive Onslaught). Higher tiers still override lower.
    for tier in DIFFICULTY_ORDER {
        if let Some(table) = def.hull_counts.get(&tier)
            && hull_damage_match(table, critters)
        {
            difficulty = Some(tier);
        }
    }
    // Any-of hull tie-break: for faction-randomized maps, whichever listed
    // variant spawned decides the tier. Same low-to-high override.
    for tier in DIFFICULTY_ORDER {
        if let Some(table) = def.hull_any.get(&tier)
            && hull_any_match(table, critters)
        {
            difficulty = Some(tier);
        }
    }

    // The global table has the last word, so that disagreements with the curated
    // per-map tables surface immediately instead of being masked by them. Any
    // disagreement is logged: on a map with hand-verified tables it means one of
    // the two is wrong, and that is exactly what we want to see.
    if let Some(global) = global {
        if let Some(from_tables) = difficulty
            && from_tables != global
            && from_tables != Difficulty::Any
        {
            warn!(
                "{}: global ship-class tier says {:?}, the map's own tables say {:?}",
                def.display_name(name),
                global,
                from_tables
            );
        }
        difficulty = Some(global);
    }

    Detected {
        map: Some(def.display_name(name)),
        difficulty,
        combat_type: def.combat_type.clone(),
    }
}

/// A tier matches when *at least one* listed entity is present and its median
/// hull damage exceeds `threshold * (1 - VAR)`. See `MapDef::hull_any`.
fn hull_any_match(table: &HashMap<String, f64>, critters: &FxHashMap<&str, &CritterMeta>) -> bool {
    table.iter().any(|(unique_name, threshold)| {
        critters.get(unique_name.as_str()).is_some_and(|meta| {
            meta.deaths > 0 && threshold * (1.0 - HULL_VARIANCE) < meta.median_hull_damage()
        })
    })
}

/// A tier matches when every listed entity is present and its median hull damage
/// suffered exceeds `threshold * (1 - VAR)`.
fn hull_damage_match(
    table: &HashMap<String, f64>,
    critters: &FxHashMap<&str, &CritterMeta>,
) -> bool {
    for (unique_name, threshold) in table.iter() {
        match critters.get(unique_name.as_str()) {
            None => return false,
            Some(meta) => {
                let low = threshold * (1.0 - HULL_VARIANCE);
                if meta.deaths == 0 || !(low < meta.median_hull_damage()) {
                    return false;
                }
            }
        }
    }
    true
}

/// A tier matches when every listed entity is present and, for entries with a
/// required count > 0, died exactly that many times.
fn death_counts_match(
    table: &HashMap<String, u32>,
    critters: &FxHashMap<&str, &CritterMeta>,
) -> bool {
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
    /// It died: a median hull figure only means the entity's HP for entities that
    /// did, so every table — per-map and global alike — ignores the others.
    fn hull_critter(name: &'static str, hull_damage: f64) -> (&'static str, CritterMeta) {
        let mut meta = CritterMeta::default();
        meta.hull_damage_per_instance.insert(0, hull_damage);
        meta.deaths = 1;
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
        assert_eq!(result.map.as_deref(), Some("[TFO] Infected: The Conduit"));
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
        assert_eq!(result.map.as_deref(), Some("[TFO] Infected: The Conduit"));
        assert_eq!(result.difficulty, Some(Difficulty::Elite));
    }

    #[test]
    fn known_map_but_wrong_counts_stays_any() {
        // The map identifier is present, but no tier's counts match.
        let owned = critters(&[("Space_Borg_Dreadnought_Raidisode_Sibrian_Final_Boss", 1)]);
        let result = detect(&bundled_rules(), &view(&owned));
        assert_eq!(result.map.as_deref(), Some("[TFO] Infected: The Conduit"));
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
        owned.push(hull_critter(
            "Space_Borg_Dreadnought_Hive_Intro",
            1_707_034.0,
        ));
        owned.push(hull_critter("Space_Borg_Cruiser_Hive_Intro1", 461_582.0));
        owned.push(hull_critter("Space_Borg_Cruiser_Hive_Intro2", 461_582.0));
        owned.push(hull_critter("Space_Borg_Battleship_Hive_Intro", 576_977.0));

        let result = detect(&bundled_rules(), &view(&owned));
        assert_eq!(result.map.as_deref(), Some("[TFO] Hive Onslaught"));
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
    fn rescue_and_search_tier_by_hull_damage() {
        // A patrol with no fixed death counts: only the median hull damage of
        // the Mokai ships (~4x higher on Elite) tells the tiers apart. The map
        // itself is identified by the rescued Lukari ships — the Mokai are
        // shared with Peril Over Pahvo and cannot identify anything.
        let advanced = vec![
            hull_critter("Msn_Space_Lukari_Science_Vessel", 0.0),
            hull_critter("Space_Klingon_Cruiser_Dsc_Mokai", 363_000.0),
            hull_critter("Space_Klingon_Battleship_Dsc_Mokai", 338_000.0),
        ];
        let result = detect(&bundled_rules(), &view(&advanced));
        assert_eq!(result.map.as_deref(), Some("[Patrol] Rescue and Search"));
        assert_eq!(result.difficulty, Some(Difficulty::Advanced));

        let elite = vec![
            hull_critter("Msn_Space_Lukari_Science_Vessel", 0.0),
            hull_critter("Space_Klingon_Cruiser_Dsc_Mokai", 1_492_000.0),
            hull_critter("Space_Klingon_Battleship_Dsc_Mokai", 1_540_000.0),
        ];
        let result = detect(&bundled_rules(), &view(&elite));
        assert_eq!(result.difficulty, Some(Difficulty::Elite));
    }

    /// Peril Over Pahvo fields the same Mokai ships as Rescue and Search, so
    /// neither can be identified by them. Each keys on its own mission objects:
    /// the rescued Lukari ships, and the Pahvo defence satellites.
    #[test]
    fn pahvo_and_rescue_are_told_apart_by_their_mission_objects() {
        let rules = bundled_rules();
        let shared = "Space_Klingon_Cruiser_Dsc_Mokai";

        let pahvo = vec![
            hull_critter(
                "Msn_Dsc_Pahvo_Defense_Queue_System_Upgradeable_Satellite",
                0.0,
            ),
            hull_critter(shared, 376_972.0),
            hull_critter("Space_Klingon_Battleship_Dsc_Mokai", 472_023.0),
        ];
        let result = detect(&rules, &view(&pahvo));
        assert_eq!(result.map.as_deref(), Some("[TFO] Peril Over Pahvo"));
        assert_eq!(result.difficulty, Some(Difficulty::Advanced));

        let rescue = vec![
            hull_critter("Msn_Space_Lukari_Science_Vessel", 0.0),
            hull_critter(shared, 363_000.0),
            hull_critter("Space_Klingon_Battleship_Dsc_Mokai", 338_000.0),
        ];
        let result = detect(&rules, &view(&rescue));
        assert_eq!(result.map.as_deref(), Some("[Patrol] Rescue and Search"));
        assert_eq!(result.difficulty, Some(Difficulty::Advanced));
    }

    #[test]
    fn trouble_over_terrh_tier_by_hull_damage() {
        // The Elachi patrol shares the rescued Lleiset with Azure Nebula Rescue,
        // so it is anchored on an Elachi enemy (Azure is anchored on a Tholian
        // one). Tiers are told apart only by the median hull damage (~4.5x
        // higher on Elite), like Rescue and Search.
        let advanced = vec![
            hull_critter("Space_Elachi_Frigate", 106_615.0),
            hull_critter("Space_Elachi_Escort", 486_256.0),
        ];
        let result = detect(&bundled_rules(), &view(&advanced));
        assert_eq!(result.map.as_deref(), Some("[Patrol] Trouble Over Terrh"));
        assert_eq!(result.difficulty, Some(Difficulty::Advanced));

        let elite = vec![
            hull_critter("Space_Elachi_Frigate", 490_858.0),
            hull_critter("Space_Elachi_Escort", 2_279_671.0),
        ];
        let result = detect(&bundled_rules(), &view(&elite));
        assert_eq!(result.difficulty, Some(Difficulty::Elite));
    }

    #[test]
    fn azure_and_terrh_are_told_apart_by_enemy_faction() {
        // Both maps rescue the Lleiset; the enemy faction disambiguates them.
        // Tholians -> Azure Nebula Rescue.
        let azure = vec![hull_critter("Space_Tholian_Cruiser_Web", 1_700_205.0)];
        let result = detect(&bundled_rules(), &view(&azure));
        assert_eq!(result.map.as_deref(), Some("[TFO] Azure Nebula Rescue"));

        // Elachi -> Trouble Over Terrh.
        let terrh = vec![hull_critter("Space_Elachi_Frigate", 106_615.0)];
        let result = detect(&bundled_rules(), &view(&terrh));
        assert_eq!(result.map.as_deref(), Some("[Patrol] Trouble Over Terrh"));
    }

    #[test]
    fn ninth_rule_tier_by_hull_damage_across_random_factions() {
        // The Ninth Rule randomizes its enemy fleet (Gorn / Nausicaan / Orion /
        // Terran-Mirror, sometimes two at once), so the tier is `hull_any`: HP is
        // per *class*, not per faction. Real medians below.

        // 2026-07-28 18:40 — Gorn, Advanced.
        let advanced = vec![
            hull_critter("Space_Federation_Cruiser_Galaxy", 34_130.0),
            hull_critter("Space_Gorn_Battleship", 326_971.0),
            hull_critter("Space_Gorn_Cruiser", 265_301.0),
            hull_critter("Space_Gorn_Frigate", 107_894.0),
        ];
        let result = detect(&bundled_rules(), &view(&advanced));
        assert_eq!(result.map.as_deref(), Some("[Patrol] The Ninth Rule"));
        assert_eq!(result.difficulty, Some(Difficulty::Advanced));

        // 2026-07-29 21:56 — Gorn *and* Orion together, Elite. Both factions'
        // ships must land in the same band (Gorn cruiser 1.205M, Orion 1.201M).
        let elite = vec![
            hull_critter("Space_Federation_Cruiser_Galaxy", 60_775.0),
            hull_critter("Space_Gorn_Battleship", 1_519_796.0),
            hull_critter("Space_Gorn_Cruiser", 1_205_138.0),
            hull_critter("Space_Orion_Pirates_Cruiser", 1_200_843.0),
            hull_critter("Space_Gorn_Frigate", 481_527.0),
            hull_critter("Space_Orion_Pirates_Frigate", 489_814.0),
        ];
        let result = detect(&bundled_rules(), &view(&elite));
        assert_eq!(result.map.as_deref(), Some("[Patrol] The Ninth Rule"));
        assert_eq!(result.difficulty, Some(Difficulty::Elite));
    }

    /// Azure Nebula Rescue is anchored but has no tier tables of its own. The
    /// global ship-class table alone must tier it. Figures are the real samples
    /// recorded in docs/DETECTION_SAMPLES.md.
    #[test]
    fn global_tier_resolves_a_map_without_its_own_tables() {
        let advanced = vec![
            hull_critter("Space_Tholian_Cruiser_Web", 355_249.0),
            hull_critter("Space_Tholian_Battleship", 652_603.0),
        ];
        let result = detect(&bundled_rules(), &view(&advanced));
        assert_eq!(result.map.as_deref(), Some("[TFO] Azure Nebula Rescue"));
        assert_eq!(result.difficulty, Some(Difficulty::Advanced));

        let elite = vec![
            hull_critter("Space_Tholian_Cruiser_Web", 1_700_205.0),
            hull_critter("Space_Tholian_Battleship", 2_888_013.0),
        ];
        let result = detect(&bundled_rules(), &view(&elite));
        assert_eq!(result.difficulty, Some(Difficulty::Elite));
    }

    /// Red Alert: Na'kuhl, anchored any-of: the escorted convoy runs the whole
    /// fight while the named Red Alert frigate only appears in its first half, so
    /// either half of a split still resolves.
    #[test]
    fn red_alert_nakuhl_resolves_from_either_anchor() {
        let rules = bundled_rules();
        for anchor in [
            "Event_Nakuhl_Space_Convoy_Transport",
            "Space_Federation_Frigate_Nakuhl_Red_Alert",
        ] {
            let owned = vec![
                hull_critter(anchor, 8_464.0),
                hull_critter("Space_Nakuhl_Battleship", 224_627.0),
            ];
            let result = detect(&rules, &view(&owned));
            assert_eq!(
                result.map.as_deref(),
                Some("[TFO] Red Alert: Na'kuhl"),
                "anchor {anchor} must identify the map on its own"
            );
            assert_eq!(result.difficulty, Some(Difficulty::Normal));
        }
    }

    /// Trouble Over Terrh and Red Alert: Elachi are told apart by anchors whose
    /// names differ only by a `Mission_` prefix — `Space_Elachi_Frigate` versus
    /// `Mission_Space_Elachi_Frigate`. Identifier lookup is exact (a hash-map
    /// key), not substring, so the two never cross; this test would fail if that
    /// ever became a `contains` match. They do share the battleships and escorts,
    /// which is why neither is anchored on those.
    #[test]
    fn elachi_maps_are_told_apart_by_the_mission_prefix() {
        let rules = bundled_rules();
        let red_alert = vec![
            hull_critter("Mission_Space_Elachi_Frigate", 62_531.0),
            hull_critter("Space_Elachi_Battleship_V1", 217_336.0),
            hull_critter("Space_Elachi_Escort", 211_767.0),
        ];
        let result = detect(&rules, &view(&red_alert));
        assert_eq!(result.map.as_deref(), Some("[TFO] Red Alert: Elachi"));
        assert_eq!(result.difficulty, Some(Difficulty::Normal));

        let terrh = vec![
            hull_critter("Space_Elachi_Frigate", 111_565.0),
            hull_critter("Space_Elachi_Battleship_V1", 606_172.0),
            hull_critter("Space_Elachi_Escort", 486_256.0),
        ];
        let result = detect(&rules, &view(&terrh));
        assert_eq!(result.map.as_deref(), Some("[Patrol] Trouble Over Terrh"));
        assert_eq!(result.difficulty, Some(Difficulty::Advanced));
    }

    /// Tzenkethi Front and Red Alert: Tzenkethi genuinely share the
    /// `Mission_Event_Tzenkethi_Red_Alert_*` ships — confirmed once a real Red
    /// Alert run appeared. Each is anchored on something the other never fields,
    /// so the shared ships cannot merge them. This is why Tzenkethi Front is
    /// deliberately *not* anchored on those entities despite them being present
    /// in all of its runs.
    #[test]
    fn tzenkethi_front_and_red_alert_stay_apart() {
        let rules = bundled_rules();
        let shared = "Mission_Event_Tzenkethi_Red_Alert_Tzenkethi_Dreadnought";

        let red_alert = vec![
            hull_critter(shared, 1_052_092.0),
            hull_critter("Msn_Event_Tzenkethi_Alert_System_Satellite", 0.0),
            hull_critter("Space_Tzenkethi_Cruiser_Var1", 109_986.0),
        ];
        let result = detect(&rules, &view(&red_alert));
        assert_eq!(result.map.as_deref(), Some("[TFO] Red Alert: Tzenkethi"));
        assert_eq!(result.difficulty, Some(Difficulty::Normal));

        let front = vec![
            hull_critter(shared, 1_682_031.0),
            hull_critter("Msn_Tzk_Tzenkethi_Assault_Ball", 0.0),
            hull_critter("Space_Tzenkethi_Cruiser_Var1", 227_332.0),
        ];
        let result = detect(&rules, &view(&front));
        assert_eq!(result.map.as_deref(), Some("[TFO] Tzenkethi Front"));
        assert_eq!(result.difficulty, Some(Difficulty::Advanced));
    }

    /// Red Alert: Borg is anchored any-of, because the named boss only shows up
    /// in the fight's later phase — an early split fragment must still be
    /// recognized by the rank-and-file `_Dse` ships.
    #[test]
    fn red_alert_borg_is_recognized_without_its_boss() {
        let rules = bundled_rules();
        let with_boss = vec![
            hull_critter("Mission_Space_Borg_Battleship_7_Of_10", 3_107_477.0),
            hull_critter("Space_Borg_Battleship_Dse", 211_201.0),
        ];
        let result = detect(&rules, &view(&with_boss));
        assert_eq!(result.map.as_deref(), Some("[TFO] Red Alert: Borg"));
        assert_eq!(result.difficulty, Some(Difficulty::Normal));

        let boss_missing = vec![
            hull_critter("Space_Borg_Cruiser_Dse", 187_210.0),
            hull_critter("Space_Borg_Frigate_Dse", 79_156.0),
        ];
        let result = detect(&rules, &view(&boss_missing));
        assert_eq!(result.map.as_deref(), Some("[TFO] Red Alert: Borg"));
        assert_eq!(result.difficulty, Some(Difficulty::Normal));
    }

    /// A Red Alert: Tholian run also contains `Space_Tholian_Cruiser_Web`, the
    /// entity Azure Nebula Rescue is anchored on, so both maps match. The Red
    /// Alert must win: it pins a difficulty, and `detect` returns as soon as it
    /// reaches a map that does, whatever the (unordered) map iteration order.
    #[test]
    fn red_alert_wins_over_azure_when_both_match() {
        let owned = vec![
            hull_critter("Space_Tholian_Dreadnought_Red_Alert", 1_619_134.0),
            hull_critter("Space_Tholian_Cruiser_Web", 198_722.0),
            hull_critter("Space_Tholian_Battleship", 230_750.0),
        ];
        for _ in 0..16 {
            let result = detect(&bundled_rules(), &view(&owned));
            assert_eq!(result.map.as_deref(), Some("[TFO] Red Alert: Tholian"));
            assert_eq!(result.difficulty, Some(Difficulty::Normal));
        }

        // Azure on its own is unaffected.
        let azure = vec![
            hull_critter("Space_Tholian_Cruiser_Web", 355_249.0),
            hull_critter("Space_Tholian_Battleship", 652_603.0),
        ];
        let result = detect(&bundled_rules(), &view(&azure));
        assert_eq!(result.map.as_deref(), Some("[TFO] Azure Nebula Rescue"));
        assert_eq!(result.difficulty, Some(Difficulty::Advanced));
    }

    /// The experimental Normal band, on a map with no tables of its own. Hull
    /// figures are below the Normal thresholds for their classes (cruiser 188k,
    /// battleship 260k) but well above nothing, so the vote is unanimous.
    #[test]
    fn global_tier_can_report_normal() {
        let owned = vec![
            hull_critter("Space_Tholian_Cruiser_Web", 175_000.0),
            hull_critter("Space_Tholian_Battleship", 220_000.0),
        ];
        let result = detect(&bundled_rules(), &view(&owned));
        assert_eq!(result.map.as_deref(), Some("[TFO] Azure Nebula Rescue"));
        assert_eq!(result.difficulty, Some(Difficulty::Normal));
    }

    /// A split vote is ambiguous, so the global tier declines rather than
    /// guessing — here one entity reads Normal and the other Advanced.
    #[test]
    fn global_tier_declines_a_tied_vote() {
        let owned = vec![
            hull_critter("Space_Tholian_Cruiser_Web", 175_000.0),
            hull_critter("Space_Tholian_Battleship", 400_000.0),
        ];
        let result = detect(&bundled_rules(), &view(&owned));
        assert_eq!(result.map.as_deref(), Some("[TFO] Azure Nebula Rescue"));
        assert_eq!(result.difficulty, None, "a tie must not resolve to a tier");
    }

    /// Ground maps scale ~1.54x, not ~4.4x, so ground entities must never vote —
    /// otherwise an Elite ground boss (well under any space threshold) would drag
    /// the combat to Advanced. Bug Hunt keeps resolving from its own tables.
    #[test]
    fn global_tier_ignores_ground_entities() {
        let rules = bundled_rules();
        let elite = vec![
            ("Bluegills_Ground_Boss", {
                let mut m = CritterMeta::default();
                m.deaths = 1;
                m.hull_damage_per_instance.insert(0, 451_781.0);
                m
            }),
            ("Bluegills_Ground_Cdr", {
                let mut m = CritterMeta::default();
                m.deaths = 1;
                m.hull_damage_per_instance.insert(0, 16_077.0);
                m
            }),
            ("Bluegills_Ground_Ens_Noautospawn_Queenfodder", {
                let mut m = CritterMeta::default();
                m.deaths = 13;
                m
            }),
        ];
        let result = detect(&rules, &view(&elite));
        assert_eq!(result.map.as_deref(), Some("[TFO] Bug Hunt"));
        assert_eq!(
            result.difficulty,
            Some(Difficulty::Elite),
            "ground entities must not vote in the global space tier"
        );
    }

    /// `Cruiser` is a substring of `Battlecruiser`, so the class bands are tried
    /// in order. 645k sits *between* the two bands — below the battlecruiser's
    /// 650k (Advanced) but above the cruiser's 640k (Elite) — so the result tells
    /// the two apart: matching `Cruiser` first would wrongly yield Elite.
    #[test]
    fn global_tier_matches_battlecruiser_before_cruiser() {
        let owned = vec![
            hull_critter("Space_Klingon_Dreadnought_Ktinga_Lrell", 0.0),
            hull_critter("Space_Klingon_Battlecruiser", 645_000.0),
        ];
        let result = detect(&bundled_rules(), &view(&owned));
        assert_eq!(result.map.as_deref(), Some("[Patrol] To Die With Honor"));
        assert_eq!(
            result.difficulty,
            Some(Difficulty::Advanced),
            "the battlecruiser band must win over the cruiser substring"
        );
    }

    /// Khitomer in Stasis offers Normal and Advanced but no Elite, so its bands
    /// live under `Normal`. That tier is only reached because it was added to
    /// `DIFFICULTY_ORDER`; before that the table was parsed and never consulted.
    #[test]
    fn ground_map_resolves_its_normal_band() {
        let rules = bundled_rules();
        let normal = vec![
            hull_critter("Mission_Borgraid03_Borg_Power_Node", 5_947.0),
            hull_critter("Ground_Borg_Capt_Melee", 6_541.0),
            hull_critter("Ground_Borg_Cdr_Melee", 2_392.0),
        ];
        let result = detect(&rules, &view(&normal));
        assert_eq!(result.map.as_deref(), Some("[TFO] Khitomer in Stasis"));
        assert_eq!(result.difficulty, Some(Difficulty::Normal));

        let advanced = vec![
            hull_critter("Mission_Borgraid03_Borg_Power_Node", 6_401.0),
            hull_critter("Ground_Borg_Capt_Melee", 14_047.0),
            hull_critter("Ground_Borg_Cdr_Melee", 5_325.0),
        ];
        let result = detect(&rules, &view(&advanced));
        assert_eq!(result.difficulty, Some(Difficulty::Advanced));
    }

    #[test]
    fn undine_infiltration_tier_by_hull_damage() {
        let rules = bundled_rules();
        let advanced = vec![
            hull_critter(
                "Mission_Ground_Undine_Capt_Range_Psi_Infiltration_Boss",
                30_710.0,
            ),
            hull_critter("Ground_Undine_Capt_Range_Psi", 19_476.0),
            hull_critter("Ground_Undine_Lt_Range_Psi", 3_513.0),
        ];
        let result = detect(&rules, &view(&advanced));
        assert_eq!(result.map.as_deref(), Some("[TFO] Undine Infiltration"));
        assert_eq!(result.difficulty, Some(Difficulty::Advanced));

        let elite = vec![
            hull_critter(
                "Mission_Ground_Undine_Capt_Range_Psi_Infiltration_Boss",
                71_916.0,
            ),
            hull_critter("Ground_Undine_Capt_Range_Psi", 31_900.0),
            hull_critter("Ground_Undine_Lt_Range_Psi", 5_695.0),
        ];
        let result = detect(&rules, &view(&elite));
        assert_eq!(result.difficulty, Some(Difficulty::Elite));
    }

    #[test]
    fn an_entity_that_survived_does_not_decide_the_tier() {
        // Modelled on the opening fragment of the 2026-07-30 12:35 Undine
        // Infiltration run: 47 lines, cut off by a 70s gap, holding the boss alone
        // and *alive*. For an entity that did not die the median is the damage we
        // happened to deal, not its HP, so it must not tier the combat - the same
        // reason the global ship-class table skips `deaths == 0`.
        //
        // 30,000 is deliberately above the Advanced firing threshold
        // (30,710 * 0.8 = 24,568): without the guard this fragment reads Advanced.
        let mut alive = CritterMeta::default();
        alive.hull_damage_per_instance.insert(0, 30_000.0);
        assert_eq!(alive.deaths, 0);

        let owned = vec![(
            "Mission_Ground_Undine_Capt_Range_Psi_Infiltration_Boss",
            alive,
        )];
        let result = detect(&bundled_rules(), &view(&owned));
        // The anchor still identifies the map - only the tier is withheld.
        assert_eq!(result.map.as_deref(), Some("[TFO] Undine Infiltration"));
        assert_eq!(result.difficulty, None);
    }

    #[test]
    fn tzenkethi_front_tier_by_hull_damage() {
        // Fixed-faction TFO. Anchored on the mission assault objects, *not* on
        // the `Mission_Event_Tzenkethi_Red_Alert_*` ships — those carry reused
        // Red Alert assets and the catalog holds a separate "Red Alert:
        // Tzenkethi" map, so anchoring there could cross the two. They are still
        // safe as a tier signal, which is only read after the map matched.
        let advanced = vec![
            hull_critter("Msn_Tzk_Tzenkethi_Assault_Ball", 0.0),
            hull_critter(
                "Mission_Event_Tzenkethi_Red_Alert_Tzenkethi_Dreadnought",
                1_682_031.0,
            ),
            hull_critter("Space_Tzenkethi_Cruiser_Var1", 227_332.0),
            hull_critter("Space_Tzenkethi_Frigate", 101_212.0),
        ];
        let result = detect(&bundled_rules(), &view(&advanced));
        assert_eq!(result.map.as_deref(), Some("[TFO] Tzenkethi Front"));
        assert_eq!(result.difficulty, Some(Difficulty::Advanced));

        let elite = vec![
            hull_critter("Msn_Tzk_Tzenkethi_Assault_Ball", 0.0),
            hull_critter(
                "Mission_Event_Tzenkethi_Red_Alert_Tzenkethi_Dreadnought",
                8_424_430.0,
            ),
            hull_critter("Space_Tzenkethi_Cruiser_Var1", 1_017_058.0),
            hull_critter("Space_Tzenkethi_Frigate", 469_240.0),
        ];
        let result = detect(&bundled_rules(), &view(&elite));
        assert_eq!(result.map.as_deref(), Some("[TFO] Tzenkethi Front"));
        assert_eq!(result.difficulty, Some(Difficulty::Elite));
    }

    #[test]
    fn ninth_rule_anchors_survive_a_combat_split() {
        // A 45s `combat_separation_time_seconds` splits this patrol. The two
        // anchors are listed any-of precisely because neither is in every
        // fragment: the 2026-07-28 18:38 fragment carries Hofmann but no Galaxy,
        // while the 2026-07-28 22:18 run carries Galaxy but no Hofmann.
        let hofmann_only = vec![
            hull_critter("Mission_Space_Federation_Science_Hofmann", 8_159.0),
            hull_critter("Space_Gorn_Cruiser", 264_812.0),
            hull_critter("Space_Gorn_Frigate", 103_119.0),
        ];
        let result = detect(&bundled_rules(), &view(&hofmann_only));
        assert_eq!(result.map.as_deref(), Some("[Patrol] The Ninth Rule"));
        assert_eq!(result.difficulty, Some(Difficulty::Advanced));

        let galaxy_only = vec![
            hull_critter("Space_Federation_Cruiser_Galaxy", 8_667.0),
            hull_critter("Space_Nausicaan_Battleship", 345_674.0),
            hull_critter("Space_Nausicaan_Escort", 210_443.0),
            hull_critter("Space_Nausicaan_Frigate", 115_184.0),
        ];
        let result = detect(&bundled_rules(), &view(&galaxy_only));
        assert_eq!(result.map.as_deref(), Some("[Patrol] The Ninth Rule"));
        assert_eq!(result.difficulty, Some(Difficulty::Advanced));
    }

    #[test]
    fn category_prefixes_the_displayed_map_name() {
        let parse = |json: &str| serde_json::from_str::<MapDef>(json).unwrap();

        assert_eq!(
            parse(r#"{"category": "TFO", "combat_type": "Space"}"#)
                .display_name("Azure Nebula Rescue"),
            "[TFO] Azure Nebula Rescue"
        );
        // No category -> bare name.
        assert_eq!(
            parse("{}").display_name("Infected: The Conduit"),
            "Infected: The Conduit"
        );
        // A whitespace-only category adds no brackets.
        assert_eq!(
            parse(r#"{"category": "  "}"#).display_name("Bug Hunt"),
            "Bug Hunt"
        );
    }

    #[test]
    fn added_tfo_maps_detected_without_tier() {
        // Identifiers taken from the user's own naming rules; no tier tables yet,
        // so the map is detected but the difficulty stays unresolved (like Bug Hunt).
        for (entity, map) in [
            (
                "Msn_Kcw_Rura_Penthe_System_Tfo_Prisoner_Transport",
                "[TFO] Best Served Cold",
            ),
            ("Event_Vault_Ext_Tholian_Weaver", "[TFO] Vault: Ensnared"),
        ] {
            let owned = critters(&[(entity, 1)]);
            let result = detect(&bundled_rules(), &view(&owned));
            assert_eq!(result.map.as_deref(), Some(map));
            assert_eq!(result.difficulty, None);
        }
    }

    #[test]
    fn unwanted_guests_tier_by_dreadnought_any_faction() {
        // Enemies are randomly regular / Mirror / Control Borg; the dreadnought's
        // hull is the tier signal and is faction-independent in value — only the
        // entity name changes — so it is matched any-of (`hull_any`). Anchored on
        // the allied Aetherian ships. NB: NEVER anchor on `Device_*` entities —
        // those are player-carried devices, present only if a player equips them.
        let advanced = vec![
            hull_critter("Space_Aetherian_Cruiser", 0.0),
            hull_critter("Space_Aetherian_Dreadnought", 0.0),
            hull_critter("Space_Borg_Dreadnought_Mirror", 1_954_225.0),
        ];
        let result = detect(&bundled_rules(), &view(&advanced));
        assert_eq!(result.map.as_deref(), Some("[Patrol] Unwanted Guests"));
        assert_eq!(result.difficulty, Some(Difficulty::Advanced));

        // Elite resolves whichever faction spawned: plain (regular) Borg …
        let elite_regular = vec![
            hull_critter("Space_Aetherian_Cruiser", 0.0),
            hull_critter("Space_Aetherian_Dreadnought", 0.0),
            hull_critter("Space_Borg_Dreadnought", 8_300_426.0),
        ];
        assert_eq!(
            detect(&bundled_rules(), &view(&elite_regular)).difficulty,
            Some(Difficulty::Elite)
        );

        // … and Control Borg.
        let elite_control = vec![
            hull_critter("Space_Aetherian_Cruiser", 0.0),
            hull_critter("Space_Aetherian_Dreadnought", 0.0),
            hull_critter("Space_Borg_Dreadnought_Control", 8_568_353.0),
        ];
        assert_eq!(
            detect(&bundled_rules(), &view(&elite_control)).difficulty,
            Some(Difficulty::Elite)
        );
    }

    #[test]
    fn to_die_with_honor_tier_by_killed_klingon_hull() {
        // Fixed-faction Klingon patrol; anchored on the named mission dreadnoughts
        // (Mrek / Lrell). Tier from Klingon ships that actually die (hull ~ HP);
        // the Mrek dreadnought is skipped for tiers — it never dies, so its "hull"
        // is just damage we happened to deal (player-dependent), not its HP.
        let advanced = vec![
            hull_critter("Space_Klingon_Dreadnought_Mrek", 1_144_742.0),
            hull_critter("Space_Klingon_Dreadnought_Ktinga_Lrell", 15_902.0),
            hull_critter("Space_Klingon_Battlecruiser", 357_756.0),
            hull_critter("Space_Klingon_Raider", 118_931.0),
        ];
        let result = detect(&bundled_rules(), &view(&advanced));
        assert_eq!(result.map.as_deref(), Some("[Patrol] To Die With Honor"));
        assert_eq!(result.difficulty, Some(Difficulty::Advanced));

        let elite = vec![
            hull_critter("Space_Klingon_Dreadnought_Mrek", 5_832_321.0),
            hull_critter("Space_Klingon_Dreadnought_Ktinga_Lrell", 45_107.0),
            hull_critter("Space_Klingon_Battlecruiser", 1_196_890.0),
            hull_critter("Space_Klingon_Raider", 523_080.0),
        ];
        assert_eq!(
            detect(&bundled_rules(), &view(&elite)).difficulty,
            Some(Difficulty::Elite)
        );

        // Any-of: either named ship alone identifies the map — robust to the fight
        // splitting into fragments that carry only one of them (a real 45s-separation
        // split had a later fragment with Lrell but only Mrek's Placate variant).
        let split_fragment = vec![
            hull_critter("Space_Klingon_Dreadnought_Ktinga_Lrell", 41_624.0),
            hull_critter("Space_Klingon_Battlecruiser", 1_196_890.0),
            hull_critter("Space_Klingon_Raider", 495_636.0),
        ];
        let result = detect(&bundled_rules(), &view(&split_fragment));
        assert_eq!(result.map.as_deref(), Some("[Patrol] To Die With Honor"));
        assert_eq!(result.difficulty, Some(Difficulty::Elite));
    }

    #[test]
    fn starbase_one_defense_and_resistance_dont_collide() {
        // The two Starbase One TFOs use different evacuation-ship entities
        // (Dsc / Discovery vs Vtx), so anchoring each on its own ship keeps them
        // apart. Defense is single-difficulty Normal (in-game queue offers only
        // Normal; the wiki's N/A/E is out of date).
        let defense = vec![hull_critter(
            "Space_Federation_Cruiser_Dsc_Tfo_Evacuation_Ship",
            0.0,
        )];
        let result = detect(&bundled_rules(), &view(&defense));
        assert_eq!(result.map.as_deref(), Some("[TFO] Defense of Starbase One"));
        assert_eq!(result.difficulty, Some(Difficulty::Normal));
    }

    #[test]
    fn resistance_of_starbase_one_tier_by_dreadnought_any() {
        // TFO with a randomized enemy group; anchored on the allied evacuation
        // ship (faction-independent). Tier by the dreadnought hull (`hull_any`),
        // listing the Borg faction variants — only Mirror was sampled; Control /
        // plain are extrapolated from the established faction-independent HP.
        let advanced = vec![
            hull_critter("Space_Federation_Cruiser_Vtx_Tfo_Evacuation_Ship", 0.0),
            hull_critter("Space_Borg_Dreadnought_Mirror", 1_909_801.0),
        ];
        let result = detect(&bundled_rules(), &view(&advanced));
        assert_eq!(
            result.map.as_deref(),
            Some("[TFO] Resistance of Starbase One")
        );
        assert_eq!(result.difficulty, Some(Difficulty::Advanced));

        let elite = vec![
            hull_critter("Space_Federation_Cruiser_Vtx_Tfo_Evacuation_Ship", 0.0),
            hull_critter("Space_Borg_Dreadnought_Mirror", 8_804_174.0),
        ];
        assert_eq!(
            detect(&bundled_rules(), &view(&elite)).difficulty,
            Some(Difficulty::Elite)
        );
    }

    #[test]
    fn out_of_control_tier_by_control_borg_hull() {
        // Fixed-faction Borg patrol at the Sitor system: anchored on its Sitor
        // barrage turrets (mission structures, present in both tiers), tiers by
        // the Control Borg battleship/cruiser hull (~3.5-4x Advanced->Elite).
        let advanced = vec![
            hull_critter("Space_Borg_Barrage_Turret_Sitor_Patrol", 0.0),
            hull_critter("Space_Borg_Battleship_Control", 421_569.0),
            hull_critter("Space_Borg_Cruiser_Control", 289_338.0),
        ];
        let result = detect(&bundled_rules(), &view(&advanced));
        assert_eq!(result.map.as_deref(), Some("[Patrol] Out of Control"));
        assert_eq!(result.difficulty, Some(Difficulty::Advanced));

        let elite = vec![
            hull_critter("Space_Borg_Barrage_Turret_Sitor_Patrol", 0.0),
            hull_critter("Space_Borg_Battleship_Control", 1_489_182.0),
            hull_critter("Space_Borg_Cruiser_Control", 1_211_985.0),
        ];
        assert_eq!(
            detect(&bundled_rules(), &view(&elite)).difficulty,
            Some(Difficulty::Elite)
        );
    }

    #[test]
    fn jupiter_elite_by_elite_only_entity() {
        // Jupiter's death counts vary per run; the tier is told by the presence
        // of the "..._Elite_Only" entity (required count 0 = must be present).
        let owned = critters(&[
            ("Msn_Ground_Capt_Mirror_Janeway_Boss_Unkillable", 0),
            (
                "Msn_Assimilated_Fed_Odyssey_Ground_Borg_Ens_Melee_Adapt_Elite_Only",
                25,
            ),
        ]);
        let result = detect(&bundled_rules(), &view(&owned));
        assert_eq!(
            result.map.as_deref(),
            Some("[Patrol] Jupiter Station Showdown")
        );
        assert_eq!(result.difficulty, Some(Difficulty::Elite));
    }

    #[test]
    fn jupiter_without_elite_only_entity_stays_any() {
        let owned = critters(&[
            ("Msn_Ground_Capt_Mirror_Janeway_Boss_Unkillable", 0),
            ("Msn_Assimilated_Fed_Odyssey_Ground_Borg_Ens_Melee", 20),
        ]);
        let result = detect(&bundled_rules(), &view(&owned));
        assert_eq!(
            result.map.as_deref(),
            Some("[Patrol] Jupiter Station Showdown")
        );
        assert_eq!(result.difficulty, Some(Difficulty::Any));
    }

    #[test]
    fn bug_hunt_tier_by_hull_and_elite_marker() {
        // Ground map; tiers scale weakly (~1.6-2x, vs ~4x in space). Two signals:
        // the Elite-only `..._Queenfodder` entity (a presence marker) and the
        // Bluegill boss + commander hull bands (double-gated for the thin margin).
        // Advanced: no marker, Advanced-level hull.
        let advanced = vec![
            hull_critter("Bluegills_Ground_Boss", 286_343.0),
            hull_critter("Bluegills_Ground_Cdr", 7_803.0),
        ];
        let result = detect(&bundled_rules(), &view(&advanced));
        assert_eq!(result.map.as_deref(), Some("[TFO] Bug Hunt"));
        assert_eq!(result.difficulty, Some(Difficulty::Advanced));

        // Elite: Queenfodder marker present + Elite-level hull (both agree).
        let elite = vec![
            hull_critter("Bluegills_Ground_Boss", 451_781.0),
            hull_critter("Bluegills_Ground_Cdr", 16_077.0),
            hull_critter("Bluegills_Ground_Ens_Noautospawn_Queenfodder", 1_095.0),
        ];
        assert_eq!(
            detect(&bundled_rules(), &view(&elite)).difficulty,
            Some(Difficulty::Elite)
        );

        // The Elite marker alone (before the hull thresholds are reached) already
        // resolves Elite.
        let marker_only = vec![
            hull_critter("Bluegills_Ground_Boss", 0.0),
            hull_critter("Bluegills_Ground_Ens_Noautospawn_Queenfodder", 1_095.0),
        ];
        assert_eq!(
            detect(&bundled_rules(), &view(&marker_only)).difficulty,
            Some(Difficulty::Elite)
        );
    }

    #[test]
    fn fixed_difficulty_map_reports_its_tier() {
        // Winter Invasion is pinned to Normal by its boss entity.
        let owned = critters(&[("Snowman_Q_Boss_Msn_Snowglobe", 1)]);
        let result = detect(&bundled_rules(), &view(&owned));
        assert_eq!(result.map.as_deref(), Some("[TFO] Winter Invasion"));
        assert_eq!(result.difficulty, Some(Difficulty::Normal));
    }
}
