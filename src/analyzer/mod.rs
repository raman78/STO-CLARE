use std::{
    borrow::Cow,
    fmt::Debug,
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    ops::Range,
    path::Path,
};

use chrono::{Duration, NaiveDateTime};
use educe::Educe;
use itertools::Itertools;
use log::warn;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

mod common;
mod damage;
mod detection;
mod groups;
mod heal;
mod name_manager;
mod parser;
pub mod settings;
mod values_manager;
pub use common::*;
pub use damage::*;
use detection::{CritterMeta, DETECTION_RULES};
pub use detection::{Difficulty, curated_map_identifiers, curated_map_names};
use groups::*;
pub use groups::{AnalysisGroup, DamageGroup, HealGroup};
pub use heal::*;
pub use name_manager::*;
pub use values_manager::*;

use self::{parser::*, settings::*};

pub struct Analyzer {
    parser: Parser,
    combat_separation_time: Duration,
    settings: AnalysisSettings,
    combats: Vec<Combat>,
}

type Players = NameMap<Player>;
type GroupingPath = SmallVec<[GroupPathSegment; 8]>;

#[derive(Clone, Debug)]
pub struct Combat {
    pub combat_names: FxHashMap<String, CombatName>,
    pub combat_time: Option<Range<NaiveDateTime>>,
    pub active_time: Range<NaiveDateTime>,
    pub total_damage_out: ShieldHullValues,
    pub total_damage_in: ShieldHullValues,
    /// Healing the players received *from someone else*.
    pub total_heal_received: ShieldHullValues,
    /// Healing the players did to *someone else* — team mates, allied
    /// NPCs and their own pets.
    pub total_heal_ally: ShieldHullValues,
    /// Healing the players did to themselves (including their own gear procs).
    pub total_heal_self: ShieldHullValues,
    pub players: Players,
    pub log_pos: Option<Range<u64>>,
    pub total_deaths: u32,
    pub total_kills: u32,
    /// Per-NPC facts (keyed by internal unique name) used to detect the map and
    /// difficulty. Accumulated as records are processed.
    critters: FxHashMap<NameHandle, CritterMeta>,
    /// Detected map name (`None` when unknown, shown as "Combat").
    pub detected_map: Option<String>,
    /// Detected difficulty (`None` when the map is unknown).
    pub detected_difficulty: Option<Difficulty>,
    /// The detected map's environment ("Space" / "Ground" / "Shuttle"), curated
    /// metadata rather than anything the log states. Appended to the combat's
    /// name in parentheses.
    pub detected_combat_type: Option<String>,
    pub name_manager: NameManager,
    pub hits_manger: HitsManager,
    pub heal_ticks_manger: HealTicksManager,
}

#[derive(Clone, Debug)]
pub struct CombatName {
    pub name: String,
    pub additional_infos: Vec<String>,
}

/// A player's four analysis trees. Healing is split into three *disjoint* pools,
/// so no tick is counted twice:
///
/// | pool | records it holds |
/// |---|---|
/// | `heal_ally` | this player healed *somebody else* — team mate, allied NPC or own pet (directly, or through a console) |
/// | `heal_received` | *somebody else* healed this player |
/// | `heal_self` | this player healed themselves — self-buffs, own trait and gear procs, own consoles targeting them |
///
/// Each pool is a [`HealPool`], which holds the same ticks nested both
/// person → ability and ability → person, so the tabs can switch order without
/// a re-parse.
///
/// The split matters because self healing dominates in practice: on a real
/// 212 MB log, 54.55 M of a player's 55.68 M total healing was self healing, and
/// it used to be counted in *both* the outgoing and the incoming pool.
#[derive(Clone, Debug)]
pub struct Player {
    pub combat_time: Option<Range<NaiveDateTime>>,
    pub active_time: Option<Range<NaiveDateTime>>,
    pub damage_out: DamageGroup,
    pub damage_in: DamageGroup,
    pub heal_ally: HealPool,
    pub heal_received: HealPool,
    pub heal_self: HealPool,
}

impl Analyzer {
    pub fn new(settings: AnalysisSettings) -> Option<Self> {
        Some(Self {
            parser: Parser::new(settings.combatlog_file())?,
            combat_separation_time: Duration::seconds(settings.combat_separation_time_seconds as _),
            settings,
            combats: Default::default(),
        })
    }

    pub fn update(&mut self) {
        let mut first_modified_combat = None;
        loop {
            match self.process_next_record(&mut first_modified_combat) {
                Ok(_) => (),
                Err(RecordError::EndReached) => break,
                Err(RecordError::InvalidRecord(invalid_record)) => {
                    warn!("failed to parse record: {}", invalid_record);
                }
            }
        }

        if let Some(first_modified_combat) = first_modified_combat {
            self.combats[first_modified_combat..]
                .iter_mut()
                .for_each(|p| p.update(&self.settings));
        }

        self.discard_combats_without_damage();
    }

    /// Drop "combats" in which no player dealt any damage at all.
    ///
    /// Standing around on a social map still writes records — fall damage
    /// (`Autodesc.Combatevent.Falling`) and self-buffs — and each burst of those
    /// becomes its own combat, cluttering the list with entries that hold
    /// nothing. Real fights always have damage out from someone.
    ///
    /// `total_damage_out` is the sum over *all* players, so a run where the user
    /// only healed is still kept as long as anyone in the group dealt damage.
    ///
    /// The newest combat is **not** exempt. Exempting it (on the grounds that it
    /// might be a fight still in progress) meant that whenever the log ended on
    /// such a burst — the game closed, or the player idling on a social map — the
    /// junk entry stayed at the top of the list for good.
    ///
    /// Nothing in the log says whether a fight is still running, so "in progress"
    /// cannot be detected; but it does not need to be. A live fight in which
    /// anyone has already dealt damage is kept by the rule itself. The only case
    /// dropped mid-fight is one where a refresh lands between a player's opening
    /// buffs and their first shot: the buffs are discarded and the combat then
    /// starts at that first shot instead (measured — the fight is *not* split in
    /// two). Damage figures are unaffected, since `combat_time` only ever starts
    /// at the first damage record anyway.
    ///
    /// Considered and rejected: hiding the in-progress combat from the list
    /// entirely, leaving live numbers to the overlay. It needs a wall clock to
    /// tell "in progress" from "last one before the game closed" — without one
    /// the final combat of a session would never appear — which means enabling
    /// chrono's `clock` feature, adding a flag to `AnalysisInfo::Refreshed`, and
    /// splitting what the overlay shows from what the main window shows. Not
    /// worth it while the damage filter alone keeps the list clean.
    fn discard_combats_without_damage(&mut self) {
        self.combats
            .retain(|combat| combat.total_damage_out.all > 0.0);
    }

    fn process_next_record(
        &mut self,
        first_modified_combat: &mut Option<usize>,
    ) -> Result<(), RecordError<'_>> {
        let record = self.parser.parse_next()?;

        match self.combats.last_mut() {
            Some(combat)
                if record.time.signed_duration_since(combat.active_time.end)
                    > self.combat_separation_time =>
            {
                self.combats.push(Combat::new(&record));
            }
            None => {
                self.combats.push(Combat::new(&record));
            }
            _ => (),
        }
        first_modified_combat.get_or_insert(self.combats.len() - 1);
        let combat = self.combats.last_mut().unwrap();

        combat.update_meta_data(&record);
        combat.update_names(&record);
        combat.update_critters(&record);

        let combat_start_offset_millis = record
            .time
            .signed_duration_since(combat.active_time.start)
            .num_milliseconds() as u32;

        if let Entity::Player { full_name, .. } = &record.source {
            let player =
                Combat::get_player(&mut combat.players, combat.name_manager.handle(full_name));
            player.add_out_value(
                &record,
                combat_start_offset_millis,
                &self.settings,
                &mut combat.name_manager,
            );
        }

        if let Entity::Player { full_name, .. } = &record.target {
            let player =
                Combat::get_player(&mut combat.players, combat.name_manager.handle(full_name));
            player.add_in_value(
                &record,
                combat_start_offset_millis,
                &self.settings,
                &mut combat.name_manager,
            );
        }

        if let (Entity::Player { full_name, .. }, Entity::NonPlayer { .. }) =
            (&record.indirect_source, &record.source)
        {
            let player =
                Combat::get_player(&mut combat.players, combat.name_manager.handle(full_name));
            player.add_in_value(
                &record,
                combat_start_offset_millis,
                &self.settings,
                &mut combat.name_manager,
            );
        }

        if let (Entity::Player { full_name, .. }, Entity::None, Entity::None) =
            (&record.source, &record.indirect_source, &record.target)
        {
            let player =
                Combat::get_player(&mut combat.players, combat.name_manager.handle(full_name));
            player.add_in_value(
                &record,
                combat_start_offset_millis,
                &self.settings,
                &mut combat.name_manager,
            );
        }

        Ok(())
    }

    pub fn result(&self) -> &Vec<Combat> {
        &self.combats
    }

    pub fn settings(&self) -> &AnalysisSettings {
        &self.settings
    }
}

impl Combat {
    fn new(start_record: &Record) -> Self {
        let time = start_record.time..start_record.time;
        Self {
            combat_time: if start_record.is_player_out_damage() {
                Some(time.clone())
            } else {
                None
            },
            active_time: time,
            combat_names: Default::default(),
            players: Default::default(),
            log_pos: start_record.log_pos.clone(),
            total_damage_out: Default::default(),
            total_damage_in: Default::default(),
            total_heal_received: Default::default(),
            total_heal_ally: Default::default(),
            total_heal_self: Default::default(),
            total_kills: 0,
            total_deaths: 0,
            critters: Default::default(),
            detected_map: None,
            detected_difficulty: None,
            detected_combat_type: None,
            name_manager: Default::default(),
            hits_manger: Default::default(),
            heal_ticks_manger: Default::default(),
        }
    }

    fn get_player(players: &mut NameMap<Player>, name: NameHandle) -> &mut Player {
        if !players.contains_key(&name) {
            let player = Player::new(name);
            players.insert(player.damage_out.name(), player);
        }
        players.get_mut(&name).unwrap()
    }

    pub fn identifier(&self) -> String {
        let date_times = format!(
            "{} {} - {}",
            self.active_time.start.date(),
            self.active_time.start.time().format("%T"),
            self.active_time.end.time().format("%T")
        );
        let name = self.name();
        format!("{} | {}", name, date_times)
    }

    /// The combat's name without the environment and difficulty that
    /// [`Self::name`] appends: the user's Combat Name Rules if any match (they
    /// take priority), else the auto-detected map, else "Combat".
    ///
    /// This is what identifies "the same kind of fight" — the compare view
    /// groups by it. Reading it back out of a formatted name by string surgery
    /// used to be the only option and broke on any rule whose own name contains
    /// a bracket.
    pub fn base_name(&self) -> String {
        if !self.combat_names.is_empty() {
            self.combat_names.values().map(|n| n.format()).join(", ")
        } else {
            self.detected_map
                .clone()
                .unwrap_or_else(|| "Combat".to_string())
        }
    }

    pub fn name(&self) -> String {
        // The detected difficulty is always appended on top of the base name —
        // it is computed from the log's entities, independent of naming, so the
        // tier shows even when a rule provides the name.
        let base =
            append_detected_combat_type(self.base_name(), self.detected_combat_type.as_deref());
        append_detected_difficulty(base, self.detected_difficulty)
    }

    pub fn file_identifier(&self) -> String {
        let date_times = format!(
            "{} {} - {}",
            self.active_time.start.date(),
            self.active_time.start.time().format("%H-%M-%S"),
            self.active_time.end.time().format("%H-%M-%S")
        );
        let name = self.name();
        format!("{} {}", name, date_times)
    }

    fn update(&mut self, settings: &AnalysisSettings) {
        self.update_combat_names(settings);

        self.hits_manger.clear();
        self.heal_ticks_manger.clear();
        self.players.values_mut().for_each(|p| {
            p.recalculate_metrics(&mut self.hits_manger, &mut self.heal_ticks_manger)
        });

        let players = self.players.values();

        self.total_damage_out = players.clone().map(|p| p.damage_out.total_damage).sum();
        self.total_damage_in = players.clone().map(|p| p.damage_in.total_damage).sum();
        self.total_heal_ally = players.clone().map(|p| p.heal_ally.total_heal).sum();
        self.total_heal_received = players.clone().map(|p| p.heal_received.total_heal).sum();
        self.total_heal_self = players.clone().map(|p| p.heal_self.total_heal).sum();
        self.total_kills = players
            .clone()
            .map(|p| p.damage_out.kills.values().copied().sum::<u32>())
            .sum();
        self.total_deaths = players
            .clone()
            .map(|p| p.damage_in.kills.values().copied().sum::<u32>())
            .sum();
        let total_hits_out: ShieldHullCounts = players
            .clone()
            .map(|p| p.damage_out.damage_metrics.hits)
            .sum();
        let total_hits_in: ShieldHullCounts = players
            .clone()
            .map(|p| p.damage_in.damage_metrics.hits)
            .sum();
        let total_heal_ticks_done = players
            .clone()
            .map(|p| p.heal_ally.heal_metrics.ticks)
            .sum();
        let total_heal_ticks_received = players
            .clone()
            .map(|p| p.heal_received.heal_metrics.ticks)
            .sum();
        let total_heal_ticks_self = players
            .clone()
            .map(|p| p.heal_self.heal_metrics.ticks)
            .sum();
        self.recalculate_damage_group_percentage(self.total_damage_out, total_hits_out, |p| {
            &mut p.damage_out
        });
        self.recalculate_damage_group_percentage(self.total_damage_in, total_hits_in, |p| {
            &mut p.damage_in
        });
        self.recalculate_heal_group_percentage(self.total_heal_ally, total_heal_ticks_done, |p| {
            &mut p.heal_ally
        });
        self.recalculate_heal_group_percentage(
            self.total_heal_received,
            total_heal_ticks_received,
            |p| &mut p.heal_received,
        );
        self.recalculate_heal_group_percentage(self.total_heal_self, total_heal_ticks_self, |p| {
            &mut p.heal_self
        });

        self.update_detection();
    }

    /// Resolve the map and difficulty from the accumulated critters.
    fn update_detection(&mut self) {
        let detected = {
            let by_name: FxHashMap<&str, &CritterMeta> = self
                .critters
                .iter()
                .map(|(handle, meta)| (handle.get(&self.name_manager), meta))
                .collect();
            detection::detect(&DETECTION_RULES, &by_name)
        };
        self.detected_map = detected.map;
        self.detected_difficulty = detected.difficulty;
        self.detected_combat_type = detected.combat_type;
    }

    fn recalculate_damage_group_percentage(
        &mut self,
        total_damage: ShieldHullValues,
        total_hits: ShieldHullCounts,
        mut group: impl FnMut(&mut Player) -> &mut DamageGroup,
    ) {
        self.players
            .values_mut()
            .for_each(|p| group(p).recalculate_percentages(&total_damage, &total_hits));
    }

    fn recalculate_heal_group_percentage(
        &mut self,
        total_heal: ShieldHullValues,
        parent_ticks: ShieldHullCounts,
        mut pool: impl FnMut(&mut Player) -> &mut HealPool,
    ) {
        self.players
            .values_mut()
            .for_each(|p| pool(p).recalculate_percentages(&total_heal, &parent_ticks));
    }

    fn update_meta_data(&mut self, record: &Record) {
        self.update_time(record);
        self.update_log_pos(record);
    }

    /// Accumulate per-NPC facts used for map/difficulty detection.
    ///
    /// *Presence* is recorded for any non-player entity in either role, because
    /// the anchors several maps key on are non-combatant allies/objectives: one
    /// may fire a single shot and never be hit (only ever a source), or only
    /// ever be healed. Keying presence off damage taken alone made those runs
    /// undetectable. *Hull damage and deaths* still come only from damage dealt
    /// to the entity — an entity present with no hull record simply has a median
    /// of 0 and matches no tier threshold.
    ///
    /// Must run after `update_names`, which interns the unique name.
    fn update_critters(&mut self, record: &Record) {
        for entity in [&record.source, &record.target] {
            if let Entity::NonPlayer { unique_name, .. } = entity {
                let handle = self.name_manager.handle(unique_name);
                self.critters.entry(handle).or_default();
            }
        }

        let RecordValue::Damage(hit) = &record.value else {
            return;
        };
        let Entity::NonPlayer {
            unique_name, _id, ..
        } = &record.target
        else {
            return;
        };
        let handle = self.name_manager.handle(unique_name);
        let meta = self.critters.entry(handle).or_default();
        if record.value_flags.contains(ValueFlags::KILL) {
            meta.deaths += 1;
        }
        if let SpecificHit::Hull { .. } = hit.specific {
            *meta.hull_damage_per_instance.entry(*_id).or_default() += hit.damage;
        }
    }

    fn update_names(&mut self, record: &Record) {
        self.name_manager.insert_some(
            record.source.name(),
            NameFlags::SOURCE.set_if(NameFlags::PLAYER, record.source.is_player()),
        );
        self.name_manager.insert_some(
            record.source.unique_name(),
            NameFlags::SOURCE_UNIQUE.set_if(NameFlags::PLAYER, record.source.is_player()),
        );
        self.name_manager.insert_some(
            record.target.name(),
            NameFlags::TARGET.set_if(NameFlags::PLAYER, record.target.is_player()),
        );
        self.name_manager.insert_some(
            record.target.unique_name(),
            NameFlags::TARGET_UNIQUE.set_if(NameFlags::PLAYER, record.target.is_player()),
        );
        self.name_manager.insert_some(
            record.indirect_source.name(),
            NameFlags::INDIRECT_SOURCE
                .set_if(NameFlags::PLAYER, record.indirect_source.is_player()),
        );
        self.name_manager.insert_some(
            record.indirect_source.unique_name(),
            NameFlags::INDIRECT_SOURCE_UNIQUE
                .set_if(NameFlags::PLAYER, record.indirect_source.is_player()),
        );
        self.name_manager
            .insert(&record.value_name, NameFlags::VALUE);
        self.name_manager
            .insert(&record.value_type, NameFlags::NONE);
    }

    fn update_combat_names(&mut self, settings: &AnalysisSettings) {
        self.combat_names.clear();

        settings
            .combat_name_rules
            .iter()
            .filter(|r| self.name_manager.matches(&r.name_rule))
            .for_each(|r| {
                self.combat_names.insert(
                    r.name_rule.name.clone(),
                    CombatName::new(r, &self.name_manager),
                );
            });
    }

    fn update_time(&mut self, record: &Record) {
        if record.is_player_out_damage() && !record.is_immune_or_zero() {
            let combat_time = self.combat_time.get_or_insert(record.time..record.time);
            combat_time.end = record.time;
        }
        self.active_time.end = record.time;
    }

    fn update_log_pos(&mut self, record: &Record) {
        if let (Some(log_pos), Some(record_log_pos)) =
            (self.log_pos.as_mut(), record.log_pos.as_ref())
        {
            log_pos.end = record_log_pos.end;
        }
    }
}

impl Player {
    fn new(full_name: NameHandle) -> Self {
        Self {
            combat_time: None,
            active_time: None,
            damage_out: DamageGroup::new_branch(GroupPathSegment::Group(full_name)),
            damage_in: DamageGroup::new_branch(GroupPathSegment::Group(full_name)),
            heal_ally: HealPool::new(full_name),
            heal_received: HealPool::new(full_name),
            heal_self: HealPool::new(full_name),
        }
    }

    /// This player's interned name.
    pub fn name(&self) -> NameHandle {
        self.damage_out.name()
    }

    /// Whether a heal *this player is the source of* lands back on themselves.
    ///
    /// Two shapes qualify, both verified against a real log:
    /// - `src=me, ind=-, tgt=-` — a self-directed buff or trait proc
    ///   (`Reflexive Emitters`, `Brace for Impact III`).
    /// - `src=me, ind=my console, tgt=me` — own gear healing its owner
    ///   (`Bio-Molecular Shield Generator Fabrication`).
    ///
    /// A heal routed through a pet with no target (`src=me, ind=my pet, tgt=-`)
    /// is deliberately *not* self healing: it lands on the pet, which
    /// `add_out_value` already credits as the recipient.
    fn heal_lands_on_self(&self, record: &Record, name_manager: &mut NameManager) -> bool {
        match &record.target {
            Entity::Player { full_name, .. } => name_manager.handle(full_name) == self.name(),
            Entity::None => record.indirect_source.is_none(),
            _ => false,
        }
    }

    fn add_out_value(
        &mut self,
        record: &Record,
        combat_start_offset_millis: u32,
        settings: &AnalysisSettings,
        name_manager: &mut NameManager,
    ) {
        if settings
            .damage_out_exclusion_rules
            .iter()
            .any(|r| r.matches_record(record))
        {
            return;
        }
        self.update_active_time(record);
        let mut path = Self::build_grouping_path(record, settings, name_manager);
        let target_name = if record.is_self_directed() {
            record.source.name()
        } else {
            record
                .target
                .name()
                .or_else(|| record.indirect_source.name())
        };
        let target_name = target_name
            .map(|n| name_manager.handle(n))
            .unwrap_or_default();
        match record.value {
            RecordValue::Damage(damage) if !record.is_direct_self_damage() => {
                path.insert(0, GroupPathSegment::Group(target_name));
                self.damage_out.add_damage(
                    &path,
                    damage,
                    record.value_flags,
                    name_manager.handle(&record.value_type),
                    combat_start_offset_millis,
                    name_manager,
                );

                self.update_combat_time(record);
            }
            RecordValue::Heal(heal) => {
                // A self heal is grouped under this player's own name; healing
                // somebody else is grouped under the recipient.
                let lands_on_self = self.heal_lands_on_self(record, name_manager);
                let pool = if lands_on_self {
                    &mut self.heal_self
                } else {
                    &mut self.heal_ally
                };
                Self::add_heal_to_pool(
                    pool,
                    path,
                    (!lands_on_self).then_some(target_name),
                    heal,
                    combat_start_offset_millis,
                );
            }
            _ => (),
        }
    }

    fn add_in_value(
        &mut self,
        record: &Record,
        combat_start_offset_millis: u32,
        settings: &AnalysisSettings,
        name_manager: &mut NameManager,
    ) {
        let source_name = record
            .source
            .name()
            .map(|n| name_manager.handle(n))
            .unwrap_or_default();
        let source_is_self = match &record.source {
            Entity::Player { full_name, .. } => name_manager.handle(full_name) == self.name(),
            _ => false,
        };
        let mut path = Self::build_grouping_path(record, settings, name_manager);
        path.push(GroupPathSegment::Group(source_name));
        match record.value {
            RecordValue::Damage(damage) => {
                self.damage_in.add_damage(
                    &path,
                    damage,
                    record.value_flags,
                    name_manager.handle(&record.value_type),
                    combat_start_offset_millis,
                    name_manager,
                );
                self.update_active_time(record);
            }
            RecordValue::Heal(heal) => {
                // Healing this player is the source of is already tracked by
                // `add_out_value`, as either Healing Ally or Self Healing.
                // Counting it here too is what used to make every self heal show
                // up in both the outgoing and the incoming pool.
                if source_is_self {
                    return;
                }
                // `path` already carries the healer as its last segment (pushed
                // for the damage case); the pool helper wants it without.
                path.pop();
                Self::add_heal_to_pool(
                    &mut self.heal_received,
                    path,
                    Some(source_name),
                    heal,
                    combat_start_offset_millis,
                );
            }
        }
    }

    /// Record one heal tick in both of a pool's grouping orders.
    ///
    /// `path` is the ability part of the path as built by
    /// [`Self::build_grouping_path`]; `person` is the other party (the recipient
    /// for outgoing healing, the healer for incoming). `add_heal` reads a path
    /// back to front — the last segment becomes the top level and the first
    /// becomes the leaf — so appending `person` nests person → ability, and
    /// putting it first nests ability → person, the way the damage tabs do it.
    fn add_heal_to_pool(
        pool: &mut HealPool,
        path: GroupingPath,
        person: Option<NameHandle>,
        heal: BaseHealTick,
        combat_start_offset_millis: u32,
    ) {
        let mut by_person = path;
        // Self healing leaves the person out: it is always the player whose
        // pool this is, so a level naming them again carries nothing.
        if let Some(person) = person {
            by_person.push(GroupPathSegment::Group(person));
        }
        pool.by_person
            .add_heal(&by_person, heal, combat_start_offset_millis);

        // The other order is the same path read the other way round. Moving the
        // person to the front instead would leave whatever `build_grouping_path`
        // put last on top — the pet or console for a heal routed through one,
        // rather than the ability the order is named after.
        let mut by_ability = by_person;
        by_ability.reverse();
        pool.by_ability
            .add_heal(&by_ability, heal, combat_start_offset_millis);
    }

    fn build_grouping_path(
        record: &Record,
        settings: &AnalysisSettings,
        name_manager: &mut NameManager,
    ) -> GroupingPath {
        let mut path = GroupingPath::new();

        match (&record.indirect_source, &record.target) {
            (Entity::None, _) | (_, Entity::None) => {
                path.push(GroupPathSegment::Value(
                    name_manager.handle(&record.value_name),
                ));
            }

            (
                Entity::NonPlayer { name, .. }
                | Entity::Player {
                    full_name: name, ..
                }
                | Entity::NonPlayerCharacter { name, .. },
                _,
            ) => {
                if settings
                    .indirect_source_grouping_revers_rules
                    .iter()
                    .any(|r| r.matches_record(record))
                {
                    path.extend_from_slice(&[
                        GroupPathSegment::Value(name_manager.handle(name)),
                        GroupPathSegment::Group(name_manager.handle(&record.value_name)),
                    ]);
                } else {
                    path.extend_from_slice(&[
                        GroupPathSegment::Value(name_manager.handle(&record.value_name)),
                        GroupPathSegment::Group(name_manager.handle(name)),
                    ]);
                }
            }
        }

        if let Some(rule) = settings
            .custom_group_rules
            .iter()
            .find(|r| r.matches_record(record))
        {
            path.push(GroupPathSegment::Group(
                name_manager.insert(rule.name.as_str(), NameFlags::NONE),
            ));
        }

        path
    }

    fn update_combat_time(&mut self, record: &Record) {
        if record.is_immune_or_zero() {
            return;
        }
        let combat_time = self.combat_time.get_or_insert(record.time..record.time);
        combat_time.end = record.time;
    }

    fn update_active_time(&mut self, record: &Record) {
        let active_time = self.active_time.get_or_insert(record.time..record.time);
        active_time.end = record.time;
    }

    fn recalculate_metrics(
        &mut self,
        hits_manager: &mut HitsManager,
        heal_ticks_manager: &mut HealTicksManager,
    ) {
        let combat_duration = Self::metrics_duration(&self.combat_time);
        let active_duration = Self::metrics_duration(&self.active_time);
        self.damage_out
            .recalculate_metrics(combat_duration, hits_manager, &mut |_, _| {});
        self.damage_in
            .recalculate_metrics(active_duration, hits_manager, &mut |_, _| {});
        self.heal_ally
            .recalculate_metrics(active_duration, heal_ticks_manager);
        self.heal_received
            .recalculate_metrics(active_duration, heal_ticks_manager);
        self.heal_self
            .recalculate_metrics(active_duration, heal_ticks_manager);
    }

    fn metrics_duration(time: &Option<Range<NaiveDateTime>>) -> f64 {
        let duration = time
            .as_ref()
            .map(|t| t.end.signed_duration_since(t.start))
            .unwrap_or(Duration::MAX);

        duration.to_std().unwrap().as_secs_f64()
    }
}

impl Combat {
    pub fn read_log_combat_data(&self, file_path: &Path) -> Option<Vec<u8>> {
        let pos = self.log_pos.clone()?;

        let file = match File::options().create(false).read(true).open(file_path) {
            Ok(f) => f,
            Err(_) => return None,
        };

        let mut combat_data = vec![0; (pos.end - pos.start) as _];
        let mut reader = BufReader::with_capacity(1 << 20, file);
        reader.seek(SeekFrom::Start(pos.start)).ok()?;

        reader.read_exact(&mut combat_data).ok()?;

        Some(combat_data)
    }
}

/// Append the auto-detected difficulty in square brackets (e.g. `[Elite]`),
/// unless the base name already mentions that tier (so a rule that already says
/// "(Elite)" is not doubled). `Any` / no difficulty leaves the name unchanged.
fn append_detected_difficulty(base: String, difficulty: Option<Difficulty>) -> String {
    match difficulty.and_then(|d| d.label()) {
        Some(label) if !base.to_lowercase().contains(&label.to_lowercase()) => {
            format!("{base} [{label}]")
        }
        _ => base,
    }
}

/// Append the map's environment in parentheses (e.g. `(Ground)`), unless the
/// base name already mentions it — a user rule named "Bug Hunt Ground" should
/// not become "Bug Hunt Ground (Ground)". Comes from the curated rules, since
/// the log itself never states whether a fight was in space or on the ground.
fn append_detected_combat_type(base: String, combat_type: Option<&str>) -> String {
    match combat_type.map(str::trim).filter(|t| !t.is_empty()) {
        Some(label) if !base.to_lowercase().contains(&label.to_lowercase()) => {
            format!("{base} ({label})")
        }
        _ => base,
    }
}

impl CombatName {
    fn new(rule: &CombatNameRule, name_manager: &NameManager) -> Self {
        let additional_infos: Vec<_> = rule
            .additional_info_rules
            .iter()
            .filter(|r| name_manager.matches(r))
            .map(|r| r.name.clone())
            .collect();
        Self {
            name: rule.name_rule.name.clone(),
            additional_infos,
        }
    }

    fn format(&self) -> Cow<'_, str> {
        if self.additional_infos.is_empty() {
            return Cow::Borrowed(&self.name);
        }

        let name = format!(
            "{} ({})",
            self.name,
            self.additional_infos.iter().join(", ")
        );
        Cow::Owned(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A non-combatant ally that only ever *fires* — never takes damage — must
    /// still register as present, otherwise it cannot anchor a map. Real case:
    /// The Ninth Rule run of 2026-07-28 22:18, where the allied Galaxy cruiser
    /// fires a single shot and is never hit, leaving the combat unidentified.
    #[test]
    fn source_only_ally_is_present_for_detection() {
        let dir = std::env::temp_dir().join("cla-source-only-critter-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("combatlog.log");

        // The ally appears solely as a source; the enemy battleship takes hull
        // damage in The Ninth Rule's Advanced band.
        let records = concat!(
            "26:07:28:22:20:17.5::U.S.S. Birmingham,C[11636 Space_Federation_Cruiser_Galaxy],,*,",
            "Talon Battleship,C[11759 Space_Nausicaan_Battleship],Phaser Array,Pn.Cedjls,Phaser,,100.0,100.0\n",
            "26:07:28:22:20:18.5::Kestrel,P[1@2 Kestrel@handle],,*,",
            "Talon Battleship,C[11759 Space_Nausicaan_Battleship],Phaser Array,Pn.Cedjls,Phaser,Kill,345674.0,345674.0\n",
        );
        std::fs::write(&log, records).unwrap();

        let mut analyzer = Analyzer::new(AnalysisSettings {
            combatlog_file: log.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .unwrap();
        analyzer.update();

        let combat = analyzer.result().first().expect("one combat");
        assert_eq!(
            combat.detected_map.as_deref(),
            Some("[Patrol] The Ninth Rule"),
            "the source-only ally must anchor the map"
        );
        assert_eq!(combat.detected_difficulty, Some(Difficulty::Advanced));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The three healing pools must be disjoint. Every record shape below was
    /// taken from a real log; the amounts are distinct so a tick landing in the
    /// wrong pool (or in two of them) shows up as a wrong total.
    ///
    /// Before the split, a self heal was counted in *both* the outgoing and the
    /// incoming pool, which on a real log meant 54.55 M of double-counted
    /// healing swamping the 0.78 M actually received from teammates.
    #[test]
    fn healing_is_split_into_three_disjoint_pools() {
        let dir = std::env::temp_dir().join("cla-heal-split-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("combatlog.log");

        let me = "Kestrel,P[1@2 Kestrel@handle]";
        let mate = "Sirak,P[3@4 Sirak@other]";
        let console = "Bio-Molecular Shield Generator,C[77 Space_Console_Bio]";
        std::fs::write(
            &log,
            format!(
                concat!(
                    // A shot, so the combat is kept and has a combat time.
                    "26:07:23:20:00:00.0::{me},,*,Target,C[9 Npc_Foo],Phaser Beam,Pn.a,Phaser,,100,100\n",
                    // Self heal, self-directed (src=me, ind=-, tgt=-): 1000 hull.
                    "26:07:23:20:00:01.0::{me},,*,,*,Brace for Impact III,Pn.b,HitPoints,,-1000,0\n",
                    // Self heal through my own console (src=me, ind=console, tgt=me): 200 shield.
                    "26:07:23:20:00:02.0::{me},{console},{me},Shield Generator,Pn.c,Shield,,-200,0\n",
                    // I heal a teammate (src=me, tgt=mate): 30 hull.
                    "26:07:23:20:00:03.0::{me},,*,{mate},Rally Cry V,Pn.d,HitPoints,,-30,0\n",
                    // A teammate heals me (src=mate, tgt=me): 7 hull.
                    "26:07:23:20:00:04.0::{mate},,*,{me},Pahvan Healing Crystal,Pn.e,HitPoints,,-7,0\n",
                ),
                me = me,
                mate = mate,
                console = console,
            ),
        )
        .unwrap();

        let mut analyzer = Analyzer::new(AnalysisSettings {
            combatlog_file: log.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .unwrap();
        analyzer.update();
        let combat = analyzer.result().first().expect("one combat");
        let me_handle = combat.name_manager.get_handle("Kestrel@handle").unwrap();
        let player = combat.players.get(&me_handle).expect("the player");

        assert_eq!(
            1200.0, player.heal_self.total_heal.all,
            "self heals: the self-directed buff plus the own console's heal"
        );
        assert_eq!(1000.0, player.heal_self.total_heal.hull);
        assert_eq!(200.0, player.heal_self.total_heal.shield);

        assert_eq!(
            30.0, player.heal_ally.total_heal.all,
            "healing done holds only what went to somebody else"
        );
        assert_eq!(
            7.0, player.heal_received.total_heal.all,
            "healing received holds only what somebody else did to this player"
        );

        // The pools do not overlap: nothing is counted twice.
        assert_eq!(
            1237.0,
            player.heal_self.total_heal.all
                + player.heal_ally.total_heal.all
                + player.heal_received.total_heal.all,
            "every heal tick lands in exactly one pool"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The teammate's side of the same log: what they gave must show up as their
    /// Healing Ally, and what they got as their Healing Received — never as self
    /// healing.
    #[test]
    fn a_heal_between_two_players_lands_in_opposite_pools() {
        let dir = std::env::temp_dir().join("cla-heal-between-players-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("combatlog.log");
        std::fs::write(
            &log,
            concat!(
                "26:07:23:20:00:00.0::Kestrel,P[1@2 Kestrel@handle],,*,Target,C[9 Npc_Foo],Phaser Beam,Pn.a,Phaser,,100,100\n",
                "26:07:23:20:00:01.0::Sirak,P[3@4 Sirak@other],,*,Kestrel,P[1@2 Kestrel@handle],Rally Cry V,Pn.d,HitPoints,,-50,0\n",
            ),
        )
        .unwrap();

        let mut analyzer = Analyzer::new(AnalysisSettings {
            combatlog_file: log.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .unwrap();
        analyzer.update();
        let combat = analyzer.result().first().expect("one combat");

        let healer = combat
            .players
            .get(&combat.name_manager.get_handle("Sirak@other").unwrap())
            .unwrap();
        let healed = combat
            .players
            .get(&combat.name_manager.get_handle("Kestrel@handle").unwrap())
            .unwrap();

        assert_eq!(50.0, healer.heal_ally.total_heal.all);
        assert_eq!(0.0, healer.heal_self.total_heal.all);
        assert_eq!(50.0, healed.heal_received.total_heal.all);
        assert_eq!(0.0, healed.heal_self.total_heal.all);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Both nestings of a pool must hold the same ticks, only arranged
    /// differently: person → ability at the top level versus ability → person.
    #[test]
    fn a_heal_pool_holds_both_grouping_orders() {
        let dir = std::env::temp_dir().join("cla-heal-grouping-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("combatlog.log");
        // Two healers, one of them using two abilities, so both nestings have
        // something to nest.
        std::fs::write(
            &log,
            concat!(
                "26:07:23:20:00:00.0::Kestrel,P[1@2 Kestrel@handle],,*,Target,C[9 Npc_Foo],Phaser Beam,Pn.a,Phaser,,100,100\n",
                "26:07:23:20:00:01.0::Sirak,P[3@4 Sirak@other],,*,Kestrel,P[1@2 Kestrel@handle],Rally Cry V,Pn.d,HitPoints,,-50,0\n",
                "26:07:23:20:00:02.0::Bora,P[5@6 Bora@third],,*,Kestrel,P[1@2 Kestrel@handle],Shield Gen,Pn.e,Shield,,-20,0\n",
                "26:07:23:20:00:03.0::Sirak,P[3@4 Sirak@other],,*,Kestrel,P[1@2 Kestrel@handle],Shield Gen,Pn.e,Shield,,-10,0\n",
            ),
        )
        .unwrap();

        let mut analyzer = Analyzer::new(AnalysisSettings {
            combatlog_file: log.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .unwrap();
        analyzer.update();
        let combat = analyzer.result().first().unwrap();
        let me = combat
            .players
            .get(&combat.name_manager.get_handle("Kestrel@handle").unwrap())
            .unwrap();

        let names = |group: &HealGroup| {
            let mut names: Vec<String> = group
                .sub_groups
                .values()
                .map(|s| s.name().get(&combat.name_manager).to_string())
                .collect();
            names.sort();
            names
        };

        assert_eq!(
            vec!["Bora@third", "Sirak@other"],
            names(&me.heal_received.by_person),
            "person first: the healers are the top level"
        );
        assert_eq!(
            vec!["Rally Cry V", "Shield Gen"],
            names(&me.heal_received.by_ability),
            "ability first: the abilities are the top level"
        );

        // Same ticks either way.
        assert_eq!(80.0, me.heal_received.by_person.total_heal.all);
        assert_eq!(80.0, me.heal_received.by_ability.total_heal.all);
        assert_eq!(
            me.heal_received.by_person.heal_metrics.ticks.all,
            me.heal_received.by_ability.heal_metrics.ticks.all
        );

        // Shield Gen came from both healers, 20 + 10.
        let shield_gen = me
            .heal_received
            .by_ability
            .sub_groups
            .values()
            .find(|s| s.name().get(&combat.name_manager) == "Shield Gen")
            .expect("the Shield Gen branch");
        assert_eq!(30.0, shield_gen.total_heal.all);
        assert_eq!(2, shield_gen.sub_groups.len(), "one branch per healer");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A heal routed through a pet or console must still nest ability first
    /// under the ability order. Taken from a real log: Wren's Jem'hadar
    /// Wingman casting Engineering Team III on Rook, which used to put the
    /// pet on top because it was the last segment of the grouping path.
    #[test]
    fn the_ability_order_puts_the_ability_on_top_even_through_a_pet() {
        let dir = std::env::temp_dir().join("cla-heal-pet-order-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("combatlog.log");
        std::fs::write(
            &log,
            concat!(
                "26:07:30:17:22:00.0::Wren,P[1@2 Wren@g],,*,Target,C[9 Npc_Foo],Phaser Beam,Pn.a,Phaser,,100,100\n",
                "26:07:30:17:22:19.1::Wren,P[1@2 Wren@g],Jem'hadar Wingman,C[10 Space_Jemhadar_Wingman_1],Rook,P[3@4 Rook@k],Engineering Team III,Pn.4,HitPoints,,-7087.5,-6750\n",
            ),
        )
        .unwrap();

        let mut analyzer = Analyzer::new(AnalysisSettings {
            combatlog_file: log.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .unwrap();
        analyzer.update();
        let combat = analyzer.result().first().unwrap();
        let player = combat
            .players
            .get(&combat.name_manager.get_handle("Wren@g").unwrap())
            .unwrap();

        let only_child = |group: &HealGroup| -> (String, HealGroup) {
            assert_eq!(1, group.sub_groups.len(), "expected a single branch");
            let child = group.sub_groups.values().next().unwrap();
            (
                child.name().get(&combat.name_manager).to_string(),
                child.clone(),
            )
        };

        let (top, rest) = only_child(&player.heal_ally.by_ability);
        assert_eq!("Engineering Team III", top, "ability first, not the pet");
        let (middle, rest) = only_child(&rest);
        assert_eq!("Jem'hadar Wingman", middle, "then what it was cast through");
        let (leaf, _) = only_child(&rest);
        assert_eq!("Rook@k", leaf, "and the recipient at the bottom");

        // The other order is the same three levels upside down.
        let (top, rest) = only_child(&player.heal_ally.by_person);
        assert_eq!("Rook@k", top);
        let (middle, rest) = only_child(&rest);
        assert_eq!("Jem'hadar Wingman", middle);
        let (leaf, _) = only_child(&rest);
        assert_eq!("Engineering Team III", leaf);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Self healing leaves the person out. It is always the player whose pool
    /// it is, so a level repeating their name would only cost a click.
    #[test]
    fn self_healing_has_no_person_level() {
        let dir = std::env::temp_dir().join("cla-heal-self-level-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("combatlog.log");
        std::fs::write(
            &log,
            concat!(
                "26:07:23:20:00:00.0::Kestrel,P[1@2 Kestrel@h],,*,Target,C[9 Npc_Foo],Phaser Beam,Pn.a,Phaser,,100,100\n",
                "26:07:23:20:00:01.0::Kestrel,P[1@2 Kestrel@h],,*,,*,Reflexive Emitters,Pn.b,Shield,,-2000,0\n",
            ),
        )
        .unwrap();

        let mut analyzer = Analyzer::new(AnalysisSettings {
            combatlog_file: log.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .unwrap();
        analyzer.update();
        let combat = analyzer.result().first().unwrap();
        let player = combat
            .players
            .get(&combat.name_manager.get_handle("Kestrel@h").unwrap())
            .unwrap();

        let names: Vec<String> = player
            .heal_self
            .by_ability
            .sub_groups
            .values()
            .map(|g| g.name().get(&combat.name_manager).to_string())
            .collect();
        assert_eq!(
            vec!["Reflexive Emitters"],
            names,
            "the ability sits directly under the player, with no name level between"
        );
        assert_eq!(2000.0, player.heal_self.total_heal.all);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Standing on a social map still writes records - fall damage and
    /// self-buffs - and each burst becomes its own "combat". Those hold no
    /// player damage at all and are dropped, except the newest, which may still
    /// be in progress.
    #[test]
    fn combats_without_player_damage_are_discarded() {
        let dir = std::env::temp_dir().join("cla-empty-combat-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("combatlog.log");

        // 1) fall damage, nobody attacking   2) a real fight   3) a self-buff
        //    4) another real fight, so the junk at 3 is not the newest combat
        let records = concat!(
            "26:07:23:20:00:00.0::,,,,Sara,P[9@9 Sara@x],falling,Autodesc.Combatevent.Falling,HitPoints,,6.2,0\n",
            "26:07:23:20:10:00.0::Kestrel,P[1@2 Kestrel@h],,*,Target,C[1 Npc_Foo],Phaser Beam,Pn.abc,Phaser,,100,100\n",
            "26:07:23:20:20:00.0::Valir,P[3@4 Valir@y],,*,,*,Personal Shields,Pn.d,Shield,,0.02,0\n",
            "26:07:23:20:30:00.0::Kestrel,P[1@2 Kestrel@h],,*,Target,C[1 Npc_Foo],Phaser Beam,Pn.abc,Phaser,,100,100\n",
        );
        std::fs::write(&log, records).unwrap();

        let mut analyzer = Analyzer::new(AnalysisSettings {
            combatlog_file: log.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .unwrap();
        analyzer.update();

        assert_eq!(
            2,
            analyzer.result().len(),
            "only the two combats with player damage should remain"
        );
        for combat in analyzer.result() {
            assert!(
                combat.total_damage_out.all > 0.0,
                "a kept combat must have damage"
            );
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The newest combat gets no exemption. A log ending on fall damage or a
    /// self-buff — the game closed, or the player idling on a social map — used
    /// to leave that junk entry sitting at the top of the list permanently.
    #[test]
    fn newest_combat_is_discarded_too_when_it_has_no_damage() {
        let dir = std::env::temp_dir().join("cla-newest-combat-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("combatlog.log");
        // A real fight, then a self-buff burst that ends the log.
        std::fs::write(
            &log,
            concat!(
                "26:07:23:20:00:00.0::Kestrel,P[1@2 Kestrel@h],,*,Target,C[1 Npc_Foo],Phaser Beam,Pn.abc,Phaser,,100,100\n",
                "26:07:23:20:10:00.0::Valir,P[3@4 Valir@y],,*,,*,Personal Shields,Pn.d,Shield,,0.02,0\n",
            ),
        )
        .unwrap();

        let mut analyzer = Analyzer::new(AnalysisSettings {
            combatlog_file: log.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .unwrap();
        analyzer.update();
        assert_eq!(
            1,
            analyzer.result().len(),
            "the trailing self-buff burst must not survive as a combat"
        );
        assert!(analyzer.result()[0].total_damage_out.all > 0.0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_combat_type_adds_parenthesised_environment() {
        assert_eq!(
            append_detected_combat_type("[TFO] Into the Hive".to_string(), Some("Ground")),
            "[TFO] Into the Hive (Ground)"
        );
        // A name that already says it is not doubled.
        assert_eq!(
            append_detected_combat_type("Bug Hunt Ground".to_string(), Some("Ground")),
            "Bug Hunt Ground"
        );
        // No curated environment, or a blank one, leaves the name alone.
        assert_eq!(
            append_detected_combat_type("Combat".to_string(), None),
            "Combat"
        );
        assert_eq!(
            append_detected_combat_type("Combat".to_string(), Some("  ")),
            "Combat"
        );
    }

    #[test]
    fn append_difficulty_adds_bracketed_level() {
        assert_eq!(
            append_detected_difficulty("Infected Space".to_string(), Some(Difficulty::Elite)),
            "Infected Space [Elite]"
        );
        assert_eq!(
            append_detected_difficulty("Khitomer".to_string(), Some(Difficulty::Advanced)),
            "Khitomer [Advanced]"
        );
    }

    #[test]
    fn append_difficulty_skips_when_name_already_has_the_tier() {
        // A user rule that already says "(Elite)" must not become "... [Elite]".
        assert_eq!(
            append_detected_difficulty(
                "Infected Conduit (Elite)".to_string(),
                Some(Difficulty::Elite)
            ),
            "Infected Conduit (Elite)"
        );
    }

    #[test]
    fn append_difficulty_unchanged_for_any_or_none() {
        assert_eq!(
            append_detected_difficulty("Azure Nebula".to_string(), None),
            "Azure Nebula"
        );
        assert_eq!(
            append_detected_difficulty("Azure Nebula".to_string(), Some(Difficulty::Any)),
            "Azure Nebula"
        );
    }

    /// Real-data check for the "delete combats" flow: keep a subset of combats
    /// by concatenating their byte ranges, then re-analyze and assert exactly
    /// those combats survive, in order (removing a middle combat must not merge
    /// its neighbours). Point `CLA_TEST_COMBATLOG` at a real combatlog.log and
    /// run with:
    /// `CLA_TEST_COMBATLOG=<path> cargo test keep_subset_of_combats -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads a real STO log"]
    fn keep_subset_of_combats() {
        let Some(src) = std::env::var_os("CLA_TEST_COMBATLOG") else {
            println!("set CLA_TEST_COMBATLOG to a combatlog.log to run this test");
            return;
        };
        let dir = std::env::temp_dir().join("cla-keep-combats-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let work = dir.join("combatlog.log");
        std::fs::copy(src, &work).unwrap();

        let analyze = |file: &Path| {
            let mut analyzer = Analyzer::new(AnalysisSettings {
                combatlog_file: file.to_string_lossy().into_owned(),
                ..Default::default()
            })
            .unwrap();
            analyzer.update();
            analyzer
        };

        let analyzer = analyze(&work);
        let combats = analyzer.result();
        let n = combats.len();
        assert!(n >= 3, "need several combats to test with, got {n}");

        // Keep the newest plus every other combat (drops several middle ones).
        let keep: Vec<usize> = (0..n).filter(|i| *i == n - 1 || i % 2 == 0).collect();
        let kept_ids: Vec<String> = keep.iter().map(|&i| combats[i].identifier()).collect();

        let mut data = Vec::new();
        for &i in &keep {
            let bytes = combats[i]
                .read_log_combat_data(&work)
                .expect("combat byte range");
            data.extend_from_slice(&bytes);
        }
        let rewritten = dir.join("kept.log");
        std::fs::write(&rewritten, &data).unwrap();

        let new_ids: Vec<String> = analyze(&rewritten)
            .result()
            .iter()
            .map(|c| c.identifier())
            .collect();
        println!("kept {} of {} combats", keep.len(), n);
        assert_eq!(
            new_ids, kept_ids,
            "rewritten log must contain exactly the kept combats, in order"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Research tool: dump every NPC's median hull damage from the live log as
    /// TSV, tagged with the combat's detected map and difficulty, so the HP
    /// bands can be compared *across* maps (are they a property of the ship
    /// class rather than the map?). Only entities that died are emitted — for
    /// the others the median is damage we happened to deal, not their HP.
    /// Point `CLA_TEST_COMBATLOG` at a real combatlog.log and run with:
    /// `CLA_TEST_COMBATLOG=<path> cargo test dump_critter_hp_by_tier -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads a real STO log"]
    fn dump_critter_hp_by_tier() {
        let Some(src) = std::env::var_os("CLA_TEST_COMBATLOG") else {
            println!("set CLA_TEST_COMBATLOG to a combatlog.log to run this test");
            return;
        };
        let mut analyzer = Analyzer::new(AnalysisSettings {
            combatlog_file: src.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .unwrap();
        analyzer.update();

        println!("combat\tmap\tdifficulty\tentity\tdeaths\tinstances\tmedian_hull");
        for (combat_index, combat) in analyzer.result().iter().enumerate() {
            let (Some(map), Some(difficulty)) = (&combat.detected_map, combat.detected_difficulty)
            else {
                continue;
            };
            for (handle, meta) in combat.critters.iter() {
                if meta.deaths == 0 || meta.hull_damage_per_instance.is_empty() {
                    continue;
                }
                let mut values: Vec<f64> =
                    meta.hull_damage_per_instance.values().copied().collect();
                values.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let n = values.len();
                let median = if n % 2 == 1 {
                    values[n / 2]
                } else {
                    (values[n / 2 - 1] + values[n / 2]) / 2.0
                };
                println!(
                    "{}\t{}\t{:?}\t{}\t{}\t{}\t{:.0}",
                    combat_index,
                    map,
                    difficulty,
                    combat.name_manager.name(*handle),
                    meta.deaths,
                    n,
                    median
                );
            }
        }
    }

    /// Real-data check for the three healing pools: totals the split across a
    /// whole log, per player, so the numbers can be compared against an
    /// independent count of the raw records. Point `CLA_TEST_COMBATLOG` at a
    /// real combatlog.log and run with:
    /// `CLA_TEST_COMBATLOG=<path> cargo test dump_heal_pools -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads a real STO log"]
    fn dump_heal_pools() {
        let Some(src) = std::env::var_os("CLA_TEST_COMBATLOG") else {
            println!("set CLA_TEST_COMBATLOG to a combatlog.log to run this test");
            return;
        };
        let mut analyzer = Analyzer::new(AnalysisSettings {
            combatlog_file: src.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .unwrap();
        analyzer.update();

        let mut per_player: FxHashMap<String, [ShieldHullValues; 3]> = Default::default();
        for combat in analyzer.result() {
            for player in combat.players.values() {
                let name = player.name().get(&combat.name_manager).to_string();
                let totals = per_player.entry(name).or_default();
                totals[0] += player.heal_received.total_heal;
                totals[1] += player.heal_ally.total_heal;
                totals[2] += player.heal_self.total_heal;
            }
        }

        let mut rows: Vec<_> = per_player.into_iter().collect();
        rows.sort_by(|a, b| {
            let sum = |t: &[ShieldHullValues; 3]| t[0].all + t[1].all + t[2].all;
            sum(&b.1).partial_cmp(&sum(&a.1)).unwrap()
        });
        println!(
            "{:<28} {:>14} {:>14} {:>14}",
            "player", "received", "done", "self"
        );
        for (name, totals) in rows.iter().take(12) {
            println!(
                "{:<28} {:>14.0} {:>14.0} {:>14.0}",
                name, totals[0].all, totals[1].all, totals[2].all
            );
        }
    }

    /// Real-data smoke test for map/difficulty detection: analyze the live log
    /// and print each combat's name alongside the detected map and difficulty.
    /// Point `CLA_TEST_COMBATLOG` at a real combatlog.log and run with:
    /// `CLA_TEST_COMBATLOG=<path> cargo test detects_maps_in_real_log -- --ignored --nocapture`.
    #[test]
    #[ignore = "reads a real STO log"]
    fn detects_maps_in_real_log() {
        let Some(src) = std::env::var_os("CLA_TEST_COMBATLOG") else {
            println!("set CLA_TEST_COMBATLOG to a combatlog.log to run this test");
            return;
        };
        let mut analyzer = Analyzer::new(AnalysisSettings {
            combatlog_file: src.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .unwrap();
        analyzer.update();

        let mut detected = 0usize;
        for combat in analyzer.result() {
            if combat.detected_map.is_some() {
                detected += 1;
            }
            println!(
                "{:<45} map={:?} difficulty={:?}",
                combat.name(),
                combat.detected_map,
                combat.detected_difficulty
            );
        }
        println!(
            "detected a map for {} of {} combats",
            detected,
            analyzer.result().len()
        );
    }

    #[test]
    #[ignore = "manual test"]
    fn analyze_log() {
        let mut analyzer = Analyzer::new(AnalysisSettings {
            combatlog_file:
                r"D:\Games\Star Trek Online_en\Star Trek Online\Live\logs\GameClient\combatlog.log"
                    .to_string(),
            ..Default::default()
        })
        .unwrap();

        analyzer.update();
        let result = analyzer.result();
        let combats: Vec<_> = result.iter().map(|c| c.identifier()).collect();
        println!("combats: {:?}", combats);
    }
}
