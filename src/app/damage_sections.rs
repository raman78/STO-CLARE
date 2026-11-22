//! A player's outgoing damage split into sections: their weapons, their
//! consoles and other abilities, and their pets.
//!
//! The split is a view of the tree the analyzer built, made when a table is
//! built, so moving a row from one section to another is a rebuild of the
//! table rather than a new pass over the log. Each section is a row of its own,
//! worked out from the hits of the rows under it the way `damage_subset` works
//! out a player's row — its DPS, crit rate and the rest are what the analyzer
//! would have said of a group holding exactly those rows.
//!
//! What goes where, for a row directly under the player:
//!
//! 1. A row the reader moved by hand goes where they put it.
//! 2. A row standing for an entity that carried the player's effect — the
//!    log's indirect source — is a pet when that entity is the player's own:
//!    it owns no lines of its own in the fight, and the player never damaged
//!    it. That is the measured test of `docs/SHOT_MODEL.md` §1.3. An entity
//!    that fails it is an enemy ship a proc passed through, or another player
//!    or an allied ship the player's effect rode on, and what it carried is
//!    the player's own ability.
//! 3. Anything else is a weapon when its name says so — the word list below —
//!    and an ability otherwise. A group of rows is a weapon when its own name
//!    says so, or when every ability in it is one.

use std::sync::OnceLock;

use regex::Regex;
use rustc_hash::FxHashSet;

use crate::{
    analyzer::{
        AnalysisGroup, Combat, DamageGroup, DamageSection, GroupPathSegment, Hit, HitsManager,
        MaxOneHit, NameFlags, NameHandle, NameManager, NameMap, NameSet, Player,
        ShieldHullOptionalValues, ShieldHullValues, UNNAMED_CARRIER, normal_weapon_fire,
    },
    app::{damage_subset::subset_group, settings::SectionChoices},
};

/// The names a weapon ends in, matched against the part of the name before
/// any mode it was fired in. Taken as it stands from the combat log coach
/// this split follows, so both file the same rows the same way.
fn weapon_name() -> &'static Regex {
    static WEAPON: OnceLock<Regex> = OnceLock::new();
    WEAPON.get_or_init(|| {
        Regex::new(
            r"(Beam Array|Beam Bank|Cannons?|Turret|Torpedo|Torpedoes|Launcher|Mines?|Array|Omni-Directional.*)$",
        )
        .expect("the weapon pattern is a valid expression")
    })
}

/// Whether an ability's name is a weapon's. The log writes a weapon fired in
/// a mode as `Weapon - Mode`, so only the part before the first " - " counts:
/// `Phaser Beam Array - Fire at Will III` is a beam array.
pub fn is_weapon_name(name: &str) -> bool {
    let family = name.split(" - ").next().unwrap_or(name);
    weapon_name().is_match(family)
}

/// What a player's rows are filed by.
pub struct SectionRules<'a> {
    name_manager: &'a NameManager,
    /// Who owned a line that did something in this fight.
    acted_on_its_own: &'a NameSet,
    choices: &'a SectionChoices,
    /// Everything the player damaged in this fight: the deepest row of every
    /// path in their tree is what was hit.
    damaged: NameSet,
}

impl<'a> SectionRules<'a> {
    /// `whole` is the player's whole outgoing tree, not a filtered part of it:
    /// a pet is told from an enemy by everything the player hit, and leaving
    /// out what a ticked-off row hit would turn the enemy into a pet.
    pub fn new(whole: &DamageGroup, combat: &'a Combat, choices: &'a SectionChoices) -> Self {
        let mut damaged = NameSet::default();
        collect_targets(whole, &mut damaged);
        Self {
            name_manager: &combat.name_manager,
            acted_on_its_own: &combat.acted_on_its_own,
            choices,
            damaged,
        }
    }

    /// The section a row directly under the player goes in.
    pub fn section_of(&self, row: &DamageGroup) -> DamageSection {
        self.choices
            .moved(row.name().get(self.name_manager))
            .unwrap_or_else(|| self.default_section(row))
    }

    /// Where the program files the row when the reader has not said.
    pub fn default_section(&self, row: &DamageGroup) -> DamageSection {
        if self.is_carrier(row) {
            return if self.is_own_pet(row.name()) {
                DamageSection::Pets
            } else {
                DamageSection::Abilities
            };
        }
        if self.is_weapon(row) {
            DamageSection::Weapons
        } else {
            DamageSection::Abilities
        }
    }

    /// Whether the row stands for an entity that carried the effect rather
    /// than for an ability: a group, not the record's own ability, named after
    /// something the log wrote as an indirect source — or the row the analyzer
    /// files a shot under when the fight says something carried it but not
    /// what.
    fn is_carrier(&self, row: &DamageGroup) -> bool {
        let GroupPathSegment::Group(name) = row.segment else {
            return false;
        };
        let info = self.name_manager.info(name);
        info.flags.contains(NameFlags::INDIRECT_SOURCE) || info.name == UNNAMED_CARRIER
    }

    /// The test of `docs/SHOT_MODEL.md` §1.3: the player's pet or device owns
    /// no line of its own that did anything, is no player, and is never
    /// damaged by its owner.
    fn is_own_pet(&self, carrier: NameHandle) -> bool {
        let flags = self.name_manager.info(carrier).flags;
        !flags.contains(NameFlags::PLAYER)
            && !self.acted_on_its_own.contains(&carrier)
            && !self.damaged.contains(&carrier)
    }

    fn is_weapon(&self, row: &DamageGroup) -> bool {
        let name = row.name().get(self.name_manager);
        // The reader's word goes first, wherever the row stands: a weapon
        // they filed under Weapons by hand is a weapon under a mode too, and
        // to Normal weapon fire — in the ability-first tree a weapon under a
        // mode carries only its group's name, which may say nothing.
        if let Some(section) = self.choices.moved(name) {
            return section == DamageSection::Weapons;
        }
        if is_weapon_name(name) {
            return true;
        }
        if row.segment.is_value() {
            return false;
        }
        // A group: the reader's own group, or a mode with the weapons fired in
        // it. The rows that are what was hit have nothing under them, so the
        // abilities are the rows that do.
        let mut abilities = row
            .sub_groups
            .values()
            .filter(|sub| !sub.sub_groups.is_empty())
            .peekable();
        abilities.peek().is_some() && abilities.all(|sub| self.is_weapon(sub))
    }
}

fn collect_targets(group: &DamageGroup, out: &mut NameSet) {
    for sub in group.sub_groups.values() {
        if sub.sub_groups.is_empty() {
            out.insert(sub.name());
        } else {
            collect_targets(sub, out);
        }
    }
}

/// The player's row with the rows under it put into their sections, one level
/// further down. The player's own figures are untouched; each section's are
/// worked out from the rows in it that are ticked, and a section with nothing
/// in it is left out.
///
/// The rows keep their share of the row above them, as every row of the tree
/// does — the row above is now their section, so `Damage %` under Weapons
/// says how much of the weapons' damage a weapon did. A ticked-off row's share
/// is of everything in its section, ticked or not, as it is of everything
/// under the player when the table is not split.
pub fn into_sections(
    mut player: DamageGroup,
    rules: &SectionRules,
    excluded: Option<&FxHashSet<NameHandle>>,
    hits_manager: &HitsManager,
    duration: f64,
) -> DamageGroup {
    let mut rows: [Vec<DamageGroup>; 3] = Default::default();
    for (_, row) in player.sub_groups.drain() {
        let index = DamageSection::ALL
            .iter()
            .position(|section| *section == rules.section_of(&row))
            .expect("every section is in the list");
        rows[index].push(row);
    }

    let is_kept = |row: &DamageGroup| !excluded.is_some_and(|out| out.contains(&row.name()));
    let mut sections = NameMap::default();
    for (section, rows) in DamageSection::ALL.into_iter().zip(rows) {
        if rows.is_empty() {
            continue;
        }
        let group = row_over(
            section.handle(rules.name_manager),
            rows,
            &player,
            is_kept,
            hits_manager,
            duration,
        );
        sections.insert(group.name(), group);
    }
    player.sub_groups = sections;
    player
}

/// A row standing over `rows`, with its figures worked out from the hits of
/// the ones `is_kept` keeps, and their shares made of it.
///
/// The rows' shares are of everything under the new row, kept or not, as they
/// are of the whole player when nothing is ticked off.
fn row_over(
    name: NameHandle,
    rows: Vec<DamageGroup>,
    player: &DamageGroup,
    is_kept: impl Fn(&DamageGroup) -> bool,
    hits_manager: &HitsManager,
    duration: f64,
) -> DamageGroup {
    let hits: Vec<Hit> = rows
        .iter()
        .filter(|row| is_kept(row))
        .flat_map(|row| row.hits.get(hits_manager).iter().copied())
        .collect();
    let mut group = subset_group(hits, duration, &player.damage_metrics.total_damage);
    group.segment = GroupPathSegment::Group(name);
    group.hits_percentage = ShieldHullOptionalValues::percentage(
        &group.damage_metrics.hits.to_values(),
        &player.damage_metrics.hits.to_values(),
    );
    // The biggest hit keeps the name of what landed it, which the pooled hits
    // no longer carry.
    group.max_one_hit = MaxOneHit::default();
    for row in rows.iter().filter(|row| is_kept(row)) {
        group
            .max_one_hit
            .update(row.max_one_hit.name, row.max_one_hit.damage);
        group.damage_types.extend(row.damage_types.iter().copied());
        for (&name, &kills) in row.kills.iter() {
            *group.kills.entry(name).or_default() += kills;
        }
    }

    let whole_damage: ShieldHullValues =
        rows.iter().map(|row| row.damage_metrics.total_damage).sum();
    let whole_hits: ShieldHullValues = rows
        .iter()
        .map(|row| row.damage_metrics.hits.to_values())
        .sum();
    for mut row in rows {
        row.damage_percentage =
            ShieldHullOptionalValues::percentage(&row.damage_metrics.total_damage, &whole_damage);
        row.hits_percentage =
            ShieldHullOptionalValues::percentage(&row.damage_metrics.hits.to_values(), &whole_hits);
        group.sub_groups.insert(row.name(), row);
    }
    group
}

/// The ability-first tree with every weapon shot fired in no mode gathered
/// under one row, `analyzer::NORMAL_WEAPON_FIRE`, beside the rows of the modes — or
/// the tree as it was when the player fired no weapon without a mode.
///
/// A row is a weapon's plain fire when it is directly under the player, is
/// none of the modes the analyzer filed weapons under (`Player::modes`), has
/// no mode in its own name, stands
/// for no carrier — a pet's plain fire stays with the pet, as its modes do —
/// and is a weapon by the rules the sections use. Made before the ticks are
/// applied, so the new row is ticked as one, the way a mode's row is.
pub fn gather_normal_fire(
    mut tree: DamageGroup,
    player: &Player,
    rules: &SectionRules,
    hits_manager: &HitsManager,
    duration: f64,
) -> DamageGroup {
    let plain: Vec<NameHandle> = tree
        .sub_groups
        .values()
        .filter(|row| {
            !player.modes.contains(&row.name())
                // A row a Source Reversal rule turned round keeps its whole
                // name, mode and all, and is no plain fire.
                && !row.name().get(rules.name_manager).contains(" - ")
                && !rules.is_carrier(row)
                && rules.is_weapon(row)
        })
        .map(|row| row.name())
        .collect();
    if plain.is_empty() {
        return tree;
    }
    let rows: Vec<DamageGroup> = plain
        .iter()
        .filter_map(|name| tree.sub_groups.remove(name))
        .collect();
    let row = row_over(
        normal_weapon_fire(rules.name_manager),
        rows,
        &tree,
        |_| true,
        hits_manager,
        duration,
    );
    tree.sub_groups.insert(row.name(), row);
    tree
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        analyzer::{Analyzer, NORMAL_WEAPON_FIRE, settings::AnalysisSettings},
        app::damage_subset::metrics_duration,
    };

    const BOSS: &str = "Borg Cube,C[610 Space_Borg_Dreadnought_Raidisode]";
    const ME: &str = "Kestrel,P[1@2 Kestrel@handle]";
    const PROBE: &str = "EMP Probe II,C[229 Space_Intel_Emp_Probe_Projectile_2]";
    const TORPEDO: &str = "Heavy Plasma Torpedo,C[612 Plasma_Torpedo_Highyield_Borg]";

    /// One fight with a row of every kind the split has to tell apart: the
    /// player's own weapon and console, their hangar pet, an enemy their proc
    /// jumped through, an enemy torpedo it jumped through, an allied ship
    /// their effect rode on, and a device of theirs. The enemy ship and the ally own lines of their
    /// own, as they do in a real log; the torpedo owns none.
    fn fight(dir: &str) -> Combat {
        let dir = std::env::temp_dir().join(dir);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("combatlog.log");
        let lines = [
            format!("{ME},,*,{BOSS},Phaser Beam Array - Fire at Will III,Pn.A,Phaser,,100,100"),
            format!("{ME},,*,{BOSS},Broadside Beam Support,Pn.B,Phaser,,200,200"),
            // A weapon's plain fire, in no mode.
            format!("{ME},,*,{BOSS},Quad Disruptor Cannons,Pn.I,Disruptor,,70,70"),
            format!(
                "{ME},Bird-of-Prey (ALPHA),C[642 Critter_Kvort_Alpha],{BOSS},\
                 Disruptor Beam Array - Fire at Will I,Pn.C,Disruptor,,400,400"
            ),
            format!(
                "{ME},{BOSS},Sphere,C[611 Space_Borg_Sphere],\
                 Refracting Tetryon Cascade,Pn.D,Tetryon,,20,20"
            ),
            format!(
                "{ME},L.S.S. Jaaleet,C[700 Lukari_Ship],{BOSS},\
                 Hellbore Light - Drain,Pn.E,Plasma,,30,30"
            ),
            // An enemy torpedo in flight: it owns no lines, but the player
            // shot it, and a proc of theirs passed through it.
            format!("{ME},,*,{TORPEDO},Phaser Beam Array - Fire at Will III,Pn.A,Phaser,,5,5"),
            format!("{ME},{TORPEDO},{BOSS},Refracting Tetryon Cascade,Pn.D,Tetryon,,150,150"),
            // A device of the player's, which owns a miss of its own, as an EMP
            // probe's explosion does in a real log.
            format!(
                "{ME},{PROBE},{BOSS},Electromagnetic Pulse Probe Explosion,Pn.H,Electrical,,60,60"
            ),
            format!("{PROBE},,*,{BOSS},Electromagnetic Pulse Probe Explosion,Pn.H,,Miss,0,0"),
            format!("{BOSS},,*,{ME},Plasma Torpedo,Pn.F,Plasma,,10,10"),
            format!(
                "L.S.S. Jaaleet,C[700 Lukari_Ship],,*,{BOSS},Piezo-Polaron Turret,Pn.G,Polaron,,50,50"
            ),
        ];
        let text: String = lines
            .iter()
            .enumerate()
            .map(|(second, line)| format!("26:09:09:22:28:{:02}.0::{line}\n", 20 + second))
            .collect();
        std::fs::write(&log, text).unwrap();

        let mut analyzer = Analyzer::new(AnalysisSettings {
            combatlog_file: log.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .unwrap();
        analyzer.update();
        analyzer.result().first().expect("one combat").clone()
    }

    fn kestrel(combat: &Combat) -> &Player {
        combat
            .players
            .values()
            .find(|player| {
                combat
                    .name_manager
                    .name(player.damage_out.name())
                    .starts_with("Kestrel")
            })
            .expect("the player is in the fight")
    }

    /// Each row of a split tree, by section name, as names.
    fn filed(combat: &Combat, split: &DamageGroup) -> Vec<(String, Vec<String>)> {
        let names = &combat.name_manager;
        let mut sections: Vec<(String, Vec<String>)> = split
            .sub_groups
            .values()
            .map(|section| {
                let mut rows: Vec<String> = section
                    .sub_groups
                    .values()
                    .map(|row| names.name(row.name()).to_string())
                    .collect();
                rows.sort();
                (names.name(section.name()).to_string(), rows)
            })
            .collect();
        sections.sort();
        sections
    }

    fn split(combat: &Combat, tree: &DamageGroup, choices: &SectionChoices) -> DamageGroup {
        let player = kestrel(combat);
        let rules = SectionRules::new(&player.damage_out, combat, choices);
        into_sections(
            tree.clone(),
            &rules,
            None,
            &combat.hits_manger,
            metrics_duration(&player.combat_time),
        )
    }

    fn rows(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    /// The pet is the only carrier that passes the ownership test; the enemy
    /// ship, the enemy torpedo and the ally carried the player's own
    /// abilities. The torpedo is told from a pet only by the player having
    /// shot it, and the probe's miss of its own does not make it a ship.
    #[test]
    fn each_row_lands_in_its_section() {
        let combat = fight("cla-sections-each-row");
        let player = kestrel(&combat);
        let split = split(&combat, &player.damage_out, &SectionChoices::default());
        assert_eq!(
            vec![
                (
                    "Consoles and abilities".to_string(),
                    rows(&[
                        "Borg Cube",
                        "Broadside Beam Support",
                        "Heavy Plasma Torpedo",
                        "L.S.S. Jaaleet"
                    ])
                ),
                (
                    "Pets and summons".to_string(),
                    rows(&["Bird-of-Prey (ALPHA)", "EMP Probe II"])
                ),
                (
                    "Weapons".to_string(),
                    rows(&[
                        "Phaser Beam Array - Fire at Will III",
                        "Quad Disruptor Cannons"
                    ])
                ),
            ],
            filed(&combat, &split)
        );
    }

    /// In the ability-first tree the weapon's row is its mode, and it is still
    /// a weapon; the mode a proc was named with is not.
    #[test]
    fn a_mode_row_is_a_weapon_when_its_weapons_are() {
        let combat = fight("cla-sections-mode-row");
        let player = kestrel(&combat);
        let split = split(
            &combat,
            &player.damage_out_by_ability,
            &SectionChoices::default(),
        );
        let weapons = filed(&combat, &split)
            .into_iter()
            .find(|(section, _)| section == "Weapons")
            .expect("a weapons section");
        assert_eq!(
            rows(&["Fire at Will III", "Quad Disruptor Cannons"]),
            weapons.1
        );
    }

    /// A section's figures are those of the rows in it, and the player's are
    /// left as they were.
    #[test]
    fn a_section_adds_up_its_rows_and_leaves_the_player_alone() {
        let combat = fight("cla-sections-sums");
        let player = kestrel(&combat);
        let split = split(&combat, &player.damage_out, &SectionChoices::default());
        assert_eq!(
            player.damage_out.damage_metrics.total_damage.all,
            split.damage_metrics.total_damage.all
        );
        let section = |name: &str| {
            split
                .sub_groups
                .values()
                .find(|section| combat.name_manager.name(section.name()) == name)
                .unwrap()
        };
        let abilities = section("Consoles and abilities");
        assert_eq!(
            400.0, abilities.damage_metrics.total_damage.all,
            "200 + 20 + 30 + 150"
        );
        assert_eq!(4, abilities.damage_metrics.hits.all);
        let whole = split.damage_metrics.total_damage.all;
        assert_eq!(Some(400.0 / whole * 100.0), abilities.damage_percentage.all);
        // A row's share is now of its section.
        let broadside = abilities
            .sub_groups
            .values()
            .find(|row| combat.name_manager.name(row.name()) == "Broadside Beam Support")
            .unwrap();
        assert_eq!(Some(50.0), broadside.damage_percentage.all, "200 of 400");
    }

    /// A ticked-off row stays in its section with its tick, but the section's
    /// figures are of the rows that are left.
    #[test]
    fn a_ticked_off_row_is_out_of_its_sections_figures() {
        let combat = fight("cla-sections-ticks");
        let player = kestrel(&combat);
        let broadside = combat.name_manager.handle("Broadside Beam Support");
        let excluded: FxHashSet<NameHandle> = [broadside].into_iter().collect();
        let choices = SectionChoices::default();
        let rules = SectionRules::new(&player.damage_out, &combat, &choices);
        let split = into_sections(
            player.damage_out.clone(),
            &rules,
            Some(&excluded),
            &combat.hits_manger,
            metrics_duration(&player.combat_time),
        );
        let abilities = split
            .sub_groups
            .get(&DamageSection::Abilities.handle(&combat.name_manager))
            .unwrap();
        assert_eq!(
            200.0, abilities.damage_metrics.total_damage.all,
            "20 + 30 + 150"
        );
        assert_eq!(
            4,
            abilities.sub_groups.len(),
            "the ticked-off row is still there"
        );
    }

    /// A row the reader moved goes where they put it, and back when handed
    /// back.
    #[test]
    fn a_moved_row_goes_where_the_reader_put_it() {
        let combat = fight("cla-sections-moved");
        let player = kestrel(&combat);
        let mut choices = SectionChoices::default();
        choices.set("Broadside Beam Support", Some(DamageSection::Weapons));
        let split = split(&combat, &player.damage_out, &choices);
        let weapons = filed(&combat, &split)
            .into_iter()
            .find(|(section, _)| section == "Weapons")
            .unwrap();
        assert_eq!(
            rows(&[
                "Broadside Beam Support",
                "Phaser Beam Array - Fire at Will III",
                "Quad Disruptor Cannons"
            ]),
            weapons.1
        );
    }

    /// A row moved by hand is read as the reader filed it wherever it stands:
    /// a weapon they took out of Weapons no longer makes the mode it was fired
    /// in a weapon.
    #[test]
    fn a_moved_weapon_counts_under_its_mode_too() {
        let combat = fight("cla-sections-moved-under-mode");
        let player = kestrel(&combat);
        let mut choices = SectionChoices::default();
        choices.set("Phaser Beam Array", Some(DamageSection::Abilities));
        let split = split(&combat, &player.damage_out_by_ability, &choices);
        let filed = filed(&combat, &split);
        let section_of = |row: &str| {
            filed
                .iter()
                .find(|(_, rows)| rows.iter().any(|r| r == row))
                .map(|(section, _)| section.clone())
        };
        assert_eq!(
            Some("Consoles and abilities".to_string()),
            section_of("Fire at Will III")
        );
    }

    /// Normal weapon fire follows the reader's moves: a console they filed
    /// under Weapons is gathered with the weapons, a weapon they took out is
    /// left where it was.
    #[test]
    fn normal_weapon_fire_follows_the_moves() {
        let combat = fight("cla-sections-normal-fire-moved");
        let player = kestrel(&combat);
        let mut choices = SectionChoices::default();
        choices.set("Broadside Beam Support", Some(DamageSection::Weapons));
        choices.set("Quad Disruptor Cannons", Some(DamageSection::Abilities));
        let rules = SectionRules::new(&player.damage_out, &combat, &choices);
        let gathered = gather_normal_fire(
            player.damage_out_by_ability.clone(),
            player,
            &rules,
            &combat.hits_manger,
            metrics_duration(&player.combat_time),
        );
        let names = &combat.name_manager;
        let normal = gathered.sub_groups.get(&normal_weapon_fire(names)).unwrap();
        let under: Vec<&str> = normal
            .sub_groups
            .values()
            .map(|row| names.name(row.name()))
            .collect();
        assert_eq!(vec!["Broadside Beam Support"], under);
        assert!(
            gathered
                .sub_groups
                .values()
                .any(|row| names.name(row.name()) == "Quad Disruptor Cannons"),
            "a weapon taken out of Weapons stays beside the modes"
        );
    }

    /// Every weapon shot in no mode goes under one row beside the modes; the
    /// modes, the console and the carriers stay where they were, and the new
    /// row is a weapon to the sections.
    #[test]
    fn plain_weapon_fire_is_gathered_beside_the_modes() {
        let combat = fight("cla-sections-normal-fire");
        let player = kestrel(&combat);
        let choices = SectionChoices::default();
        let rules = SectionRules::new(&player.damage_out, &combat, &choices);
        let duration = metrics_duration(&player.combat_time);
        let gathered = gather_normal_fire(
            player.damage_out_by_ability.clone(),
            player,
            &rules,
            &combat.hits_manger,
            duration,
        );
        let names = &combat.name_manager;
        let mut top: Vec<&str> = gathered
            .sub_groups
            .values()
            .map(|row| names.name(row.name()))
            .collect();
        top.sort();
        assert_eq!(
            vec![
                "Bird-of-Prey (ALPHA)",
                "Borg Cube",
                "Broadside Beam Support",
                "EMP Probe II",
                "Fire at Will III",
                "Heavy Plasma Torpedo",
                "L.S.S. Jaaleet",
                NORMAL_WEAPON_FIRE,
            ],
            top
        );
        let normal = gathered.sub_groups.get(&normal_weapon_fire(names)).unwrap();
        assert_eq!(70.0, normal.damage_metrics.total_damage.all);
        assert_eq!(1, normal.sub_groups.len());
        assert_eq!(
            player.damage_out_by_ability.damage_metrics.total_damage.all,
            gathered.damage_metrics.total_damage.all,
            "the player's own figures are untouched"
        );

        let split = into_sections(gathered, &rules, None, &combat.hits_manger, duration);
        let weapons = filed(&combat, &split)
            .into_iter()
            .find(|(section, _)| section == "Weapons")
            .unwrap();
        assert_eq!(rows(&["Fire at Will III", NORMAL_WEAPON_FIRE]), weapons.1);
    }

    /// The word list is matched on the weapon's own name, not on the mode it
    /// was fired in, and only at the end of it.
    #[test]
    fn the_word_list_reads_the_name_before_the_mode() {
        assert!(is_weapon_name("Phaser Beam Array - Fire at Will III"));
        assert!(is_weapon_name("Quantum Phase Torpedo Launcher"));
        assert!(is_weapon_name("Dual Heavy Cannons - Rapid Fire II"));
        assert!(is_weapon_name("Omni-Directional Antiproton Beam Array"));
        assert!(!is_weapon_name("Hellbore Light - Drain"));
        assert!(
            !is_weapon_name("Beam Array Overload - X"),
            "the mode is not the name"
        );
        assert!(!is_weapon_name("Gravity Well III"));
    }
}
