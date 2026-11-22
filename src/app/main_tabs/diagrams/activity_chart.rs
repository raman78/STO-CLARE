//! When each series was producing values: one lane per series, one bar per
//! stretch of hits or heal ticks.
//!
//! The log records hits and heals, never the moment an ability is switched on
//! or a buff lands, so a stretch here is *when the series dealt damage or
//! healed*, not when it was active. A buff that deals no damage has no lane at
//! all, a damage-over-time effect stretches its bar past the activation, and
//! two uses closer together than the gap read as one stretch.

use eframe::egui::*;
use egui_plot::*;
use itertools::Itertools;

use crate::{app::settings::Settings, helpers::number_formatting::NumberFormatter};

use super::common::*;

/// Seconds without a value that end one stretch and start the next, until the
/// reader moves the slider.
pub const DEFAULT_ACTIVITY_GAP_S: f64 = 5.0;

/// How long a bar is drawn when its stretch is a single moment. A stretch of
/// one hit has no length and would not be drawn at all; the tooltip still
/// gives its real extent.
const MIN_DRAWN_LENGTH_S: f64 = 1.0;

/// How tall a lane's bar is, as a share of the distance between lanes.
const BAR_THICKNESS: f64 = 0.6;

/// Longest lane name the axis prints; the whole name is on the bar's tooltip.
const MAX_LABEL_CHARS: usize = 32;

/// One stretch in which a series kept producing values, none of them further
/// apart than the gap.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActivityWindow {
    pub start_s: f64,
    pub end_s: f64,
    /// How many values fell inside it, hits or heal ticks.
    pub values: usize,
}

impl ActivityWindow {
    pub fn length_s(&self) -> f64 {
        self.end_s - self.start_s
    }
}

/// Splits value times into stretches. A value more than `gap_s` after the one
/// before it opens a new stretch; one exactly `gap_s` after still belongs to
/// the old one.
///
/// `times_millis` must be in ascending order, which is how a prepared data set
/// holds them.
pub fn activity_windows(
    times_millis: impl IntoIterator<Item = u32>,
    gap_s: f64,
) -> Vec<ActivityWindow> {
    let gap_millis = seconds_to_millis(gap_s);
    let mut windows: Vec<ActivityWindow> = Vec::new();
    let mut last_millis = None;
    for time in times_millis {
        match (windows.last_mut(), last_millis) {
            (Some(window), Some(last)) if time - last <= gap_millis => {
                window.end_s = millis_to_seconds(time);
                window.values += 1;
            }
            _ => windows.push(ActivityWindow {
                start_s: millis_to_seconds(time),
                end_s: millis_to_seconds(time),
                values: 1,
            }),
        }
        last_millis = Some(time);
    }
    windows
}

/// How long a series was producing values over a fight, the figure the
/// Active column and the Activity chart's tooltips both state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActiveTime {
    pub seconds: f64,
    /// `seconds` as a share of the fight, 0 to 1.
    pub share: f64,
    pub stretches: usize,
}

/// The stretches of a series, counted the one way both the chart and the
/// table count them: a point is activity when it counts above zero
/// (`count_type`, `components`), and stretches are split at `gap_s`.
pub fn series_windows<T: PreparedValue>(
    data: &PreparedDataSet<T>,
    gap_s: f64,
    count_type: DiagramType,
    components: HealComponents,
) -> Vec<ActivityWindow> {
    let counted = data
        .values
        .iter()
        .filter(|p| p.value(count_type, components) > 0.0)
        .map(|p| p.time_millis);
    activity_windows(counted, gap_s)
}

/// The total of some stretches, against the fight the series spans.
pub fn active_time(windows: &[ActivityWindow], fight_s: f64) -> ActiveTime {
    // Folded from +0 rather than summed: an empty `f64` sum is -0, which a
    // row with no stretches would then show as "-0 s".
    let seconds = windows
        .iter()
        .map(ActivityWindow::length_s)
        .fold(0.0, |total, length| total + length);
    ActiveTime {
        seconds,
        share: if fight_s > 0.0 {
            seconds / fight_s
        } else {
            0.0
        },
        stretches: windows.len(),
    }
}

/// An [`ActiveTime`] written out, the one way the Active column and the
/// Activity chart both write it, so the two always read the same.
pub struct ActiveText {
    /// "118 s".
    pub seconds: String,
    /// "62%", or `None` with no time to take a share of.
    pub share: Option<String>,
}

impl ActiveText {
    pub fn new(
        active: ActiveTime,
        base_s: f64,
        more_decimals: bool,
        formatter: &mut NumberFormatter,
    ) -> Self {
        let seconds = format!("{} s", formatter.format(active.seconds, 0));
        let share = (base_s > 0.0).then(|| {
            let decimals = if more_decimals { 1 } else { 0 };
            format!("{}%", formatter.format(active.share * 100.0, decimals))
        });
        Self { seconds, share }
    }
}

/// "4 stretches", "1 stretch".
fn stretches_text(count: usize) -> String {
    match count {
        1 => "1 stretch".to_string(),
        n => format!("{n} stretches"),
    }
}

pub struct ActivityChart<T: PreparedValue> {
    /// What counts one value of a point: hits for damage, ticks for heals. A
    /// point that counts zero — a heal of the half the picker has switched
    /// off — is not activity.
    count_type: DiagramType,
    components: HealComponents,
    newly_created: bool,
    lanes: Vec<Lane<T>>,
    /// The row the lanes are the parts of, when the chart was opened on a row
    /// to show what is under it. With Total on it is drawn instead of them:
    /// one lane, under the row's own name, of everything the row holds.
    whole: Option<Lane<T>>,
    gap_s: f64,
    /// The lanes have to be worked out again before they are drawn.
    stale: bool,
    /// The stretch of the fight marked with Shift and a drag, in seconds, as
    /// on the per-second charts; the chart shades it and says how much of it
    /// each lane was active.
    selection: Option<(f64, f64)>,
    /// Where a Shift drag began, while it is still under way.
    selecting_from: Option<f64>,
}

pub type DamageActivityChart = ActivityChart<PreparedHitValue>;
pub type HealActivityChart = ActivityChart<PreparedHealValue>;

struct Lane<T: PreparedValue> {
    data: PreparedDataSet<T>,
    windows: Vec<ActivityWindow>,
}

impl<T: PreparedValue> ActivityChart<T> {
    pub fn empty(count_type: DiagramType) -> Self {
        Self {
            count_type,
            components: HealComponents::ALL,
            newly_created: true,
            lanes: Vec::new(),
            whole: None,
            gap_s: DEFAULT_ACTIVITY_GAP_S,
            stale: true,
            selection: None,
            selecting_from: None,
        }
    }

    pub fn from_data(
        count_type: DiagramType,
        data: impl Iterator<Item = PreparedDataSet<T>>,
        gap_s: f64,
    ) -> Self {
        let mut chart = Self {
            lanes: data.map(Lane::new).collect(),
            gap_s,
            ..Self::empty(count_type)
        };
        chart.sort();
        chart
    }

    pub fn add_lane(&mut self, data: PreparedDataSet<T>, gap_s: f64) {
        self.lanes.push(Lane::new(data));
        self.sort();
        self.update(gap_s);
    }

    /// The row the lanes are the parts of; see [`Self::whole`].
    pub fn set_whole(&mut self, data: PreparedDataSet<T>) {
        self.whole = Some(Lane::new(data));
        self.stale = true;
    }

    /// The row the lanes are the parts of, if the chart was opened on one.
    pub fn whole(&self) -> Option<PreparedDataSet<T>> {
        self.whole.as_ref().map(|lane| lane.data.clone())
    }

    /// The lanes drawn: the row itself with Total on and the chart opened on
    /// a row, every lane otherwise — each of which is already one row's own
    /// total when the rows were picked one by one.
    #[cfg(test)]
    fn shown(&self, total: bool) -> Vec<&Lane<T>> {
        shown_lanes(&self.whole, &self.lanes, total)
    }

    pub fn remove_lane(&mut self, name: &str) {
        if let Some((index, _)) = self.lanes.iter().find_position(|l| l.data.name == name) {
            self.lanes.remove(index);
            self.stale = true;
        }
    }

    /// Which halves of a heal count as activity; see
    /// `ValuePerSecondGraph::set_components`.
    pub fn set_components(&mut self, components: HealComponents) {
        self.components = components;
        self.stale = true;
    }

    pub fn update(&mut self, gap_s: f64) {
        self.gap_s = gap_s;
        self.stale = true;
    }

    /// Draws one lane per series, or — with `total`, on a chart opened on a
    /// row — one lane for the row itself.
    pub fn show(&mut self, settings: &Settings, ui: &mut Ui, total: bool) {
        if self.stale {
            self.stale = false;
            for lane in &mut self.lanes {
                lane.update(self.gap_s, self.count_type, self.components);
            }
            if let Some(whole) = &mut self.whole {
                whole.update(self.gap_s, self.count_type, self.components);
            }
        }
        // Taken before the lanes are borrowed for drawing.
        let reset = std::mem::take(&mut self.newly_created);
        let lanes = shown_lanes(&self.whole, &self.lanes, total);
        // The whole fight, whatever part of it the lanes happen to cover.
        let end = lanes.iter().map(|l| l.data.duration_s).fold(0.0, f64::max);
        let drawn: Vec<DrawnLane> = lanes
            .iter()
            .enumerate()
            .map(|(index, lane)| DrawnLane {
                name: lane.data.name.as_str(),
                windows: lane.windows.as_slice(),
                active_base_s: lane.data.active_base_s,
                color: series_color(&lane.data, index),
            })
            .collect();

        let names: Vec<String> = drawn.iter().map(|lane| lane.name.to_string()).collect();
        let lane_count = drawn.len();
        let label_width = axis_label_width(ui, &names);

        // A Shift drag marks a stretch instead of moving the chart, as on the
        // per-second charts.
        let shift = ui.input(|i| i.modifiers.shift);
        let pan = if shift || self.selecting_from.is_some() {
            Vec2b::FALSE
        } else {
            PAN_SIDEWAYS_ONLY
        };
        // Set explicitly so the legend's record of hidden lanes can be read
        // back after the plot is drawn.
        let plot_id = ui.make_persistent_id(("activity chart", self.count_type.name()));
        let mut plot = Plot::new(["activity chart", self.count_type.name()])
            .id(plot_id)
            .auto_bounds(true)
            .allow_scroll(PAN_SIDEWAYS_ONLY)
            .allow_drag(pan)
            .y_axis_min_width(label_width)
            .y_grid_spacer(move |_| {
                (0..lane_count)
                    .map(|index| GridMark {
                        value: lane_y(index, lane_count),
                        step_size: 1.0,
                    })
                    .collect()
            })
            .y_axis_formatter(move |mark, _| lane_label(&names, mark.value))
            .x_axis_formatter(format_axis)
            .label_formatter(|_, p| {
                let mut formatter = NumberFormatter::new();
                format!("Time: {}", formatter.format(p.x, 1))
            })
            .legend(Legend::default());

        if reset {
            plot = plot.reset();
        }

        if lanes.is_empty() {
            plot = plot.include_x(60.0);
        } else {
            plot = plot
                .include_x(0.0)
                .include_x(end)
                .include_y(lane_y(0, lane_count) + 0.5)
                .include_y(-0.5);
        }

        let more_decimals = settings.general.more_decimals;
        let noun = count_noun(self.count_type);
        let selection_color = ui.visuals().selection.bg_fill;
        let selection = &mut self.selection;
        let selecting_from = &mut self.selecting_from;
        let plot_response = plot.show(ui, |p| {
            track_range_selection(p, shift, (0.0, end), selection, selecting_from);
            if let Some((from, to)) = *selection {
                p.span(
                    Span::new("", from..=to)
                        .fill(selection_color.gamma_multiply(0.25))
                        .border_color(selection_color),
                );
            }
            for (index, lane) in drawn.iter().enumerate() {
                p.bar_chart(lane_chart(lane, lane_y(index, lane_count)).color(lane.color));
            }
        });

        // Only while egui_plot itself says this lane is hovered — it then
        // highlights the bar — so the tooltip and the highlight agree, and a
        // pointer over the legend gets none.
        if let Some(pointer) = plot_response.response.hover_pos()
            && let Some((lane, stretch)) = bar_under(
                &drawn,
                &plot_response.transform,
                pointer,
                ui.style().interaction.interact_radius,
            )
            && plot_response.hovered_plot_item == Some(lane_id(drawn[lane].name))
        {
            let text = lane_tooltip(&drawn[lane], stretch, noun, more_decimals);
            show_pointer_tooltip(&plot_response.response, &text);
        }

        if let Some(range) = self.selection {
            let hidden = PlotMemory::load(ui.ctx(), plot_id)
                .map(|m| m.hidden_items)
                .unwrap_or_default();
            let shown: Vec<&DrawnLane> = drawn
                .iter()
                .filter(|lane| !hidden.contains(&lane_id(lane.name)))
                .collect();
            show_range_summary(ui, plot_response.response.rect, range, &shown);
        }
    }

    /// Largest first, the order every chart puts its series in, so a lane
    /// carries the colour its row has on the other charts.
    fn sort(&mut self) {
        sort_lanes(&mut self.lanes);
        self.stale = true;
    }
}

fn sort_lanes<T: PreparedValue>(lanes: &mut [Lane<T>]) {
    lanes.sort_unstable_by(|l1, l2| {
        l1.data
            .total_value
            .total_cmp(&l2.data.total_value)
            .reverse()
    });
}

impl<T: PreparedValue> Lane<T> {
    fn new(data: PreparedDataSet<T>) -> Self {
        Self {
            data,
            windows: Vec::new(),
        }
    }

    fn update(&mut self, gap_s: f64, count_type: DiagramType, components: HealComponents) {
        self.windows = series_windows(&self.data, gap_s, count_type, components);
    }
}

/// One lane's bars, at height `y`, each with its tooltip.
fn lane_chart(lane: &DrawnLane, y: f64) -> BarChart {
    let bars = lane
        .windows
        .iter()
        .map(|w| {
            Bar::new(y, w.length_s().max(MIN_DRAWN_LENGTH_S))
                .base_offset(w.start_s)
                .width(BAR_THICKNESS)
        })
        .collect();
    BarChart::new(lane.name, bars)
        .id(lane_id(lane.name))
        .horizontal()
        // egui_plot writes a bar's text at the bar's far corner, which on a
        // long horizontal bar is nowhere near the pointer. It is given
        // nothing to write; the tooltip is drawn at the pointer instead, by
        // `show_pointer_tooltip`, the way the line charts show theirs.
        .element_formatter(Box::new(|_, _| String::new()))
}

/// The lane and the stretch under the pointer, as `(lane, stretch)` indices
/// into `lanes`: the bar whose drawn rectangle is nearest the pointer, within
/// `radius` points — the same reach egui_plot gives hovering — and the later
/// lane on a tie, since it is drawn on top.
fn bar_under(
    lanes: &[DrawnLane],
    transform: &PlotTransform,
    pointer: Pos2,
    radius: f32,
) -> Option<(usize, usize)> {
    let lane_count = lanes.len();
    let mut best: Option<(f32, usize, usize)> = None;
    for (index, lane) in lanes.iter().enumerate() {
        let y = lane_y(index, lane_count);
        for (stretch, window) in lane.windows.iter().enumerate() {
            let rect = transform.rect_from_values(
                &PlotPoint::new(window.start_s, y - BAR_THICKNESS / 2.0),
                &PlotPoint::new(
                    window.start_s + window.length_s().max(MIN_DRAWN_LENGTH_S),
                    y + BAR_THICKNESS / 2.0,
                ),
            );
            let distance = rect.distance_sq_to_pos(pointer);
            if distance <= radius * radius && best.is_none_or(|(nearest, ..)| distance <= nearest) {
                best = Some((distance, index, stretch));
            }
        }
    }
    best.map(|(_, lane, stretch)| (lane, stretch))
}

/// The tooltip of one bar of a lane; see [`window_description`].
fn lane_tooltip(lane: &DrawnLane, stretch: usize, noun: &str, more_decimals: bool) -> String {
    let base_s = lane.active_base_s.unwrap_or(0.0);
    let all = active_time(lane.windows, base_s);
    let mut formatter = NumberFormatter::new();
    let active = ActiveText::new(all, base_s, more_decimals, &mut formatter);
    window_description(
        lane.name,
        &lane.windows[stretch],
        &active,
        all.stretches,
        noun,
        more_decimals,
    )
}

/// A tooltip at the pointer, drawn the way egui_plot draws the line charts'
/// (`rulers_and_tooltip_at_value`): open at once, the style's tooltip width,
/// a gap of 12 points from the pointer.
fn show_pointer_tooltip(plot_area: &Response, text: &str) {
    let mut tooltip = Tooltip::always_open(
        plot_area.ctx.clone(),
        plot_area.layer_id,
        plot_area.id,
        PopupAnchor::Pointer,
    );
    let width = plot_area.ctx.global_style().spacing.tooltip_width;
    tooltip.popup = tooltip.popup.width(width);
    tooltip.gap(12.0).show(|ui| {
        ui.set_max_width(width);
        ui.label(text);
    });
}

/// The lanes a chart draws: the row it was opened on, with Total on and a
/// chart opened on a row; every lane otherwise.
fn shown_lanes<'a, T: PreparedValue>(
    whole: &'a Option<Lane<T>>,
    lanes: &'a [Lane<T>],
    total: bool,
) -> Vec<&'a Lane<T>> {
    match (whole, total) {
        (Some(whole), true) => vec![whole],
        _ => lanes.iter().collect(),
    }
}

/// How much of the stretch from `from_s` to `to_s` a lane was active: the
/// parts of its stretches that fall inside it, and how many of its stretches
/// reach into it at all. The share is of the marked stretch.
fn active_within(windows: &[ActivityWindow], from_s: f64, to_s: f64) -> ActiveTime {
    let length = to_s - from_s;
    let mut seconds = 0.0;
    let mut stretches = 0;
    for window in windows {
        if window.end_s < from_s || window.start_s > to_s {
            continue;
        }
        stretches += 1;
        seconds += window.end_s.min(to_s) - window.start_s.max(from_s);
    }
    ActiveTime {
        seconds,
        share: if length > 0.0 { seconds / length } else { 0.0 },
        stretches,
    }
}

/// How many lanes the range summary lists before it says how many more there
/// are, as the per-second charts do.
const RANGE_SUMMARY_MAX_LANES: usize = 10;

/// The box in the chart's top left corner: the marked stretch, and for each
/// lane shown how long of it the lane was active, what share of the stretch
/// that is, and in how many stretches of its own — in the lane's colour and
/// in the chart's order.
fn show_range_summary(ui: &mut Ui, plot_rect: Rect, (from, to): (f64, f64), lanes: &[&DrawnLane]) {
    let mut formatter = NumberFormatter::new();
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
        for lane in lanes.iter().take(RANGE_SUMMARY_MAX_LANES) {
            let active = active_within(lane.windows, from, to);
            let text = format!(
                "{}: {} s · {}% · {}",
                lane.name,
                formatter.format(active.seconds, 1),
                formatter.format(active.share * 100.0, 0),
                stretches_text(active.stretches),
            );
            ui.label(RichText::new(text).color(lane.color));
        }
        if lanes.len() > RANGE_SUMMARY_MAX_LANES {
            ui.label(format!(
                "… and {} more",
                lanes.len() - RANGE_SUMMARY_MAX_LANES
            ));
        }
        if lanes.is_empty() {
            ui.label("No lanes shown");
        }
    });
}

/// The id a lane is drawn under, which is also how the legend records that
/// it was hidden.
fn lane_id(name: &str) -> Id {
    Id::new(("activity lane", name))
}

/// What a lane is drawn from, whichever lanes the chart is showing.
struct DrawnLane<'a> {
    name: &'a str,
    windows: &'a [ActivityWindow],
    active_base_s: Option<f64>,
    color: Color32,
}

/// What one counted value of a lane is called in its tooltip.
fn count_noun(count_type: DiagramType) -> &'static str {
    match count_type {
        DiagramType::HealTicksCount => "heals",
        _ => "hits",
    }
}

/// The first lane at the top, the last at zero.
fn lane_y(index: usize, lane_count: usize) -> f64 {
    (lane_count - 1 - index) as f64
}

fn lane_label(names: &[String], y: f64) -> String {
    let rounded = y.round();
    if (y - rounded).abs() > 1e-6 || rounded < 0.0 {
        return String::new();
    }
    let Some(index) = names.len().checked_sub(1 + rounded as usize) else {
        return String::new();
    };
    let name = &names[index];
    if name.chars().count() > MAX_LABEL_CHARS {
        format!(
            "{}…",
            name.chars().take(MAX_LABEL_CHARS - 1).collect::<String>()
        )
    } else {
        name.clone()
    }
}

/// Room for the longest label the axis will print.
fn axis_label_width(ui: &Ui, names: &[String]) -> f32 {
    let widest = names
        .iter()
        .map(|n| n.chars().count().min(MAX_LABEL_CHARS))
        .max()
        .unwrap_or(0);
    let digit = ui.fonts_mut(|fonts| fonts.glyph_width(&TextStyle::Body.resolve(ui.style()), '0'));
    (digit * widest as f32 + ui.spacing().item_spacing.x * 2.0).max(y_axis_width(ui))
}

/// A bar's tooltip: the row, then this bar, then the row's whole Active
/// figure as the Active column writes it — the share only where the lane
/// knows what it is of.
///
/// ```text
/// Fire at Will III
/// This bar: 12.3 s – 45.6 s · 33.3 s long · 120 hits
/// Active: 118 s · 62% · 4 stretches
/// ```
fn window_description(
    name: &str,
    window: &ActivityWindow,
    active: &ActiveText,
    stretches: usize,
    noun: &str,
    more_decimals: bool,
) -> String {
    let mut formatter = NumberFormatter::new();
    let decimals = if more_decimals { 2 } else { 1 };
    let from = formatter.format(window.start_s, decimals).to_string();
    let to = formatter.format(window.end_s, decimals).to_string();
    let length = formatter.format(window.length_s(), decimals).to_string();
    let values = window.values;
    let mut whole = vec![active.seconds.clone()];
    whole.extend(active.share.clone());
    whole.push(stretches_text(stretches));
    format!(
        "{name}\nThis bar: {from} s – {to} s · {length} s long · {values} {noun}\nActive: {}",
        whole.join(" · ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(start_s: f64, end_s: f64, values: usize) -> ActivityWindow {
        ActivityWindow {
            start_s,
            end_s,
            values,
        }
    }

    #[test]
    fn no_values_no_windows() {
        assert!(activity_windows([], 5.0).is_empty());
    }

    #[test]
    fn one_value_is_a_window_without_length() {
        assert_eq!(activity_windows([2_000], 5.0), vec![window(2.0, 2.0, 1)]);
    }

    #[test]
    fn values_within_the_gap_join_one_window() {
        assert_eq!(
            activity_windows([1_000, 3_000, 7_500], 5.0),
            vec![window(1.0, 7.5, 3)]
        );
    }

    #[test]
    fn a_pause_longer_than_the_gap_starts_a_new_window() {
        assert_eq!(
            activity_windows([1_000, 2_000, 7_001, 8_000], 5.0),
            vec![window(1.0, 2.0, 2), window(7.001, 8.0, 2)]
        );
    }

    #[test]
    fn a_pause_of_exactly_the_gap_stays_in_the_window() {
        assert_eq!(
            activity_windows([1_000, 6_000], 5.0),
            vec![window(1.0, 6.0, 2)]
        );
    }

    #[test]
    fn a_smaller_gap_splits_what_a_larger_one_joins() {
        let times = [0, 3_000, 6_000];
        assert_eq!(activity_windows(times, 5.0).len(), 1);
        assert_eq!(activity_windows(times, 2.0).len(), 3);
    }

    #[test]
    fn active_time_is_the_stretches_against_the_fight() {
        let windows = [
            window(0.0, 10.0, 3),
            window(20.0, 20.0, 1),
            window(30.0, 40.0, 2),
        ];
        assert_eq!(
            active_time(&windows, 80.0),
            ActiveTime {
                seconds: 20.0,
                share: 0.25,
                stretches: 3
            }
        );
        assert_eq!(active_time(&[], 0.0).share, 0.0);
    }

    /// A row that never hit has no stretches, and its time is a plain zero —
    /// an empty float sum is -0, which the column printed as "-0 s".
    #[test]
    fn no_stretches_is_zero_seconds_not_minus_zero() {
        let none = active_time(&[], 80.0);
        assert!(none.seconds == 0.0 && none.seconds.is_sign_positive());
        assert_eq!(none.stretches, 0);
    }

    fn series(name: &str) -> PreparedDataSet<PreparedHitValue> {
        PreparedDataSet {
            name: name.to_string(),
            total_value: 0.0,
            values: std::sync::Arc::from([]),
            start_time_s: 0.0,
            duration_s: 60.0,
            color: None,
            active_base_s: None,
        }
    }

    fn shown_names(chart: &DamageActivityChart, total: bool) -> Vec<String> {
        chart
            .shown(total)
            .iter()
            .map(|lane| lane.data.name.clone())
            .collect()
    }

    /// A chart opened on a row shows what is under it, and with Total the row
    /// itself, under its own name.
    #[test]
    fn total_draws_the_row_the_chart_was_opened_on() {
        let mut chart = DamageActivityChart::from_data(
            DiagramType::HitsCount,
            [series("Phaser Beam Array"), series("Polaron Beam Array")].into_iter(),
            5.0,
        );
        chart.set_whole(series("Fire at Will III"));
        assert_eq!(
            vec!["Phaser Beam Array", "Polaron Beam Array"],
            shown_names(&chart, false)
        );
        assert_eq!(vec!["Fire at Will III"], shown_names(&chart, true));
    }

    /// Rows picked one by one are each their own lane already, so Total
    /// leaves them as they are: one lane per row, each that row's own total.
    #[test]
    fn total_keeps_one_lane_per_row_picked_one_by_one() {
        let chart = DamageActivityChart::from_data(
            DiagramType::HitsCount,
            [
                series("Fire at Will III"),
                series("Salvo I"),
                series("Spread I"),
            ]
            .into_iter(),
            5.0,
        );
        assert_eq!(shown_names(&chart, false), shown_names(&chart, true));
    }

    /// A bar's tooltip says what the bar is, then the row's whole Active
    /// figure the way the Active column says it.
    #[test]
    fn a_bar_tells_its_own_stretch_and_the_rows_active_figure() {
        let windows = [window(12.3, 45.6, 120), window(60.0, 60.0, 1)];
        let all = active_time(&windows, 100.0);
        let mut formatter = NumberFormatter::new();
        let active = ActiveText::new(all, 100.0, false, &mut formatter);
        assert_eq!(
            "Fire at Will III\n\
             This bar: 12.3 s – 45.6 s · 33.3 s long · 120 hits\n\
             Active: 33 s · 33% · 2 stretches",
            window_description("Fire at Will III", &windows[0], &active, 2, "hits", false)
        );

        // With no time to take a share of, the share is left out.
        let active = ActiveText::new(all, 0.0, false, &mut formatter);
        assert!(
            window_description("Salvo I", &windows[1], &active, 1, "hits", false)
                .ends_with("Active: 33 s · 1 stretch")
        );
    }

    /// A marked stretch counts the parts of the lane's stretches inside it,
    /// and every stretch that reaches into it.
    #[test]
    fn a_marked_stretch_counts_what_falls_inside_it() {
        let windows = [
            window(0.0, 10.0, 5),
            window(20.0, 30.0, 5),
            window(35.0, 35.0, 1),
            window(50.0, 60.0, 5),
        ];
        assert_eq!(
            active_within(&windows, 5.0, 40.0),
            ActiveTime {
                seconds: 15.0,
                share: 15.0 / 35.0,
                stretches: 3
            }
        );
        assert_eq!(active_within(&windows, 40.0, 45.0).stretches, 0);
        assert_eq!(active_within(&windows, 40.0, 45.0).seconds, 0.0);
    }

    /// The bar under the pointer is found anywhere along it, not only at its
    /// end — a long bar is pointed at in the middle as often as anywhere.
    #[test]
    fn the_bar_under_the_pointer_is_found_along_its_whole_length() {
        // 100 s across 1000 points, two lanes over 200 points of height.
        let transform = PlotTransform::new(
            Rect::from_min_size(Pos2::ZERO, Vec2::new(1000.0, 200.0)),
            PlotBounds::from_min_max([0.0, -0.5], [100.0, 1.5]),
            false,
        );
        let beams = [window(10.0, 60.0, 50)];
        let salvo = [window(20.0, 20.0, 1), window(70.0, 80.0, 3)];
        let lanes = [
            DrawnLane {
                name: "Fire at Will III",
                windows: &beams,
                active_base_s: None,
                color: Color32::WHITE,
            },
            DrawnLane {
                name: "Salvo I",
                windows: &salvo,
                active_base_s: None,
                color: Color32::WHITE,
            },
        ];
        let at = |x: f64, y: f64| {
            bar_under(
                &lanes,
                &transform,
                transform.position_from_point(&PlotPoint::new(x, y)),
                2.0,
            )
        };
        // The first lane is at the top (y = 1), in the middle of its bar.
        assert_eq!(Some((0, 0)), at(35.0, 1.0));
        // A one-hit stretch is pointed at across its drawn width.
        assert_eq!(Some((1, 0)), at(20.5, 0.0));
        assert_eq!(Some((1, 1)), at(75.0, 0.1));
        // Between the bars, and between the lanes, there is none.
        assert_eq!(None, at(65.0, 1.0));
        assert_eq!(None, at(35.0, 0.5));
    }

    #[test]
    fn the_first_lane_is_drawn_at_the_top() {
        assert_eq!(lane_y(0, 3), 2.0);
        assert_eq!(lane_y(2, 3), 0.0);
        let names = ["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(lane_label(&names, 2.0), "a");
        assert_eq!(lane_label(&names, 0.0), "c");
        assert_eq!(lane_label(&names, 0.5), "");
        assert_eq!(lane_label(&names, 3.0), "");
        assert_eq!(lane_label(&names, -1.0), "");
    }

    #[test]
    fn a_long_lane_name_is_cut_on_the_axis() {
        let names = ["x".repeat(MAX_LABEL_CHARS + 5)];
        let label = lane_label(&names, 0.0);
        assert_eq!(label.chars().count(), MAX_LABEL_CHARS);
        assert!(label.ends_with('…'));
    }
}
