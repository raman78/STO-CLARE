use eframe::egui::*;
use itertools::Itertools;

use crate::{
    analyzer::*,
    app::settings::CombatNotes,
    custom_widgets::popup_button::PopupButton,
    helpers::{
        format_duration, number_formatting::NumberFormatter, time_range_to_duration_or_zero,
    },
};

pub struct SummaryCopy {
    aspects: Vec<Aspect>,
    /// Whether the user's own note for the combat goes into the summary. On by
    /// default: a note says which build or which attempt the run was, which is
    /// the one thing the numbers cannot say and the reason the summary is being
    /// pasted at all.
    include_note: bool,
}

struct Aspect {
    name: &'static str,
    header: &'static str,
    include: bool,
    get: fn(&Player) -> f64,
    format: fn(f64, &mut NumberFormatter) -> String,
    reverse_sort: bool,
}

impl SummaryCopy {
    pub fn show(&mut self, combat: Option<&Combat>, notes: &CombatNotes, ui: &mut Ui) {
        if ui
            .add_enabled(combat.is_some(), Button::new("Copy Combat Summary"))
            .clicked()
        {
            let combat = combat.unwrap();
            ui.ctx()
                .copy_text(self.build_summary(combat, notes.get(&CombatNotes::key(combat))));
        }

        ui.add_enabled(combat.is_some(), |ui: &mut Ui| {
            PopupButton::new("⛭")
                .show(ui, |ui| {
                    ui.label("Configure copy elements");
                    ui.checkbox(&mut self.include_note, "Your note for the combat")
                        .on_hover_text(
                            "Adds the note you wrote for this combat after its name. Nothing is \
                             added when there is no note.",
                        );
                    for aspect in self.aspects.iter_mut() {
                        ui.checkbox(&mut aspect.include, aspect.name);
                    }

                    ui.label("Limit the number of elements,\nif you wish to paste the summary into the game chat.\nSo that it will not be truncated by the game.");
                })
                .response
        });
    }

    fn build_summary(&self, combat: &Combat, note: &str) -> String {
        let mut number_formatter = NumberFormatter::new();
        let aspects = self.aspects.iter().filter(|a| a.include);
        let first_aspect = aspects.clone().nth(0).unwrap_or(&self.aspects[0]);
        let players = combat
            .players
            .values()
            .sorted_by(|p1, p2| {
                let cmp = (first_aspect.get)(p1).total_cmp(&(first_aspect.get)(p2));
                if first_aspect.reverse_sort {
                    return cmp.reverse();
                }
                cmp
            })
            .map(|p| {
                let aspects = aspects
                    .clone()
                    .map(|a| {
                        let value = (a.get)(p);
                        (a.format)(value, &mut number_formatter)
                    })
                    .join("|");

                format!(
                    "{} {}",
                    String::from_iter(
                        p.damage_in
                            .name()
                            .get(&combat.name_manager)
                            .chars()
                            .skip_while(|c| *c != '@')
                    ),
                    aspects
                )
            });

        let aspects = aspects.clone().map(|a| a.header).join("|");
        let aspects_header = format!("Name {}", aspects);

        let header_and_players = std::iter::once(aspects_header).chain(players).join(" / ");

        let duration = format_duration(time_range_to_duration_or_zero(&combat.combat_time));

        format!(
            "CLA - {}{} ({}): {}",
            combat.name(),
            self.note_part(note),
            duration,
            header_and_players
        )
    }

    /// The note as it reads after the combat's name, or nothing at all — a run
    /// nobody wrote a note for must not leave a dash hanging in the middle of a
    /// line pasted into the game chat.
    fn note_part(&self, note: &str) -> String {
        if !self.include_note || note.trim().is_empty() {
            return String::new();
        }
        format!(" — {}", note.trim())
    }
}

impl Default for SummaryCopy {
    fn default() -> Self {
        Self {
            aspects: vec![
                aspect(
                    "DPS",
                    "DPS",
                    true,
                    |p| p.damage_out.dps.all,
                    |v, f| f.format_with_automated_suffixes(v),
                    true,
                ),
                aspect(
                    "Damage",
                    "Dmg",
                    false,
                    |p| p.damage_out.total_damage.all,
                    |v, f| f.format_with_automated_suffixes(v),
                    true,
                ),
                aspect(
                    "Damage %",
                    "Dmg%",
                    false,
                    |p| p.damage_out.damage_percentage.all.unwrap_or(0.0),
                    |v, f| f.format(v, 1),
                    true,
                ),
                aspect(
                    "Critical %",
                    "Crit%",
                    false,
                    |p| p.damage_out.critical_percentage.unwrap_or(0.0),
                    |v, f| f.format(v, 1),
                    true,
                ),
                aspect(
                    "Damage Resistance Out %",
                    "DmgResOut%",
                    false,
                    |p| p.damage_out.damage_resistance_percentage.unwrap_or(0.0),
                    |v, f| f.format(v, 1),
                    false,
                ),
                aspect(
                    "Damage In",
                    "DmgIn",
                    false,
                    |p| p.damage_in.total_damage.all,
                    |v, f| f.format_with_automated_suffixes(v),
                    true,
                ),
                aspect(
                    "Damage In %",
                    "DmgIn%",
                    false,
                    |p| p.damage_in.damage_percentage.all.unwrap_or(0.0),
                    |v, f| f.format(v, 1),
                    true,
                ),
                aspect(
                    "Damage Resistance In %",
                    "DmgResIn%",
                    false,
                    |p| p.damage_in.damage_resistance_percentage.unwrap_or(0.0),
                    |v, f| f.format(v, 1),
                    true,
                ),
            ],
            include_note: true,
        }
    }
}

fn aspect(
    name: &'static str,
    header: &'static str,
    include: bool,
    get: fn(&Player) -> f64,
    format: fn(f64, &mut NumberFormatter) -> String,
    reverse_sort: bool,
) -> Aspect {
    Aspect {
        name,
        header,
        include,
        get,
        format,
        reverse_sort,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The note is on to begin with — it is the one thing in the line the
    /// numbers cannot say.
    #[test]
    fn the_note_is_included_by_default() {
        let copy = SummaryCopy::default();
        assert_eq!(" — Cheops build", copy.note_part("Cheops build"));
    }

    /// A run nobody named must not leave a dash hanging in the middle of a line
    /// pasted into the game chat — nor must whitespace somebody typed by
    /// accident count as a note.
    #[test]
    fn an_unnamed_run_adds_nothing() {
        let copy = SummaryCopy::default();
        assert_eq!("", copy.note_part(""));
        assert_eq!("", copy.note_part("   "));
    }

    /// Switched off, the note stays out however long it is.
    #[test]
    fn the_note_can_be_switched_off() {
        let copy = SummaryCopy {
            include_note: false,
            ..Default::default()
        };
        assert_eq!("", copy.note_part("Cheops build"));
    }

    /// A note is trimmed on the way in, so a stray space does not read as two.
    #[test]
    fn a_note_is_trimmed() {
        let copy = SummaryCopy::default();
        assert_eq!(" — FAW build", copy.note_part("  FAW build  "));
    }
}
