use chrono::{DateTime, Datelike, NaiveDate, TimeZone};
use chrono_english::{Dialect, parse_date_string};
use std::fmt::Display;

/// Parses a natural language date/time string into a DateTime object.
///
/// Supported formats include:
/// - Relative: "+3 hours", "2 days ago", "next friday"
/// - Absolute: "2025-12-25"
/// - Custom phrases: "last day of this month"
///
/// # Arguments
///
/// * `input_raw` - A string slice containing the natural language date/time
/// * `now` - The reference DateTime to use for relative calculations
///
/// # Returns
///
/// Returns `Ok(DateTime)` on successful parsing, or `Err(String)` with an error message.
///
/// # Examples
///
/// ```
/// use chrono::{Utc, TimeZone};
/// use funchain::strtotime;
///
/// let now = Utc.with_ymd_and_hms(2023, 6, 15, 12, 0, 0).unwrap();
///
/// // Relative time with plus sign
/// let result = strtotime::parse("+3 hours", now).unwrap();
/// assert_eq!(result, Utc.with_ymd_and_hms(2023, 6, 15, 15, 0, 0).unwrap());
///
/// // Relative time with "ago"
/// let result = strtotime::parse("2 days ago", now).unwrap();
/// assert_eq!(result, Utc.with_ymd_and_hms(2023, 6, 13, 12, 0, 0).unwrap());
///
/// // Invalid input returns an error
/// let result = strtotime::parse("invalid", now);
/// assert!(result.is_err());
/// ```
///
/// # Example: Last day of month
///
/// ```
/// use chrono::{Utc, TimeZone, Datelike};
/// use funchain::strtotime;
///
/// let now = Utc.with_ymd_and_hms(2024, 2, 10, 12, 0, 0).unwrap();
/// let result = strtotime::parse("last day of this month", now).unwrap();
/// // February 2024 is a leap year, so last day is 29
/// assert_eq!(result.day(), 29);
/// ```
pub fn parse<Tz>(input_raw: &str, now: DateTime<Tz>) -> Result<DateTime<Tz>, String>
where
    Tz: TimeZone,
    Tz::Offset: Display + Copy,
{
    // 1. Custom handlers
    if input_raw.eq_ignore_ascii_case("last day of this month") {
        let (year, month) = (now.year(), now.month());
        let (next_y, next_m) = if month == 12 {
            (year + 1, 1)
        } else {
            (year, month + 1)
        };

        if let Some(first_next) = NaiveDate::from_ymd_opt(next_y, next_m, 1)
            && let Some(last_day) = first_next.pred_opt()
            && let Some(dt) = last_day.and_hms_opt(0, 0, 0)
        {
            // single() is best effort for timezone mapping
            if let Some(local_dt) = now.timezone().from_local_datetime(&dt).single() {
                return Ok(local_dt);
            }
        }
        return Err("Could not calculate last day of month".to_string());
    }

    // 2. Preprocess "+3 hours" -> "3 hours"
    let input = if input_raw.trim().starts_with('+') {
        input_raw.trim().trim_start_matches('+').to_string()
    } else {
        input_raw.to_string()
    };

    // 3. Delegate to chrono-english
    match parse_date_string(&input, now, Dialect::Us) {
        Ok(dt) => Ok(dt),
        Err(e) => Err(format!(
            "Could not parse time string '{}': {}",
            input_raw, e
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn test_parse_relative_plus() {
        let now = Utc.with_ymd_and_hms(2023, 1, 1, 12, 0, 0).unwrap();
        let result = parse("+3 hours", now).unwrap();
        assert_eq!(result, Utc.with_ymd_and_hms(2023, 1, 1, 15, 0, 0).unwrap());
    }

    #[test]
    fn test_parse_relative_no_plus() {
        let now = Utc.with_ymd_and_hms(2023, 1, 1, 12, 0, 0).unwrap();
        let result = parse("3 hours", now).unwrap();
        assert_eq!(result, Utc.with_ymd_and_hms(2023, 1, 1, 15, 0, 0).unwrap());
    }

    #[test]
    fn test_parse_ago() {
        let now = Utc.with_ymd_and_hms(2023, 1, 1, 12, 0, 0).unwrap();
        let result = parse("2 days ago", now).unwrap();
        assert_eq!(
            result,
            Utc.with_ymd_and_hms(2022, 12, 30, 12, 0, 0).unwrap()
        );
    }

    #[test]
    fn test_parse_last_day_of_month() {
        // Jan 2023 -> Jan 31 2023
        let now = Utc.with_ymd_and_hms(2023, 1, 15, 12, 0, 0).unwrap();
        let result = parse("last day of this month", now).unwrap();
        assert_eq!(result, Utc.with_ymd_and_hms(2023, 1, 31, 0, 0, 0).unwrap());

        // Feb 2024 (Leap) -> Feb 29 2024
        let now = Utc.with_ymd_and_hms(2024, 2, 10, 10, 0, 0).unwrap();
        let result = parse("last day of this month", now).unwrap();
        assert_eq!(result, Utc.with_ymd_and_hms(2024, 2, 29, 0, 0, 0).unwrap());

        // Dec 2023 -> Dec 31 2023
        let now = Utc.with_ymd_and_hms(2023, 12, 1, 12, 0, 0).unwrap();
        let result = parse("last day of this month", now).unwrap();
        assert_eq!(result, Utc.with_ymd_and_hms(2023, 12, 31, 0, 0, 0).unwrap());
    }

    #[test]
    fn test_parse_next_friday() {
        // Jan 1 2023 is Sunday.
        // "next friday" from Sunday Jan 1 -> Friday Jan 6.
        let now = Utc.with_ymd_and_hms(2023, 1, 1, 12, 0, 0).unwrap();
        let result = parse("next friday", now).unwrap();
        // chrono-english "next friday" usually sets time to 00:00:00
        assert_eq!(
            result.date_naive(),
            NaiveDate::from_ymd_opt(2023, 1, 6).unwrap()
        );
    }

    #[test]
    fn test_invalid_input() {
        let now = Utc.with_ymd_and_hms(2023, 1, 1, 12, 0, 0).unwrap();
        let result = parse("invalid garbage", now);
        assert!(result.is_err());
    }
}
