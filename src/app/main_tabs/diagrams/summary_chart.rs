use eframe::egui::Ui;
use egui_plot::*;

use crate::{app::settings::Settings, helpers::number_formatting::NumberFormatter};

use super::common::*;

/// Width of a bar in plot units — egui_plot's own default, spelled out here
/// because the label formatter needs it to tell which bar the pointer is over.
const BAR_WIDTH: f64 = 0.5;

pub struct SummaryChart {
    identifier: String,
    /// What the bars measure, for the hover label. The same widget draws DPS
    /// and two damage totals, and it used to call all three "DPS".
    value_name: &'static str,
    players: Vec<Bar>,
}

impl SummaryChart {
    pub fn empty() -> Self {
        Self {
            identifier: String::new(),
            value_name: "DPS",
            players: Default::default(),
        }
    }

    pub fn from_data<'a>(
        identifier: &str,
        value_name: &'static str,
        players: impl Iterator<Item = (&'a str, f64)>,
    ) -> Self {
        let mut players: Vec<_> = players.map(|(n, v)| Bar::new(0.0, v).name(n)).collect();

        players.sort_unstable_by(|p1, p2| p1.value.total_cmp(&p2.value).reverse());

        players.iter_mut().enumerate().for_each(|(i, p)| {
            p.argument = i as f64 + 1.0;
        });

        Self {
            identifier: identifier.to_string(),
            value_name,
            players,
        }
    }

    pub fn show(&mut self, settings: &Settings, ui: &mut Ui) {
        let more_decimals = settings.general.more_decimals;
        let value_name = self.value_name;
        // Name and value per bar, so the label can be built from the pointer
        // position alone (see the label formatter below).
        let bars: Vec<(String, f64, f64)> = self
            .players
            .iter()
            .map(|bar| (bar.name.clone(), bar.argument, bar.value))
            .collect();
        Plot::new(&self.identifier)
            .auto_bounds(true)
            .y_axis_formatter(|_, _| String::new())
            .x_axis_formatter(format_axis)
            // With the bars themselves not hoverable (below), this is what
            // describes the one under the pointer. It is the same framed
            // tooltip the line charts use, which egui keeps on screen by
            // turning it over at an edge — unlike the label egui_plot paints
            // for a bar, which only ever grows upwards and is cut off by the
            // top of the frame.
            .label_formatter(move |_, point| {
                let mut formatter = NumberFormatter::new();
                let precision = if more_decimals { 2 } else { 0 };
                match bars
                    .iter()
                    .find(|(_, argument, value)| {
                        (point.y - argument).abs() <= BAR_WIDTH / 2.0
                            && (0.0..=*value).contains(&point.x)
                    }) {
                    Some((name, _, value)) => {
                        format!("{}\n{}: {}", name, value_name, formatter.format(*value, precision))
                    }
                    None => format!("{}: {}", value_name, formatter.format(point.x, precision)),
                }
            })
            .y_axis_min_width(0.0)
            .legend(Legend::default())
            .include_y(0.0)
            .show(ui, |p| {
                for player in self.players.iter() {
                    let chart = BarChart::new(&player.name, vec![player.clone()])
                        // Hovering a bar would make egui_plot paint its own
                        // label instead — the one that grows upwards out of
                        // the frame. Leaving the bars out of hover testing
                        // hands the job to the plot's label formatter, which
                        // shows the framed tooltip.
                        .allow_hover(false)
                        .horizontal();
                    p.bar_chart(chart);
                }
            });
    }
}
