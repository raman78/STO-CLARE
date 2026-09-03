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
            // A frame is drawn around its content's `min_rect`, so a box of two
            // stacked lines ends wherever the longer of them ends — a ragged
            // edge in the middle of the window, moving with the length of the
            // fight's name. It belongs to everything below it, so it is as wide
            // as everything below it.
            ui.set_min_width(ui.available_width());
            // The note under the name rather than beside it: it is part of
            // which fight this is, not a field the line happens to end with,
            // and the field is where the reader looks to change what the line
            // above now says.
            ui.vertical(|ui| {
                ui.heading(self.title());
                self.show_note(settings, ui);
            });
        });
    }

    /// Which fight it is, with the note after it in the same face — the note is
    /// the one thing that tells two runs of the same map apart, so it belongs
    /// in the line that says which run this is rather than only in the field
    /// below. Set as one string so the whole line is one heading, the way the
    /// name and the times already are.
    ///
    /// Built from the field being edited rather than from the settings, so the
    /// line follows the typing.
    fn title(&self) -> String {
        let note = self.note.trim();
        if note.is_empty() {
            return self.identifier.clone();
        }
        format!("{} — {}", self.identifier, note)
    }

    /// The note field, on its own line under the name.
    ///
    /// The text is written into the settings on every keystroke but saved to
    /// disk only once the field is left, so typing does not rewrite the
    /// settings file per character. **Clear** is the exception: it is a
    /// deliberate act with nothing to leave afterwards, so it writes there and
    /// then.
    fn show_note(&mut self, settings: &mut Settings, ui: &mut Ui) {
        if self.note_key.is_empty() {
            return;
        }

        // The same room the Note column in the list of fights reserves, so a
        // note that fits here fits there.
        let note_width = crate::app::tuning::note_width(ui);

        ui.horizontal(|ui| {
            ui.label("Note:");
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
            ui.label(
                RichText::new(format!("{}/{}", self.note.chars().count(), MAX_NOTE_CHARS)).weak(),
            );
            // Only there while there is something to clear, the way the combats
            // list offers "Clear filter" only under an active one.
            if !self.note.is_empty() && ui.button("Clear").clicked() {
                self.note.clear();
                settings.combat_notes.set(&self.note_key, "");
                settings.save();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(note: &str) -> CombatHeader {
        CombatHeader {
            identifier: "[Solo] [Space] [TFO] Infected: The Conduit [Elite] | 2026-09-02 \
                         14:33:32 - 14:41:10"
                .to_owned(),
            note_key: "2026-09-02 14:33:32.800".to_owned(),
            note: note.to_owned(),
        }
    }

    /// A named run says its name in the line above the tabs, so the six tabs
    /// are read under the note as well as under the map.
    #[test]
    fn a_noted_combat_carries_its_note_in_the_heading() {
        assert!(header("Full BA").title().ends_with(" — Full BA"));
    }

    /// A fight nobody named keeps the line it always had — no trailing dash
    /// with nothing after it.
    #[test]
    fn an_unnamed_combat_keeps_the_plain_heading() {
        let header = header("");
        assert_eq!(header.identifier, header.title());
    }

    /// The heading follows the field as it is typed, and a note that is still
    /// only a space is not a name yet.
    #[test]
    fn a_note_of_nothing_but_spaces_adds_nothing() {
        let header = header("   ");
        assert_eq!(header.identifier, header.title());
    }

    /// What is written in the field is what lands in the heading, without the
    /// spaces a half-typed note carries.
    #[test]
    fn the_heading_takes_the_note_as_written_but_trimmed() {
        assert!(header(" FAW build ").title().ends_with(" — FAW build"));
    }

    /// The box reaches across the window, whatever is written in it. A frame is
    /// drawn around its content, so two stacked lines ended it wherever the
    /// longer of them ended — an edge that moved with the length of the fight's
    /// name and stopped in the middle of a window full of full-width tables.
    ///
    /// Nothing here can write the settings: `show` only saves on a keystroke,
    /// on the field losing focus, or on **Clear** being pressed, and a single
    /// pass over an empty `RawInput` does none of the three.
    #[test]
    fn the_box_is_as_wide_as_what_it_stands_over() {
        for note in ["", "Full BA"] {
            let ctx = Context::default();
            crate::app::fonts::install(&ctx);
            let mut header = header(note);
            let mut settings = Settings::default();
            let (mut room, mut taken) = (0.0, 0.0);
            let _ = ctx.run_ui(RawInput::default(), |ui| {
                room = ui.available_width();
                header.show(&mut settings, ui);
                taken = ui.min_rect().width();
            });
            assert!(taken >= room - 1.0, "note {note:?}: {taken} of {room}");
        }
    }
}
