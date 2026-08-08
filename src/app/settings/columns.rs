//! Which columns the main window's tables show.
//!
//! Stored per kind of table rather than per tab: the two damage tabs are the
//! same table with the same metrics, and so are the three healing ones, so a
//! column hidden in Damage Dealt stays hidden in Damage Taken — which is what
//! "hide the flanking column, I do not fly a flanker" means.
//!
//! What is written down is what the user **hid**, not what they kept. A metric
//! added by a later version is then on screen the first time they open the tab,
//! instead of missing from a list written before it existed.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// The three column sets the main window has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableKind {
    Summary,
    Damage,
    Heal,
}

impl TableKind {
    /// The name this kind is filed under in the settings file. Spelled out, so
    /// the file stays readable and renaming the enum cannot orphan a choice.
    fn key(self) -> &'static str {
        match self {
            TableKind::Summary => "summary",
            TableKind::Damage => "damage",
            TableKind::Heal => "heal",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ColumnVisibility {
    /// Hidden column names per table kind. Empty — the default — is everything
    /// on screen.
    #[serde(default)]
    hidden: BTreeMap<String, BTreeSet<String>>,
}

impl ColumnVisibility {
    pub fn is_shown(&self, kind: TableKind, column: &str) -> bool {
        !self
            .hidden
            .get(kind.key())
            .is_some_and(|hidden| hidden.contains(column))
    }

    pub fn set_shown(&mut self, kind: TableKind, column: &str, shown: bool) {
        let hidden = self.hidden.entry(kind.key().to_string()).or_default();
        if shown {
            hidden.remove(column);
        } else {
            hidden.insert(column.to_string());
        }
        if hidden.is_empty() {
            self.hidden.remove(kind.key());
        }
    }

    /// How many of `columns` are hidden, for the picker to say so on its button
    /// — a table missing a metric is otherwise a puzzle rather than a setting.
    pub fn hidden_count(&self, kind: TableKind, columns: &[&str]) -> usize {
        columns
            .iter()
            .filter(|column| !self.is_shown(kind, column))
            .count()
    }

    pub fn show_all(&mut self, kind: TableKind) {
        self.hidden.remove(kind.key());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nothing written down means nothing hidden: a fresh install, and any
    /// column added after the user last touched the picker.
    #[test]
    fn everything_is_shown_until_something_is_hidden() {
        let visibility = ColumnVisibility::default();
        assert!(visibility.is_shown(TableKind::Damage, "Flanking %"));
        assert!(visibility.is_shown(TableKind::Heal, "Ticks %"));
        assert!(visibility.is_shown(TableKind::Summary, "Deaths"));
    }

    /// The three kinds are kept apart, so hiding a metric in the damage tables
    /// leaves the one that happens to share its name in the healing tables.
    #[test]
    fn the_kinds_do_not_share_their_choices() {
        let mut visibility = ColumnVisibility::default();
        visibility.set_shown(TableKind::Damage, "Critical %", false);

        assert!(!visibility.is_shown(TableKind::Damage, "Critical %"));
        assert!(visibility.is_shown(TableKind::Heal, "Critical %"));
        assert!(visibility.is_shown(TableKind::Damage, "DPS"));
    }

    /// Putting a column back leaves nothing behind, so the settings file does
    /// not grow an entry per column the user ever toggled.
    #[test]
    fn showing_a_column_again_clears_the_entry() {
        let mut visibility = ColumnVisibility::default();
        visibility.set_shown(TableKind::Damage, "Flanking %", false);
        visibility.set_shown(TableKind::Damage, "Flanking %", true);

        assert_eq!(ColumnVisibility::default(), visibility);
    }

    /// "Show all" is one click rather than one per hidden column.
    #[test]
    fn show_all_puts_every_column_back() {
        let mut visibility = ColumnVisibility::default();
        for column in ["Flanking %", "Accuracy %", "Base DPS"] {
            visibility.set_shown(TableKind::Damage, column, false);
        }
        assert_eq!(
            3,
            visibility.hidden_count(TableKind::Damage, &["Flanking %", "Accuracy %", "Base DPS"])
        );

        visibility.show_all(TableKind::Damage);

        assert_eq!(ColumnVisibility::default(), visibility);
    }

    /// The count is of the columns asked about, not of everything ever hidden —
    /// a name left over from an older version must not make the button claim a
    /// column is missing when none is.
    #[test]
    fn the_count_only_covers_the_columns_asked_about() {
        let mut visibility = ColumnVisibility::default();
        visibility.set_shown(TableKind::Damage, "Metric From An Older Version", false);
        assert_eq!(0, visibility.hidden_count(TableKind::Damage, &["DPS"]));
    }
}
