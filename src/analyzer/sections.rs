//! The parts the Damage Dealt tab can split a player's rows into, and the
//! other rows its views add.
//!
//! Which row lands in which part is decided by the program's view of the tree,
//! not here (`app::damage_sections`). What the analyzer owns is the names: a
//! section is a row of the tree like any other, and a row is named by a handle
//! of the fight's own `NameManager`, which only the analyzer can add to. Every
//! combat therefore carries those names from the moment it is created.

use serde::{Deserialize, Serialize};

use super::{NameFlags, NameHandle, NameManager};

/// The row the ability-first view of Damage Dealt can gather every weapon shot
/// fired in no mode under, beside the rows of the modes.
pub const NORMAL_WEAPON_FIRE: &str = "Normal weapon fire";

/// The handle of [`NORMAL_WEAPON_FIRE`] in this fight.
pub fn normal_weapon_fire(name_manager: &NameManager) -> NameHandle {
    name_manager.handle(NORMAL_WEAPON_FIRE)
}

/// One part of a player's outgoing damage.
///
/// Written to the settings file under the spelled-out keys below, wherever the
/// reader filed a row by hand. Renaming a key orphans every choice already
/// written, which is why the keys are pinned by a test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum DamageSection {
    #[serde(rename = "weapons")]
    Weapons,
    #[serde(rename = "abilities")]
    Abilities,
    #[serde(rename = "pets")]
    Pets,
}

impl DamageSection {
    /// In the order the sections are listed in a menu.
    pub const ALL: [Self; 3] = [Self::Weapons, Self::Abilities, Self::Pets];

    /// What the section's row is called on screen.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Weapons => "Weapons",
            Self::Abilities => "Consoles and abilities",
            Self::Pets => "Pets and summons",
        }
    }

    /// The handle the section's row is named by in this fight.
    pub fn handle(self, name_manager: &NameManager) -> NameHandle {
        name_manager.handle(self.name())
    }

    /// Puts the names of the rows the program's views add — the sections and
    /// [`NORMAL_WEAPON_FIRE`] — into a fight's names, so [`Self::handle`] and
    /// [`normal_weapon_fire`] can find them. Flagged as nothing at all: none of
    /// them is an entity or an ability, and they must not turn up where the
    /// reader's rules match names.
    pub(super) fn register(name_manager: &mut NameManager) {
        for section in Self::ALL {
            name_manager.insert(section.name(), NameFlags::NONE);
        }
        name_manager.insert(NORMAL_WEAPON_FIRE, NameFlags::NONE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The keys a hand-filed row is written under. A renamed key is every such
    /// choice already on disk refused along with the whole settings file, so
    /// changing one means converting the files first.
    #[test]
    fn the_section_keys_in_the_settings_file_are_pinned() {
        let keys: Vec<String> = DamageSection::ALL
            .iter()
            .map(|section| serde_json::to_string(section).unwrap())
            .collect();
        assert_eq!(
            vec!["\"weapons\"", "\"abilities\"", "\"pets\""],
            keys,
            "a renamed section key refuses every settings file that names it"
        );
    }

    /// A fresh fight can name every section, which is what lets a split table
    /// be built for any combat at all.
    #[test]
    fn every_section_has_a_name_in_a_new_fight() {
        let mut name_manager = NameManager::default();
        DamageSection::register(&mut name_manager);
        for section in DamageSection::ALL {
            assert_eq!(
                section.name(),
                section.handle(&name_manager).get(&name_manager)
            );
            assert_eq!(
                NameFlags::NONE,
                name_manager.info(section.handle(&name_manager)).flags
            );
        }
        assert_eq!(
            NORMAL_WEAPON_FIRE,
            normal_weapon_fire(&name_manager).get(&name_manager)
        );
    }
}
