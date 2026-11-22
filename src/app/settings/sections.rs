//! Rows the reader filed in a section by hand.
//!
//! The program files every row of a split Damage Dealt table on its own
//! (`app::damage_sections`), and is sometimes wrong — a console whose name ends
//! in "Array", a pet that also flies as an enemy. A right click on the row
//! moves it, and the move is written here so it holds for every fight and
//! every later start.
//!
//! Kept by the row's name, because that is all that stays the same from one
//! fight to the next. Only the moves are written: a row the reader never
//! touched follows the program's own filing, including any improvement to it a
//! later version makes.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::analyzer::DamageSection;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct SectionChoices {
    #[serde(default)]
    moved: BTreeMap<String, DamageSection>,
}

impl SectionChoices {
    /// Where the reader put this row, if they put it anywhere.
    pub fn moved(&self, row: &str) -> Option<DamageSection> {
        self.moved.get(row).copied()
    }

    /// Files the row in `section`, or — with `None` — hands it back to the
    /// program's own filing.
    pub fn set(&mut self, row: &str, section: Option<DamageSection>) {
        match section {
            Some(section) => {
                self.moved.insert(row.to_string(), section);
            }
            None => {
                self.moved.remove(row);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape a hand-filed row is written in. If this fails, a field was
    /// renamed or retyped: either keep reading the old shape, or convert the
    /// files already written — a renamed field is read as "nothing was moved".
    #[test]
    fn the_written_shape_is_pinned() {
        let mut choices = SectionChoices::default();
        choices.set("Kinetic Cutting Beam", Some(DamageSection::Weapons));
        assert_eq!(
            r#"{"moved":{"Kinetic Cutting Beam":"weapons"}}"#,
            serde_json::to_string(&choices).unwrap()
        );
        let read: SectionChoices =
            serde_json::from_str(r#"{"moved":{"Kinetic Cutting Beam":"weapons"}}"#).unwrap();
        assert_eq!(choices, read);
    }

    /// Handing a row back leaves nothing behind, so the file does not keep an
    /// entry for every row ever moved and moved back.
    #[test]
    fn a_row_handed_back_leaves_no_entry() {
        let mut choices = SectionChoices::default();
        choices.set("Photon Torpedo", Some(DamageSection::Abilities));
        choices.set("Photon Torpedo", None);
        assert_eq!(SectionChoices::default(), choices);
        assert_eq!(None, choices.moved("Photon Torpedo"));
    }
}
