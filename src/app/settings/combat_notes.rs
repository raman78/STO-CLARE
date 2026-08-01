//! Short notes the user writes to recognize a combat later.
//!
//! A combat has no identifier that survives a rename: [`Combat::identifier`]
//! carries the name the Combat Name Rules (or the map detection) produced, so
//! editing a rule would orphan every note attached under the old text. The
//! start of the combat's active time is the one thing the log itself fixes, so
//! that is the key.
//!
//! Its limit: `combat_separation_time_seconds` decides where one combat ends
//! and the next begins, so changing it re-cuts the log and the notes of the
//! combats that moved no longer find their fight.

use std::collections::BTreeMap;

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};

use crate::analyzer::Combat;

/// Longest note kept, counted in characters rather than bytes so the limit
/// means the same whatever alphabet it is written in.
pub const MAX_NOTE_CHARS: usize = 50;

/// The notes, keyed by combat start time. Kept out of the analysis settings on
/// purpose: that section is compared when the settings dialog is applied and a
/// difference there re-analyzes the log, which writing a note is no reason for.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct CombatNotes {
    /// Sorted so the settings file stays readable and its diff stable.
    notes: BTreeMap<String, String>,
}

impl CombatNotes {
    /// The key a combat's note is stored under.
    pub fn key(combat: &Combat) -> String {
        Self::key_at(combat.active_time.start)
    }

    /// Same key from the start time alone, for the views that hold the combats
    /// list as parallel arrays rather than as whole combats.
    ///
    /// Spelled out rather than left to `NaiveDateTime`'s own formatting, so the
    /// keys already written stay readable by later versions.
    pub fn key_at(start: NaiveDateTime) -> String {
        start.format("%Y-%m-%d %H:%M:%S%.3f").to_string()
    }

    pub fn get(&self, key: &str) -> &str {
        self.notes.get(key).map(String::as_str).unwrap_or("")
    }

    /// Stores `note` under `key`, cut to [`MAX_NOTE_CHARS`]. A note that holds
    /// nothing is removed instead of stored, so clearing the field leaves no
    /// entry behind. Deliberately does not trim: it runs on every keystroke,
    /// and a user typing a space between two words must keep it.
    pub fn set(&mut self, key: &str, note: &str) {
        let note: String = note.chars().take(MAX_NOTE_CHARS).collect();
        if note.is_empty() {
            self.notes.remove(key);
        } else {
            self.notes.insert(key.to_owned(), note);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_is_cut_to_the_limit() {
        let mut notes = CombatNotes::default();
        notes.set("k", &"x".repeat(MAX_NOTE_CHARS + 10));
        assert_eq!(MAX_NOTE_CHARS, notes.get("k").chars().count());
    }

    /// The limit counts characters: fifty accented letters are fifty, not the
    /// hundred bytes they take up.
    #[test]
    fn the_limit_counts_characters_not_bytes() {
        let mut notes = CombatNotes::default();
        let note = "ą".repeat(MAX_NOTE_CHARS);
        notes.set("k", &note);
        assert_eq!(note, notes.get("k"));
    }

    #[test]
    fn clearing_a_note_removes_it() {
        let mut notes = CombatNotes::default();
        notes.set("k", "first run of the evening");
        notes.set("k", "");
        assert_eq!("", notes.get("k"));
        assert_eq!(CombatNotes::default(), notes);
    }

    #[test]
    fn a_combat_without_a_note_reads_as_empty() {
        assert_eq!("", CombatNotes::default().get("k"));
    }

    /// Two combats of the same log must not share a note.
    #[test]
    fn the_key_follows_the_start_time() {
        let at = |s: &str| NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f").unwrap();
        assert_eq!(
            CombatNotes::key_at(at("2026-07-23 20:07:22.6")),
            CombatNotes::key_at(at("2026-07-23 20:07:22.600"))
        );
        assert_ne!(
            CombatNotes::key_at(at("2026-07-23 20:07:22.6")),
            CombatNotes::key_at(at("2026-07-23 20:07:22.7"))
        );
    }
}
