//! A combat boiled down to what a *list* of combats needs to say about it.
//!
//! The combats list travels from the analysis thread to every view that offers
//! a choice of fight (the main window's panel, the compare picker, the delete
//! dialog). It used to travel as half a dozen `Vec`s indexed alongside each
//! other — identifiers, difficulties, base names, environments, start times,
//! whether each was fought alone — where a single list left behind read the
//! wrong entry for every combat after it. One value per combat cannot come
//! apart that way.
//!
//! Everything here is worked out once, when the log is read, rather than per
//! frame: a list of a few hundred fights is redrawn many times a second and
//! formatting a name each time is work for nothing.

use std::sync::Arc;

use chrono::{Duration, NaiveDateTime};
use rustc_hash::FxHashMap;

use super::{Combat, Difficulty};
use crate::helpers::time_range_to_duration_or_zero;

/// The combats list as it is passed around: cheap to clone, because every
/// handler that asked for it gets a copy of the message.
pub type CombatSummaries = Arc<[CombatSummary]>;

/// One combat, as a list of combats knows it.
#[derive(Debug, Clone, PartialEq)]
pub struct CombatSummary {
    /// The fight's full name, as [`Combat::name`] writes it — team size, map,
    /// environment and difficulty.
    pub name: String,
    /// Name and date/time together, as [`Combat::identifier`] writes it. What
    /// the older lists showed as their one line per combat.
    pub identifier: String,
    /// The map alone, without the environment and difficulty suffixes. What
    /// "the same kind of fight" means, and what the filters group by.
    ///
    /// Still carries the `[TFO]` prefix the detection puts on it: this string is
    /// what naming rules are written against and what a saved log is called, so
    /// it is not the place to tidy anything away. The Map column shows
    /// [`Self::map`] instead.
    pub base_name: String,
    /// What kind of content it was ("TFO", "Patrol", …), where the map is one
    /// the program knows. Its own column, and one of the ways the list orders.
    pub category: Option<String>,
    /// "Space", "Ground", … where the map was recognized.
    pub environment: Option<String>,
    pub difficulty: Option<Difficulty>,
    /// Whether one player fought it — the ladder's test, see [`Combat::is_solo`].
    pub solo: bool,
    /// Start of the fight's active time. The one per-combat value the log itself
    /// fixes, so it is both the Start column and the key a user note hangs on.
    pub start: NaiveDateTime,
    /// How long there was fighting: the span of `combat_time`, first damage to
    /// last. Deliberately not the active time, which reaches wider — this is the
    /// span the DPS figures below are per second of, so the two agree.
    pub duration: Duration,
    /// Everyone who fought it, by DPS, highest first.
    pub players: Vec<PlayerSummary>,
}

/// One player of a combat, as the list knows them.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerSummary {
    /// The account handle including its `@`, e.g. `@ramanwaleczny`. The
    /// character name is left off: a player is one account across all their
    /// captains, and the handle is what says two fights were the same person.
    pub handle: String,
    /// Damage dealt per second, over this player's own combat time — the very
    /// figure the Summary tab shows for them.
    pub dps: f64,
    /// How many times they were killed in this fight. The same count
    /// [`Combat::total_deaths`] adds up over everyone: the kills recorded
    /// against the damage they took.
    pub deaths: u32,
}

impl Combat {
    /// What the combats list needs to know about this fight.
    pub fn summary(&self) -> CombatSummary {
        let mut players: Vec<PlayerSummary> = self
            .players
            .values()
            .map(|player| PlayerSummary {
                handle: handle_of(player.name().get(&self.name_manager)),
                dps: player.damage_out.dps.all,
                deaths: player.damage_in.kills.values().copied().sum(),
            })
            .collect();
        // Highest first, which is the order they are read in: the question a
        // list of fights answers is "how did that one go", and the answer starts
        // at the top of the table.
        players.sort_unstable_by(|a, b| b.dps.total_cmp(&a.dps));

        CombatSummary {
            name: self.name(),
            identifier: self.identifier(),
            base_name: self.base_name(),
            category: self.detected_category.clone(),
            environment: self.detected_combat_type.clone(),
            difficulty: self.detected_difficulty,
            solo: self.is_solo(),
            start: self.active_time.start,
            duration: time_range_to_duration_or_zero(&self.combat_time),
            players,
        }
    }
}

impl CombatSummary {
    /// The map as a column shows it: the name without the `[TFO]` prefix that
    /// says what kind of content it is, since that stands in a column of its
    /// own. A name the prefix is not on — a user's own naming rule, or a map
    /// the program does not know — is shown whole.
    pub fn map(&self) -> &str {
        let Some(category) = self.category.as_deref() else {
            return &self.base_name;
        };
        self.base_name
            .strip_prefix(&format!("[{category}] "))
            .unwrap_or(&self.base_name)
    }
}

/// The account the log belongs to: the handle present in more of its combats
/// than any other.
///
/// A combat log is written by one player's client and only records fights that
/// player took part in, so their handle is in every single combat while even a
/// regular team-mate is in a fraction of them. Measured against a real 350 MB
/// log: the owner appeared in 813 of its combats, the next handle in 24.
///
/// `None` when the log holds no players at all, or when two handles are level —
/// which is what a log of nothing but duo runs looks like, and there is nothing
/// in it that says which of the two is holding the keyboard. The reader names
/// themselves in the settings in that case.
pub fn detect_log_owner(combats: &[CombatSummary]) -> Option<String> {
    let mut appearances: FxHashMap<&str, usize> = FxHashMap::default();
    for combat in combats {
        for player in &combat.players {
            *appearances.entry(player.handle.as_str()).or_default() += 1;
        }
    }

    let most = appearances.values().copied().max()?;
    let mut leaders = appearances
        .iter()
        .filter(|(_, count)| **count == most)
        .map(|(handle, _)| *handle);
    let leader = leaders.next()?;
    if leaders.next().is_some() {
        return None;
    }
    Some(leader.to_owned())
}

/// The `@handle` part of a player's full `Name@handle`, or the whole thing when
/// there is no handle in it — a name the log wrote in some other shape is still
/// better shown than dropped.
fn handle_of(full_name: &str) -> String {
    match full_name.find('@') {
        Some(at) => full_name[at..].to_owned(),
        None => full_name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(players: &[(&str, f64)]) -> CombatSummary {
        CombatSummary {
            name: "[Team] Infected Space [Elite]".to_owned(),
            identifier: "[Team] Infected Space [Elite] | 2026-08-19 21:14:03 - 21:18:15".to_owned(),
            base_name: "[TFO] Infected Space".to_owned(),
            category: Some("TFO".to_owned()),
            environment: Some("Space".to_owned()),
            difficulty: Some(Difficulty::Elite),
            solo: players.len() == 1,
            start: NaiveDateTime::default(),
            duration: Duration::seconds(252),
            players: players
                .iter()
                .map(|&(handle, dps)| PlayerSummary {
                    handle: handle.to_owned(),
                    dps,
                    deaths: 0,
                })
                .collect(),
        }
    }

    /// The Map column drops the `[TFO]` prefix, because that stands in its own
    /// column — but only its own prefix, and only when there is one.
    #[test]
    fn the_map_column_leaves_the_content_type_to_its_own_column() {
        assert_eq!("Infected Space", summary(&[]).map());

        let mut combat = summary(&[]);
        combat.category = None;
        assert_eq!(
            "[TFO] Infected Space",
            combat.map(),
            "nothing known about the prefix means nothing is taken off"
        );

        // A user's own naming rule produces a name with no prefix at all.
        let mut combat = summary(&[]);
        combat.base_name = "my own name for it".to_owned();
        assert_eq!("my own name for it", combat.map());
    }

    #[test]
    fn a_handle_is_the_part_from_the_at_sign() {
        assert_eq!("@ramanwaleczny", handle_of("Kestrel@ramanwaleczny"));
        // Some handles carry a discriminator; it belongs to the handle.
        assert_eq!("@Ordog#93372", handle_of("T'Vek@Ordog#93372"));
    }

    /// A name the log wrote without a handle is kept whole rather than lost —
    /// an empty cell in the list would read as a bug in the list.
    #[test]
    fn a_name_without_a_handle_is_kept_whole() {
        assert_eq!("Nameless", handle_of("Nameless"));
    }

    #[test]
    fn the_owner_is_the_handle_in_the_most_combats() {
        let combats = vec![
            summary(&[("@me", 100.0), ("@friend", 90.0)]),
            summary(&[("@me", 110.0), ("@stranger", 95.0)]),
            summary(&[("@me", 120.0)]),
        ];
        assert_eq!(Some("@me".to_owned()), detect_log_owner(&combats));
    }

    /// Nothing but duo runs with the same person: the log cannot say which of
    /// the two is reading it, so it does not guess.
    #[test]
    fn two_handles_level_with_each_other_leave_the_owner_unknown() {
        let combats = vec![
            summary(&[("@me", 100.0), ("@always_together", 90.0)]),
            summary(&[("@me", 110.0), ("@always_together", 95.0)]),
        ];
        assert_eq!(None, detect_log_owner(&combats));
    }

    #[test]
    fn a_log_without_players_has_no_owner() {
        assert_eq!(None, detect_log_owner(&[]));
        assert_eq!(None, detect_log_owner(&[summary(&[])]));
    }
}
