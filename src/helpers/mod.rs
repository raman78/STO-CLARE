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

/// Puts two fights into one log, older first, so they can be compared.
///
/// The analyzer splits combats on a gap in time and reads a log forwards, so a
/// fight appended after a newer one is not a second combat — it is folded into
/// the first, producing one mangled fight out of two. Order is therefore not a
/// nicety here.
///
/// Log lines open with `YY:MM:DD:HH:MM:SS.mmm`, which sorts as text in the same
/// order it sorts in time, so the first line of each is enough to tell which
/// came first — no parsing, and nothing to get wrong about time zones the log
/// does not carry anyway.
pub fn compose_comparison_log(one: &[u8], other: &[u8]) -> Vec<u8> {
    let (first, second) = if first_line(one) <= first_line(other) {
        (one, other)
    } else {
        (other, one)
    };
    let mut composed = Vec::with_capacity(first.len() + second.len() + 1);
    composed.extend_from_slice(first);
    if !first.ends_with(b"\n") {
        composed.push(b'\n');
    }
    composed.extend_from_slice(second);
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
        assert_eq!(
            compose_comparison_log(OLDER, NEWER),
            compose_comparison_log(NEWER, OLDER)
        );
        assert!(compose_comparison_log(NEWER, OLDER).starts_with(OLDER));
    }

    /// A fight cut out of a log may not end in a newline, and without one the
    /// last line of the first would run into the first line of the second.
    #[test]
    fn the_two_fights_never_run_into_one_line() {
        let unterminated = b"26:07:19:12:00:01.0::older";
        let composed = compose_comparison_log(unterminated, NEWER);
        assert!(composed.starts_with(b"26:07:19:12:00:01.0::older\n26:08:09"));
    }
}
