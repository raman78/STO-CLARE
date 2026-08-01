use eframe::egui::*;

use crate::{
    analyzer::*,
    app::settings::{CombatNotes, MAX_NOTE_CHARS, Settings},
    custom_widgets::{splitter::Splitter, table::*},
    helpers::{number_formatting::NumberFormatter, *},
};

use super::{common::*, diagrams::SummaryChart, tables::SummaryTable};

pub struct SummaryTab {
    identifier: String,
    name: String,
    /// Key of the shown combat's note, and the text being edited. The text is
    /// held here rather than edited straight in the settings so a keystroke
    /// does not write the settings file.
    note_key: String,
    note: String,

    combat_duration: TextDuration,
    active_duration: TextDuration,
    total_damage_out: ShieldAndHullTextValue,
    total_damage_in: ShieldAndHullTextValue,
    total_kills: TextCount,
    total_deaths: TextCount,
    summary_table: SummaryTable,
    summary_dps_chart: SummaryChart,
    summary_damage_out_chart: SummaryChart,
    summary_damage_in_chart: SummaryChart,

    chart_tab: ChartTab,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum ChartTab {
    #[default]
    Dps,
    DamageOut,
    DamageIn,
}

impl SummaryTab {
    pub fn empty() -> Self {
        let nothing_loaded = "<no data loaded>".to_string();
        Self {
            identifier: nothing_loaded.clone(),
            name: nothing_loaded,
            note_key: String::new(),
            note: String::new(),
            summary_table: SummaryTable::empty(),
            combat_duration: Default::default(),
            active_duration: Default::default(),
            total_damage_out: Default::default(),
            total_damage_in: Default::default(),
            total_kills: Default::default(),
            total_deaths: Default::default(),
            summary_dps_chart: SummaryChart::empty(),
            summary_damage_out_chart: SummaryChart::empty(),
            summary_damage_in_chart: SummaryChart::empty(),
            chart_tab: Default::default(),
        }
    }

    pub fn update(&mut self, settings: &Settings, combat: &Combat) {
        self.identifier = combat.identifier();
        self.name = combat.name();
        self.note_key = CombatNotes::key(combat);
        self.note = settings.combat_notes.get(&self.note_key).to_owned();

        self.combat_duration =
            TextDuration::new(time_range_to_duration_or_zero(&combat.combat_time));
        self.active_duration = TextDuration::new(time_range_to_duration(&combat.active_time));

        let mut number_formatter = NumberFormatter::new();
        self.total_damage_out = ShieldAndHullTextValue::new(
            &combat.total_damage_out,
            if settings.general.more_decimals { 2 } else { 0 },
            &mut number_formatter,
        );
        self.total_damage_in = ShieldAndHullTextValue::new(
            &combat.total_damage_in,
            if settings.general.more_decimals { 2 } else { 0 },
            &mut number_formatter,
        );
        self.total_kills = TextCount::new(combat.total_kills as _);
        self.total_deaths = TextCount::new(combat.total_deaths as _);

        self.summary_table = SummaryTable::new(settings, combat);
        self.summary_dps_chart = SummaryChart::from_data(
            "summary dps chart",
            combat.players.values().map(|p| {
                (
                    p.damage_out.name().get(&combat.name_manager),
                    p.damage_out.dps.all,
                )
            }),
        );
        self.summary_damage_out_chart = SummaryChart::from_data(
            "summary damage in chart",
            combat.players.values().map(|p| {
                (
                    p.damage_out.name().get(&combat.name_manager),
                    p.damage_out.total_damage.all,
                )
            }),
        );
        self.summary_damage_in_chart = SummaryChart::from_data(
            "summary damage out chart",
            combat.players.values().map(|p| {
                (
                    p.damage_out.name().get(&combat.name_manager),
                    p.damage_in.total_damage.all,
                )
            }),
        );
    }

    /// The user's own one-line description of this combat, repeated in the
    /// compare view's legend so a run can be told apart from the four others of
    /// the same map on the same evening.
    ///
    /// The text is written into the settings on every keystroke but saved to
    /// disk only once the field is left, so typing does not rewrite the
    /// settings file per character.
    fn show_note(&mut self, settings: &mut Settings, ui: &mut Ui) {
        // No combat loaded yet: there is nothing to attach a note to.
        if self.note_key.is_empty() {
            return;
        }

        // Room for the whole note at once. Measured from the font in use rather
        // than fixed, so it holds at any UI scale; the slack covers letters
        // wider than a digit, since the face is proportional and no width can
        // fit every possible fifty characters.
        let note_width = ui.fonts_mut(|fonts| {
            fonts.glyph_width(&TextStyle::Body.resolve(ui.style()), '0')
        }) * MAX_NOTE_CHARS as f32
            * 1.2;

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
        });
    }

    pub fn show(&mut self, settings: &mut Settings, top_ui: &mut Ui) {
        top_ui.heading(&self.name);
        self.show_note(settings, top_ui);

        Splitter::horizontal()
            .initial_ratio(0.7)
            .show(top_ui, |top_ui, bottom_ui| {
                ScrollArea::both()
                    .min_scrolled_height(0.0)
                    .show(top_ui, |ui| {
                        ui.add_space(20.0);

                        ui.push_id("combat summary table", |ui| {
                            self.show_combat_summary_table(
                                settings.general.split_shield_hull_columns,
                                ui,
                            );
                        });

                        ui.add_space(20.0);

                        self.summary_table.show(ui);
                    });

                bottom_ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.chart_tab, ChartTab::Dps, "DPS");
                    ui.selectable_value(&mut self.chart_tab, ChartTab::DamageOut, "Damage Dealt");
                    ui.selectable_value(&mut self.chart_tab, ChartTab::DamageIn, "Damage Taken");
                });

                match self.chart_tab {
                    ChartTab::Dps => self.summary_dps_chart.show(settings, bottom_ui),
                    ChartTab::DamageOut => self.summary_damage_out_chart.show(settings, bottom_ui),
                    ChartTab::DamageIn => self.summary_damage_in_chart.show(settings, bottom_ui),
                }
            });
    }

    fn show_combat_summary_table(&mut self, split: bool, ui: &mut Ui) {
        let body = |t: &mut TableBody| {
            Self::simple_summary_row(t, "Combat Duration", &self.combat_duration.text, split);
            Self::simple_summary_row(
                t,
                "Active Duration (duration of everything)",
                &self.active_duration.text,
                split,
            );

            Self::hull_shield_summary_row(t, "Total Damage Dealt", &self.total_damage_out, split);

            Self::hull_shield_summary_row(t, "Total Damage Taken", &self.total_damage_in, split);

            Self::simple_summary_row(t, "Total Kills", &self.total_kills.text, split);
            Self::simple_summary_row(t, "Total Deaths", &self.total_deaths.text, split);
        };

        // Without split columns this is a plain description/value list and needs
        // no header; with them the extra cells have to say what they are.
        if split {
            Table::new(ui)
                .header(HEADER_HEIGHT, |r| {
                    r.cell(|_| {});
                    // The All heading matches the totals under it (see
                    // `ShieldAndHullTextValue::show`); the halves stay plain.
                    r.cell_with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(bold_text("All"));
                    });
                    for name in ["Hull", "Shield"] {
                        r.cell_with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(name);
                        });
                    }
                })
                .body(ROW_HEIGHT, |t| body(t));
        } else {
            Table::new(ui).body(ROW_HEIGHT, |t| body(t));
        }
    }

    fn simple_summary_row(table: &mut TableBody, description: &str, value: &str, split: bool) {
        table.row(|r| {
            Self::show_description(r, description);
            r.cell_with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(value);
            });
            // Keep the row as wide as the split ones so the columns line up.
            if split {
                r.cell(|_| {});
                r.cell(|_| {});
            }
        });
    }

    fn hull_shield_summary_row(
        table: &mut TableBody,
        description: &str,
        value: &ShieldAndHullTextValue,
        split: bool,
    ) {
        table.row(|r| {
            Self::show_description(r, description);
            // Without the extra columns the tooltip is the only way to the
            // halves, so it comes back exactly then.
            value.show(r, !split);
            if split {
                value.show_hull(r);
                value.show_shield(r);
            }
        });
    }

    fn show_description(row: &mut TableRow, description: &str) {
        row.cell(|ui| {
            ui.label(description);
        });
    }
}
