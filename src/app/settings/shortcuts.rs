//! The Shortcuts tab: which key does what, and which of them is global.
//!
//! A row per action, and the combination is a button: press it and the next
//! keys pressed are recorded. Recording is the only way to set one, rather than
//! a text field — a field would accept `Alt+Nonsense`, and the reader would
//! find out it was nonsense by the shortcut never working.
//!
//! The row's other two buttons are what the reader does to a combination they
//! do not want: **Reset** puts back the shipped one, **Clear** leaves the
//! action on no key at all. Clear is the only way out of a shortcut that gets
//! in the way of the game, which otherwise could only ever be moved.

use eframe::egui::{
    Align2, Button, Checkbox, Color32, Event, Grid, Key, Label, RichText, Sense, TextStyle, Ui,
    Vec2, Window,
};

use super::Settings;
use crate::{
    app::{
        shortcuts::{Combination, GlobalState, ShortcutAction},
        theme,
    },
    custom_widgets::{dialog::escape_closes, tooltip::CloseTooltip},
};

/// The width of the column carrying the warning sign, held whether a row has
/// one or not: a sign that appeared and pushed the buttons sideways would move
/// the thing the reader was about to click.
const WARNING_SIGN_WIDTH: f32 = 16.0;

/// The narrowest a combination button may be, for a table whose rows have all
/// been cleared. A button sized by its text alone is a few pixels wide when
/// there is no text, which is neither visible nor clickable.
const NARROWEST_COMBINATION: f32 = 90.0;

/// What a row that is listening for a key says.
///
/// Its width counts towards `combination_width` like any combination's would.
/// `Ui::add_sized` sets the space a widget is given, not a limit it is held to,
/// so a longer label than the column was measured for widens the column — and
/// the row's buttons would then slide sideways the moment a row started
/// listening, which is exactly when the reader is about to press one of them.
const LISTENING: &str = "press a combination…";

#[derive(Default)]
pub struct ShortcutsTab {
    /// The action whose next key press is being recorded.
    recording: Option<ShortcutAction>,
    /// Why the last attempt was not taken. Kept until the next attempt, rather
    /// than drawn for the one frame the key arrived in — which nobody can read.
    refused: Option<String>,
    /// Whether the question about putting every shortcut back is up.
    confirming_reset_all: bool,
}

impl ShortcutsTab {
    pub fn show(&mut self, modified_settings: &mut Settings, global: &GlobalState, ui: &mut Ui) {
        if ui
            .button("Reset all shortcuts")
            .hover("Put every key back to what the program shipped with.")
            .clicked()
        {
            self.confirming_reset_all = true;
            // A row left listening would answer the Escape that the question
            // needs to be backed out of.
            self.recording = None;
        }
        ui.add_space(6.0);

        let combination_width = combination_width(modified_settings, ui);

        // Gathered as the rows are drawn and printed underneath, so a reader
        // who sees a sign in a row has the sentence in one place rather than in
        // a tooltip they have to go looking for.
        let mut warnings = Vec::new();

        Grid::new("shortcuts")
            .num_columns(6)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                for action in ShortcutAction::ALL {
                    self.show_row(
                        action,
                        modified_settings,
                        global,
                        combination_width,
                        &mut warnings,
                        ui,
                    );
                    ui.end_row();
                }
            });

        for warning in &warnings {
            ui.add_space(4.0);
            ui.colored_label(theme::palette().warn, format!("⚠ {warning}"));
        }

        // A combination that was just turned down. It belongs to the key the
        // reader pressed a second ago rather than to a row, so it is said in
        // words here instead of raising a sign they would have to hunt for.
        if let Some(refused) = &self.refused {
            ui.add_space(4.0);
            ui.colored_label(theme::palette().warn, format!("⚠ {refused}"));
        }

        // Keys the file holds for something this build does not have. They are
        // not registered and not touched, and saying so is the only way the
        // reader can tell "set in a newer version" from "lost".
        if modified_settings
            .shortcuts
            .holds_shortcuts_for_a_newer_version()
        {
            ui.add_space(4.0);
            ui.label(
                RichText::new(
                    "The settings file also holds shortcuts this version cannot use. They are \
                     ignored and left as they are — Reset all shortcuts clears them.",
                )
                .weak(),
            );
        }

        self.confirm_reset_all(modified_settings, ui);
    }

    fn show_row(
        &mut self,
        action: ShortcutAction,
        modified_settings: &mut Settings,
        global: &GlobalState,
        combination_width: f32,
        warnings: &mut Vec<String>,
        ui: &mut Ui,
    ) {
        let combination = modified_settings.shortcuts.effective(action);

        // Beside the combination it applies to, rather than under the table:
        // this is a property of that one shortcut, and at the foot of the page
        // it read as a setting of its own that happened to mention the overlay.
        // A column of its own, so the rows stay in line.
        if action.can_be_system_wide() {
            ui.add_enabled(
                combination.is_some(),
                Checkbox::new(&mut modified_settings.shortcuts.system_wide, "Global"),
            )
            .hover(
                "The key works while the game is in front. Nothing else on the desktop can use \
                 that combination while this is on — the game included.",
            )
            .disabled_hover("Set a combination first: there is no key here to take.");
        } else {
            ui.label("");
        }

        ui.label(action.label()).hover(action.hint());

        let recording = self.recording == Some(action);
        let label = match (recording, combination) {
            (true, _) => LISTENING.to_owned(),
            // A cleared row is an empty button rather than a word standing in
            // for one: the field is the thing that is empty, and it is still
            // what the reader presses to put a key back in it.
            (false, None) => String::new(),
            (false, Some(combination)) => combination.to_string(),
        };
        if ui
            .add_sized(
                [combination_width, ui.spacing().interact_size.y],
                Button::new(label),
            )
            .hover(match recording {
                true => "Press the keys you want, or Escape to leave it as it was.",
                false => "Press this, then the keys you want.",
            })
            .clicked()
        {
            // A second press on the same row gives up; pressing another row
            // moves the recording to it, so only one is ever live.
            self.recording = (!recording).then_some(action);
            self.refused = None;
        }

        // Counted from where this row started rather than taken off the end of
        // the list: a row with nothing to say would otherwise raise a sign for
        // whatever the row above it said.
        let mine = warnings.len();
        // What the file holds and could not be read. The shortcut still works —
        // on its shipped combination — so this says what is in force as well as
        // what is wrong with what was written.
        if let Err(problem) = modified_settings.shortcuts.combination(action) {
            warnings.push(format!(
                "{}: the settings file says {problem}; {} is in force.",
                action.label(),
                action.default_combination()
            ));
        }
        // A key asked of the desktop and not given belongs to the row that
        // asked for it.
        if action.can_be_system_wide() && global.is_problem() {
            warnings.push(format!("{}: {}", action.label(), global.message()));
        }
        // Two rows on one key. Said once, in the row of the one that answers,
        // and both rows raise the sign for it — the point of the warning is the
        // pair, and a sentence per row would state the same fact twice.
        let sharing = combination
            .map(|combination| modified_settings.shortcuts.on_the_same_key(combination))
            .unwrap_or_default();
        let shared = match sharing.as_slice() {
            [answers, ..] if sharing.len() > 1 => {
                let sentence = format!(
                    "{} is on {}. Only {} answers it: a key runs the first thing that \
                     holds it and the rest never see the press.",
                    combination.expect("a shared key is a key"),
                    names(&sharing),
                    answers.label()
                );
                if *answers == action {
                    warnings.push(sentence.clone());
                }
                Some(sentence)
            }
            _ => None,
        };
        warning_sign(&warnings[mine..], shared.as_deref(), ui);

        let mut reset = false;
        if modified_settings.shortcuts.is_custom(action) {
            reset = ui
                .button("Reset")
                .hover(format!("Back to {}.", action.default_combination()))
                .clicked();
        } else {
            ui.label("");
        }

        let mut clear = false;
        if combination.is_some() {
            clear = ui
                .button("Clear")
                .hover("Leave this on no key at all.")
                .clicked();
        } else {
            ui.label("");
        }

        if reset {
            modified_settings.shortcuts.reset(action);
            self.refused = None;
        }
        if clear {
            modified_settings.shortcuts.clear(action);
            self.recording = None;
            self.refused = None;
        }

        if recording {
            self.record(action, modified_settings, ui);
        }
    }

    /// The question asked before every shortcut goes back to the shipped one.
    ///
    /// A window rather than a second click on the same button, and the button
    /// that does it names the thing it does — the same shape the combats list
    /// asks its delete question with.
    fn confirm_reset_all(&mut self, modified_settings: &mut Settings, ui: &mut Ui) {
        if !self.confirming_reset_all {
            return;
        }
        Window::new("Reset all shortcuts")
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ui.ctx(), |ui| {
                ui.label("Put every key back to what the program shipped with?");
                ui.label(RichText::new("Anything you set here will be reset to default.").weak());
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.scope(|ui| {
                        theme::accent_rim(ui);
                        if ui.button("Reset all shortcuts").clicked() {
                            modified_settings.shortcuts.reset_all();
                            self.recording = None;
                            self.refused = None;
                            self.confirming_reset_all = false;
                        }
                    });
                    if ui.button("Cancel").clicked() || escape_closes(ui.ctx()) {
                        self.confirming_reset_all = false;
                    }
                });
            });
    }

    /// Takes the next key press out of the frame and makes a shortcut of it.
    ///
    /// The event is **consumed** whatever happens to it, so a key pressed at a
    /// recording row never also runs the shortcut it is being bound to — and
    /// Escape here leaves the recording rather than closing the whole window,
    /// since this is asked before the Ok/Cancel row asks for the same key.
    fn record(&mut self, action: ShortcutAction, modified_settings: &mut Settings, ui: &mut Ui) {
        let pressed = ui.ctx().input_mut(|input| {
            let mut pressed = None;
            input.events.retain(|event| match event {
                Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } if pressed.is_none() => {
                    pressed = Some((*key, *modifiers));
                    false
                }
                _ => true,
            });
            pressed
        });

        let Some((key, modifiers)) = pressed else {
            return;
        };
        if key == Key::Escape && !modifiers.any() {
            self.recording = None;
            return;
        }

        let combination = Combination { modifiers, key };
        self.recording = None;
        if !combination.is_supported() {
            self.refused = Some(format!(
                "{combination} is not one we can take — a shortcut needs a modifier (Ctrl, Alt or \
                 Shift) and a letter, a digit or a function key."
            ));
            return;
        }
        // A combination another row already holds is **taken**, not turned
        // down. Only one of them will answer it, and that is said in both rows
        // and under the table — a refusal told the reader about a state the
        // table then could not show them.
        self.refused = None;
        modified_settings.shortcuts.set(action, combination);
    }
}

/// The sign that says "this row is the one the sentence below is about".
///
/// Always allocated, whether the row has something to say or not, so a warning
/// appearing does not shift the buttons beside it. The sentences are repeated
/// under the pointer, which saves looking down the page when two rows are
/// marked.
fn warning_sign(warnings: &[String], shared: Option<&str>, ui: &mut Ui) {
    let mut under_the_pointer: Vec<&str> = warnings.iter().map(String::as_str).collect();
    // The row that only *joins* a shared key has no line of its own under the
    // table — the line is in the row that answers — but it still says why it is
    // marked.
    if let Some(shared) = shared
        && !under_the_pointer.contains(&shared)
    {
        under_the_pointer.push(shared);
    }

    let sign = match under_the_pointer.is_empty() {
        false => RichText::new("⚠").color(theme::palette().warn),
        true => RichText::new(""),
    };
    let response = ui.add_sized(
        [WARNING_SIGN_WIDTH, ui.spacing().interact_size.y],
        Label::new(sign).sense(Sense::hover()),
    );
    if !under_the_pointer.is_empty() {
        response.hover(under_the_pointer.join("\n\n"));
    }
}

/// A list of action names as a sentence reads them: "Overlay and Ladder",
/// "Overlay, Ladder and Settings".
fn names(actions: &[ShortcutAction]) -> String {
    let labels: Vec<&str> = actions.iter().map(|action| action.label()).collect();
    match labels.split_last() {
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// The width every combination button is given: the widest combination the
/// table currently holds, and never less than a button that can be seen and
/// hit.
///
/// Without one width for all of them the column is as wide as whichever row
/// happens to be longest, so clearing or rebinding one row moves every other
/// row's buttons sideways.
fn combination_width(modified_settings: &Settings, ui: &Ui) -> f32 {
    let widest = ShortcutAction::ALL
        .into_iter()
        .filter_map(|action| modified_settings.shortcuts.effective(action))
        .map(|combination| combination.to_string())
        .chain([LISTENING.to_owned()])
        .map(|text| text_width(&text, ui))
        .fold(0.0_f32, f32::max);
    (widest + 2.0 * ui.spacing().button_padding.x).max(NARROWEST_COMBINATION)
}

/// How wide a piece of text is in the font the buttons are drawn with.
fn text_width(text: &str, ui: &Ui) -> f32 {
    let font = TextStyle::Button.resolve(ui.style());
    ui.ctx().fonts_mut(|fonts| {
        fonts
            .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER)
            .rect
            .width()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Context, Modifiers, RawInput};

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

    /// One pass over the tab with a key pressed while a row is recording.
    fn record(key: Key, modifiers: Modifiers) -> (ShortcutsTab, Settings) {
        let mut tab = ShortcutsTab {
            recording: Some(ShortcutAction::ToggleLadder),
            refused: None,
            confirming_reset_all: false,
        };
        let mut settings = Settings::default();
        let ctx = Context::default();
        let _ = ctx.run_ui(press(key, modifiers), |ui| {
            tab.record(ShortcutAction::ToggleLadder, &mut settings, ui);
        });
        (tab, settings)
    }

    /// The plain case: a supported combination is recorded and the row stops
    /// listening.
    #[test]
    fn a_pressed_combination_becomes_the_shortcut() {
        let (tab, settings) = record(Key::F8, Modifiers::CTRL);
        assert_eq!(None, tab.recording);
        assert_eq!(None, tab.refused);
        assert_eq!(
            Some("Ctrl+F8".to_owned()),
            settings
                .shortcuts
                .effective(ShortcutAction::ToggleLadder)
                .map(|combination| combination.to_string())
        );
    }

    /// A bare key is refused, and the refusal says why — a key that silently
    /// did nothing would read as a broken button.
    #[test]
    fn a_key_with_no_modifier_is_refused_out_loud() {
        let (tab, settings) = record(Key::F8, Modifiers::NONE);
        assert!(tab.refused.is_some(), "the reason is kept for the reader");
        assert!(
            !settings.shortcuts.is_custom(ShortcutAction::ToggleLadder),
            "and nothing was recorded"
        );
    }

    /// A combination another action already answers is **taken**, and both rows
    /// then hold it. Turning it down instead told the reader about a state the
    /// table could not show them, and left the row it was refused in looking
    /// like nothing had happened.
    #[test]
    fn a_combination_another_action_holds_is_recorded_all_the_same() {
        let (tab, settings) = record(Key::O, Modifiers::ALT);
        assert_eq!(None, tab.refused, "nothing was turned down");
        assert_eq!(
            vec![ShortcutAction::ToggleOverlay, ShortcutAction::ToggleLadder],
            settings
                .shortcuts
                .on_the_same_key(ShortcutAction::ToggleOverlay.default_combination()),
            "both rows are on Alt+O, with the one that answers first"
        );
    }

    /// The names in that warning read as a sentence, because that is where they
    /// are used.
    #[test]
    fn the_actions_on_one_key_are_listed_as_a_sentence() {
        assert_eq!("Overlay", names(&[ShortcutAction::ToggleOverlay]));
        assert_eq!(
            "Overlay and Ladder",
            names(&[ShortcutAction::ToggleOverlay, ShortcutAction::ToggleLadder])
        );
        assert_eq!(
            "Overlay, Ladder and Settings",
            names(&[
                ShortcutAction::ToggleOverlay,
                ShortcutAction::ToggleLadder,
                ShortcutAction::OpenSettings,
            ])
        );
    }

    /// Escape leaves the recording alone rather than closing the settings
    /// window — the tab asks for the key before the Ok/Cancel row does.
    #[test]
    fn escape_gives_up_the_recording() {
        let (tab, settings) = record(Key::Escape, Modifiers::NONE);
        assert_eq!(None, tab.recording);
        assert_eq!(None, tab.refused);
        assert!(!settings.shortcuts.is_custom(ShortcutAction::ToggleLadder));
    }

    /// A key a cleared action used to hold is free: nothing answers it, so the
    /// row that takes it is the only one on it.
    #[test]
    fn a_cleared_action_is_not_in_the_way_of_its_old_key() {
        let mut settings = Settings::default();
        settings.shortcuts.clear(ShortcutAction::ToggleOverlay);

        let mut tab = ShortcutsTab {
            recording: Some(ShortcutAction::ToggleLadder),
            refused: None,
            confirming_reset_all: false,
        };
        let ctx = Context::default();
        let _ = ctx.run_ui(press(Key::O, Modifiers::ALT), |ui| {
            tab.record(ShortcutAction::ToggleLadder, &mut settings, ui);
        });

        assert_eq!(
            vec![ShortcutAction::ToggleLadder],
            settings
                .shortcuts
                .on_the_same_key(ShortcutAction::ToggleOverlay.default_combination()),
            "Alt+O belongs to the ladder alone now"
        );
    }
}
