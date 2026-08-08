//! A start-time window for the compare view's combats list.
//!
//! The pickers beside it narrow by what a combat *was* — its map, its level,
//! where it was fought. This one narrows by when it was played, down to the
//! minute, which is how a session is picked out: "everything I flew this
//! evening" is a window in time, not a map.

use chrono::{Duration, NaiveDateTime};
use eframe::egui::*;

use crate::app::theme;

/// What the two fields are typed in. Minutes rather than seconds: a combats
/// list is never dense enough for a second to tell two runs apart, and typing
/// six more characters for nothing is worse than the precision is worth.
const FORMAT: &str = "%Y-%m-%d %H:%M";

/// The presets, as a label and how far back the window starts.
const PRESETS: [(&str, i64); 3] = [("24 h", 24), ("7 days", 24 * 7), ("30 days", 24 * 30)];

/// A window of start times. Both ends are optional: an empty field is no bound
/// at that end, so one field alone reads as "since then" or "up to then".
#[derive(Default, Clone, PartialEq, Eq)]
pub struct DateRange {
    from: String,
    to: String,
}

impl DateRange {
    /// Whether the window actually excludes anything. An unparseable field does
    /// not count — it bounds nothing until it is typed out in full.
    pub fn is_active(&self) -> bool {
        parse(&self.from).is_some() || parse(&self.to).is_some()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Types both ends at once, for tests that need a window without a UI to
    /// type it into.
    #[cfg(test)]
    pub fn set(&mut self, from: &str, to: &str) {
        self.from = from.to_string();
        self.to = to.to_string();
    }

    /// Whether a combat starting at `start` falls inside the window.
    ///
    /// The upper bound covers its whole minute: the fields are typed to the
    /// minute, so a combat that started at 20:07:45 is inside a window ending
    /// at 20:07 — anything else would drop the very run the user typed the
    /// time of.
    pub fn matches(&self, start: NaiveDateTime) -> bool {
        if let Some(from) = parse(&self.from)
            && start < from
        {
            return false;
        }
        if let Some(to) = parse(&self.to)
            && start >= to + Duration::minutes(1)
        {
            return false;
        }
        true
    }

    /// A window ending at the newest combat and reaching `hours` back.
    ///
    /// Counted from the log rather than from the wall clock: the times in a
    /// combat log are the game's, and a log opened from elsewhere (or a machine
    /// whose clock has moved on) would otherwise answer "the last 24 hours"
    /// with an empty list.
    fn preset(&mut self, newest: NaiveDateTime, hours: i64) {
        self.from = format(newest - Duration::hours(hours));
        self.to.clear();
    }

    /// Draws the two fields and the presets. `bounds` is the oldest and newest
    /// combat in the list: the starting point for a field the user clicks into,
    /// and what the presets count back from.
    pub fn show(&mut self, bounds: Option<(NaiveDateTime, NaiveDateTime)>, ui: &mut Ui) {
        ui.label("Played:");
        field(&mut self.from, bounds.map(|(oldest, _)| oldest), "from", ui);
        // A word rather than an arrow: the bundled fonts have no U+2192, which
        // draws as an empty box, and "to" says what the box could not.
        ui.label("to");
        field(&mut self.to, bounds.map(|(_, newest)| newest), "to", ui);

        if let Some((_, newest)) = bounds {
            for (label, hours) in PRESETS {
                if ui
                    .button(label)
                    .on_hover_text(format!(
                        "The last {label} of play, counted back from the newest combat in the list"
                    ))
                    .clicked()
                {
                    self.preset(newest, hours);
                }
            }
        }
        if self.is_active() && ui.button("Any time").clicked() {
            self.clear();
        }
    }
}

/// One end of the window.
///
/// Clicking into an empty field fills it with that end of the list, so a window
/// is edited from a real time rather than typed from nothing — while a field
/// nobody has touched stays empty, and so bounds nothing.
fn field(text: &mut String, fill_with: Option<NaiveDateTime>, id: &str, ui: &mut Ui) {
    let invalid = !text.trim().is_empty() && parse(text).is_none();
    let mut edit = TextEdit::singleline(text)
        .id(Id::new(("compare date range", id)))
        .hint_text(FORMAT)
        .desired_width(115.0);
    if invalid {
        edit = edit.text_color(theme::palette().worse);
    }
    let response = ui.add(edit).on_hover_text(match fill_with {
        Some(bound) => format!(
            "Date and time as {FORMAT}, e.g. {}. Leave it empty for no bound at this end.",
            format(bound)
        ),
        None => format!("Date and time as {FORMAT}. Leave it empty for no bound at this end."),
    });
    if response.gained_focus()
        && text.is_empty()
        && let Some(bound) = fill_with
    {
        *text = format(bound);
    }
}

fn parse(text: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(text.trim(), FORMAT).ok()
}

fn format(time: NaiveDateTime) -> String {
    time.format(FORMAT).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> NaiveDateTime {
        parse(text).unwrap()
    }

    fn range(from: &str, to: &str) -> DateRange {
        DateRange {
            from: from.to_string(),
            to: to.to_string(),
        }
    }

    /// An untouched window bounds nothing, so the list is what the other
    /// filters made it.
    #[test]
    fn an_empty_range_matches_everything() {
        let range = DateRange::default();
        assert!(!range.is_active());
        assert!(range.matches(at("2026-07-23 20:07")));
        assert!(range.matches(at("2020-01-01 00:00")));
    }

    /// Each end bounds on its own, so "since Friday" needs one field, not two.
    #[test]
    fn each_end_bounds_on_its_own() {
        let since = range("2026-07-23 20:00", "");
        assert!(since.is_active());
        assert!(!since.matches(at("2026-07-23 19:59")));
        assert!(since.matches(at("2026-07-23 20:00")));
        assert!(since.matches(at("2030-01-01 00:00")));

        let until = range("", "2026-07-23 20:00");
        assert!(until.is_active());
        assert!(until.matches(at("2026-07-23 19:59")));
        assert!(!until.matches(at("2026-07-23 20:01")));
    }

    /// The fields are typed to the minute, so the minute the user typed is
    /// inside the window however many seconds into it the combat began.
    #[test]
    fn the_upper_bound_covers_its_whole_minute() {
        let range = range("2026-07-23 20:00", "2026-07-23 20:07");
        let start = at("2026-07-23 20:07") + Duration::seconds(45);
        assert!(range.matches(start));
        assert!(!range.matches(at("2026-07-23 20:08")));
    }

    /// Half-typed text bounds nothing rather than emptying the list under the
    /// user's hands while they are still typing.
    #[test]
    fn an_unparseable_field_bounds_nothing() {
        let half_typed = range("2026-07-", "");
        assert!(!half_typed.is_active());
        assert!(half_typed.matches(at("2020-01-01 00:00")));

        // ...and the other field still works while one is being typed.
        let mixed = range("2026-07-", "2026-07-23 20:07");
        assert!(mixed.is_active());
        assert!(!mixed.matches(at("2026-07-23 20:09")));
    }

    /// A preset reaches back from the newest combat in the list — from the log,
    /// not from the machine's clock — and leaves the far end open.
    #[test]
    fn a_preset_counts_back_from_the_newest_combat() {
        let mut range = DateRange::default();
        range.preset(at("2026-07-23 22:00"), 24);

        assert_eq!("2026-07-22 22:00", range.from);
        assert!(range.to.is_empty(), "the window stays open at the top end");
        assert!(range.matches(at("2026-07-23 20:07")));
        assert!(range.matches(at("2026-07-24 08:00")), "and past it too");
        assert!(!range.matches(at("2026-07-22 21:59")));
    }

    /// A preset replaces the window rather than adding to it, so picking "24 h"
    /// after typing a window by hand gives the 24 hours, not their overlap.
    #[test]
    fn a_preset_replaces_what_was_typed() {
        let mut range = range("2020-01-01 00:00", "2020-01-02 00:00");
        range.preset(at("2026-07-23 22:00"), 24);
        assert_eq!("2026-07-22 22:00", range.from);
        assert!(range.to.is_empty());
    }

    /// A window that has been given up on stops filtering entirely.
    #[test]
    fn clearing_gives_up_both_ends() {
        let mut range = range("2026-07-23 20:00", "2026-07-23 21:00");
        range.clear();
        assert!(!range.is_active());
        assert!(range.matches(at("2026-07-23 22:00")));
    }
}
