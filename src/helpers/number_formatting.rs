use std::fmt::Write;

pub struct NumberFormatter {
    buffer: String,
}

impl NumberFormatter {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
        }
    }

    /// The number with `precision` decimals and an apostrophe every three
    /// digits, the way every table in the program shows a figure.
    ///
    /// Rounded **once, over the whole number**. Rounding the fraction on its
    /// own and gluing it back onto the integer part loses the carry: 8.972 came
    /// out as `8.0` rather than `9.0`, since the fraction rounded to `1.0` and
    /// only the digits after its point were kept. That went for anything whose
    /// fraction rounded up — about one figure in twenty at a single decimal.
    pub fn format(&mut self, number: f64, precision: usize) -> String {
        let is_negative = number.is_sign_negative();

        self.buffer.clear();
        write!(&mut self.buffer, "{:.*}", precision, number.abs()).unwrap();

        let digits = self.buffer.find('.').unwrap_or(self.buffer.len());
        let mut result = String::with_capacity(self.buffer.len() + digits / 3);
        for (index, character) in self.buffer.chars().enumerate() {
            if index > 0 && index < digits && (digits - index).is_multiple_of(3) {
                result.push('\'');
            }
            result.push(character);
        }

        Self::add_sign(result, is_negative)
    }

    pub fn format_with_automated_suffixes(&mut self, number: f64) -> String {
        if number.abs() == 0.0 {
            return "0.0".to_string();
        }

        let is_negative = number.is_sign_negative();

        let number = number.abs();

        const THRESHOLD_AND_SUFFIX: &[(f64, &str)] = &[
            (1e-6, "n"),
            (1e-3, "u"),
            (0.0, "m"),
            (1.0e3, ""),
            (1.0e6, "k"),
            (1.0e9, "M"),
            (1.0e12, "G"),
            (1.0e15, "T"),
        ];

        const PRECISION_THRESHOLD: &[(f64, usize)] = &[(10.0, 2), (100.0, 1), (1000.0, 0)];

        for (threshold, suffix) in THRESHOLD_AND_SUFFIX.iter().copied() {
            if number < threshold {
                let normalized_number = number / (threshold / 1e3);
                let precision = PRECISION_THRESHOLD
                    .iter()
                    .copied()
                    .find_map(|(t, p)| if normalized_number < t { Some(p) } else { None })
                    .unwrap_or(0);
                return Self::add_sign(
                    format!("{}{}", self.format(normalized_number, precision), suffix),
                    is_negative,
                );
            }
        }

        "<too large>".to_string()
    }

    fn add_sign(mut result: String, is_negative: bool) -> String {
        if is_negative {
            result.insert(0, '-');
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fraction that rounds up has to carry into the integer part: 8.972 to
    /// one decimal is 9.0, not 8.0.
    #[test]
    fn a_fraction_that_rounds_up_carries() {
        let mut f = NumberFormatter::new();
        assert_eq!("9.0", f.format(8.972, 1));
        assert_eq!("9.00", f.format(8.9972, 2));
        assert_eq!("1'000.0", f.format(999.97, 1));
        assert_eq!("-9.0", f.format(-8.972, 1));
        // The carry can also cross a group boundary, at any precision.
        assert_eq!("1'000", f.format(999.6, 0));
        assert_eq!("1'000'000.00", f.format(999_999.999, 2));
    }

    /// A figure past what a `u64` holds is written out rather than pinned to
    /// that ceiling — the digits come from the float, not from a cast.
    #[test]
    fn a_very_large_number_keeps_its_digits() {
        let mut f = NumberFormatter::new();
        assert_eq!("100'000'000'000'000'000'000", f.format(1e20, 0));
    }

    #[test]
    fn format_numbers() {
        let mut formatter = NumberFormatter::new();

        assert_eq!(formatter.format(123.1, 2), "123.10");
        assert_eq!(formatter.format(12345.1, 2), "12'345.10");
        assert_eq!(formatter.format(12345.123, 2), "12'345.12");
        assert_eq!(formatter.format(123456789.0, 2), "123'456'789.00");

        assert_eq!(formatter.format(12012.0, 2), "12'012.00");
        assert_eq!(formatter.format(12012012.0, 2), "12'012'012.00");

        assert_eq!(formatter.format(12012012.0, 0), "12'012'012");

        assert_eq!(formatter.format(1.567, 2), "1.57");
        assert_eq!(formatter.format(-1.567, 2), "-1.57");

        assert_eq!(formatter.format(-100.0, 0), "-100");
    }

    #[test]
    fn format_with_automated_suffixes() {
        let mut formatter = NumberFormatter::new();

        assert_eq!(formatter.format_with_automated_suffixes(123.1), "123");
        assert_eq!(formatter.format_with_automated_suffixes(12345.1), "12.3k");
        assert_eq!(formatter.format_with_automated_suffixes(12345.123), "12.3k");
        assert_eq!(
            formatter.format_with_automated_suffixes(123456789.0),
            "123M"
        );

        assert_eq!(formatter.format_with_automated_suffixes(12012.0), "12.0k");
        assert_eq!(
            formatter.format_with_automated_suffixes(12012012.0),
            "12.0M"
        );

        assert_eq!(formatter.format_with_automated_suffixes(1.567), "1.57");
        assert_eq!(formatter.format_with_automated_suffixes(-1.567), "-1.57");

        assert_eq!(formatter.format_with_automated_suffixes(0.0), "0.0");
        assert_eq!(formatter.format_with_automated_suffixes(-0.0), "0.0");
    }
}
