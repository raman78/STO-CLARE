//! What fight is on screen, said once above the tabs.
//!
//! It used to be the Summary tab's heading, which meant that four of the six
//! tabs showed a table of figures with nothing on the screen saying which fight
//! they were of — and the reader's own note, the one thing that tells two runs
//! of the same map apart, was only reachable by going back to Summary.

use eframe::egui::*;

use crate::{
    analyzer::Combat,
    app::settings::{CombatNotes, MAX_NOTE_CHARS, Settings},
};

/// Which fight it is — name, date and the times it ran between — and the
/// reader's note on it.
pub struct CombatHeader {
    /// Name and times as one line, the way [`Combat::identifier`] writes it:
    /// `[Team] [Space] [TFO] Infected: The Conduit [Elite] | 2026-08-19
    /// 21:14:03 - 21:18:15`. One string rather than two so it is set in one
    /// face — the date is as much a part of which fight this is as the name.
    identifier: String,
    /// The note's key, empty until a combat is loaded — there is nothing to
    /// attach a note to before that.
    note_key: String,
    /// The text being edited. Held here rather than edited straight in the
    /// settings so a keystroke does not write the settings file.
    note: String,
}

impl CombatHeader {
    pub fn empty() -> Self {
        Self {
            identifier: "<no data loaded>".to_owned(),
            note_key: String::new(),
            note: String::new(),
        }
    }

    pub fn update(&mut self, settings: &Settings, combat: &Combat) {
        self.identifier = combat.identifier();
        self.note_key = CombatNotes::key(combat);
        self.note = settings.combat_notes.get(&self.note_key).to_owned();
    }

    pub fn show(&mut self, settings: &mut Settings, ui: &mut Ui) {
        // A framed box rather than a bare line: it belongs to every tab below
        // it rather than to the one that happens to be open, and a frame is
        // what says so without a word.
        Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading(&self.identifier);
                self.show_note(settings, ui);
            });
        });
    }

    /// The note field, at the right-hand end of the line.
    ///
    /// The text is written into the settings on every keystroke but saved to
    /// disk only once the field is left, so typing does not rewrite the
    /// settings file per character.
    fn show_note(&mut self, settings: &mut Settings, ui: &mut Ui) {
        if self.note_key.is_empty() {
            return;
        }

        // The same room the Note column in the list of fights reserves, so a
        // note that fits here fits there.
        let note_width = crate::app::tuning::note_width(ui);

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(
                RichText::new(format!("{}/{}", self.note.chars().count(), MAX_NOTE_CHARS)).weak(),
            );
            let response = ui.add(
                TextEdit::singleline(&mut self.note)
                    .desired_width(note_width)
                    .hint_text("your own description of this combat"),
            );
            if response.changed() {
                // A hard limit — the field takes nothing past it.
                if self.note.chars().count() > MAX_NOTE_CHARS {
                    self.note = self.note.chars().take(MAX_NOTE_CHARS).collect();
                }
                settings.combat_notes.set(&self.note_key, &self.note);
            }
            if response.lost_focus() {
                self.note = self.note.trim().to_owned();
                settings.combat_notes.set(&self.note_key, &self.note);
                settings.save();
            }
            ui.label("Note:");
        });
    }
}
