use std::f64::consts::PI;

use eframe::egui::*;
use egui_plot::*;
use itertools::Itertools;

use crate::{
    app::{settings::Settings, theme},
    helpers::number_formatting::NumberFormatter,
};

use super::common::*;

const SAMPLE_RATE: f64 = 10.0;

/// Half-width of the smoothing kernel, in standard deviations. Past it the
/// weight counts as zero, which is what lets the sampling walk stop instead of
/// running over every value in the fight.
const KERNEL_CUTOFF_SIGMAS: f64 = 4.0;

/// How much of a normal distribution lies within [`KERNEL_CUTOFF_SIGMAS`]. The
/// kernel is divided by it so that the *cut* kernel still integrates to one,
/// and a value per second therefore comes out at its true height.
///
/// The cut used to be made by subtracting a constant from the kernel and
/// compensating with a fixed 0.1%, which does not depend on the standard
/// deviation while the mass lost that way does: the line read 0.2% low at the
/// default smoothing of 0.4, 3.8% low at the slider's 6.0 and 60% low at the
/// 120 the text field accepts.
const KERNEL_MASS_WITHIN_CUTOFF: f64 = 0.999_936_657_5;

pub struct ValuePerSecondGraph<T: PreparedValue> {
    lines: Vec<GraphLine<T>>,
    largest_point: f64,
    newly_created: bool,
    updated_filter: Option<f64>,
    diagram_type: DiagramType,
    components: HealComponents,
}

pub type DpsGraph = ValuePerSecondGraph<PreparedHitValue>;
pub type HitsPerSecondGraph = ValuePerSecondGraph<PreparedHitValue>;
pub type HpsGraph = ValuePerSecondGraph<PreparedHealValue>;
pub type HealTicksPerSeondGraph = ValuePerSecondGraph<PreparedHealValue>;

pub struct GraphLine<T: PreparedValue> {
    points: Vec<[f64; 2]>,
    data: PreparedDataSet<T>,
}

impl<T: PreparedValue> ValuePerSecondGraph<T> {
    pub fn empty(diagram_type: DiagramType) -> Self {
        Self {
            lines: Vec::new(),
            largest_point: 100_000.0,
            newly_created: true,
            updated_filter: None,
            diagram_type,
            components: HealComponents::ALL,
        }
    }

    pub fn from_data(
        diagram_type: DiagramType,
        lines: impl Iterator<Item = PreparedDataSet<T>>,
        filter: f64,
    ) -> Self {
        let lines: Vec<_> = lines.map(|l| GraphLine::new(l)).collect();
        let mut _self = Self {
            lines,
            updated_filter: Some(filter),
            ..Self::empty(diagram_type)
        };
        _self.sort();
        _self.compute_largest_point();

        _self
    }

    pub fn add_line(&mut self, line: PreparedDataSet<T>, filter: f64) {
        self.lines.push(GraphLine::new(line));
        self.sort();
        self.compute_largest_point();
        self.update(filter);
    }

    pub fn remove_line(&mut self, line: &str) {
        if let Some((index, _)) = self.lines.iter().find_position(|l| l.data.name == line) {
            self.lines.remove(index);
            // The tallest line may have been the one just dropped, and the
            // y range includes this figure — without recomputing, the chart
            // keeps room for a line that is no longer drawn.
            self.compute_largest_point();
        }
    }

    /// Largest total first, the same order the bar charts use, so a series
    /// keeps its colour and its place in the legend from one chart to the next.
    fn sort(&mut self) {
        self.lines.sort_unstable_by(|l1, l2| {
            l1.data
                .total_value
                .total_cmp(&l2.data.total_value)
                .reverse()
        });
    }

    /// Which halves of a heal to draw. Only the heal charts offer the choice;
    /// everything else stays on the default of both.
    pub fn set_components(&mut self, components: HealComponents) {
        self.components = components;
    }

    pub fn update(&mut self, filter: f64) {
        self.updated_filter = Some(filter);
    }

    pub fn show(&mut self, settings: &Settings, ui: &mut Ui) {
        if let Some(filter) = self.updated_filter.take() {
            self.lines
                .iter_mut()
                .for_each(|l| l.update(filter, self.diagram_type, self.components));
            self.compute_largest_point();
        }

        let mut plot = Plot::new(("per second graph", self.diagram_type.name()))
            .auto_bounds(true)
            .y_axis_min_width(y_axis_width(ui))
            .y_axis_formatter(format_axis)
            .x_axis_formatter(format_axis)
            .label_formatter(|n, p| {
                Self::format_label(n, p, self.diagram_type.value_name(), settings)
            })
            .include_y(self.largest_point)
            .legend(Legend::default());

        if self.newly_created {
            plot = plot.reset();
            self.newly_created = false;
        }

        if self.lines.is_empty() {
            plot = plot.include_x(60.0);
        }

        plot.show(ui, |p| {
            // Series are sorted largest first, so the colour a series gets is
            // the same on every chart it appears on.
            for (index, line) in self.lines.iter().enumerate() {
                p.line(line.to_line().color(theme::series_color(index)));
            }
        });
    }

    pub fn format_label(
        name: &str,
        point: &PlotPoint,
        value_name: &str,
        settings: &Settings,
    ) -> String {
        let mut formatter = NumberFormatter::new();
        let y = formatter.format(point.y, if settings.general.more_decimals { 2 } else { 0 });
        let x = formatter.format(point.x, 1);
        if name.is_empty() {
            return format!("{}: {}\nTime: {}", value_name, y, x);
        }
        format!("{}\n{}: {}\nTime: {}", name, value_name, y, x)
    }

    fn compute_largest_point(&mut self) {
        self.largest_point = self
            .lines
            .iter()
            .flat_map(|l| l.points.iter())
            .map(|p| p[1])
            .max_by(|p1, p2| p1.total_cmp(p2))
            .unwrap_or(0.0);
    }
}

impl<T: PreparedValue> GraphLine<T> {
    fn new(data: PreparedDataSet<T>) -> Self {
        Self {
            points: Vec::new(),
            data,
        }
    }

    fn update(&mut self, filter: f64, diagram_type: DiagramType, components: HealComponents) {
        let duration = self.data.duration_s.max(1.0);
        let points_count = (duration * SAMPLE_RATE).round().max(1.0) as _;
        let mut points = Vec::with_capacity(points_count);
        for i in 0..points_count {
            let start_offset = i as f64 / (points_count - 1) as f64;
            let time = self.data.start_time_s + duration * start_offset;
            let point = [
                time,
                Self::get_sample_gauss_filtered(
                    &self.data.values,
                    time,
                    filter,
                    diagram_type,
                    components,
                ),
            ];
            points.push(point);
        }

        self.points = points;
    }

    fn get_sample_entry(points: &[PreparedPoint<T>], time_millis: u32) -> usize {
        match points.binary_search_by_key(&time_millis, |h| h.time_millis) {
            Ok(i) => i,
            Err(i) => i,
        }
    }

    fn gauss_probability_density_function(t: f64, offset: f64, standard_deviation: f64) -> f64 {
        let t_sub_off_over_sigma = (t - offset) / standard_deviation;
        1.0 / (standard_deviation * f64::sqrt(2.0 * PI))
            * f64::exp(-0.5 * t_sub_off_over_sigma * t_sub_off_over_sigma)
    }

    fn get_gauss_value(
        points: &[PreparedPoint<T>],
        index: usize,
        time_seconds: f64,
        sigma_seconds: f64,
        diagram_type: DiagramType,
        components: HealComponents,
    ) -> Option<f64> {
        let hit = points.get(index)?;
        let t = millis_to_seconds(hit.time_millis);
        // Outside the kernel's half-width there is nothing left to add, and
        // since the walk moves away from the sample in time, nothing beyond
        // this point can come back into it either.
        if (t - time_seconds).abs() > KERNEL_CUTOFF_SIGMAS * sigma_seconds {
            return None;
        }
        let weight = Self::gauss_probability_density_function(t, time_seconds, sigma_seconds)
            / KERNEL_MASS_WITHIN_CUTOFF;

        Some(weight * hit.value(diagram_type, components))
    }

    fn get_sample_gauss_filtered_half(
        points: &[PreparedPoint<T>],
        time_seconds: f64,
        sigma_seconds: f64,
        entry_index: usize,
        diagram_type: DiagramType,
        components: HealComponents,
        mut index_change: impl FnMut(usize) -> Option<usize>,
    ) -> f64 {
        let mut value = 0.0;
        let mut index = entry_index;
        loop {
            value += match Self::get_gauss_value(
                points,
                index,
                time_seconds,
                sigma_seconds,
                diagram_type,
                components,
            ) {
                Some(v) => v,
                None => break,
            };
            index = match index_change(index) {
                Some(i) => i,
                None => break,
            };
        }

        value
    }

    fn get_sample_gauss_filtered(
        points: &[PreparedPoint<T>],
        time_seconds: f64,
        sigma_seconds: f64,
        diagram_type: DiagramType,
        components: HealComponents,
    ) -> f64 {
        let time_millis = seconds_to_millis(time_seconds);

        let entry_index = Self::get_sample_entry(points, time_millis);

        entry_index
            .checked_sub(1)
            .map(|i| {
                Self::get_sample_gauss_filtered_half(
                    points,
                    time_seconds,
                    sigma_seconds,
                    i,
                    diagram_type,
                    components,
                    |i| i.checked_sub(1),
                )
            })
            .unwrap_or(0.0)
            + Self::get_gauss_value(
                points,
                entry_index,
                time_seconds,
                sigma_seconds,
                diagram_type,
                components,
            )
            .unwrap_or(0.0)
            + Self::get_sample_gauss_filtered_half(
                points,
                time_seconds,
                sigma_seconds,
                entry_index + 1,
                diagram_type,
                components,
                |i| Some(i + 1),
            )
    }

    fn to_line(&self) -> Line<'_> {
        Line::new(&self.data.name, self.points.clone()).width(2.0)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::app::main_tabs::diagrams::common::{PreparedHitValue, PreparedPoint};

    /// Steady fire: one hit every tenth of a second, so the true rate is a
    /// round number and any bias in the kernel shows up directly.
    fn steady_fire(damage_per_hit: f64, seconds: u32) -> Vec<PreparedPoint<PreparedHitValue>> {
        (0..seconds * 10)
            .map(|i| PreparedPoint {
                value: PreparedHitValue {
                    damage: damage_per_hit,
                    hull_damage: damage_per_hit,
                    shield_damage: 0.0,
                    base_damage: damage_per_hit,
                    drain_damage: 0.0,
                    damage_prevented_to_hull: 0.0,
                    hits_count: 1,
                },
                time_millis: i * 100,
            })
            .collect()
    }

    /// The height of the line must be the rate itself, whatever the smoothing.
    /// The kernel used to be cut by subtracting a constant and compensating with
    /// a fixed 0.1%, so the line read low — by 3.8% at the slider's widest and
    /// by 60% at the widest the text field takes.
    #[test]
    fn the_smoothing_width_does_not_change_the_height_of_the_line() {
        // 10 hits a second of 100 damage each
        let expected = 1000.0;

        for sigma in [0.4f64, 1.0, 2.0, 6.0, 30.0, 120.0] {
            // Long enough that the kernel sits inside the fight: a kernel
            // hanging over the start or the end of the record reads low
            // whatever the normalisation, which is a property of smoothing a
            // finite fight and not what this test is about.
            let seconds = (sigma * 10.0).max(60.0) as u32;
            let points = steady_fire(100.0, seconds);
            let sampled = GraphLine::<PreparedHitValue>::get_sample_gauss_filtered(
                &points,
                seconds as f64 / 2.0,
                sigma,
                DiagramType::Dps,
                HealComponents::ALL,
            );
            let error = (sampled / expected - 1.0).abs();
            assert!(
                error < 0.01,
                "at sigma {sigma} the line reads {sampled:.1} instead of {expected:.1} \
                 ({:.1}% off)",
                error * 100.0
            );
        }
    }

    /// Series are drawn largest first in every chart, so one player keeps one
    /// colour and one place in the legend across all of them.
    #[test]
    fn lines_are_ordered_by_their_total() {
        let line = |name: &str, total: f64| PreparedDataSet {
            name: name.to_string(),
            total_value: total,
            values: Arc::from(Vec::new()),
            start_time_s: 0.0,
            duration_s: 10.0,
        };
        let graph = ValuePerSecondGraph::<PreparedHitValue>::from_data(
            DiagramType::Dps,
            [line("small", 10.0), line("big", 100.0), line("mid", 50.0)].into_iter(),
            1.0,
        );

        let names: Vec<&str> = graph.lines.iter().map(|l| l.data.name.as_str()).collect();
        assert_eq!(vec!["big", "mid", "small"], names);
    }

    /// Dropping the tallest line has to shrink the y range with it, or the
    /// chart keeps room for something it no longer draws.
    #[test]
    fn removing_a_line_lets_the_y_range_shrink() {
        let mut graph = ValuePerSecondGraph::<PreparedHitValue>::from_data(
            DiagramType::Dps,
            [PreparedDataSet {
                name: "only".to_string(),
                total_value: 100.0,
                values: Arc::from(steady_fire(100.0, 10)),
                start_time_s: 0.0,
                duration_s: 10.0,
            }]
            .into_iter(),
            1.0,
        );
        graph.lines[0].update(1.0, DiagramType::Dps, HealComponents::ALL);
        graph.compute_largest_point();
        assert!(graph.largest_point > 0.0);

        graph.remove_line("only");
        assert_eq!(0.0, graph.largest_point);
    }
}
