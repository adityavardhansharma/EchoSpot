//! Triggers: when pack code runs without being asked. Schedules (`every = "15m"`,
//! `at = "09:00 weekdays"`) and events (`on = "clipboard.changed"`).

/// A duration such as `30s`, `15m`, `2h`, `1d`, in seconds.
pub fn parse_every(text: &str) -> Option<f64> {
    let text = text.trim();
    let split = text.find(|c: char| !c.is_ascii_digit() && c != '.')?;
    let n: f64 = text[..split].parse().ok()?;
    let unit = match text[split..].trim() {
        "s" | "sec" | "secs" => 1.0,
        "m" | "min" | "mins" => 60.0,
        "h" | "hr" | "hrs" => 3600.0,
        "d" | "day" | "days" => 86400.0,
        "w" | "week" | "weeks" => 604800.0,
        _ => return None,
    };
    (n > 0.0).then_some(n * unit)
}

/// A time of day and the days it applies to (Monday is 0): `09:00`, `09:00 weekdays`, `18:30 mon,wed`.
pub fn parse_at(text: &str) -> Option<(u32, u32, [bool; 7])> {
    let mut words = text.split_whitespace();
    let (h, m) = words.next()?.split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    if h > 23 || m > 59 {
        return None;
    }
    let mut days = [false; 7];
    let rest: Vec<&str> = words.collect();
    if rest.is_empty() {
        days = [true; 7];
    }
    for word in rest.iter().flat_map(|w| w.split(',')) {
        match word.to_lowercase().as_str() {
            "daily" | "everyday" => days = [true; 7],
            "weekdays" => days[..5].fill(true),
            "weekends" => days[5..].fill(true),
            day => days[["mon", "tue", "wed", "thu", "fri", "sat", "sun"].iter().position(|d| day.starts_with(d))?] = true,
        }
    }
    Some((h, m, days))
}

/// One thing that runs unasked: a pack's trigger, or an ability with `every`, `at` or `on`.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// `<pack>.triggers.<name>`, or the ability's address.
    pub key: String,
    pub pack: String,
    pub name: String,
    pub trigger: bool,
    pub every: String,
    pub at: String,
    pub on: Vec<String>,
}

impl Entry {
    /// How its schedule reads in Settings: `every 15m`, `at 09:00 weekdays`, `on clipboard.changed`.
    pub fn when(&self) -> String {
        let mut parts = Vec::new();
        if !self.every.is_empty() {
            parts.push(format!("every {}", self.every));
        }
        if !self.at.is_empty() {
            parts.push(format!("at {}", self.at));
        }
        if !self.on.is_empty() {
            parts.push(format!("on {}", self.on.join(", ")));
        }
        parts.join(" · ")
    }
}

/// The last moment at or before `now` that `at` names, in seconds since 1970.
fn last_at(at: &str, now: f64) -> Option<f64> {
    use chrono::{Datelike, Duration, Local, TimeZone};
    let (h, m, days) = parse_at(at)?;
    let today = Local.timestamp_opt(now as i64, 0).single()?;
    (0..8).find_map(|back| {
        let day = today.date_naive() - Duration::days(back);
        let when = Local.from_local_datetime(&day.and_hms_opt(h, m, 0)?).single()?;
        (days[day.weekday().num_days_from_monday() as usize] && (when.timestamp() as f64) <= now).then_some(when.timestamp() as f64)
    })
}

/// Whether a schedule has come due: `last` is when it last ran (zero for never).
pub fn due(entry: &Entry, last: f64, now: f64) -> bool {
    if let Some(every) = parse_every(&entry.every)
        && now - last >= every {
        return true;
    }
    // A time of day runs once for each time it passes; one missed while EchoSpot was closed
    // runs when it next starts, if that is within the hour.
    match last_at(&entry.at, now) {
        Some(when) => last < when && now - when < 3600.0,
        None => false,
    }
}

/// Whether a trigger's `on` entry hears `event`. `file.changed:<glob>` hears a change to a
/// path its glob matches.
pub fn listens(on: &str, event: &str, path: &str) -> bool {
    match on.split_once(':') {
        Some((name, glob)) => name == event && super::builtins::fs::glob(&super::builtins::fs::expand(glob).display().to_string(), path),
        None => on == event,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schedules_parse() {
        assert_eq!(parse_every("15m"), Some(900.0));
        assert_eq!(parse_every("30d"), Some(2592000.0));
        assert_eq!(parse_every("soon"), None);
        assert_eq!(parse_at("09:00 weekdays"), Some((9, 0, [true, true, true, true, true, false, false])));
        assert_eq!(parse_at("18:30 mon,wed").unwrap().2, [true, false, true, false, false, false, false]);
        assert!(parse_at("25:00").is_none() && parse_at("09:00 someday").is_none());
    }

    #[test]
    fn schedules_come_due() {
        let entry = |every: &str, at: &str| Entry { key: "p.x".into(), pack: "p".into(), name: "x".into(), trigger: false, every: every.into(), at: at.into(), on: vec![] };
        assert!(due(&entry("15m", ""), 0.0, 1_000_000.0));
        assert!(!due(&entry("15m", ""), 1_000_000.0 - 60.0, 1_000_000.0));
        assert!(due(&entry("15m", ""), 1_000_000.0 - 901.0, 1_000_000.0));
        // A time of day: due just after it passes, once.
        use chrono::{Local, TimeZone};
        let nine = Local.with_ymd_and_hms(2026, 11, 16, 9, 0, 0).single().unwrap().timestamp() as f64;
        assert!(due(&entry("", "09:00"), nine - 86_400.0, nine + 30.0));
        assert!(!due(&entry("", "09:00"), nine + 10.0, nine + 30.0));
        assert!(!due(&entry("", "09:00"), 0.0, nine - 30.0 + 7200.0 + 86_400.0 * 0.0 - 7200.0));
        // 2026-11-16 is a Monday; a weekend schedule waits.
        assert!(!due(&entry("", "09:00 weekends"), 0.0, nine + 30.0));
        assert!(listens("clipboard.changed", "clipboard.changed", ""));
        assert!(listens("file.changed:/tmp/notes/*.md", "file.changed", "/tmp/notes/a.md"));
        assert!(!listens("file.changed:/tmp/notes/*.md", "file.changed", "/tmp/other/a.md"));
        assert_eq!(entry("15m", "09:00").when(), "every 15m · at 09:00");
    }
}
