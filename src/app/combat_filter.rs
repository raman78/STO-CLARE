//! Filtering a combats list by what the analyzer worked out about each fight:
//! its environment, its difficulty, which map it was and how it went for the
//! players in it.
//!
//! Shared by the combat picker on the main screen and the selection list in the
//! compare view, so the two offer the same choices and mean the same thing by
//! them.

use eframe::egui::*;

use std::collections::BTreeSet;

use crate::{
    analyzer::{CombatSummary, Difficulty, PlayerSummary},
    app::tuning::{DEATHS_MENU_HEIGHT, DEATHS_MENU_WIDTH, PICKER_MIN_WIDTH},
    custom_widgets::tooltip::CloseTooltip,
};

/// The difficulty picker. `Any` matches everything; `Unknown` catches combats
/// whose tier could not be worked out, which would otherwise be invisible under
/// every other setting.
#[derive(PartialEq, Eq, Clone, Copy, Default)]
pub enum DifficultyFilter {
    #[default]
    Any,
    Normal,
    Advanced,
    Elite,
    Unknown,
}

impl DifficultyFilter {
    pub const ALL: &'static [(DifficultyFilter, &'static str)] = &[
        (DifficultyFilter::Any, "Any"),
        (DifficultyFilter::Normal, "Normal"),
        (DifficultyFilter::Advanced, "Advanced"),
        (DifficultyFilter::Elite, "Elite"),
        (DifficultyFilter::Unknown, "Unknown"),
    ];

    pub fn matches(self, difficulty: Option<Difficulty>) -> bool {
        match self {
            DifficultyFilter::Any => true,
            DifficultyFilter::Normal => difficulty == Some(Difficulty::Normal),
            DifficultyFilter::Advanced => difficulty == Some(Difficulty::Advanced),
            DifficultyFilter::Elite => difficulty == Some(Difficulty::Elite),
            // `Difficulty::Any` means a known map whose tier was not resolved,
            // so it reads as unknown here just like a missing value.
            DifficultyFilter::Unknown => {
                difficulty.is_none() || difficulty == Some(Difficulty::Any)
            }
        }
    }
}

/// Which way the deaths menu reads.
///
/// The same list of handles answers two opposite questions, and which one is
/// being asked follows what the list is *for*: browsing a log, the interesting
/// runs are the clean ones; clearing a log, the interesting runs are the ones
/// worth deleting, which is where somebody died. See
/// [`crate::app::combats_list::CombatsPanel`], which flips it with the mode.
///
/// The two are complements, and that reaches the quantifier as well as the
/// death count: **every** ticked player has to have come through a fight for it
/// to read as clean, and **any** of them dying is enough to make it one that did
/// not go well. So a group's runs split in two — the ones that went perfectly
/// for all of them, and the ones somebody has something to say about.
#[derive(PartialEq, Eq, Clone, Copy, Default, Debug)]
pub enum DeathsFilter {
    /// Fights **every** ticked player came through with 0 deaths.
    #[default]
    Without,
    /// Fights **any** ticked player died in.
    With,
}

impl DeathsFilter {
    /// Whether a death count is one this direction is looking for.
    fn wants(self, deaths: u32) -> bool {
        match self {
            DeathsFilter::Without => deaths == 0,
            DeathsFilter::With => deaths > 0,
        }
    }

    /// Whether this player answers the question in this fight.
    ///
    /// Being in the fight is part of it either way: the menu asks how *their*
    /// runs went, and a run they sat out is not an answer to it — neither one
    /// they came through nor one they died in. Under [`Self::With`] that is
    /// also what stops a fight being lined up for deletion because a ticked
    /// player was **absent** from it.
    fn matches(self, players: &[PlayerSummary], handle: &str) -> bool {
        players
            .iter()
            .any(|player| player.handle == handle && self.wants(player.deaths))
    }

    /// Whether a fight answers the ticked handles taken together.
    ///
    /// An empty set asks nothing, which has to be said out loud: `all` over
    /// nothing is true and `any` over nothing is false, so the two directions
    /// would disagree about a menu with no ticks in it — and the second would
    /// hide every fight in the log.
    fn matches_all(self, players: &[PlayerSummary], handles: &BTreeSet<String>) -> bool {
        if handles.is_empty() {
            return true;
        }
        match self {
            DeathsFilter::Without => handles.iter().all(|h| self.matches(players, h)),
            DeathsFilter::With => handles.iter().any(|h| self.matches(players, h)),
        }
    }

    /// The word for how the ticks add up, for the menu to say.
    fn quantifier(self) -> &'static str {
        match self {
            DeathsFilter::Without => "all",
            DeathsFilter::With => "any of",
        }
    }
}

/// What a combat has to match to stay in the list. Every part defaults to "all".
#[derive(Default, Clone, PartialEq, Eq)]
pub struct CombatFilter {
    /// `Some(true)` keeps the fights fought alone, `Some(false)` the rest. The
    /// test is the ladder's — one player in the log — so the two agree about
    /// which runs are which. See [`crate::analyzer::Combat::is_solo`].
    pub solo: Option<bool>,
    /// "Space", "Ground", … — the curated environment of the detected map.
    /// `None` means any.
    pub environment: Option<String>,
    pub difficulty: DifficultyFilter,
    /// The combat's base name, i.e. which map it was. `None` means any.
    pub map: Option<String>,
    /// The handles the deaths menu is about — nearly always the reader's own,
    /// alone. Empty means deaths are not asked about at all.
    ///
    /// How several of them add up is [`DeathsFilter`]'s to say, and it is not
    /// the same both ways round.
    pub deaths_of: BTreeSet<String>,
    /// Which of the two questions the handles above are being asked.
    pub deaths: DeathsFilter,
}

impl CombatFilter {
    pub fn is_active(&self) -> bool {
        self.environment.is_some()
            || self.map.is_some()
            || self.solo.is_some()
            || !self.deaths_of.is_empty()
            || self.difficulty != DifficultyFilter::Any
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn matches(&self, combat: &CombatEntry) -> bool {
        if let Some(wanted) = self.solo
            && wanted != combat.solo
        {
            return false;
        }
        if let Some(wanted) = &self.environment
            && combat.environment != Some(wanted.as_str())
        {
            return false;
        }
        if let Some(wanted) = &self.map
            && combat.base_name != wanted
        {
            return false;
        }
        if !self.deaths.matches_all(combat.players, &self.deaths_of) {
            return false;
        }
        self.difficulty.matches(combat.difficulty)
    }

    /// The values a menu may offer: those present among the combats that pass
    /// the *other* two filters. Picking from a menu can then never produce an
    /// empty list, because every option still has at least one combat behind it.
    fn options(&self, combats: &[CombatEntry], dimension: Dimension) -> Options {
        let mut without = self.clone();
        match dimension {
            Dimension::Environment => without.environment = None,
            Dimension::Difficulty => without.difficulty = DifficultyFilter::Any,
            Dimension::Map => without.map = None,
            Dimension::Solo => without.solo = None,
            Dimension::Deaths => without.deaths_of.clear(),
        }
        let matching = combats.iter().filter(|c| without.matches(c));

        let mut options = Options::default();
        for combat in matching {
            if let Some(environment) = combat.environment {
                options.environments.push(environment.to_string());
            }
            options.maps.push(combat.base_name.to_string());
            options.difficulties.push(combat.difficulty);
            options.solos.push(combat.solo);
            // Once per fight a handle answers the deaths question in, so how
            // often it appears here is how many fights it would leave on
            // screen — and a handle that answers none is not offered at all.
            for player in combat
                .players
                .iter()
                .filter(|p| self.deaths.wants(p.deaths))
            {
                options.handles.push(player.handle.clone());
            }
        }
        options.environments.sort_unstable();
        options.environments.dedup();
        options.maps.sort_unstable();
        options.maps.dedup();
        options
    }

    /// Drops a choice that the other filters have made impossible, so the list
    /// can never end up empty through a combination nothing matches.
    fn drop_impossible_choices(&mut self, combats: &[CombatEntry]) {
        if let Some(solo) = self.solo
            && !self
                .options(combats, Dimension::Solo)
                .solos
                .iter()
                .any(|s| *s == solo)
        {
            self.solo = None;
        }
        if let Some(environment) = &self.environment
            && !self
                .options(combats, Dimension::Environment)
                .environments
                .iter()
                .any(|e| e == environment)
        {
            self.environment = None;
        }
        if let Some(map) = &self.map
            && !self
                .options(combats, Dimension::Map)
                .maps
                .iter()
                .any(|m| m == map)
        {
            self.map = None;
        }
        if !self.deaths_of.is_empty() {
            // Only a handle that answers nothing at all any more is given up —
            // which is also what turns the menu round when the panel starts
            // clearing the log: a player who never died has nothing to offer
            // the other question. Under `Without`, a tick that empties the list
            // *together with* another tick is left alone: the reader made that
            // pair deliberately, and unticking one of them behind their back
            // would be the list answering a question they did not ask. Under
            // `With` the case cannot arise — the ticks are `or`ed, so every one
            // of them that is offered brings its own fights with it.
            let handles = self.options(combats, Dimension::Deaths).handles;
            self.deaths_of
                .retain(|handle| handles.iter().any(|h| h == handle));
        }
        if self.difficulty != DifficultyFilter::Any
            && !self
                .options(combats, Dimension::Difficulty)
                .difficulties
                .iter()
                .any(|d| self.difficulty.matches(*d))
        {
            self.difficulty = DifficultyFilter::Any;
        }
    }

    /// Draws the pickers inline. Each menu offers only what the others leave
    /// reachable, so no combination can empty the list.
    pub fn show(&mut self, id: &str, combats: &[CombatEntry], ui: &mut Ui) {
        let debug = std::env::var("CLA_PANEL_DEBUG").is_ok();
        if debug {
            println!(
                "        filter start {} max {}",
                ui.min_rect().width(),
                ui.max_rect().width()
            );
        }
        self.drop_impossible_choices(combats);
        let mut solos = self.options(combats, Dimension::Solo).solos;
        solos.sort_unstable();
        solos.dedup();
        let environments = self.options(combats, Dimension::Environment).environments;
        let maps = self.options(combats, Dimension::Map).maps;
        let difficulties = self.options(combats, Dimension::Difficulty).difficulties;
        let candidates = by_fights_matched(self.options(combats, Dimension::Deaths).handles);
        // Greyed out where the list holds one kind only — with everything solo,
        // or everything not, the menu would be a choice between one thing and
        // the same thing. Drawn either way: a picker that comes and goes moves
        // every picker beside it, and this row is read by where things are.
        ui.add_enabled_ui(solos.len() > 1, |ui| {
            ComboBox::new((id, "solo"), "")
                .selected_text(match self.solo {
                    Some(true) => "Solo",
                    Some(false) => "Team",
                    None => "Any size",
                })
                .width(fitting(ui, 90.0))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.solo, None, "Any size");
                    ui.selectable_value(&mut self.solo, Some(true), "Solo");
                    ui.selectable_value(&mut self.solo, Some(false), "Team");
                })
                .response
                .disabled_hover("Every fight here was fought the same way.");
        });

        if debug {
            println!("        after solo {}", ui.min_rect().width());
        }
        ComboBox::new((id, "environment"), "")
            .selected_text(self.environment.as_deref().unwrap_or("Any type"))
            .width(fitting(ui, 90.0))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.environment, None, "Any type");
                for environment in environments {
                    ui.selectable_value(
                        &mut self.environment,
                        Some(environment.clone()),
                        environment,
                    );
                }
            });

        if debug {
            println!("        after env {}", ui.min_rect().width());
        }
        ComboBox::new((id, "difficulty"), "")
            .selected_text(match self.difficulty {
                DifficultyFilter::Any => "Any level",
                other => DifficultyFilter::ALL
                    .iter()
                    .find(|(f, _)| *f == other)
                    .map(|(_, label)| *label)
                    .unwrap_or("Any level"),
            })
            .width(fitting(ui, 100.0))
            .show_ui(ui, |ui| {
                for &(filter, label) in DifficultyFilter::ALL {
                    let reachable = filter == DifficultyFilter::Any
                        || difficulties.iter().any(|d| filter.matches(*d));
                    if !reachable {
                        continue;
                    }
                    let label = if filter == DifficultyFilter::Any {
                        "Any level"
                    } else {
                        label
                    };
                    ui.selectable_value(&mut self.difficulty, filter, label);
                }
            });

        if debug {
            println!("        after level {}", ui.min_rect().width());
        }
        ComboBox::new((id, "map"), "")
            .selected_text(self.map.as_deref().unwrap_or("Any map"))
            .width(fitting(ui, 220.0))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.map, None, "Any map");
                for map in maps {
                    ui.selectable_value(&mut self.map, Some(map.clone()), map);
                }
            });

        if debug {
            println!("        after map {}", ui.min_rect().width());
        }
        // Greyed out where nobody answers the question — an empty list is
        // nothing to open — but drawn all the same, so the row does not shuffle
        // as the other pickers narrow it.
        let anybody = !candidates.is_empty();
        ui.add_enabled_ui(anybody, |ui| {
            self.show_deaths(id, candidates, ui);
        });
    }

    /// The deaths menu: a list of the players on screen to tick, rather than one
    /// to pick from.
    ///
    /// A list because the question is often about more than one player — a
    /// team's clean runs — and because a tick box says "and this one too" where
    /// a drop-down says "instead of that one". It carries a search box, since a
    /// log of a year's play holds more handles than a menu can be read down, and
    /// the two buttons every other list of ticks in the program has.
    ///
    /// It opens under a line saying what the ticks do, because a column of
    /// handles on its own does not say it and the box that opened the menu is
    /// too narrow to.
    fn show_deaths(&mut self, id: &str, candidates: Vec<String>, ui: &mut Ui) {
        let search_id = Id::new((id, "deaths search"));
        let direction = self.deaths;
        // Where this menu's popup lives, written down so anything outside can
        // ask whether it is open. It cannot be worked out from here: a combo
        // box takes its id from the `Ui` it is drawn in, and this one is drawn
        // in a child of the filter row (see the greying-out above), whose id is
        // egui's to choose.
        // Worked out the way `ComboBox` works it out: the button's id is the
        // `Ui`'s own with the salt below mixed in, and the popup's is that with
        // "popup" on the end.
        let popup_id = ui
            .make_persistent_id(Id::new((id, DEATHS_SALT)))
            .with("popup");
        ui.data_mut(|data| data.insert_temp(deaths_popup_key(id), popup_id));

        ComboBox::new((id, DEATHS_SALT), "")
            .selected_text(deaths_text(self.deaths, &self.deaths_of))
            // The handles are as long as they are; the box says how many there
            // are rather than growing to hold them, and hovers to name them.
            .truncate()
            .width(fitting(ui, 170.0))
            .height(DEATHS_MENU_HEIGHT)
            // A tick is not a choice made and done with: the list stays up until
            // the reader clicks away from it.
            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
            .show_ui(ui, |ui| {
                ui.set_min_width(DEATHS_MENU_WIDTH);
                // Wrapped deliberately: a popup lays its contents out with
                // wrapping off, so a sentence would widen the whole menu to one
                // long line instead of folding inside it.
                ui.add(Label::new(RichText::new(deaths_prompt(self.deaths)).weak()).wrap());

                // Which handles are being searched for lives in egui's own
                // memory rather than in the filter: it narrows the *menu*, not
                // the combats, and a filter that changed as it was typed in
                // would have the list re-measured on every keystroke.
                let mut search: String =
                    ui.data(|data| data.get_temp(search_id).unwrap_or_default());
                TextEdit::singleline(&mut search)
                    .hint_text("search players")
                    .desired_width(DEATHS_MENU_WIDTH)
                    .show(ui);
                let needle = search.trim().to_lowercase();
                ui.data_mut(|data| data.insert_temp(search_id, search));

                let listed: Vec<&String> = candidates
                    .iter()
                    .filter(|handle| handle.to_lowercase().contains(&needle))
                    .collect();

                // Both buttons act on what the search has left on screen, like
                // every other pair in the program: what is out of sight is not
                // ticked or unticked behind the reader's back.
                ui.horizontal(|ui| {
                    if ui.button("Select all").clicked() {
                        self.deaths_of
                            .extend(listed.iter().map(|handle| (*handle).clone()));
                    }
                    if ui.button("Unselect all").clicked() {
                        self.deaths_of.retain(|handle| !listed.contains(&handle));
                    }
                });
                ui.separator();

                for handle in listed {
                    let mut ticked = self.deaths_of.contains(handle);
                    if ui.checkbox(&mut ticked, handle).changed() {
                        if ticked {
                            self.deaths_of.insert(handle.clone());
                        } else {
                            self.deaths_of.remove(handle);
                        }
                    }
                }
            })
            .response
            .hover(deaths_hover(self.deaths, &self.deaths_of))
            .disabled_hover(match direction {
                DeathsFilter::Without => "Somebody died in every fight on screen.",
                DeathsFilter::With => "Nobody died in any of the fights on screen.",
            });
    }
}

/// What the deaths menu's own id is made of, shared by the menu and by the
/// note of where its popup is.
const DEATHS_SALT: &str = "no death";

/// Where [`CombatFilter::show_deaths`] writes down the id of its popup.
pub fn deaths_popup_key(id: &str) -> Id {
    Id::new((id, "deaths popup"))
}

/// What the deaths menu says the ticks are for, over its list of players.
///
/// "Show", not "keep": this menu narrows the list, and what happens to a fight
/// afterwards is the reader's next move, not the menu's. It matters most in the
/// turned-round direction, where the fights on screen are the ones about to be
/// deleted — a line starting "keep" would name the opposite set from the one
/// under it.
fn deaths_prompt(direction: DeathsFilter) -> &'static str {
    match direction {
        DeathsFilter::Without => "Show fights where every ticked player has 0 deaths",
        DeathsFilter::With => "Show fights where any ticked player died — the ones to delete",
    }
}

/// What the deaths box says it is doing. One handle is named; several are
/// counted, because a box that grows to hold them widens the whole panel.
fn deaths_text(direction: DeathsFilter, picked: &BTreeSet<String>) -> String {
    let what = match direction {
        DeathsFilter::Without => "No Deaths of",
        DeathsFilter::With => "Deaths of",
    };
    match picked.len() {
        // The two directions read differently even with nothing ticked. They
        // used to share a word, so turning the menu round changed nothing on
        // screen until a handle was picked — the one moment the reader most
        // needs to see that it now means the opposite.
        0 => match direction {
            DeathsFilter::Without => "Any deaths".to_owned(),
            DeathsFilter::With => "All fights".to_owned(),
        },
        1 => format!("{what}: {}", picked.iter().next().unwrap_or(&String::new())),
        // The word that says how they add up, since that is the one thing the
        // two directions do not share and a count alone would hide.
        count => format!("{what}: {} {count}", direction.quantifier()),
    }
}

/// What the deaths box says when it is hovered: what it means, and — where the
/// box itself only counts them — who is ticked.
fn deaths_hover(direction: DeathsFilter, picked: &BTreeSet<String>) -> String {
    let what = match direction {
        DeathsFilter::Without => {
            "Show only the fights every ticked player came through alive. A fight one of them \
             was not in is not one they survived, so it is left out."
        }
        DeathsFilter::With => {
            "Show the fights any ticked player died in — the ones to clear out of the log. One \
             death by one of them is enough; a fight none of them was in is nobody's bad run, \
             so it is left out."
        }
    };
    match picked.len() {
        0 | 1 => what.to_owned(),
        _ => format!(
            "{what}\n\n{}",
            picked.iter().cloned().collect::<Vec<_>>().join(", ")
        ),
    }
}

/// The handles a menu offers, by how many of the fights on screen each answers
/// for — most first.
///
/// Which puts the reader's own handle at the top of nearly every log, since a
/// combat log holds far more of their fights than of anyone else's, without the
/// filter having to be told whose log it is. Ties read alphabetically so the
/// menu does not shuffle between frames.
fn by_fights_matched(mut handles: Vec<String>) -> Vec<String> {
    handles.sort_unstable();
    let mut counted: Vec<(usize, String)> = Vec::new();
    for handle in handles {
        match counted.last_mut() {
            Some((count, last)) if *last == handle => *count += 1,
            _ => counted.push((1, handle)),
        }
    }
    counted.sort_by(|(a_count, a), (b_count, b)| b_count.cmp(a_count).then_with(|| a.cmp(b)));
    counted.into_iter().map(|(_, handle)| handle).collect()
}

/// How wide a picker may be drawn here: what it wants, or what is left of the
/// row it stands in, whichever is less.
///
/// A picker wider than the room it has does not wrap onto the next line the way
/// a label does — it draws past the edge, and everything that sizes itself to
/// what it holds (the combats panel, which is exactly as wide as its table)
/// grows to take it. So the pickers give way instead.
fn fitting(ui: &Ui, wanted: f32) -> f32 {
    wanted
        .min(ui.available_width())
        .min(ui.max_rect().width())
        .at_least(PICKER_MIN_WIDTH)
}

/// What the filter knows about one combat in the list.
#[derive(Clone, Copy)]
pub struct CombatEntry<'a> {
    /// Whether one player fought it.
    pub solo: bool,
    pub environment: Option<&'a str>,
    pub difficulty: Option<Difficulty>,
    pub base_name: &'a str,
    /// Everyone who fought it, which is what the deaths menu reads.
    pub players: &'a [PlayerSummary],
}

impl<'a> From<&'a CombatSummary> for CombatEntry<'a> {
    fn from(combat: &'a CombatSummary) -> Self {
        Self {
            solo: combat.solo,
            environment: combat.environment.as_deref(),
            difficulty: combat.difficulty,
            base_name: combat.base_name.as_str(),
            players: &combat.players,
        }
    }
}

#[derive(Clone, Copy)]
enum Dimension {
    Environment,
    Difficulty,
    Map,
    Solo,
    Deaths,
}

#[derive(Default)]
struct Options {
    solos: Vec<bool>,
    environments: Vec<String>,
    maps: Vec<String>,
    difficulties: Vec<Option<Difficulty>>,
    /// One entry per fight a handle answers the deaths question in, so the same
    /// handle is in here as many times as it has fights behind it.
    handles: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fight with nobody in it, which is all the tests about the other
    /// pickers need to say about its players.
    fn entry<'a>(
        environment: Option<&'a str>,
        difficulty: Option<Difficulty>,
        base_name: &'a str,
        solo: bool,
    ) -> CombatEntry<'a> {
        CombatEntry {
            environment,
            difficulty,
            base_name,
            solo,
            players: &[],
        }
    }

    #[test]
    fn an_empty_filter_matches_everything() {
        let filter = CombatFilter::default();
        assert!(!filter.is_active());
        assert!(filter.matches(&entry(
            Some("Space"),
            Some(Difficulty::Elite),
            "Infected Space",
            false
        )));
        assert!(filter.matches(&entry(None, None, "Combat", false)));
    }

    #[test]
    fn each_part_narrows_on_its_own() {
        let filter = CombatFilter {
            environment: Some("Ground".to_string()),
            ..Default::default()
        };
        assert!(filter.matches(&entry(Some("Ground"), None, "Bug Hunt", false)));
        assert!(!filter.matches(&entry(Some("Space"), None, "Bug Hunt", false)));
        // A combat whose map was never recognized has no environment at all.
        assert!(!filter.matches(&entry(None, None, "Combat", false)));

        let filter = CombatFilter {
            map: Some("Infected Space".to_string()),
            ..Default::default()
        };
        assert!(filter.matches(&entry(Some("Space"), None, "Infected Space", false)));
        assert!(!filter.matches(&entry(Some("Space"), None, "Hive Onslaught", false)));

        let filter = CombatFilter {
            difficulty: DifficultyFilter::Elite,
            ..Default::default()
        };
        assert!(filter.matches(&entry(None, Some(Difficulty::Elite), "x", false)));
        assert!(!filter.matches(&entry(None, Some(Difficulty::Normal), "x", false)));
    }

    fn entries() -> Vec<CombatEntry<'static>> {
        vec![
            CombatEntry {
                environment: Some("Space"),
                difficulty: Some(Difficulty::Elite),
                base_name: "Infected Space",
                solo: false,
                players: &[],
            },
            CombatEntry {
                environment: Some("Space"),
                difficulty: Some(Difficulty::Normal),
                base_name: "Azure Nebula",
                solo: false,
                players: &[],
            },
            CombatEntry {
                environment: Some("Ground"),
                difficulty: Some(Difficulty::Advanced),
                base_name: "Bug Hunt",
                solo: false,
                players: &[],
            },
            CombatEntry {
                environment: None,
                difficulty: None,
                base_name: "Combat",
                solo: false,
                players: &[],
            },
        ]
    }

    /// Choosing an environment must leave only the maps and levels that still
    /// have a combat behind them — otherwise the next pick empties the list.
    #[test]
    fn a_chosen_environment_narrows_the_other_menus() {
        let entries = entries();
        let filter = CombatFilter {
            environment: Some("Ground".to_string()),
            ..Default::default()
        };

        let maps = filter.options(&entries, Dimension::Map).maps;
        assert_eq!(vec!["Bug Hunt".to_string()], maps);

        let levels = filter.options(&entries, Dimension::Difficulty).difficulties;
        assert!(
            levels
                .iter()
                .any(|d| DifficultyFilter::Advanced.matches(*d))
        );
        assert!(!levels.iter().any(|d| DifficultyFilter::Elite.matches(*d)));
    }

    /// The menu being opened is not narrowed by its own current value, or it
    /// would offer nothing but what is already picked.
    #[test]
    fn a_menu_is_not_narrowed_by_its_own_choice() {
        let entries = entries();
        let filter = CombatFilter {
            environment: Some("Ground".to_string()),
            ..Default::default()
        };

        let environments = filter
            .options(&entries, Dimension::Environment)
            .environments;
        assert_eq!(
            vec!["Ground".to_string(), "Space".to_string()],
            environments
        );
    }

    /// The cascade stops a contradictory pair from being picked, but the list
    /// itself can change under a filter that is already set — a refresh, or a
    /// combat deleted. Whichever of the two is dropped then is arbitrary; what
    /// has to hold is that the list does not come back empty.
    #[test]
    fn an_impossible_pair_is_resolved_rather_than_left_empty() {
        let entries = entries();
        for (environment, map) in [("Ground", "Infected Space"), ("Space", "Bug Hunt")] {
            let mut filter = CombatFilter {
                environment: Some(environment.to_string()),
                map: Some(map.to_string()),
                ..Default::default()
            };
            assert!(
                !entries.iter().any(|c| filter.matches(c)),
                "the pair really is contradictory to begin with"
            );

            filter.drop_impossible_choices(&entries);

            assert!(
                entries.iter().any(|c| filter.matches(c)),
                "after resolving it, something matches again"
            );
            assert!(
                filter.environment.is_some() || filter.map.is_some(),
                "only the conflicting half is given up, not both"
            );
        }
    }

    fn picked(handles: &[&str]) -> BTreeSet<String> {
        handles.iter().map(|h| (*h).to_string()).collect()
    }

    fn player(handle: &str, deaths: u32) -> PlayerSummary {
        PlayerSummary {
            handle: handle.to_owned(),
            dps: 0.0,
            deaths,
        }
    }

    /// A fight of these players, with everything else the same — the deaths
    /// picker is the only thing these tests are asking about.
    fn fought_by(players: &[PlayerSummary]) -> CombatEntry<'_> {
        CombatEntry {
            environment: Some("Space"),
            difficulty: Some(Difficulty::Elite),
            base_name: "Infected Space",
            solo: false,
            players,
        }
    }

    #[test]
    fn the_deaths_picker_keeps_the_fights_that_player_came_through() {
        let came_through = [player("@me", 0), player("@friend", 2)];
        let died = [player("@me", 1), player("@friend", 0)];
        let without_me = [player("@friend", 0)];

        let filter = CombatFilter {
            deaths_of: picked(&["@me"]),
            ..Default::default()
        };
        assert!(filter.is_active());
        assert!(filter.matches(&fought_by(&came_through)));
        assert!(
            !filter.matches(&fought_by(&died)),
            "one death is enough to drop the fight, whoever else survived it"
        );
        assert!(
            !filter.matches(&fought_by(&without_me)),
            "a fight they were not in is not one they came through"
        );
    }

    /// The menu leads with the handle that has the most fights behind it, which
    /// is what puts the reader's own at the top without being told whose log it
    /// is. Handles level with each other read alphabetically.
    #[test]
    fn the_deaths_menu_leads_with_the_handle_in_the_most_fights() {
        let order = by_fights_matched(vec![
            "@friend".to_string(),
            "@me".to_string(),
            "@me".to_string(),
            "@ally".to_string(),
        ]);
        assert_eq!(vec!["@me", "@ally", "@friend"], order);
    }

    /// The deaths menu is narrowed by the other pickers like every other menu,
    /// and a handle they have made unreachable is given up rather than left
    /// showing nothing.
    #[test]
    fn the_deaths_picker_is_part_of_the_cascade() {
        let mine = [player("@me", 0)];
        let theirs = [player("@friend", 0)];
        let entries = vec![
            CombatEntry {
                environment: Some("Ground"),
                difficulty: None,
                base_name: "Bug Hunt",
                solo: false,
                players: &mine,
            },
            CombatEntry {
                environment: Some("Space"),
                difficulty: None,
                base_name: "Infected Space",
                solo: false,
                players: &theirs,
            },
        ];

        let ground = CombatFilter {
            environment: Some("Ground".to_string()),
            ..Default::default()
        };
        assert_eq!(
            vec!["@me".to_string()],
            by_fights_matched(ground.options(&entries, Dimension::Deaths).handles),
            "only the handles the other pickers still leave reachable"
        );

        let mut filter = CombatFilter {
            environment: Some("Space".to_string()),
            deaths_of: picked(&["@me"]),
            ..Default::default()
        };
        assert!(!entries.iter().any(|c| filter.matches(c)));
        filter.drop_impossible_choices(&entries);
        assert!(
            entries.iter().any(|c| filter.matches(c)),
            "after resolving it, something matches again"
        );
    }

    /// Several ticks ask for the run *all* of them came through, not any of
    /// them: a list of handles under "no deaths of" is one question about one
    /// clean run, which is what a team looks for after a wipe.
    #[test]
    fn every_ticked_player_has_to_have_come_through_it() {
        let both = [player("@me", 0), player("@friend", 0)];
        let only_me = [player("@me", 0), player("@friend", 1)];

        let filter = CombatFilter {
            deaths_of: picked(&["@me", "@friend"]),
            ..Default::default()
        };
        assert!(filter.matches(&fought_by(&both)));
        assert!(
            !filter.matches(&fought_by(&only_me)),
            "one of the two died, so it is not a run they both came through"
        );
    }

    /// Turned round, the same ticks pick out the fights to delete: every ticked
    /// player died in it. Presence still counts — a run they were not in is
    /// neither one they came through nor one they died in, and deleting it
    /// because they were absent would throw away somebody else's fight.
    #[test]
    fn the_menu_turned_round_keeps_the_fights_they_died_in() {
        let died = [player("@me", 1), player("@friend", 0)];
        let came_through = [player("@me", 0), player("@friend", 2)];
        let without_me = [player("@friend", 3)];

        let filter = CombatFilter {
            deaths_of: picked(&["@me"]),
            deaths: DeathsFilter::With,
            ..Default::default()
        };
        assert!(filter.matches(&fought_by(&died)));
        assert!(!filter.matches(&fought_by(&came_through)));
        assert!(!filter.matches(&fought_by(&without_me)));

        // Several ticks are `or`ed this way round, where the other way round
        // they are `and`ed: one death by one of them is a run that did not go
        // well, and that is what is being cleared out.
        let both_died = [player("@me", 1), player("@friend", 1)];
        let neither = [player("@me", 0), player("@friend", 0)];
        let filter = CombatFilter {
            deaths_of: picked(&["@me", "@friend"]),
            deaths: DeathsFilter::With,
            ..Default::default()
        };
        assert!(filter.matches(&fought_by(&both_died)));
        assert!(
            filter.matches(&fought_by(&died)),
            "@me died in it, which is enough"
        );
        assert!(
            !filter.matches(&fought_by(&neither)),
            "a run that went fine"
        );
        assert!(
            !filter.matches(&fought_by(&[player("@stranger", 4)])),
            "somebody else's bad run is not one of theirs"
        );
    }

    /// The two directions are complements, quantifier and all: a fight either
    /// went perfectly for everyone ticked, or somebody has something to say
    /// about it. Nothing falls between them where all of them were there.
    #[test]
    fn the_two_directions_split_the_fights_between_them() {
        let fights = [
            vec![player("@me", 0), player("@friend", 0)],
            vec![player("@me", 1), player("@friend", 0)],
            vec![player("@me", 0), player("@friend", 2)],
            vec![player("@me", 3), player("@friend", 1)],
        ];
        let clean = CombatFilter {
            deaths_of: picked(&["@me", "@friend"]),
            ..Default::default()
        };
        let messy = CombatFilter {
            deaths: DeathsFilter::With,
            ..clean.clone()
        };

        for players in &fights {
            let fight = fought_by(players);
            assert_ne!(
                clean.matches(&fight),
                messy.matches(&fight),
                "every fight is on exactly one side of the two"
            );
        }
    }

    /// `all` over nothing is true and `any` over nothing is false, so a menu
    /// with no ticks in it had to be said out loud: turned round, it would
    /// otherwise hide the whole log the moment the reader pressed Clear Log
    /// File.
    #[test]
    fn an_empty_menu_asks_nothing_either_way_round() {
        let players = [player("@me", 0), player("@friend", 1)];
        for deaths in [DeathsFilter::Without, DeathsFilter::With] {
            let filter = CombatFilter {
                deaths,
                ..Default::default()
            };
            assert!(!filter.is_active());
            assert!(filter.matches(&fought_by(&players)), "{deaths:?}");
        }
    }

    /// `or`ed ticks cannot come back empty: every handle the menu offers has at
    /// least one fight behind it (the cascade sees to that), and the fights
    /// shown are the union of theirs. It is the `and`ed direction that can be
    /// narrowed down to nothing, deliberately.
    #[test]
    fn the_turned_round_menu_cannot_empty_the_list() {
        let mine = [player("@me", 2), player("@friend", 0)];
        let theirs = [player("@me", 0), player("@friend", 3)];
        let entries = [fought_by(&mine), fought_by(&theirs)];

        let both = CombatFilter {
            deaths_of: picked(&["@me", "@friend"]),
            deaths: DeathsFilter::With,
            ..Default::default()
        };
        assert_eq!(
            2,
            entries.iter().filter(|c| both.matches(c)).count(),
            "each tick brings its own fights rather than taking the other's away"
        );

        // The same pair the other way round has nothing to show, which is the
        // asymmetry these two tests are about.
        let clean = CombatFilter {
            deaths: DeathsFilter::Without,
            ..both
        };
        assert_eq!(0, entries.iter().filter(|c| clean.matches(c)).count());
    }

    /// The menu offers the handles that answer the question it is *currently*
    /// asking: turned round for clearing the log, a player who never died has
    /// nothing to offer and their tick is given up rather than left holding an
    /// empty list.
    #[test]
    fn turning_the_menu_round_re_reads_who_it_can_offer() {
        let players = [player("@me", 0), player("@friend", 2)];
        let entries = vec![fought_by(&players)];

        let browsing = CombatFilter::default();
        assert_eq!(
            vec!["@me".to_string()],
            by_fights_matched(browsing.options(&entries, Dimension::Deaths).handles)
        );

        let mut clearing = CombatFilter {
            deaths_of: picked(&["@me"]),
            deaths: DeathsFilter::With,
            ..Default::default()
        };
        assert_eq!(
            vec!["@friend".to_string()],
            by_fights_matched(clearing.options(&entries, Dimension::Deaths).handles)
        );
        assert!(!entries.iter().any(|c| clearing.matches(c)));
        clearing.drop_impossible_choices(&entries);
        assert!(
            clearing.deaths_of.is_empty(),
            "@me never died, so the tick has nothing left to mean"
        );
        assert!(entries.iter().any(|c| clearing.matches(c)));
    }

    /// The box says who is ticked while it can, and counts them once it cannot
    /// — a box that grew to hold four handles would widen the whole panel.
    #[test]
    fn the_deaths_box_names_one_player_and_counts_more() {
        let without = DeathsFilter::Without;
        assert_eq!("Any deaths", deaths_text(without, &picked(&[])));
        assert_eq!("No Deaths of: @me", deaths_text(without, &picked(&["@me"])));
        // Past one name the box says how they add up, because that is what the
        // two directions no longer share.
        assert_eq!(
            "No Deaths of: all 2",
            deaths_text(without, &picked(&["@me", "@friend"]))
        );
        assert_eq!(
            "Deaths of: any of 2",
            deaths_text(DeathsFilter::With, &picked(&["@me", "@friend"]))
        );
        // Turned round for clearing the log, the same ticks read the other way
        // — including with nothing ticked, which is the moment the reader has
        // to be able to see that the menu now means the opposite.
        assert_eq!(
            "Deaths of: @me",
            deaths_text(DeathsFilter::With, &picked(&["@me"]))
        );
        assert_ne!(
            deaths_text(without, &picked(&[])),
            deaths_text(DeathsFilter::With, &picked(&[])),
            "an empty menu says which way round it is"
        );
        assert_eq!("All fights", deaths_text(DeathsFilter::With, &picked(&[])));
        // What the box no longer has room to say is one hover away.
        assert!(deaths_hover(without, &picked(&["@me", "@friend"])).contains("@friend, @me"));
    }

    /// The deaths menu is the one picker here that is not a column of
    /// `selectable_value`s — a search box, two buttons and a tick per player,
    /// in a popup that only exists while it is open. Drawn once open, at the
    /// width it asks for.
    #[test]
    fn the_deaths_menu_draws_its_list_when_it_is_opened() {
        let players = [player("@me", 0), player("@friend", 1)];
        let entries = vec![fought_by(&players)];
        let mut filter = CombatFilter::default();
        let ctx = Context::default();
        crate::app::theme::apply(&ctx, crate::app::theme::Theme::Dark);

        let _ = ctx.run_ui(RawInput::default(), |ui| {
            filter.show("filters", &entries, ui);
        });
        // Where the menu says its popup is, rather than a guess at it: the box
        // is drawn in a child `Ui` whose id egui chooses.
        let popup: Id = ctx
            .data(|data| data.get_temp(deaths_popup_key("filters")))
            .expect("the deaths menu was drawn");
        assert!(
            AreaState::load(&ctx, popup).is_none(),
            "the menu is closed until it is asked for"
        );

        Popup::open_id(&ctx, popup);
        let _ = ctx.run_ui(RawInput::default(), |ui| {
            filter.show("filters", &entries, ui);
        });

        let menu = AreaState::load(&ctx, popup)
            .expect("the menu is open")
            .rect();
        assert!(
            menu.width() >= DEATHS_MENU_WIDTH,
            "the menu is at least as wide as its rows ask for, not as wide as \
             the box that opened it: {menu:?}"
        );
        assert!(menu.height() > 0.0 && menu.height() <= DEATHS_MENU_HEIGHT + menu.width());
    }
}
