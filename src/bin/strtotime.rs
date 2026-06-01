use chrono::{DateTime, Local};
use funchain::strtotime;
use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() > 1 && (args[1] == "-h" || args[1] == "--help") {
        println!("Usage: strtotime <DATE_STRING>");
        println!("Parses a human-readable date/time string and outputs the Unix timestamp.");
        println!("Example: strtotime \"next friday\"");
        process::exit(0);
    }

    if args.len() < 2 {
        eprintln!("Usage: strtotime <date/time string>");
        eprintln!("Example: strtotime 'next friday'");
        eprintln!("Try 'strtotime --help' for more information.");
        process::exit(1);
    }

    let input_raw = args[1..].join(" ");
    let now = Local::now();

    match process_input(&input_raw, now) {
        Ok(dt) => {
            println!("{}", dt.timestamp());
            eprintln!("Resolved: {}", dt.format("%Y-%m-%d %H:%M:%S %z"));
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    }
}

fn process_input(input: &str, now: DateTime<Local>) -> Result<DateTime<Local>, String> {
    strtotime::parse(input, now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, TimeZone};

    #[test]
    fn test_process_input_now() {
        // Create a fixed time for testing
        let now = Local.timestamp_opt(1600000000, 0).unwrap();

        let res = process_input("now", now);
        assert!(res.is_ok());
        assert_eq!(res.unwrap().timestamp(), 1600000000);
    }

    #[test]
    fn test_process_input_invalid() {
        let now = Local.timestamp_opt(1600000000, 0).unwrap();

        let res = process_input("invalid garbage string", now);
        assert!(res.is_err());

        let res = process_input("not a date", now);
        assert!(res.is_err());
    }

    #[test]
    fn test_process_input_relative_hours() {
        let now = Local.timestamp_opt(1600000000, 0).unwrap();

        // +3 hours = 1600000000 + 3*3600 = 1600010800
        let res = process_input("+3 hours", now);
        assert!(res.is_ok());
        assert_eq!(res.unwrap().timestamp(), 1600010800);
    }

    #[test]
    fn test_process_input_relative_days() {
        let now = Local.timestamp_opt(1600000000, 0).unwrap();

        // 2 days ago = 1600000000 - 2*86400 = 1599827200
        let res = process_input("2 days ago", now);
        assert!(res.is_ok());
        assert_eq!(res.unwrap().timestamp(), 1599827200);
    }

    #[test]
    fn test_process_input_relative_minutes() {
        let now = Local.timestamp_opt(1600000000, 0).unwrap();

        // +30 minutes = 1600000000 + 30*60 = 1600001800
        let res = process_input("+30 minutes", now);
        assert!(res.is_ok());
        assert_eq!(res.unwrap().timestamp(), 1600001800);
    }

    #[test]
    fn test_process_input_relative_weeks() {
        let now = Local.timestamp_opt(1600000000, 0).unwrap();

        // 1 week ago = 1600000000 - 7*86400 = 1599395200
        let res = process_input("1 week ago", now);
        assert!(res.is_ok());
        assert_eq!(res.unwrap().timestamp(), 1599395200);
    }

    #[test]
    fn test_process_input_yesterday_tomorrow() {
        let now = Local.timestamp_opt(1600000000, 0).unwrap();

        let res = process_input("yesterday", now);
        assert!(res.is_ok());
        // yesterday sets time to midnight, so we just check it's before now
        assert!(res.unwrap().timestamp() < now.timestamp());

        let res = process_input("tomorrow", now);
        assert!(res.is_ok());
        // tomorrow sets time to midnight of next day
        assert!(res.unwrap().timestamp() > now.timestamp());
    }

    #[test]
    fn test_process_input_last_day_of_month() {
        // September 13, 2020 (1600000000)
        let now = Local.timestamp_opt(1600000000, 0).unwrap();

        let res = process_input("last day of this month", now);
        assert!(res.is_ok());
        let result = res.unwrap();
        // September has 30 days
        assert_eq!(result.day(), 30);
        assert_eq!(result.month(), 9);
    }

    #[test]
    fn test_process_input_empty() {
        let now = Local.timestamp_opt(1600000000, 0).unwrap();

        // Empty string should fail
        let res = process_input("", now);
        assert!(res.is_err());
    }

    #[test]
    fn test_process_input_whitespace_only() {
        let now = Local.timestamp_opt(1600000000, 0).unwrap();

        let res = process_input("   ", now);
        assert!(res.is_err());
    }
}
