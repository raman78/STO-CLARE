use eframe::egui::*;

use crate::{
    analyzer::*,
    app::settings::Settings,
    custom_widgets::{splitter::Splitter, table::*},
    helpers::{number_formatting::NumberFormatter, *},
};

use super::{common::*, diagrams::SummaryChart, tables::SummaryTable};

pub struct SummaryTab {
    identifier: String,
    name: String,

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

    pub fn show(&mut self, settings: &Settings, top_ui: &mut Ui) {
        top_ui.heading(&self.name);

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
                    for name in ["All", "Hull", "Shield"] {
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
            value.show(r);
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
