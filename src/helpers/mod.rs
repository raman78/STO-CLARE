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

#[cfg(test)]
mod tests {
    use super::*;

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
