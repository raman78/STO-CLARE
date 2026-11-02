use std::f64::consts::PI;

use eframe::egui::*;
use egui_plot::*;
use itertools::Itertools;

use crate::{app::settings::Settings, helpers::number_formatting::NumberFormatter};

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

/// How many series the range summary lists before it says how many more there
/// are. A tree level can chart dozens of abilities, and a box that long would
/// cover the chart it describes.
const RANGE_SUMMARY_MAX_SERIES: usize = 10;

pub struct ValuePerSecondGraph<T: PreparedValue> {
    lines: Vec<GraphLine<T>>,
    largest_point: f64,
    newly_created: bool,
    updated_filter: Option<f64>,
    diagram_type: DiagramType,
    components: HealComponents,
    /// The stretch of the fight picked out with Shift and a drag, in seconds
    /// from its start, lower bound first. The chart shades it and lists every
    /// series' average over it.
    selection: Option<(f64, f64)>,
    /// Where a Shift drag began, while it is still under way.
    selecting_from: Option<f64>,
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
            selection: None,
            selecting_from: None,
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

        // A drag with Shift held picks out a stretch of time instead of moving
        // the chart, so the pan is off while Shift is down — and for the rest of
        // a selection already begun, or letting go of Shift halfway would turn
        // the same drag into a pan.
        let shift = ui.input(|i| i.modifiers.shift);
        let pan = if shift || self.selecting_from.is_some() {
            Vec2b::FALSE
        } else {
            PAN_SIDEWAYS_ONLY
        };

        // Set explicitly so the legend's record of hidden series can be read
        // back after the plot is drawn.
        let plot_id = ui.make_persistent_id(("per second graph", self.diagram_type.name()));
        let diagram_type = self.diagram_type;
        let mut plot = Plot::new(("per second graph", self.diagram_type.name()))
            .id(plot_id)
            .auto_bounds(true)
            .allow_scroll(PAN_SIDEWAYS_ONLY)
            .allow_drag(pan)
            .y_axis_min_width(y_axis_width(ui))
            .y_axis_formatter(format_axis)
            .x_axis_formatter(format_axis)
            .label_formatter(move |n, p| {
                Self::format_label(n, p, diagram_type.value_name(), settings)
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

        let fight = self.time_range();
        let selection_color = ui.visuals().selection.bg_fill;
        let lines = &self.lines;
        let selection = &mut self.selection;
        let selecting_from = &mut self.selecting_from;
        let plot_response = plot.show(ui, |p| {
            Self::track_selection(p, shift, fight, selection, selecting_from);

            if let Some((from, to)) = *selection {
                p.span(
                    Span::new("", from..=to)
                        .fill(selection_color.gamma_multiply(0.25))
                        .border_color(selection_color),
                );
            }

            // Series are sorted largest first, so the colour a series gets is
            // the same on every chart it appears on — unless it brought a
            // colour of its own, which is how a caller keeps one series the
            // same colour while the totals move under it.
            for (index, line) in lines.iter().enumerate() {
                p.line(line.to_line().color(series_color(&line.data, index)));
            }
        });

        if let Some(range) = self.selection {
            let hidden = PlotMemory::load(ui.ctx(), plot_id)
                .map(|m| m.hidden_items)
                .unwrap_or_default();
            self.show_range_summary(settings, ui, plot_response.response.rect, range, |name| {
                hidden.contains(&line_id(name))
            });
        }
    }

    /// The span every series covers: from the start of the fight to its end.
    fn time_range(&self) -> (f64, f64) {
        let start = self
            .lines
            .iter()
            .map(|l| l.data.start_time_s)
            .min_by(f64::total_cmp)
            .unwrap_or(0.0);
        let end = self
            .lines
            .iter()
            .map(|l| l.data.start_time_s + l.data.duration_s)
            .max_by(f64::total_cmp)
            .unwrap_or(0.0);
        (start, end)
    }

    /// Follows a Shift drag across the chart. The ends are held to the fight,
    /// so dragging past either edge selects up to the edge rather than adding
    /// seconds in which nothing could have happened — the shaded band shows
    /// where the range really ends. A plain click clears the selection.
    fn track_selection(
        p: &PlotUi,
        shift: bool,
        (fight_start, fight_end): (f64, f64),
        selection: &mut Option<(f64, f64)>,
        selecting_from: &mut Option<f64>,
    ) {
        let response = p.response().clone();
        let to_time = |pos: Pos2| p.plot_from_screen(pos).x.clamp(fight_start, fight_end);

        if shift && response.drag_started_by(PointerButton::Primary) {
            // The press, not where the drag was recognised: egui only calls it a
            // drag once the pointer has moved a few points.
            *selecting_from = p
                .ctx()
                .input(|i| i.pointer.press_origin())
                .map(to_time);
        }

        if let Some(from) = *selecting_from {
            if let Some(pos) = p.ctx().input(|i| i.pointer.latest_pos()) {
                let to = to_time(pos);
                *selection = Some((from.min(to), from.max(to)));
            }
            if response.drag_stopped() {
                *selecting_from = None;
                // A drag that ended where it began, or wholly outside the
                // fight, has no length to average over.
                if selection.is_some_and(|(from, to)| to <= from) {
                    *selection = None;
                }
            }
        } else if response.clicked() {
            *selection = None;
        }
    }

    /// The box in the chart's top left corner: the selected range and each
    /// shown series' average over it, in the series' colour and in the
    /// legend's order.
    fn show_range_summary(
        &self,
        settings: &Settings,
        ui: &mut Ui,
        plot_rect: Rect,
        (from, to): (f64, f64),
        is_hidden: impl Fn(&str) -> bool,
    ) {
        let decimals = if settings.general.more_decimals { 2 } else { 0 };
        let mut formatter = NumberFormatter::new();
        let value_name = self.diagram_type.value_name();
        let shown = self
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| !is_hidden(&l.data.name))
            .collect_vec();

        let area = plot_rect.shrink(ui.spacing().item_spacing.x * 2.0);
        let mut overlay = ui.new_child(
            UiBuilder::new()
                .max_rect(area)
                .layout(Layout::top_down(Align::LEFT)),
        );
        overlay.set_clip_rect(plot_rect);
        Frame::popup(ui.style()).show(&mut overlay, |ui| {
            ui.label(format!(
                "{} s – {} s ({} s)",
                formatter.format(from, 1),
                formatter.format(to, 1),
                formatter.format(to - from, 1),
            ));
            for (index, line) in shown.iter().take(RANGE_SUMMARY_MAX_SERIES) {
                let average = formatter.format(
                    line.average_over(from, to, self.diagram_type, self.components),
                    decimals,
                );
                let text = if line.data.name.is_empty() {
                    format!("{value_name}: {average}")
                } else {
                    format!("{}: {average} {value_name}", line.data.name)
                };
                ui.label(RichText::new(text).color(series_color(&line.data, *index)));
            }
            if shown.len() > RANGE_SUMMARY_MAX_SERIES {
                ui.label(format!(
                    "… and {} more",
                    shown.len() - RANGE_SUMMARY_MAX_SERIES
                ));
            }
            if shown.is_empty() {
                ui.label("No series shown");
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

    /// The series' average per second from `from_s` to `to_s`: everything it
    /// did in that stretch over the stretch's length.
    ///
    /// The range is half-open, so two ranges laid end to end count each value
    /// once and steady fire averages to its true rate — except at the end of
    /// the fight, which is the time of its last value: a range reaching it
    /// includes that value, and the whole fight then averages to the series'
    /// total over its duration, as the tables count it.
    ///
    /// Taken from the values themselves rather than from the drawn line. The
    /// line is smoothed, so it leaks across the ends of the range, and an
    /// average read off it would move with the smoothing slider.
    fn average_over(
        &self,
        from_s: f64,
        to_s: f64,
        diagram_type: DiagramType,
        components: HealComponents,
    ) -> f64 {
        let length = to_s - from_s;
        if length <= 0.0 {
            return 0.0;
        }
        let from = seconds_to_millis(from_s);
        let to = seconds_to_millis(to_s);
        let reaches_the_end = to >= seconds_to_millis(self.data.start_time_s + self.data.duration_s);
        let values = &*self.data.values;
        let first = values.partition_point(|v| v.time_millis < from);
        let past_last = if reaches_the_end {
            values.partition_point(|v| v.time_millis <= to)
        } else {
            values.partition_point(|v| v.time_millis < to)
        };
        let total: f64 = values[first..past_last.max(first)]
            .iter()
            .map(|v| v.value(diagram_type, components))
            .sum();
        total / length
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
        Line::new(&self.data.name, self.points.clone())
            .id(line_id(&self.data.name))
            .width(2.0)
    }
}

/// The id a series is drawn under, which is also how the legend records that
/// it was hidden.
fn line_id(name: &str) -> Id {
    Id::new(("per second line", name))
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
                    hits_count: 1.0,
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
            color: None,
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
                color: None,
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

    fn steady_line(seconds: u32) -> GraphLine<PreparedHitValue> {
        let points = steady_fire(100.0, seconds);
        let last = millis_to_seconds(points.last().unwrap().time_millis);
        GraphLine::new(PreparedDataSet {
            name: "steady".to_string(),
            total_value: 100.0 * points.len() as f64,
            values: Arc::from(points),
            start_time_s: 0.0,
            duration_s: last,
            color: None,
        })
    }

    /// A range inside the fight counts each value once: steady fire of 1000
    /// a second averages to exactly 1000 over any whole number of tenths.
    #[test]
    fn a_range_inside_the_fight_averages_to_the_true_rate() {
        let line = steady_line(60);
        for (from, to) in [(10.0, 20.0), (0.0, 0.5), (12.3, 45.6)] {
            let average = line.average_over(from, to, DiagramType::Dps, HealComponents::ALL);
            assert!(
                (average - 1000.0).abs() < 1e-9,
                "{from}..{to} averages to {average}"
            );
        }
    }

    /// Two ranges laid end to end add up to the range they make together.
    #[test]
    fn adjoining_ranges_do_not_count_the_shared_end_twice() {
        let line = steady_line(60);
        let average = |from, to| line.average_over(from, to, DiagramType::Dps, HealComponents::ALL);
        let joined = average(5.0, 25.0) * 20.0;
        let parts = average(5.0, 15.0) * 10.0 + average(15.0, 25.0) * 10.0;
        assert!((joined - parts).abs() < 1e-9, "{joined} vs {parts}");
    }

    /// The whole fight, last value included, gives what the tables give: the
    /// total over the duration.
    #[test]
    fn the_whole_fight_averages_to_the_total_over_the_duration() {
        let line = steady_line(60);
        let average = line.average_over(
            0.0,
            line.data.duration_s,
            DiagramType::Dps,
            HealComponents::ALL,
        );
        let expected = line.data.total_value / line.data.duration_s;
        assert!((average - expected).abs() < 1e-9, "{average} vs {expected}");
    }
}
