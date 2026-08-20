use std::ops::Range;

use chrono::*;

pub mod number_formatting;
pub mod paths;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct F64TotalOrd(pub f64);

impl PartialOrd for F64TotalOrd {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Eq for F64TotalOrd {}

impl Ord for F64TotalOrd {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

pub fn time_range_to_duration(time_range: &Range<NaiveDateTime>) -> Duration {
    time_range.end.signed_duration_since(time_range.start)
}

pub fn time_range_to_duration_or_zero(time_range: &Option<Range<NaiveDateTime>>) -> Duration {
    time_range
        .as_ref()
        .map(time_range_to_duration)
        .unwrap_or(Duration::zero())
}

pub fn format_duration(duration: Duration) -> String {
    let time = NaiveTime::from_hms_opt(0, 0, 0).unwrap() + duration;
    if duration >= Duration::hours(1) {
        return format!("{}", time.format("%T%.3f"));
    }
    format!("{}", time.format("%M:%S%.3f"))
}

/// How long a fight ran, as a *list* of fights says it: `04:12`, growing an
/// hours part only once there is one.
///
/// [`format_duration`] keeps milliseconds, which matter when a single combat is
/// being read closely and only push a column of lengths apart when a hundred of
/// them are being skimmed.
pub fn format_duration_hms(duration: Duration) -> String {
    // A fight cannot run backwards; a length that came out negative means the
    // combat had no damage in it at all, and reads as no time.
    let seconds = duration.num_seconds().max(0);
    let (hours, minutes, seconds) = (seconds / 3600, (seconds % 3600) / 60, seconds % 60);
    if hours > 0 {
        return format!("{hours}:{minutes:02}:{seconds:02}");
    }
    format!("{minutes:02}:{seconds:02}")
}

#[macro_export]
macro_rules! unwrap_or_continue {
    ($expression:expr) => {
        match $expression {
            Some(thing) => thing,
            None => continue,
        }
    };
}

#[macro_export]
macro_rules! unwrap_or_break {
    ($expression:expr) => {
        match $expression {
            Some(thing) => thing,
            None => break,
        }
    };

    ($expression:expr, $label:lifetime) => {
        match $expression {
            Some(thing) => thing,
            None => break $label,
        }
    };
}

#[macro_export]
macro_rules! unwrap_or_return {
    ($expression:expr) => {
        match $expression {
            Some(thing) => thing,
            None => return,
        }
    };

    ($expression:expr, $ret:expr) => {
        match $expression {
            Some(thing) => thing,
            None => return $ret,
        }
    };
}

/// Puts fights into one log, oldest first, so they can be compared.
///
/// The analyzer splits combats on a gap in time and reads a log forwards, so a
/// fight written after a newer one is not a second combat — it is folded into
/// the one before it, producing one mangled fight out of two. Order is
/// therefore not a nicety here.
///
/// Every fight is placed in one go rather than added to what is already
/// composed one at a time. Added one at a time, each was only weighed against
/// the *first* of them: a fight from between two already in the log went on the
/// end, behind a newer one, and was swallowed by it. Which fights that happened
/// to depended on the order they were picked in, which is why it looked like
/// some of them simply would not load.
///
/// Log lines open with `YY:MM:DD:HH:MM:SS.mmm`, which sorts as text in the same
/// order it sorts in time, so the first line of each is enough to tell which
/// came first — no parsing, and nothing to get wrong about time zones the log
/// does not carry anyway.
pub fn compose_comparison_log(fights: impl IntoIterator<Item = Vec<u8>>) -> Vec<u8> {
    let mut fights: Vec<Vec<u8>> = fights.into_iter().filter(|f| !f.is_empty()).collect();
    fights.sort_by(|one, other| first_line(one).cmp(first_line(other)));

    let mut composed =
        Vec::with_capacity(fights.iter().map(Vec::len).sum::<usize>() + fights.len());
    for fight in fights {
        composed.extend_from_slice(&fight);
        if !composed.ends_with(b"\n") {
            composed.push(b'\n');
        }
    }
    composed
}

fn first_line(data: &[u8]) -> &[u8] {
    let end = data
        .iter()
        .position(|byte| *byte == b'\n')
        .unwrap_or(data.len());
    &data[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLDER: &[u8] = b"26:07:19:12:00:01.0::older\n26:07:19:12:00:02.0::older\n";
    const NEWER: &[u8] = b"26:08:09:12:00:01.0::newer\n";

    /// The analyzer reads forwards and splits on a gap, so a fight written after
    /// a newer one is folded into it rather than becoming a combat of its own.
    #[test]
    fn the_older_fight_is_written_first_whichever_way_round_it_comes() {
        let one = compose_comparison_log([OLDER.to_vec(), NEWER.to_vec()]);
        let other = compose_comparison_log([NEWER.to_vec(), OLDER.to_vec()]);
        assert_eq!(one, other);
        assert!(other.starts_with(OLDER));
    }

    /// Every fight is placed against every other, not against the first of
    /// them. A fight from between two already in the log used to go on the end,
    /// behind a newer one — where the analyzer reads it as part of that one
    /// rather than as a fight of its own, so it never appeared in the
    /// comparison at all.
    #[test]
    fn a_fight_from_between_two_others_lands_between_them() {
        const MIDDLE: &[u8] = b"26:08:01:12:00:01.0::middle\n";
        let composed = compose_comparison_log([OLDER.to_vec(), NEWER.to_vec(), MIDDLE.to_vec()]);
        let text = String::from_utf8(composed).unwrap();
        let at = |needle: &str| text.find(needle).unwrap();
        assert!(at("older") < at("middle"), "{text}");
        assert!(at("middle") < at("newer"), "{text}");
    }

    /// Whatever order they are handed over in.
    #[test]
    fn the_order_they_are_picked_in_makes_no_difference() {
        const MIDDLE: &[u8] = b"26:08:01:12:00:01.0::middle\n";
        let parts = [OLDER.to_vec(), MIDDLE.to_vec(), NEWER.to_vec()];
        let composed = compose_comparison_log(parts.clone());
        for order in [[2, 0, 1], [1, 2, 0], [2, 1, 0]] {
            let shuffled = order.map(|i| parts[i].clone());
            assert_eq!(composed, compose_comparison_log(shuffled));
        }
    }

    /// A fight cut out of a log may not end in a newline, and without one the
    /// last line of the first would run into the first line of the second.
    #[test]
    fn the_two_fights_never_run_into_one_line() {
        let unterminated = b"26:07:19:12:00:01.0::older".to_vec();
        let composed = compose_comparison_log([unterminated, NEWER.to_vec()]);
        assert!(composed.starts_with(b"26:07:19:12:00:01.0::older\n26:08:09"));
    }

    #[test]
    fn a_fight_length_reads_as_minutes_and_seconds() {
        assert_eq!("04:12", format_duration_hms(Duration::seconds(252)));
        assert_eq!("00:07", format_duration_hms(Duration::milliseconds(7400)));
        assert_eq!("00:00", format_duration_hms(Duration::zero()));
    }

    /// An hours part appears only when the fight actually ran that long — a
    /// column of `00:04:12` would spend a third of its width on a zero.
    #[test]
    fn an_hours_part_appears_only_once_there_is_one() {
        assert_eq!("59:59", format_duration_hms(Duration::seconds(3599)));
        assert_eq!("1:00:00", format_duration_hms(Duration::seconds(3600)));
        assert_eq!("2:03:04", format_duration_hms(Duration::seconds(7384)));
    }
}
