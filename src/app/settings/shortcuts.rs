//! The Shortcuts tab: which key does what, and whether the overlay's key is
//! taken from the whole desktop.
//!
//! A row per action, and the combination is a button: press it and the next
//! keys pressed are recorded. Recording is the only way to set one, rather than
//! a text field — a field would accept `Alt+Nonsense`, and the reader would
//! find out it was nonsense by the shortcut never working.

use eframe::egui::{Event, Grid, Key, RichText, Ui};

use super::Settings;
use crate::{
    app::{
        shortcuts::{Combination, GlobalState, ShortcutAction},
        theme,
    },
    custom_widgets::tooltip::CloseTooltip,
};

#[derive(Default)]
pub struct ShortcutsTab {
    /// The action whose next key press is being recorded.
    recording: Option<ShortcutAction>,
    /// Why the last attempt was not taken. Kept until the next attempt, rather
    /// than drawn for the one frame the key arrived in — which nobody can read.
    refused: Option<String>,
}

impl ShortcutsTab {
    pub fn show(&mut self, modified_settings: &mut Settings, global: &GlobalState, ui: &mut Ui) {
        ui.label("These keys work while this window is in front.");
        ui.add_space(4.0);

        Grid::new("shortcuts")
            .num_columns(3)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                for action in ShortcutAction::ALL {
                    self.show_row(action, modified_settings, ui);
                    ui.end_row();
                }
            });

        if let Some(refused) = &self.refused {
            ui.add_space(4.0);
            ui.colored_label(theme::palette().warn, format!("⚠ {refused}"));
        }

        // Keys the file holds for something this build does not have. They are
        // kept and written back untouched, and saying so is the only way the
        // reader can tell "set in a newer version" from "lost".
        let newer = modified_settings.shortcuts.set_by_a_newer_version();
        if !newer.is_empty() {
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!(
                    "The settings file also holds {} shortcut(s) for a newer version of the \
                     program ({}). They are left exactly as they are.",
                    newer.len(),
                    newer.join(", ")
                ))
                .weak(),
            );
        }

        ui.add_space(10.0);
        ui.separator();
        self.show_system_wide(modified_settings, global, ui);
    }

    fn show_row(&mut self, action: ShortcutAction, modified_settings: &mut Settings, ui: &mut Ui) {
        ui.label(action.label()).hover(action.hint());

        let recording = self.recording == Some(action);
        let label = match recording {
            true => "press a combination…".to_owned(),
            false => modified_settings.shortcuts.effective(action).to_string(),
        };
        if ui
            .button(label)
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

        let is_custom = modified_settings.shortcuts.is_custom(action);
        // What the file holds and could not be read. The shortcut still works —
        // on its shipped combination — so this says what is in force as well as
        // what is wrong with what was written.
        let unreadable = modified_settings.shortcuts.combination(action).err();
        let mut reset = false;
        ui.horizontal(|ui| {
            if is_custom
                && ui
                    .button("Reset")
                    .hover(format!("Back to {}.", action.default_combination()))
                    .clicked()
            {
                reset = true;
            }
            if let Some(problem) = &unreadable {
                ui.colored_label(
                    theme::palette().warn,
                    format!(
                        "⚠ the settings file says {problem}; {} is in force",
                        action.default_combination()
                    ),
                );
            }
        });
        if reset {
            modified_settings.shortcuts.reset(action);
            self.refused = None;
        }

        if recording {
            self.record(action, modified_settings, ui);
        }
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
        let taken_by = modified_settings.shortcuts.taken_by(combination, action);
        if let Some(other) = taken_by.first() {
            self.refused = Some(format!(
                "{combination} is already {}'s. Two things on one key means only the first of \
                 them ever answers.",
                other.label()
            ));
            return;
        }
        self.refused = None;
        modified_settings.shortcuts.set(action, combination);
    }

    fn show_system_wide(
        &mut self,
        modified_settings: &mut Settings,
        global: &GlobalState,
        ui: &mut Ui,
    ) {
        let overlay = ShortcutAction::ToggleOverlay;
        ui.checkbox(
            &mut modified_settings.shortcuts.system_wide,
            format!("Take {} from the whole desktop", overlay.label()),
        )
        .hover(
            "Off, the shortcut only reaches the program while its window is in front — which it \
             is not while you are playing. On, the key belongs to STO-CLARE wherever it is \
             pressed, and nothing else on the desktop can use it, the game included.",
        );

        // What is actually held right now, which is not the same as what the
        // box says: the key is taken when the window is closed with Ok, and it
        // can be refused (another program holds it).
        let message = global.message();
        match global.is_problem() {
            true => {
                ui.colored_label(theme::palette().warn, format!("⚠ {message}"));
            }
            false => {
                ui.label(RichText::new(message).weak());
            }
        }
        ui.label(
            RichText::new("A change here takes effect when this window is closed with Ok.").weak(),
        );
    }
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
            "Ctrl+F8",
            settings
                .shortcuts
                .effective(ShortcutAction::ToggleLadder)
                .to_string()
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

    /// A combination another action already answers is refused by name: two on
    /// one key means only the first of them ever runs.
    #[test]
    fn a_combination_another_action_holds_is_refused_by_name() {
        let (tab, settings) = record(Key::O, Modifiers::ALT);
        assert!(
            tab.refused.as_deref().is_some_and(|text| text.contains("Overlay")),
            "the refusal names what holds the key: {:?}",
            tab.refused
        );
        assert!(!settings.shortcuts.is_custom(ShortcutAction::ToggleLadder));
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
}
