//! The keyboard shortcuts the program answers.
//!
//! Five things a reader does over and over — the overlay, the ladder, the
//! settings, the grouping rules, a comparison — each on one key, and the
//! overlay's key taken from the whole desktop so it works while the game has
//! the screen.
//!
//! **Escape and Tab are deliberately not in here.** Those two are structural:
//! Escape belongs to whichever dialog is open and is answered in one place
//! (`custom_widgets::dialog::escape_closes`), and Tab has to be stripped from
//! the frame before egui walks the focus with it (`App::raw_input_hook`).
//! Neither is a preference, and putting them in a table the reader can edit
//! would offer to break the window.
//!
//! ## Two sources, one table
//!
//! A shortcut arrives either from the window's own keyboard — egui's event
//! stream, which only carries keys while the window has focus — or from the
//! system-wide grab in [`global`], which carries the one key wherever it was
//! pressed. Both end up as a [`ShortcutAction`], and `App::ui` acts on them in
//! one place.
//!
//! The in-window half of the *global* action is skipped while the grab holds
//! it (see [`Shortcuts::triggered`]): a passive X11 grab takes the key away
//! from the focused window, so our own window never sees it — measured on
//! KWin/Wayland, where the key reached the grab both with the game in front and
//! with a native Wayland window in front.
//!
//! ## What is stored
//!
//! [`ShortcutSettings`] keeps **only what the reader changed**. An action left
//! alone is not in the file at all, so a shortcut added in a later version
//! arrives with its own default rather than missing from a file written before
//! it existed — the same rule the hidden-columns setting follows.

use std::{collections::BTreeMap, fmt, str::FromStr};

use eframe::egui::{Context, Event, Key, Modifiers};
use serde::{Deserialize, Serialize};

pub mod global;

use global::GlobalHotkey;
pub use global::GlobalState;

/// What a shortcut asks the program to do.
///
/// Named in the settings file by [`Self::key`], so a name may be **added but
/// never changed** — the same rule the theme follows.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum ShortcutAction {
    /// Show or hide the overlay. The one action that can also be taken
    /// system-wide, because it is the one pressed while the game is in front.
    ToggleOverlay,
    /// Open or close the Ladder window.
    ToggleLadder,
    /// Open the Settings window.
    OpenSettings,
    /// Open Settings on the Analysis tab, where the grouping rules are.
    OpenGrouping,
    /// Put the ticked fights side by side, or leave the comparison.
    ToggleCompare,
}

impl ShortcutAction {
    /// Every action, in the order the settings tab lists them.
    pub const ALL: [Self; 5] = [
        Self::ToggleOverlay,
        Self::ToggleLadder,
        Self::OpenSettings,
        Self::OpenGrouping,
        Self::ToggleCompare,
    ];

    /// What the settings file calls it. Written, and read back by
    /// [`Self::from_key`], so it may be added to but never changed.
    pub fn key(self) -> &'static str {
        match self {
            Self::ToggleOverlay => "ToggleOverlay",
            Self::ToggleLadder => "ToggleLadder",
            Self::OpenSettings => "OpenSettings",
            Self::OpenGrouping => "OpenGrouping",
            Self::ToggleCompare => "ToggleCompare",
        }
    }

    /// The action a settings file names, where this build has it. A name it
    /// does not know is not an error — it is a shortcut from a later version,
    /// and it is kept exactly as written (see [`ShortcutSettings`]).
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.key() == key)
    }

    /// What the settings tab calls it — the name of the thing it operates,
    /// as the toolbar spells it.
    pub fn label(self) -> &'static str {
        match self {
            Self::ToggleOverlay => "Overlay",
            Self::ToggleLadder => "Ladder",
            Self::OpenSettings => "Settings",
            Self::OpenGrouping => "Custom grouping",
            Self::ToggleCompare => "Compare combats",
        }
    }

    /// What pressing it does, for the tooltip in the settings tab.
    pub fn hint(self) -> &'static str {
        match self {
            Self::ToggleOverlay => {
                "Show or hide the overlay in front of the game. Marked Global, the key \
                 works while the game has the screen rather than only in this window."
            }
            Self::ToggleLadder => "Open or close the window that reads the OSCR ladder.",
            Self::OpenSettings => "Open this window.",
            Self::OpenGrouping => {
                "Open this window on the Analysis tab, where the custom grouping rules are."
            }
            Self::ToggleCompare => "Put the fights ticked in the list side by side, or leave it.",
        }
    }

    /// The combination the program ships with.
    pub fn default_combination(self) -> Combination {
        let key = match self {
            Self::ToggleOverlay => Key::O,
            Self::ToggleLadder => Key::L,
            Self::OpenSettings => Key::S,
            Self::OpenGrouping => Key::G,
            Self::ToggleCompare => Key::C,
        };
        Combination {
            modifiers: Modifiers::ALT,
            key,
        }
    }

    /// Whether this action can also be taken from the whole desktop.
    ///
    /// Only the overlay is: it is the one thing a player reaches for while the
    /// game is in front, and every key taken system-wide is a key the game and
    /// every other program can no longer use.
    pub fn can_be_system_wide(self) -> bool {
        matches!(self, Self::ToggleOverlay)
    }
}

/// A key with the modifiers held down with it.
///
/// Written to the settings file as the text the tab shows — `Alt+O` — rather
/// than as a struct of five booleans: this is a file a player opens, and a
/// shortcut is one of the few settings in it that can be read and corrected by
/// hand.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Combination {
    pub modifiers: Modifiers,
    pub key: Key,
}

impl Combination {
    /// Whether this is a combination we are prepared to take.
    ///
    /// Two limits, and both are about what the rest of the program can do with
    /// it rather than about taste:
    ///
    /// - **At least one modifier.** A bare letter is what a reader types into
    ///   the note field and the rule patterns.
    /// - **A letter, a digit or a function key.** Those are the keys whose
    ///   place on the keyboard does not move between layouts, which is what the
    ///   system-wide grab has to name them by (see [`global`]).
    pub fn is_supported(&self) -> bool {
        self.modifiers.any() && supported_key(self.key)
    }
}

/// Whether a key is one the settings tab accepts and the grab can name.
///
/// Asked of the key's own name rather than of a list of variants, so a key egui
/// adds later is judged by the same rule instead of quietly falling outside a
/// list written today. A letter and a digit are one character (`A`, `7`); a
/// function key is `F` and a number.
fn supported_key(key: Key) -> bool {
    let name = key.name();
    let mut characters = name.chars();
    match (characters.next(), characters.next()) {
        (Some(single), None) => single.is_ascii_uppercase() || single.is_ascii_digit(),
        (Some('F'), Some(_)) => name[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)),
        _ => false,
    }
}

impl fmt::Display for Combination {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // One order, always, so the same combination is one string — the file
        // is compared against what the tab would write, and two spellings of
        // the same keys would read as a change nobody made.
        if self.modifiers.ctrl || self.modifiers.command {
            write!(f, "Ctrl+")?;
        }
        if self.modifiers.alt {
            write!(f, "Alt+")?;
        }
        if self.modifiers.shift {
            write!(f, "Shift+")?;
        }
        if self.modifiers.mac_cmd {
            write!(f, "Cmd+")?;
        }
        f.write_str(self.key.name())
    }
}

impl FromStr for Combination {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let mut modifiers = Modifiers::NONE;
        let mut key = None;
        for part in text.split('+').map(str::trim).filter(|p| !p.is_empty()) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => modifiers.ctrl = true,
                "alt" | "option" => modifiers.alt = true,
                "shift" => modifiers.shift = true,
                "cmd" | "command" => modifiers.mac_cmd = true,
                _ => {
                    if key.is_some() {
                        return Err(format!("{text}: more than one key in it"));
                    }
                    key = Some(
                        Key::from_name(part)
                            .ok_or_else(|| format!("{text}: {part} is not a key we know"))?,
                    );
                }
            }
        }
        let key = key.ok_or_else(|| format!("{text}: no key in it, only modifiers"))?;
        let combination = Self { modifiers, key };
        match combination.is_supported() {
            true => Ok(combination),
            false => Err(format!(
                "{text}: a shortcut needs a modifier and a letter, a digit or a function key"
            )),
        }
    }
}

/// The shortcut settings: what the reader changed, and whether the overlay's
/// key is taken from the whole desktop.
///
/// Its own section rather than part of `general`, for the reason every other
/// section has one: the settings dialog compares sections to decide what a
/// change costs, and rebinding a key is no reason to read the log again.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShortcutSettings {
    /// Only the actions whose combination the reader changed, as **text on both
    /// sides** — the action's name and the combination — rather than as types
    /// serde would refuse a file over.
    ///
    /// Two things that would otherwise be lost:
    ///
    /// - A combination that cannot be understood costs that one shortcut and is
    ///   reported, instead of failing the whole settings file — which is read
    ///   as a damaged installation and puts every other setting back to its
    ///   default.
    /// - A shortcut from a **later** version, whose action this build has never
    ///   heard of, is kept and written back untouched. Running an older build
    ///   for an evening is then not a way to lose the keys you set in the newer
    ///   one.
    #[serde(default)]
    custom: BTreeMap<String, String>,
    /// Whether the overlay's shortcut is also taken from the whole desktop.
    #[serde(default = "yes")]
    pub system_wide: bool,
}

fn yes() -> bool {
    true
}

/// What the settings file holds for a shortcut the reader cleared.
///
/// A third spelling is needed because the other two are spoken for: **no entry
/// at all** means the shipped combination, and any readable combination means
/// itself. Without it a cleared row could not be written down, and would come
/// back on its default the next time the program started.
///
/// Not the empty string: unreadable text already means "the file is damaged
/// here, the default is in force", and a cleared row would then come back with
/// a warning beside it. `Off` is not the name of any key egui knows, so it
/// cannot collide with a combination.
const CLEARED: &str = "Off";

impl Default for ShortcutSettings {
    fn default() -> Self {
        Self {
            custom: BTreeMap::new(),
            system_wide: true,
        }
    }
}

impl ShortcutSettings {
    /// What an action answers to, or why the text in the file could not be
    /// read. The default is what stands when nothing was written for it, and
    /// `None` is a row the reader cleared: it answers no key at all.
    pub fn combination(&self, action: ShortcutAction) -> Result<Option<Combination>, String> {
        match self.custom.get(action.key()) {
            None => Ok(Some(action.default_combination())),
            Some(text) if text == CLEARED => Ok(None),
            Some(text) => text.parse().map(Some),
        }
    }

    /// What an action answers to, falling back to the default where the file
    /// holds something unreadable — which is what the program has to act on,
    /// the tab being the place that says so. `None` is a cleared row.
    pub fn effective(&self, action: ShortcutAction) -> Option<Combination> {
        self.combination(action)
            .unwrap_or_else(|_| Some(action.default_combination()))
    }

    /// Records a combination, or forgets it again when it is the default:
    /// the file holds changes, not a copy of the shipped table.
    pub fn set(&mut self, action: ShortcutAction, combination: Combination) {
        if combination == action.default_combination() {
            self.custom.remove(action.key());
        } else {
            self.custom
                .insert(action.key().to_owned(), combination.to_string());
        }
    }

    /// Leaves an action on no key at all, until the reader sets one or resets
    /// it. A shortcut that gets in the way of the game is otherwise only
    /// movable, never removable.
    pub fn clear(&mut self, action: ShortcutAction) {
        self.custom
            .insert(action.key().to_owned(), CLEARED.to_owned());
    }

    /// Puts an action back to the shipped combination.
    pub fn reset(&mut self, action: ShortcutAction) {
        self.custom.remove(action.key());
    }

    /// Puts the whole table back to what the program ships with.
    ///
    /// This is also the one thing that clears the entries written by a newer
    /// version (see [`Self::set_by_a_newer_version`]): they are left alone
    /// everywhere else, and this is the reader saying to be rid of them.
    pub fn reset_all(&mut self) {
        self.custom.clear();
    }

    /// Whether the reader has changed this one — a cleared row included, since
    /// clearing is a change and has to be undoable.
    pub fn is_custom(&self, action: ShortcutAction) -> bool {
        self.custom.contains_key(action.key())
    }

    /// Whether the file holds a shortcut for an action this build does not
    /// have.
    ///
    /// Such an entry is written back untouched rather than dropped — it is a
    /// key set by a newer version, and running an older build for an evening
    /// must not be a way to lose it. It is not registered here, and the tab
    /// says as much: a file holding keys that do nothing only makes sense to a
    /// reader who is told why.
    pub fn holds_shortcuts_for_a_newer_version(&self) -> bool {
        self.custom
            .keys()
            .any(|name| ShortcutAction::from_key(name).is_none())
    }

    /// Every action on this combination, in the order they are tried.
    ///
    /// Two actions on one key is allowed, and one of them then never runs: the
    /// first in [`ShortcutAction::ALL`] takes the press out of the frame and
    /// the rest find nothing (see [`Shortcuts::triggered`]). Refusing the
    /// second one instead would be the tidier rule and was what the tab did at
    /// first, but it left the reader told about a state the table could not
    /// show them. Written down, marked in both rows and named in a warning, it
    /// is a thing they can see and undo.
    ///
    /// A cleared action is on no key, so it is never one of these.
    pub fn on_the_same_key(&self, combination: Combination) -> Vec<ShortcutAction> {
        ShortcutAction::ALL
            .into_iter()
            .filter(|action| self.effective(*action) == Some(combination))
            .collect()
    }
}

/// The live shortcuts: the table the window answers, and the one key held
/// against the whole desktop.
pub struct Shortcuts {
    /// What the table below was built from, so a frame that changed nothing
    /// costs a comparison rather than a grab given back and taken again.
    from: ShortcutSettings,
    table: Vec<(ShortcutAction, Combination)>,
    global: GlobalHotkey,
}

impl Shortcuts {
    /// `ctx` is the main window's, which the desktop-wide key has to wake: a
    /// press that arrives while the game is in front reaches a program that is
    /// drawing nothing.
    pub fn new(settings: &ShortcutSettings, ctx: &Context) -> Self {
        let mut shortcuts = Self {
            from: ShortcutSettings::default(),
            table: Vec::new(),
            global: GlobalHotkey::off(ctx.clone()),
        };
        shortcuts.rebuild(settings);
        shortcuts
    }

    /// Follows the live settings. Asked every frame rather than wired to the
    /// one button that applies them: the settings object is replaced wholesale
    /// when the dialog is closed with Ok, and a shortcut that only followed one
    /// route would be missed by every other way the settings can change.
    pub fn follow(&mut self, settings: &ShortcutSettings) {
        if self.from != *settings {
            self.rebuild(settings);
        }
    }

    /// Rebuilds the table and, where the system-wide key changed, takes the new
    /// one and gives the old one back.
    fn rebuild(&mut self, settings: &ShortcutSettings) {
        self.from = settings.clone();
        // A cleared action is not in the table at all, which is the whole of
        // what "no key" costs: nothing to match, so nothing can answer it.
        self.table = ShortcutAction::ALL
            .into_iter()
            .filter_map(|action| Some((action, settings.effective(action)?)))
            .collect();

        // Nothing to take from the desktop when the overlay is on no key. The
        // tick stays as the reader left it — it is the standing answer to "and
        // globally?", and turning it off behind their back would hand them a
        // window-only shortcut on the day they set a combination again.
        let wanted = settings
            .system_wide
            .then(|| settings.effective(ShortcutAction::ToggleOverlay))
            .flatten();
        self.global.bind(wanted);
    }

    /// What was pressed this frame, wherever it was pressed.
    ///
    /// The window's own keys are **taken** out of the frame rather than read,
    /// so nothing else answers them as well — the same discipline Tab and
    /// Escape are handled with.
    pub fn triggered(&mut self, ctx: &Context) -> Vec<ShortcutAction> {
        let mut actions = Vec::new();
        // The desktop-wide key first: it is the one that can arrive while the
        // window is not even in front.
        for _ in 0..self.global.presses() {
            actions.push(ShortcutAction::ToggleOverlay);
        }
        for (action, combination) in &self.table {
            // While the grab holds it, the window never sees that key — the
            // grab takes it from whoever has focus, ourselves included. Asking
            // anyway would cost nothing today and toggle twice on the day some
            // compositor delivers it to both.
            if action.can_be_system_wide() && self.global.holds_a_key() {
                continue;
            }
            if consume(ctx, *combination) {
                actions.push(*action);
            }
        }
        actions
    }

    /// What became of the desktop-wide key, for the settings tab to report.
    pub fn global_state(&self) -> &GlobalState {
        self.global.state()
    }
}

/// Takes one combination out of this frame's events, and says whether it was
/// there.
///
/// `Modifiers::matches_exact` rather than egui's own `consume_key`, which
/// matches logically and would let Alt+Shift+C answer a shortcut bound to
/// Alt+C — with these being the reader's own to bind, a shortcut has to mean
/// the keys it names and no others.
///
/// A held key repeats, and a toggle answering thirty times a second is a
/// flicker rather than a command, so a repeat is dropped — but still taken out
/// of the frame, since it is the same press as far as the reader is concerned.
fn consume(ctx: &Context, combination: Combination) -> bool {
    ctx.input_mut(|input| {
        let mut pressed = false;
        input.events.retain(|event| match event {
            Event::Key {
                key,
                pressed: true,
                repeat,
                modifiers,
                ..
            } if *key == combination.key && modifiers.matches_exact(combination.modifiers) => {
                pressed |= !repeat;
                false
            }
            _ => true,
        });
        pressed
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Context, RawInput};

    /// Shortcuts with nothing taken from the desktop.
    ///
    /// Every test here builds them this way on purpose: taking a key is a
    /// change to the desktop the test is running on, and `cargo test` on
    /// somebody's own machine must not quietly hold Alt+O for the length of
    /// the suite.
    fn window_only() -> Shortcuts {
        Shortcuts::new(
            &ShortcutSettings {
                system_wide: false,
                ..Default::default()
            },
            &Context::default(),
        )
    }

    fn press(key: Key, modifiers: Modifiers) -> RawInput {
        let mut input = RawInput::default();
        input.events.push(Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        });
        input
    }

    /// One pass of a window that answers the shortcuts: what fired, and what
    /// was left in the frame afterwards.
    ///
    /// The context is the caller's, because egui works out for itself whether a
    /// press is the keyboard repeating — `InputState::begin_pass` overwrites the
    /// flag from the keys it saw held on the previous pass — so a repeat can
    /// only be staged by two passes of one context.
    fn frame(
        shortcuts: &mut Shortcuts,
        ctx: &Context,
        input: RawInput,
    ) -> (Vec<ShortcutAction>, usize) {
        let mut fired = Vec::new();
        let mut left = 0;
        let _ = ctx.run_ui(input, |ui| {
            fired = shortcuts.triggered(ui.ctx());
            left = ui.ctx().input(|input| input.events.len());
        });
        (fired, left)
    }

    /// One pass of a fresh window, for the cases where nothing was held before.
    fn one_frame(shortcuts: &mut Shortcuts, input: RawInput) -> (Vec<ShortcutAction>, usize) {
        frame(shortcuts, &Context::default(), input)
    }

    /// The plain case, and the half of it that is easy to lose: the key is
    /// **taken** out of the frame, so nothing else answers the same press.
    #[test]
    fn a_key_runs_its_action_and_is_taken_out_of_the_frame() {
        let mut shortcuts = window_only();
        let (fired, left) = one_frame(&mut shortcuts, press(Key::L, Modifiers::ALT));

        assert_eq!(vec![ShortcutAction::ToggleLadder], fired);
        assert_eq!(0, left, "the press is taken, not merely read");
    }

    /// A shortcut means the keys it names and no others. egui's own
    /// `consume_key` matches logically and would let this run the ladder.
    #[test]
    fn extra_modifiers_are_a_different_shortcut() {
        let mut shortcuts = window_only();
        let (fired, left) = one_frame(
            &mut shortcuts,
            press(Key::L, Modifiers::ALT | Modifiers::SHIFT),
        );

        assert!(fired.is_empty(), "Alt+Shift+L is not Alt+L");
        assert_eq!(
            1, left,
            "and the press is left for whoever it was meant for"
        );
    }

    /// A key held down repeats, and a toggle answering thirty times a second
    /// is a flicker rather than a command. The key is never released here, so
    /// the second pass is the keyboard repeating rather than a second press.
    #[test]
    fn a_repeat_of_a_held_key_does_not_run_it_again() {
        let mut shortcuts = window_only();
        let ctx = Context::default();

        let (first, _) = frame(&mut shortcuts, &ctx, press(Key::L, Modifiers::ALT));
        assert_eq!(vec![ShortcutAction::ToggleLadder], first);

        let (repeated, left) = frame(&mut shortcuts, &ctx, press(Key::L, Modifiers::ALT));
        assert!(repeated.is_empty(), "a repeat is the same press");
        assert_eq!(0, left, "and is still taken, so nothing else sees it");
    }

    /// The key taken from the desktop is the one in the Overlay row, whatever
    /// the reader put there — not the combination the program ships with.
    ///
    /// Ignored for the reason the grab's own test is: taking a key is a change
    /// to the desktop the suite is running on. Run it on a throwaway display:
    ///
    /// ```text
    /// Xvfb :99 &
    /// DISPLAY=:99 cargo test the_desktop_wide_key -- --ignored
    /// ```
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "takes a key from the desktop it runs on"]
    fn the_desktop_wide_key_is_the_one_the_overlay_row_holds() {
        let chosen = Combination {
            modifiers: Modifiers::CTRL | Modifiers::ALT,
            key: Key::Y,
        };
        let mut settings = ShortcutSettings {
            system_wide: true,
            ..Default::default()
        };
        settings.set(ShortcutAction::ToggleOverlay, chosen);

        let shortcuts = Shortcuts::new(&settings, &Context::default());

        assert_eq!(
            &GlobalState::Held(chosen.to_string()),
            shortcuts.global_state(),
            "the desktop holds what the row says: {}",
            shortcuts.global_state().message()
        );
    }

    /// What the reader rebound is what answers.
    #[test]
    fn a_rebound_key_is_the_one_that_answers() {
        let mut settings = ShortcutSettings {
            system_wide: false,
            ..Default::default()
        };
        settings.set(
            ShortcutAction::ToggleCompare,
            Combination {
                modifiers: Modifiers::CTRL,
                key: Key::F9,
            },
        );
        let mut shortcuts = Shortcuts::new(&settings, &Context::default());

        let (old, _) = one_frame(&mut shortcuts, press(Key::C, Modifiers::ALT));
        assert!(old.is_empty(), "the shipped combination is no longer bound");

        let (new, _) = one_frame(&mut shortcuts, press(Key::F9, Modifiers::CTRL));
        assert_eq!(vec![ShortcutAction::ToggleCompare], new);
    }

    /// A cleared action is on no key: the combination it used to hold is left
    /// in the frame for whoever else wants it, rather than merely ignored.
    #[test]
    fn a_cleared_shortcut_answers_nothing_and_keeps_its_hands_off_the_frame() {
        let mut settings = ShortcutSettings {
            system_wide: false,
            ..Default::default()
        };
        settings.clear(ShortcutAction::ToggleLadder);
        let mut shortcuts = Shortcuts::new(&settings, &Context::default());

        let (fired, left) = one_frame(&mut shortcuts, press(Key::L, Modifiers::ALT));

        assert!(fired.is_empty(), "the action is on no key");
        assert_eq!(1, left, "and the press was never ours to take");
    }

    /// Clearing has to survive a restart, which is the whole reason it needs a
    /// spelling of its own: an action absent from the file is one on its
    /// shipped combination, so "cleared" cannot be written as "nothing".
    #[test]
    fn a_cleared_shortcut_survives_the_settings_file() {
        let mut settings = ShortcutSettings::default();
        settings.clear(ShortcutAction::ToggleCompare);

        let written = serde_json::to_string(&settings).unwrap();
        let read: ShortcutSettings = serde_json::from_str(&written).unwrap();

        assert_eq!(
            None,
            read.effective(ShortcutAction::ToggleCompare),
            "it came back on a key: {written}"
        );
        assert!(
            read.is_custom(ShortcutAction::ToggleCompare),
            "and Reset has to be there to undo it"
        );
    }

    /// The spelling of a cleared shortcut, pinned the way the rest of the
    /// section is: `docs/SHORTCUTS.md` prints it, and a file written by one
    /// build is read by the next.
    #[test]
    fn a_cleared_shortcut_is_written_the_way_the_document_says() {
        let mut settings = ShortcutSettings::default();
        settings.clear(ShortcutAction::ToggleOverlay);

        assert_eq!(
            r#"{"custom":{"ToggleOverlay":"Off"},"system_wide":true}"#,
            serde_json::to_string(&settings).unwrap(),
            "the spelling of a cleared shortcut changed — update docs/SHORTCUTS.md, and note \
             that the old spelling then reads as an unreadable entry and hands the shortcut \
             back to its default"
        );
        assert!(
            Combination::from_str(CLEARED).is_err(),
            "and it must not also be the name of a key"
        );
    }

    /// Nothing is taken from the desktop for an action that is on no key. The
    /// tick is left alone — it is the standing answer to "and globally?", and
    /// turning it off here would quietly hand back a shortcut the reader set a
    /// combination for a moment later.
    #[test]
    fn the_desktop_takes_nothing_when_the_overlay_row_is_empty() {
        let mut settings = ShortcutSettings {
            system_wide: true,
            ..Default::default()
        };
        settings.clear(ShortcutAction::ToggleOverlay);

        let shortcuts = Shortcuts::new(&settings, &Context::default());

        assert_eq!(&GlobalState::Off, shortcuts.global_state());
    }

    /// Reset all is the one place the entries this build cannot use are got rid
    /// of: everywhere else they are left exactly as they are.
    #[test]
    fn reset_all_puts_back_the_shipped_table_and_the_unusable_entries_with_it() {
        let mut settings: ShortcutSettings = serde_json::from_str(
            r#"{"custom":{"ToggleLadder":"Ctrl+F9","ToggleCompare":"Off",
                "SomethingAddedLater":"Alt+K"}}"#,
        )
        .unwrap();
        assert!(settings.holds_shortcuts_for_a_newer_version());

        settings.reset_all();

        for action in ShortcutAction::ALL {
            assert_eq!(
                Some(action.default_combination()),
                settings.effective(action),
                "{} is not back on the shipped combination",
                action.label()
            );
        }
        assert!(
            !settings.holds_shortcuts_for_a_newer_version(),
            "the reader asked to be rid of those too"
        );
    }

    /// Every shipped combination round-trips through the text the file holds.
    /// The file is written by one build and read by the next, so a key whose
    /// name does not come back is a shortcut silently lost.
    #[test]
    fn every_default_survives_the_settings_file() {
        for action in ShortcutAction::ALL {
            let combination = action.default_combination();
            let text = combination.to_string();
            assert_eq!(
                Ok(combination),
                text.parse(),
                "{} wrote itself as {text} and did not come back",
                action.label()
            );
        }
    }

    /// The spelling is fixed, so the same keys are one string. Two spellings
    /// would read as a change the reader never made.
    #[test]
    fn the_modifiers_are_written_in_one_order() {
        let combination = Combination {
            modifiers: Modifiers {
                alt: true,
                ctrl: true,
                shift: true,
                mac_cmd: false,
                command: false,
            },
            key: Key::F5,
        };
        assert_eq!("Ctrl+Alt+Shift+F5", combination.to_string());
        assert_eq!(Ok(combination), "shift+ALT+ctrl+F5".parse());
    }

    /// A shortcut has to carry a modifier: a bare letter is what a reader types
    /// into a note or a rule pattern.
    #[test]
    fn a_bare_key_is_not_a_shortcut() {
        assert!(Combination::from_str("O").is_err());
        assert!(Combination::from_str("Alt").is_err());
        assert!(
            !Combination {
                modifiers: Modifiers::NONE,
                key: Key::O,
            }
            .is_supported()
        );
    }

    /// Keys whose place moves between layouts are refused, because the
    /// system-wide grab names a key by the symbol on it.
    #[test]
    fn only_the_keys_the_grab_can_name_are_taken() {
        assert!(Combination::from_str("Alt+O").is_ok());
        assert!(Combination::from_str("Ctrl+5").is_ok());
        assert!(Combination::from_str("Alt+F9").is_ok());
        assert!(Combination::from_str("Alt+Comma").is_err());
        assert!(Combination::from_str("Alt+Space").is_err());
    }

    /// Nothing is written for an action left alone, so a build that adds one
    /// brings its own default to a file written before it existed.
    #[test]
    fn the_file_holds_only_what_was_changed() {
        let mut settings = ShortcutSettings::default();
        assert_eq!(
            "{}",
            serde_json::to_string(&settings.custom).unwrap(),
            "a fresh installation writes no shortcuts at all"
        );

        let changed = Combination {
            modifiers: Modifiers::CTRL,
            key: Key::F8,
        };
        settings.set(ShortcutAction::ToggleLadder, changed);
        assert!(settings.is_custom(ShortcutAction::ToggleLadder));
        assert_eq!(
            Ok(Some(changed)),
            settings.combination(ShortcutAction::ToggleLadder)
        );

        // Put back by hand rather than by the Reset button: the same rule has
        // to hold, or the file keeps a copy of the shipped table.
        settings.set(
            ShortcutAction::ToggleLadder,
            ShortcutAction::ToggleLadder.default_combination(),
        );
        assert!(!settings.is_custom(ShortcutAction::ToggleLadder));
    }

    /// An entry that cannot be read costs that one shortcut, and says so. The
    /// alternative — refusing the settings file — reads as a damaged
    /// installation and puts every other setting back to its default.
    #[test]
    fn an_unreadable_entry_costs_only_its_own_shortcut() {
        let settings: ShortcutSettings =
            serde_json::from_str(r#"{"custom":{"ToggleLadder":"Alt+Nonsense"}}"#).unwrap();

        assert!(settings.combination(ShortcutAction::ToggleLadder).is_err());
        assert_eq!(
            Some(ShortcutAction::ToggleLadder.default_combination()),
            settings.effective(ShortcutAction::ToggleLadder),
            "the shipped combination stands until the reader fixes it"
        );
        assert_eq!(
            Some(ShortcutAction::OpenSettings.default_combination()),
            settings.effective(ShortcutAction::OpenSettings),
            "and nothing else is affected"
        );
    }

    /// The shape written to the settings file, spelled out.
    ///
    /// `docs/SHORTCUTS.md` prints this example for a reader to compare their
    /// own file against, and a settings file is read by builds that did not
    /// write it — so a renamed field is a setting every installation silently
    /// loses. The same guard the rules file has.
    #[test]
    fn the_written_shape_is_what_the_document_says() {
        let mut settings = ShortcutSettings::default();
        settings.set(
            ShortcutAction::ToggleLadder,
            Combination {
                modifiers: Modifiers::CTRL,
                key: Key::F9,
            },
        );

        assert_eq!(
            r#"{"custom":{"ToggleLadder":"Ctrl+F9"},"system_wide":true}"#,
            serde_json::to_string(&settings).unwrap(),
            "the shortcuts section changed shape — update docs/SHORTCUTS.md, and \
             note that a renamed field reads as 'never set' in every file already \
             written"
        );
    }

    /// Every action's name survives the file it is written to. A name that
    /// changed would read as "the player never set that" and quietly hand the
    /// shortcut back to its default.
    #[test]
    fn every_action_is_named_the_same_way_in_both_directions() {
        for action in ShortcutAction::ALL {
            assert_eq!(
                Some(action),
                ShortcutAction::from_key(action.key()),
                "{} does not come back from its own name",
                action.label()
            );
        }
    }

    /// A shortcut written by a later version — for an action this build has
    /// never heard of — is kept and written back. Running an older build for an
    /// evening must not be a way to lose the keys set in the newer one.
    #[test]
    fn a_shortcut_this_build_does_not_know_survives_it() {
        let settings: ShortcutSettings = serde_json::from_str(
            r#"{"custom":{"ToggleLadder":"Ctrl+F9","SomethingAddedLater":"Alt+K"}}"#,
        )
        .expect("an unknown action is not a broken settings file");

        assert_eq!(
            Some("Ctrl+F9".to_owned()),
            settings
                .effective(ShortcutAction::ToggleLadder)
                .map(|combination| combination.to_string()),
            "the shortcuts this build does know still work"
        );
        assert!(
            serde_json::to_string(&settings)
                .unwrap()
                .contains("SomethingAddedLater"),
            "and the one it does not is written back untouched"
        );
    }

    /// Two actions on one key is allowed, so the tab has to be able to name
    /// both of them and say which one answers.
    #[test]
    fn both_actions_on_one_key_are_named_in_the_order_they_are_tried() {
        let mut settings = ShortcutSettings::default();
        let overlay = ShortcutAction::ToggleOverlay.default_combination();
        settings.set(ShortcutAction::ToggleLadder, overlay);

        assert_eq!(
            vec![ShortcutAction::ToggleOverlay, ShortcutAction::ToggleLadder],
            settings.on_the_same_key(overlay),
            "the one that answers is first"
        );
    }

    /// The one that answers is the first of them, and the other never sees the
    /// press — which is what the tab's warning tells the reader, so it has to
    /// be what actually happens.
    #[test]
    fn only_the_first_of_two_actions_on_one_key_runs() {
        let mut settings = ShortcutSettings {
            system_wide: false,
            ..Default::default()
        };
        let overlay = ShortcutAction::ToggleOverlay.default_combination();
        settings.set(ShortcutAction::ToggleLadder, overlay);
        let mut shortcuts = Shortcuts::new(&settings, &Context::default());

        let (fired, left) = one_frame(&mut shortcuts, press(Key::O, Modifiers::ALT));

        assert_eq!(
            vec![ShortcutAction::ToggleOverlay],
            fired,
            "the ladder must not answer the same press"
        );
        assert_eq!(0, left, "and the press is taken once");
    }
}
