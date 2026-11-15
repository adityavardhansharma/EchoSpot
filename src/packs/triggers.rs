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
}
