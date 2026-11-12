//! Dates, countdowns and time zones in plain words: `days until 25 dec`, `3 weeks from friday`,
//! `5pm london in tokyo`, `time in new york`.

use chrono::{DateTime, Datelike, Duration, Local, Months, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Weekday};

use super::format_number;
use super::zones::{self, Zone};
use crate::model::Card;

/// A date written without its year, so the year can follow the question asked of it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Anchor {
    Fixed,
    MonthDay(u32, u32),
    Holiday(usize),
}

#[derive(Clone, Copy, Debug)]
struct Point {
    date: Option<NaiveDate>,
    time: Option<NaiveTime>,
    anchor: Anchor,
    /// Written on a 24-hour clock, so the answer is too.
    h24: bool,
    is_now: bool,
}

/// A stretch of time: calendar months, whole days, and seconds.
#[derive(Clone, Copy, Debug, Default)]
struct Span {
    months: i32,
    days: i64,
    secs: i64,
    /// Finer than a day, so the answer carries a time.
    fine: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Count {
    Days,
    Weeks,
    Months,
    Years,
    Hours,
    Minutes,
    Seconds,
    Workdays,
    Time,
}

const MONTHS: &[(&str, &str)] = &[
    ("january", "jan"), ("february", "feb"), ("march", "mar"), ("april", "apr"), ("may", "may"), ("june", "jun"),
    ("july", "jul"), ("august", "aug"), ("september", "sep"), ("october", "oct"), ("november", "nov"), ("december", "dec"),
];

fn month(word: &str) -> Option<u32> {
    let word = word.trim_end_matches('.');
    MONTHS.iter().position(|(full, short)| word == *full || word == *short || (word == "sept" && *short == "sep")).map(|i| i as u32 + 1)
}

fn weekday(word: &str) -> Option<Weekday> {
    Some(match word {
        "monday" | "mon" => Weekday::Mon,
        "tuesday" | "tue" | "tues" => Weekday::Tue,
        "wednesday" | "wed" => Weekday::Wed,
        "thursday" | "thu" | "thur" | "thurs" => Weekday::Thu,
        "friday" | "fri" => Weekday::Fri,
        "saturday" | "sat" => Weekday::Sat,
        "sunday" | "sun" => Weekday::Sun,
        _ => return None,
    })
}

fn ymd(y: i32, m: u32, d: u32) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(y, m, d)
}

/// The nth (from one) given weekday of a month.
fn nth_weekday(y: i32, m: u32, day: Weekday, n: u32) -> Option<NaiveDate> {
    NaiveDate::from_weekday_of_month_opt(y, m, day, n as u8)
}

fn easter(y: i32) -> Option<NaiveDate> {
    // The anonymous Gregorian computus.
    let (a, b, c) = (y % 19, y / 100, y % 100);
    let (d, e) = (b / 4, b % 4);
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let (i, k) = (c / 4, c % 4);
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    ymd(y, ((h + l - 7 * m + 114) / 31) as u32, ((h + l - 7 * m + 114) % 31 + 1) as u32)
}

type Holiday = (&'static [&'static str], fn(i32) -> Option<NaiveDate>);

static HOLIDAYS: &[Holiday] = &[
    (&["christmas eve", "xmas eve"], |y| ymd(y, 12, 24)),
    (&["christmas", "christmas day", "xmas"], |y| ymd(y, 12, 25)),
    (&["boxing day"], |y| ymd(y, 12, 26)),
    (&["new years eve", "new year's eve", "new year eve", "nye"], |y| ymd(y, 12, 31)),
    (&["new year", "new years", "new year's", "new years day", "new year's day", "new year day"], |y| ymd(y, 1, 1)),
    (&["valentines", "valentine's", "valentines day", "valentine's day"], |y| ymd(y, 2, 14)),
    (&["pi day"], |y| ymd(y, 3, 14)),
    (&["st patricks day", "st patrick's day", "saint patrick's day", "st. patrick's day"], |y| ymd(y, 3, 17)),
    (&["april fools", "april fools day", "april fool's day"], |y| ymd(y, 4, 1)),
    (&["earth day"], |y| ymd(y, 4, 22)),
    (&["may day"], |y| ymd(y, 5, 1)),
    (&["independence day"], |y| ymd(y, 7, 4)),
    (&["halloween"], |y| ymd(y, 10, 31)),
    (&["good friday"], |y| Some(easter(y)? - Duration::days(2))),
    (&["easter", "easter sunday"], easter),
    (&["easter monday"], |y| Some(easter(y)? + Duration::days(1))),
    (&["mothers day", "mother's day"], |y| nth_weekday(y, 5, Weekday::Sun, 2)),
    (&["fathers day", "father's day"], |y| nth_weekday(y, 6, Weekday::Sun, 3)),
    (&["labor day", "labour day"], |y| nth_weekday(y, 9, Weekday::Mon, 1)),
    (&["thanksgiving", "thanksgiving day"], |y| nth_weekday(y, 11, Weekday::Thu, 4)),
    (&["black friday"], |y| Some(nth_weekday(y, 11, Weekday::Thu, 4)? + Duration::days(1))),
    (&["cyber monday"], |y| Some(nth_weekday(y, 11, Weekday::Thu, 4)? + Duration::days(4))),
];

fn anchored(anchor: Anchor, year: i32) -> Option<NaiveDate> {
    match anchor {
        Anchor::Fixed => None,
        // 29 February falls back a day outside leap years.
        Anchor::MonthDay(m, d) => ymd(year, m, d).or_else(|| ymd(year, m, d.saturating_sub(1))),
        Anchor::Holiday(i) => (HOLIDAYS[i].1)(year),
    }
}

fn year_word(word: &str) -> Option<i32> {
    (word.len() == 4 && word.bytes().all(|b| b.is_ascii_digit())).then(|| word.parse().ok()).flatten().filter(|y| (1000..=9999).contains(y))
}

fn day_word(word: &str) -> Option<u32> {
    let digits = word.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let suffix = &word[digits.len()..];
    if !matches!(suffix, "" | "st" | "nd" | "rd" | "th") || digits.is_empty() || digits.len() > 2 {
        return None;
    }
    digits.parse().ok().filter(|d| (1..=31).contains(d))
}

/// Whether dates here are written month first, as the United States does.
fn month_first() -> bool {
    ["LC_ALL", "LC_TIME", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty())).is_some_and(|l| l.starts_with("en_US"))
}

/// `2026-12-25`, `25/12/2026`, `25.12.2026`.
fn numeric_date(word: &str) -> Option<NaiveDate> {
    let sep = ['-', '/', '.'].into_iter().find(|s| word.matches(*s).count() == 2)?;
    let parts: Vec<&str> = word.split(sep).collect();
    if parts.iter().any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let n: Vec<u32> = parts.iter().filter_map(|p| p.parse().ok()).collect();
    if parts[0].len() == 4 {
        return ymd(n[0] as i32, n[1], n[2]);
    }
    if parts[2].len() != 4 {
        return None;
    }
    let (a, b, y) = (n[0], n[1], n[2] as i32);
    if sep == '/' && month_first() && a <= 12 {
        return ymd(y, a, b).or_else(|| ymd(y, b, a));
    }
    ymd(y, b, a).or_else(|| ymd(y, a, b))
}

fn shift_months(date: NaiveDate, months: i32) -> Option<NaiveDate> {
    if months >= 0 { date.checked_add_months(Months::new(months as u32)) } else { date.checked_sub_months(Months::new(months.unsigned_abs())) }
}

/// The date the leading words name: the date, how its year was given, and the words used.
fn date_part(w: &[&str], today: NaiveDate) -> Option<(NaiveDate, Anchor, usize)> {
    let first = *w.first()?;
    let fixed = |date: NaiveDate, n: usize| Some((date, Anchor::Fixed, n));
    match first {
        "today" | "tonight" => return fixed(today, 1),
        "tomorrow" | "tmrw" | "tmr" => return fixed(today + Duration::days(1), 1),
        "yesterday" => return fixed(today - Duration::days(1), 1),
        "eom" => return fixed(shift_months(today.with_day(1)?, 1)? - Duration::days(1), 1),
        "eoy" => return fixed(ymd(today.year(), 12, 31)?, 1),
        _ => {}
    }
    if w.len() >= 3 && w[..3] == ["day", "after", "tomorrow"] {
        return fixed(today + Duration::days(2), 3);
    }
    if w.len() >= 3 && w[..3] == ["day", "before", "yesterday"] {
        return fixed(today - Duration::days(2), 3);
    }
    // `end of the month`, `start of next year`.
    if matches!(first, "end" | "start" | "beginning") && w.get(1) == Some(&"of") {
        let mut i = 2;
        let mut ahead = 0;
        while let Some(word) = w.get(i).filter(|x| matches!(**x, "the" | "this" | "next")) {
            ahead += (*word == "next") as i32;
            i += 1;
        }
        let end = first == "end";
        let date = match *w.get(i)? {
            "month" => {
                let start = shift_months(today.with_day(1)?, ahead)?;
                if end { shift_months(start, 1)? - Duration::days(1) } else { start }
            }
            "year" => if end { ymd(today.year() + ahead, 12, 31)? } else { ymd(today.year() + ahead, 1, 1)? },
            "week" => {
                let monday = today - Duration::days(today.weekday().num_days_from_monday() as i64) + Duration::days(7 * ahead as i64);
                if end { monday + Duration::days(6) } else { monday }
            }
            _ => return None,
        };
        return fixed(date, i + 1);
    }
    if matches!(first, "next" | "last" | "this" | "coming" | "previous") {
        let second = *w.get(1)?;
        let sign: i64 = match first {
            "next" | "coming" => 1,
            "this" => 0,
            _ => -1,
        };
        if let Some(day) = weekday(second) {
            let ahead = (day.num_days_from_monday() as i64 - today.weekday().num_days_from_monday() as i64).rem_euclid(7);
            let days = match sign {
                1 => if ahead == 0 { 7 } else { ahead },
                0 => ahead,
                _ => if ahead == 0 { -7 } else { ahead - 7 },
            };
            return fixed(today + Duration::days(days), 2);
        }
        return match second {
            "week" => fixed(today + Duration::days(7 * sign), 2),
            "month" => fixed(shift_months(today, sign as i32)?, 2),
            "year" => fixed(shift_months(today, 12 * sign as i32)?, 2),
            _ => None,
        };
    }
    if let Some(day) = weekday(first) {
        let ahead = (day.num_days_from_monday() as i64 - today.weekday().num_days_from_monday() as i64).rem_euclid(7);
        return fixed(today + Duration::days(ahead), 1);
    }
    // Holidays, the longest name first.
    for n in (1..=w.len().min(4)).rev() {
        let name = w[..n].join(" ");
        if let Some(index) = HOLIDAYS.iter().position(|(names, _)| names.contains(&name.as_str())) {
            if let Some(year) = w.get(n).and_then(|x| year_word(x)) {
                return Some(((HOLIDAYS[index].1)(year)?, Anchor::Fixed, n + 1));
            }
            return Some((anchored(Anchor::Holiday(index), today.year())?, Anchor::Holiday(index), n));
        }
    }
    if let Some(date) = numeric_date(first) {
        return fixed(date, 1);
    }
    // `25 dec`, `25th of december 2026`, `dec 25`, `december 2026`.
    let with_year = |m: u32, d: u32, at: usize| match w.get(at).and_then(|x| year_word(x)) {
        Some(year) => Some((ymd(year, m, d)?, Anchor::Fixed, at + 1)),
        None => Some((anchored(Anchor::MonthDay(m, d), today.year())?, Anchor::MonthDay(m, d), at)),
    };
    if let Some(d) = day_word(first) {
        let at = if w.get(1) == Some(&"of") { 2 } else { 1 };
        let m = month(w.get(at)?)?;
        return with_year(m, d, at + 1);
    }
    if let Some(m) = month(first) {
        let at = if w.get(1) == Some(&"the") { 2 } else { 1 };
        if let Some(d) = w.get(at).and_then(|x| day_word(x)).filter(|_| w.get(at).and_then(|x| year_word(x)).is_none()) {
            return with_year(m, d, at + 1);
        }
        return with_year(m, 1, 1);
    }
    None
}

/// The clock time the leading words name: `5pm`, `5:30 pm`, `17:30`, `noon`.
fn time_part(w: &[&str]) -> Option<(NaiveTime, bool, usize)> {
    let first = *w.first()?;
    match first {
        "noon" | "midday" => return Some((NaiveTime::from_hms_opt(12, 0, 0)?, false, 1)),
        "midnight" => return Some((NaiveTime::from_hms_opt(0, 0, 0)?, false, 1)),
        _ => {}
    }
    let half = |s: &str| match s {
        "am" | "a.m." | "a.m" => Some(false),
        "pm" | "p.m." | "p.m" => Some(true),
        _ => None,
    };
    let split = first.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(first.len());
    let (body, suffix) = first.split_at(split);
    let (pm, used) = if suffix.is_empty() {
        match w.get(1).and_then(|x| half(x)) {
            Some(pm) => (Some(pm), 2),
            None => (None, 1),
        }
    } else {
        (Some(half(suffix)?), 1)
    };
    let mut fields = body.split(':');
    let (h, m, s) = (fields.next()?, fields.next(), fields.next());
    if fields.next().is_some() || h.is_empty() || h.len() > 2 {
        return None;
    }
    let field = |x: Option<&str>| match x {
        Some(x) if x.len() == 2 => x.parse::<u32>().ok(),
        Some(_) => None,
        None => Some(0),
    };
    let (mut hour, minute, second): (u32, u32, u32) = (h.parse().ok()?, field(m)?, field(s)?);
    match pm {
        Some(pm) => {
            if !(1..=12).contains(&hour) {
                return None;
            }
            hour = hour % 12 + if pm { 12 } else { 0 };
        }
        // A bare number is a time only with its minutes: `17:30`.
        None if m.is_none() => return None,
        None => {}
    }
    Some((NaiveTime::from_hms_opt(hour, minute, second)?, pm.is_none(), used))
}

/// A date, a time, or both in either order, from the leading words.
fn point(w: &[&str], now: NaiveDateTime) -> Option<(Point, usize)> {
    let mut p = Point { date: None, time: None, anchor: Anchor::Fixed, h24: false, is_now: false };
    if w.first() == Some(&"now") || (w.len() >= 2 && w[..2] == ["right", "now"]) {
        p = Point { date: Some(now.date()), time: Some(now.time()), is_now: true, ..p };
        return Some((p, if w[0] == "now" { 1 } else { 2 }));
    }
    let mut i = 0;
    loop {
        // `on friday`, `at 5pm`, `the 25th`.
        let skip = usize::from(i < w.len() && matches!(w[i], "at" | "on" | "the" | "@"));
        if p.date.is_none() && let Some((date, anchor, n)) = date_part(&w[(i + skip).min(w.len())..], now.date()) {
            (p.date, p.anchor) = (Some(date), anchor);
            i += skip + n;
            continue;
        }
        if p.time.is_none() && let Some((time, h24, n)) = time_part(&w[(i + skip).min(w.len())..]) {
            (p.time, p.h24) = (Some(time), h24);
            i += skip + n;
            continue;
        }
        break;
    }
    (i > 0).then_some((p, i))
}

/// A point that takes every word given.
fn whole_point(w: &[&str], now: NaiveDateTime) -> Option<Point> {
    point(w, now).filter(|(_, n)| *n == w.len()).map(|(p, _)| p)
}

/// The date a point means. A date without a year is the next one coming (`ahead`), or the last
/// one gone.
fn resolve(p: &Point, today: NaiveDate, ahead: bool) -> Option<NaiveDate> {
    let date = p.date?;
    if p.anchor == Anchor::Fixed {
        return Some(date);
    }
    if ahead && date < today {
        return anchored(p.anchor, today.year() + 1);
    }
    if !ahead && date > today {
        return anchored(p.anchor, today.year() - 1);
    }
    Some(date)
}

fn small_number(word: &str) -> Option<f64> {
    const WORDS: &[&str] = &["one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven", "twelve"];
    match word {
        "a" | "an" => Some(1.0),
        "half" => Some(0.5),
        _ => WORDS.iter().position(|x| *x == word).map(|i| i as f64 + 1.0).or_else(|| word.replace(',', "").parse().ok().filter(|n: &f64| n.is_finite() && *n >= 0.0)),
    }
}

fn add_to_span(span: &mut Span, n: f64, unit: &str) -> bool {
    let secs = |span: &mut Span, each: f64| {
        span.secs += (n * each).round() as i64;
        span.fine = true;
    };
    match unit {
        "s" | "sec" | "secs" | "second" | "seconds" => secs(span, 1.0),
        "m" | "min" | "mins" | "minute" | "minutes" => secs(span, 60.0),
        "h" | "hr" | "hrs" | "hour" | "hours" => secs(span, 3600.0),
        "d" | "day" | "days" => {
            span.days += n.trunc() as i64;
            if n.fract() != 0.0 {
                span.secs += (n.fract() * 86_400.0).round() as i64;
                span.fine = true;
            }
        }
        "w" | "wk" | "wks" | "week" | "weeks" => span.days += (n * 7.0).round() as i64,
        "fortnight" | "fortnights" => span.days += (n * 14.0).round() as i64,
        "mo" | "mos" | "month" | "months" => {
            span.months += n.trunc() as i32;
            span.days += (n.fract() * 30.44).round() as i64;
        }
        "y" | "yr" | "yrs" | "year" | "years" => span.months += (n * 12.0).round() as i32,
        "decade" | "decades" => span.months += (n * 120.0).round() as i32,
        "century" | "centuries" => span.months += (n * 1200.0).round() as i32,
        _ => return false,
    }
    true
}

/// `3 weeks`, `1 year and 2 months`, `a day`, `1h30m`.
fn span(w: &[&str]) -> Option<(Span, usize)> {
    let mut out = Span::default();
    let mut i = 0;
    loop {
        let skip = usize::from(i > 0 && i < w.len() && matches!(w[i], "and" | "&"));
        let Some(word) = w.get(i + skip) else { break };
        // A number then its unit.
        if let Some(n) = small_number(word) && let Some(unit) = w.get(i + skip + 1) && add_to_span(&mut out, n, unit) {
            i += skip + 2;
            continue;
        }
        // Or the two run together, once or more: `90min`, `1h30m`.
        let mut rest = *word;
        let mut trial = out;
        let mut pieces = 0;
        while !rest.is_empty() {
            let digits = rest.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(rest.len());
            let letters = rest[digits..].find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(rest.len() - digits);
            let Ok(n) = rest[..digits].parse::<f64>() else { break };
            if letters == 0 || !add_to_span(&mut trial, n, &rest[digits..digits + letters]) {
                break;
            }
            rest = &rest[digits + letters..];
            pieces += 1;
        }
        if pieces == 0 || !rest.is_empty() {
            break;
        }
        out = trial;
        i += skip + 1;
    }
    (i > 0).then_some((out, i))
}

fn apply(at: NaiveDateTime, span: &Span, sign: i64) -> Option<NaiveDateTime> {
    let date = shift_months(at.date(), span.months * sign as i32)?;
    NaiveDateTime::new(date, at.time()).checked_add_signed(Duration::days(span.days * sign) + Duration::seconds(span.secs * sign))
}

fn plural(n: i64, unit: &str) -> String {
    format!("{} {unit}{}", format_number(n as f64), if n == 1 { "" } else { "s" })
}

fn join(parts: &[(i64, &str)]) -> String {
    let shown: Vec<String> = parts.iter().filter(|(n, _)| *n != 0).map(|(n, unit)| plural(*n, unit)).collect();
    if shown.is_empty() { plural(0, parts.last().map_or("day", |p| p.1)) } else { shown.join(" ") }
}

/// Whole calendar years, months and days between two dates.
fn calendar_gap(a: NaiveDate, b: NaiveDate) -> (i64, i64, i64) {
    let mut months = (b.year() - a.year()) * 12 + b.month() as i32 - a.month() as i32;
    if shift_months(a, months).is_none_or(|d| d > b) {
        months -= 1;
    }
    let days = (b - shift_months(a, months).unwrap_or(a)).num_days();
    ((months / 12) as i64, (months % 12) as i64, days)
}

fn fmt_date(date: NaiveDate) -> String {
    date.format("%a, %-d %b %Y").to_string()
}

fn fmt_time(time: NaiveTime, h24: bool) -> String {
    time.format(if h24 { "%H:%M" } else { "%-I:%M %p" }).to_string()
}

fn fmt_moment(at: NaiveDateTime, with_time: bool, h24: bool) -> String {
    if with_time { format!("{}, {}", fmt_date(at.date()), fmt_time(at.time(), h24)) } else { fmt_date(at.date()) }
}

fn relative(today: NaiveDate, date: NaiveDate) -> String {
    match (date - today).num_days() {
        0 => "Today".into(),
        1 => "Tomorrow".into(),
        -1 => "Yesterday".into(),
        n if n > 0 => format!("In {}", plural(n, "day")),
        n => format!("{} ago", plural(-n, "day")),
    }
}

/// The gap between two moments in the asked unit, and a second reading of it for the badge.
fn gap(from: NaiveDateTime, to: NaiveDateTime, count: Count, with_time: bool) -> (String, String) {
    let (a, b) = if to < from { (to, from) } else { (from, to) };
    let secs = (b - a).num_seconds();
    let days = if with_time { secs / 86_400 } else { (b.date() - a.date()).num_days() };
    let (y, m, d) = calendar_gap(a.date(), b.date());
    let weeks = join(&[(days / 7, "week"), (days % 7, "day")]);
    let clock = || {
        let (h, min) = ((secs % 86_400) / 3600, (secs % 3600) / 60);
        if days > 0 { join(&[(days, "day"), (h, "hour"), (min, "minute")]) } else if secs >= 60 { join(&[(h, "hour"), (min, "minute")]) } else { plural(secs, "second") }
    };
    match count {
        Count::Days if with_time => (join(&[(days, "day"), ((secs % 86_400) / 3600, "hour")]), weeks),
        Count::Days => (plural(days, "day"), if days >= 14 { weeks } else { fmt_date(to.date()) }),
        Count::Weeks => (weeks, plural(days, "day")),
        Count::Months => (join(&[(y * 12 + m, "month"), (d, "day")]), plural(days, "day")),
        Count::Years => (join(&[(y, "year"), (m, "month"), (d, "day")]), plural(days, "day")),
        Count::Hours => (join(&[(secs / 3600, "hour"), ((secs % 3600) / 60, "minute")]), plural(days, "day")),
        Count::Minutes => (plural(secs / 60, "minute"), join(&[(secs / 3600, "hour"), ((secs % 3600) / 60, "minute")])),
        Count::Seconds => (plural(secs, "second"), join(&[(secs / 3600, "hour"), ((secs % 3600) / 60, "minute")])),
        Count::Workdays => {
            let n = (1..=days).filter(|i| (a.date() + Duration::days(*i)).weekday().num_days_from_monday() < 5).count() as i64;
            (plural(n, "weekday"), plural(days, "day"))
        }
        Count::Time if with_time => (clock(), "Countdown".into()),
        Count::Time if y == 0 && m == 0 => (plural(days, "day"), fmt_date(to.date())),
        Count::Time => (join(&[(y, "year"), (m, "month"), (d, "day")]), plural(days, "day")),
    }
}

/// The unit a count is asked in, and the words that named it.
fn count_word(w: &[&str]) -> Option<(Count, usize)> {
    if w.len() >= 2 && matches!(w[0], "working" | "business" | "work" | "week") && matches!(w[1], "days" | "day") {
        return Some((Count::Workdays, 2));
    }
    Some((match *w.first()? {
        "days" | "day" | "sleeps" | "nights" => Count::Days,
        "weeks" | "week" => Count::Weeks,
        "months" | "month" => Count::Months,
        "years" | "year" => Count::Years,
        "hours" | "hour" | "hrs" => Count::Hours,
        "minutes" | "minute" | "mins" => Count::Minutes,
        "seconds" | "second" | "secs" => Count::Seconds,
        "weekdays" | "workdays" => Count::Workdays,
        "time" | "countdown" | "long" => Count::Time,
        _ => return None,
    }, 1))
}

fn midnight(date: NaiveDate) -> NaiveDateTime {
    date.and_time(NaiveTime::MIN)
}

/// `days until 25 dec`, `time until 5pm`, `weeks since 1 jan`.
fn until_or_since(w: &[&str], now: NaiveDateTime) -> Option<Card> {
    let (count, n) = count_word(w).unwrap_or((Count::Time, 0));
    let mut i = n;
    while w.get(i).is_some_and(|x| matches!(*x, "left" | "remaining")) {
        i += 1;
    }
    if w.len() > i + 1 && w[i..i + 2] == ["to", "go"] {
        i += 2;
    }
    let ahead = match *w.get(i)? {
        "until" | "till" | "til" | "'til" | "before" | "untill" => true,
        "to" if n > 0 => true,
        "since" => false,
        _ => return None,
    };
    let p = whole_point(&w[i + 1..], now)?;
    let clocked = p.time.is_some() || matches!(count, Count::Hours | Count::Minutes | Count::Seconds);
    let mut date = match p.date {
        Some(_) => resolve(&p, now.date(), ahead)?,
        None => now.date(),
    };
    let time = p.time.unwrap_or(NaiveTime::MIN);
    // A bare time already gone today means tomorrow's (or, looking back, yesterday's).
    if p.date.is_none() && ahead && date.and_time(time) < now {
        date += Duration::days(1);
    } else if p.date.is_none() && !ahead && date.and_time(time) > now {
        date -= Duration::days(1);
    }
    let target = date.and_time(time);
    let from = if clocked { now } else { midnight(now.date()) };
    let (main, second) = if ahead { gap(from, target, count, clocked) } else { gap(target, from, count, clocked) };
    let passed = if ahead { target < from } else { target > from };
    Some(Card {
        left: fmt_moment(target, p.time.is_some(), p.h24),
        left_badge: if ahead { "Until" } else { "Since" }.into(),
        right: main,
        right_badge: if passed { if ahead { "Ago".into() } else { "From now".into() } } else { second },
    })
}

/// `days between 1 jan and 25 dec`, `from monday to friday`, `9am to 5:30pm`.
fn between(w: &[&str], now: NaiveDateTime) -> Option<Card> {
    let (asked, n) = match count_word(w) {
        Some((count, n)) if w.get(n).is_some_and(|x| matches!(*x, "between" | "from")) => (Some(count), n + 1),
        _ if matches!(w.first(), Some(&"between") | Some(&"from")) => (None, 1),
        _ => (None, 0),
    };
    let (a, used) = point(&w[n..], now)?;
    let joiner = *w.get(n + used)?;
    if !matches!(joiner, "to" | "until" | "till" | "and" | "&" | "-" | "–" | "→" | "->") || (n == 0 && joiner == "and") {
        return None;
    }
    let b = whole_point(&w[n + used + 1..], now)?;
    if a.date.is_some() != b.date.is_some() && !(a.is_now || b.is_now) {
        return None;
    }
    let clocked = (a.time.is_some() || b.time.is_some()) && !(a.is_now && b.time.is_none()) && !(b.is_now && a.time.is_none());
    let first = a.date.unwrap_or(now.date());
    let mut second = b.date.unwrap_or(now.date());
    if b.anchor != Anchor::Fixed && second < first {
        second = anchored(b.anchor, first.year() + 1)?;
    }
    let (from, mut to) = (first.and_time(a.time.unwrap_or(NaiveTime::MIN)), second.and_time(b.time.unwrap_or(NaiveTime::MIN)));
    if !clocked {
        to = midnight(second);
    }
    let from = if clocked { from } else { midnight(first) };
    // `10pm to 6am` runs overnight.
    if a.date.is_none() && b.date.is_none() && to < from {
        to += Duration::days(1);
    }
    let dated = a.date.is_some();
    let count = asked.unwrap_or(if clocked { Count::Time } else { Count::Days });
    let (main, second_reading) = gap(from, to, count, clocked);
    let side = |at: NaiveDateTime, p: &Point| match (dated, p.time.is_some() && !p.is_now) {
        (true, true) => fmt_moment(at, true, p.h24),
        (true, false) => at.date().format("%-d %b %Y").to_string(),
        _ => fmt_time(at.time(), p.h24),
    };
    Some(Card { left: format!("{} – {}", side(from, &a), side(to, &b)), left_badge: "Between".into(), right: main, right_badge: second_reading })
}

/// `3 weeks from now`, `in 45 days`, `10 days before christmas`, `today + 2 months`, `2 days ago`.
fn shifted(w: &[&str], now: NaiveDateTime, q: &str) -> Option<Card> {
    let here = Point { date: Some(now.date()), time: Some(now.time()), anchor: Anchor::Fixed, h24: false, is_now: true };
    let (base, amount, sign) = if w.first() == Some(&"in") {
        let (amount, n) = span(&w[1..])?;
        (n + 1 == w.len()).then_some((here, amount, 1))?
    } else if let Some((amount, n)) = span(w) {
        match w.get(n).copied() {
            Some("ago") if n + 1 == w.len() => (here, amount, -1),
            Some("later" | "hence") if n + 1 == w.len() => (here, amount, 1),
            Some("from" | "after" | "past" | "since") => (whole_point(&w[n + 1..], now)?, amount, 1),
            Some("before") => (whole_point(&w[n + 1..], now)?, amount, -1),
            _ => return None,
        }
    } else {
        let (base, n) = point(w, now)?;
        let sign = match *w.get(n)? {
            "+" | "plus" => 1,
            "-" | "−" | "minus" => -1,
            _ => return None,
        };
        let (amount, used) = span(&w[n + 1..])?;
        (n + 1 + used == w.len()).then_some((base, amount, sign))?
    };
    let date = match base.date {
        Some(_) => resolve(&base, now.date(), true)?,
        None => now.date(),
    };
    let with_time = amount.fine || (base.time.is_some() && !base.is_now);
    let start = date.and_time(base.time.unwrap_or(NaiveTime::MIN));
    let result = apply(start, &amount, sign)?;
    Some(Card {
        left: q.to_string(),
        left_badge: "Date".into(),
        right: fmt_moment(result, with_time, base.h24),
        right_badge: relative(now.date(), result.date()),
    })
}

/// `week number`, `day of year`, `days in february`, `what day is 25 dec`.
fn facts(w: &[&str], now: NaiveDateTime, q: &str) -> Option<Card> {
    let today = now.date();
    let tail = |from: usize| -> Option<NaiveDate> {
        let mut rest = &w[from.min(w.len())..];
        while let Some(first) = rest.first().filter(|x| matches!(**x, "of" | "for" | "on" | "is" | "was" | "will" | "be" | "in")) {
            let _ = first;
            rest = &rest[1..];
        }
        if rest.is_empty() { Some(today) } else { resolve(&whole_point(rest, now)?, today, true) }
    };
    if w.len() >= 2 && (w[..2] == ["week", "number"] || w[..2] == ["week", "no"] || (w.len() >= 3 && w[..3] == ["week", "of", "year"])) {
        let date = tail(if w[1] == "of" { 3 } else { 2 })?;
        let week = date.iso_week();
        return Some(Card { left: fmt_date(date), left_badge: "Date".into(), right: format!("Week {}", week.week()), right_badge: format!("ISO Week of {}", week.year()) });
    }
    if w.len() >= 3 && w[..3] == ["day", "of", "year"] {
        let date = tail(3)?;
        let total = if date.leap_year() { 366 } else { 365 };
        return Some(Card { left: fmt_date(date), left_badge: "Date".into(), right: format!("Day {}", date.ordinal()), right_badge: format!("of {total}") });
    }
    if w.len() >= 3 && w[..2] == ["days", "in"] {
        if let Some(year) = year_word(w[2]).filter(|_| w.len() == 3) {
            let leap = ymd(year, 1, 1)?.leap_year();
            return Some(Card { left: year.to_string(), left_badge: if leap { "Leap Year" } else { "Year" }.into(), right: plural(if leap { 366 } else { 365 }, "day"), right_badge: "Length".into() });
        }
        let m = month(w[2])?;
        let year = match w.get(3) {
            Some(x) if w.len() == 4 => year_word(x)?,
            None => today.year(),
            _ => return None,
        };
        let first = ymd(year, m, 1)?;
        let days = (shift_months(first, 1)? - first).num_days();
        return Some(Card { left: first.format("%B %Y").to_string(), left_badge: "Month".into(), right: plural(days, "day"), right_badge: "Length".into() });
    }
    // `what day is 25 dec`, `day of the week for july 4 2030`, `weekday 1 jan`.
    let from = match w {
        ["day", "of", "the", "week", ..] => 4,
        ["day", "of", "week", ..] => 3,
        ["day" | "weekday", ..] => 1,
        _ => return None,
    };
    if w.len() == from {
        return None;
    }
    let date = tail(from)?;
    Some(Card { left: q.to_string(), left_badge: "Date".into(), right: date.format("%A").to_string(), right_badge: date.format("%-d %B %Y").to_string() })
}

/// `unix time`, `1700000000 to date`, `tomorrow 5pm as unix`.
fn unix(w: &[&str], now: NaiveDateTime, utc: i64) -> Option<Card> {
    let stamp = |word: &str| -> Option<i64> {
        let digits = word.replace(',', "");
        (matches!(digits.len(), 9..=13) && digits.bytes().all(|b| b.is_ascii_digit())).then(|| digits.parse().ok()).flatten()
    };
    let is_unix = |x: &str| matches!(x, "unix" | "timestamp" | "epoch");
    let joiner = |x: &str| matches!(x, "to" | "as" | "in" | "->" | "→");
    match w {
        ["unix" | "epoch", "time" | "timestamp"] | ["current", "timestamp"] | ["unix"] | ["timestamp", "now"] | ["now", "to" | "as" | "in", "unix" | "timestamp" | "epoch"] => {
            Some(Card { left: fmt_moment(now, true, false), left_badge: zones::local().label, right: utc.to_string(), right_badge: "Unix Time".into() })
        }
        [n, j, "date" | "time" | "datetime" | "human"] | ["unix" | "timestamp" | "epoch", j, n] if stamp(n).is_some() && (joiner(j) || stamp(j).is_none()) => {
            let raw = stamp(n)?;
            // Thirteen digits count milliseconds.
            let secs = if n.replace(',', "").len() >= 12 { raw / 1000 } else { raw };
            let at = DateTime::from_timestamp(secs, 0)?.with_timezone(&Local).naive_local();
            Some(Card { left: raw.to_string(), left_badge: "Unix Time".into(), right: fmt_moment(at, true, false), right_badge: zones::local().label })
        }
        ["unix" | "timestamp" | "epoch", n] if stamp(n).is_some() => {
            let raw = stamp(n)?;
            let secs = if n.len() >= 12 { raw / 1000 } else { raw };
            let at = DateTime::from_timestamp(secs, 0)?.with_timezone(&Local).naive_local();
            Some(Card { left: raw.to_string(), left_badge: "Unix Time".into(), right: fmt_moment(at, true, false), right_badge: zones::local().label })
        }
        [head @ .., j, last] if joiner(j) && is_unix(last) => {
            let p = whole_point(head, now)?;
            let at = resolve(&p, now.date(), true).unwrap_or(now.date()).and_time(p.time.unwrap_or(NaiveTime::MIN));
            let secs = Local.from_local_datetime(&at).earliest()?.timestamp();
            Some(Card { left: fmt_moment(at, true, p.h24), left_badge: zones::local().label, right: secs.to_string(), right_badge: "Unix Time".into() })
        }
        _ => None,
    }
}

fn wall(utc: i64, zone: &Zone) -> Option<(NaiveDateTime, String)> {
    let (offset, abbr) = zones::offset(zone, utc)?;
    Some((DateTime::from_timestamp(utc + offset as i64, 0)?.naive_utc(), abbr))
}

fn zone_badge(zone: &Zone, abbr: &str, at: NaiveDateTime, beside: NaiveDate) -> String {
    let mut badge = if zone.label.eq_ignore_ascii_case(abbr) { zone.label.clone() } else { format!("{} · {abbr}", zone.label) };
    if at.date() != beside {
        badge.push_str(&at.date().format(" · %a %-d %b").to_string());
    }
    badge
}

/// A zone that takes every word given, allowing `time` after it.
fn whole_zone(w: &[&str]) -> Option<Zone> {
    let (zone, n) = zones::find(w)?;
    let rest = &w[n..];
    (rest.is_empty() || rest == ["time"] || rest == ["time", "now"] || rest == ["now"]).then_some(zone)
}

/// `time in tokyo`, `5pm london in tokyo`, `2pm est to ist`, `time difference between london and tokyo`.
fn zoned(w: &[&str], now: NaiveDateTime, utc: i64) -> Option<Card> {
    let local = zones::local();
    let joiner = |x: &str| matches!(x, "to" | "in" | "into" | "as" | "->" | "→" | "and" | "vs" | "&");
    // The gap between two zones.
    let mut rest = w;
    let mut asked_gap = false;
    while let Some(first) = rest.first().filter(|x| matches!(**x, "time" | "difference" | "diff" | "between" | "offset")) {
        asked_gap |= matches!(*first, "difference" | "diff" | "offset");
        rest = &rest[1..];
    }
    if asked_gap {
        let (a, n) = zones::find(rest)?;
        let b = match rest.get(n) {
            Some(j) if joiner(j) => whole_zone(&rest[n + 1..])?,
            None => local.clone(),
            _ => return None,
        };
        let (a, b) = if rest.get(n).is_none() { (b, a) } else { (a, b) };
        let (off_a, off_b) = (zones::offset(&a, utc)?.0, zones::offset(&b, utc)?.0);
        let diff = (off_b - off_a) as i64;
        let amount = join(&[(diff.abs() / 3600, "hour"), (diff.abs() % 3600 / 60, "minute")]);
        let (right, badge) = match diff {
            0 => ("Same time".to_string(), format!("{} and {}", a.label, b.label)),
            d if d > 0 => (format!("{amount} ahead"), format!("{} is ahead of {}", b.label, a.label)),
            _ => (format!("{amount} behind"), format!("{} is behind {}", b.label, a.label)),
        };
        return Some(Card { left: format!("{} → {}", a.label, b.label), left_badge: "Time Difference".into(), right, right_badge: badge });
    }

    // The time somewhere, now.
    let mut rest = w;
    let mut said_time = false;
    while let Some(first) = rest.first().filter(|x| matches!(**x, "current" | "time" | "now" | "in" | "at" | "right" | "date")) {
        said_time |= matches!(*first, "time" | "now");
        rest = &rest[1..];
    }
    let trailing = rest.len() >= 2 && matches!(rest[rest.len() - 1], "time" | "now");
    if (said_time || trailing) && let Some(zone) = whole_zone(rest) {
        let (here, here_abbr) = wall(utc, &local)?;
        let (there, abbr) = wall(utc, &zone)?;
        return Some(Card {
            left: fmt_time(here.time(), false),
            left_badge: zone_badge(&local, &here_abbr, here, here.date()),
            right: fmt_time(there.time(), false),
            right_badge: zone_badge(&zone, &abbr, there, here.date()),
        });
    }

    // A time in one zone, read in another.
    let (p, from, rest) = if let Some((p, n)) = point(w, now).filter(|(p, _)| p.time.is_some() && !p.is_now) {
        let rest = &w[n..];
        match rest.first().copied() {
            Some("to" | "->" | "→" | "into") => (p, local.clone(), rest),
            _ => {
                let skip = usize::from(matches!(rest.first(), Some(&"in") | Some(&"at")));
                let (zone, used) = zones::find(&rest[skip..])?;
                let mut after = &rest[skip + used..];
                if after.first() == Some(&"time") {
                    after = &after[1..];
                }
                (p, zone, after)
            }
        }
    } else {
        let (zone, n) = zones::find(w)?;
        let skip = usize::from(w.get(n) == Some(&"time")) + n;
        let (p, used) = point(&w[skip.min(w.len())..], now).filter(|(p, _)| p.time.is_some() && !p.is_now)?;
        (p, zone, &w[skip + used..])
    };
    let to = match rest.first() {
        None => local.clone(),
        Some(j) if joiner(j) && *j != "and" => whole_zone(&rest[1..])?,
        _ => return None,
    };
    if from == to {
        return None;
    }
    // Without a date, the day is today where the time was said.
    let date = match p.date {
        Some(_) => resolve(&p, now.date(), true)?,
        None => wall(utc, &from)?.0.date(),
    };
    let said = date.and_time(p.time?);
    let instant = zones::to_utc(&from, said.and_utc().timestamp())?;
    let (_, from_abbr) = zones::offset(&from, instant)?;
    let (there, abbr) = wall(instant, &to)?;
    Some(Card {
        left: fmt_time(said.time(), p.h24),
        left_badge: zone_badge(&from, &from_abbr, said, if p.date.is_some() { NaiveDate::MIN } else { said.date() }),
        right: fmt_time(there.time(), p.h24),
        right_badge: zone_badge(&to, &abbr, there, if p.date.is_some() { NaiveDate::MIN } else { said.date() }),
    })
}

/// A date on its own: `25 dec`, `next friday`, `2026-12-25`.
fn bare(w: &[&str], now: NaiveDateTime, q: &str) -> Option<Card> {
    let p = whole_point(w, now)?;
    if p.is_now || (w.len() < 2 && !q.chars().any(|c| c.is_ascii_digit())) {
        return None;
    }
    let date = resolve(&p, now.date(), true)?;
    Some(Card {
        left: q.to_string(),
        left_badge: date.format("%A").to_string(),
        right: fmt_moment(date.and_time(p.time.unwrap_or(NaiveTime::MIN)), p.time.is_some(), p.h24),
        right_badge: relative(now.date(), date),
    })
}

const FILLER: &[&str] = &[
    "how", "many", "much", "what", "what's", "whats", "when", "when's", "whens", "is", "are", "was", "will", "be", "the", "number", "of", "there",
    "it", "its", "it's", "tell", "me", "show", "calculate", "count",
];

pub fn evaluate(q: &str) -> Option<Card> {
    let now = Local::now();
    evaluate_at(q, now.naive_local(), now.timestamp())
}

fn evaluate_at(q: &str, now: NaiveDateTime, utc: i64) -> Option<Card> {
    let lower = q.to_lowercase().replace([',', '?'], " ").replace('+', " + ").replace("utc + ", "utc+").replace("gmt + ", "gmt+");
    let all: Vec<&str> = lower.split_whitespace().collect();
    let mut w = &all[..];
    while w.len() > 1 && FILLER.contains(&w[0]) {
        w = &w[1..];
    }
    if w.is_empty() || w.len() > 14 {
        return None;
    }
    until_or_since(w, now)
        .or_else(|| facts(w, now, q))
        .or_else(|| between(w, now))
        .or_else(|| shifted(w, now, q))
        .or_else(|| unix(w, now, utc))
        .or_else(|| zoned(w, now, utc))
        .or_else(|| bare(w, now, q))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Thursday 12 November 2026, 14:30, in a zone two hours ahead of UTC.
    fn at(q: &str) -> Option<Card> {
        let now = ymd(2026, 11, 12).unwrap().and_hms_opt(14, 30, 0).unwrap();
        evaluate_at(q, now, now.and_utc().timestamp() - 7200)
    }

    fn r(q: &str) -> String {
        at(q).map(|c| c.right).unwrap_or_default()
    }

    #[test]
    fn countdowns() {
        assert_eq!(r("days until christmas"), "43 days");
        assert_eq!(r("days until 25 dec"), "43 days");
        assert_eq!(r("how many days until december 25th?"), "43 days");
        assert_eq!(r("days till 1 jan"), "50 days");
        assert_eq!(r("weeks until christmas"), "6 weeks 1 day");
        assert_eq!(r("days until 2026-12-25"), "43 days");
        assert_eq!(r("time until 5pm"), "2 hours 30 minutes");
        assert_eq!(r("hours until midnight"), "9 hours 30 minutes");
        assert_eq!(r("days to new year"), "50 days");
        assert_eq!(r("months until 1 march 2027"), "3 months 17 days");
        assert_eq!(r("days until 10 nov"), "363 days");
        assert_eq!(r("weekdays until 20 nov"), "6 weekdays");
        assert_eq!(r("until friday"), "1 day");
    }

    #[test]
    fn since_and_between() {
        assert_eq!(r("days since 1 jan"), "315 days");
        assert_eq!(r("days since christmas"), "322 days");
        assert_eq!(r("days between 1 jan 2026 and 1 mar 2026"), "59 days");
        assert_eq!(r("1 jan 2026 to 1 jan 2027"), "365 days");
        assert_eq!(r("from 1 dec to 5 jan"), "35 days");
        assert_eq!(r("9am to 5:30pm"), "8 hours 30 minutes");
        assert_eq!(r("10pm to 6am"), "8 hours");
        assert_eq!(r("weeks between today and 25 dec"), "6 weeks 1 day");
    }

    #[test]
    fn date_maths() {
        assert_eq!(r("today + 3 weeks"), "Thu, 3 Dec 2026");
        assert_eq!(r("in 45 days"), "Sun, 27 Dec 2026");
        assert_eq!(r("3 weeks from now"), "Thu, 3 Dec 2026");
        assert_eq!(r("2 days ago"), "Tue, 10 Nov 2026");
        assert_eq!(r("10 days before christmas"), "Tue, 15 Dec 2026");
        assert_eq!(r("1 year and 2 months after 1 jan 2026"), "Mon, 1 Mar 2027");
        assert_eq!(r("31 jan 2027 + 1 month"), "Sun, 28 Feb 2027");
        assert_eq!(r("in 2 hours"), "Thu, 12 Nov 2026, 4:30 PM");
        assert_eq!(r("tomorrow 5pm - 90min"), "Fri, 13 Nov 2026, 3:30 PM");
        assert_eq!(r("next friday"), "Fri, 13 Nov 2026");
        assert_eq!(r("last monday"), "Mon, 9 Nov 2026");
        assert_eq!(r("25 dec"), "Fri, 25 Dec 2026");
        assert_eq!(at("25 dec").unwrap().right_badge, "In 43 days");
        assert_eq!(r("end of month"), "Mon, 30 Nov 2026");
        assert_eq!(r("easter 2027"), "Sun, 28 Mar 2027");
        assert_eq!(r("black friday"), "Fri, 27 Nov 2026");
    }

    #[test]
    fn calendar_facts() {
        assert_eq!(r("what day is 25 dec"), "Friday");
        assert_eq!(r("week number"), "Week 46");
        assert_eq!(r("day of year"), "Day 316");
        assert_eq!(r("days in february 2028"), "29 days");
        assert_eq!(r("how many days in 2028"), "366 days");
    }

    #[test]
    fn time_zones() {
        assert_eq!(r("5pm london in tokyo"), "2:00 AM");
        assert_eq!(at("5pm london in tokyo").unwrap().right_badge, "Tokyo · JST · Fri 13 Nov");
        assert_eq!(r("2pm est to ist"), "12:30 AM");
        assert_eq!(r("17:00 utc to new york"), "12:00");
        assert_eq!(r("9am pst in utc+5:30"), "10:30 PM");
        assert_eq!(r("1 jul 2026 12:00 london to utc"), "11:00");
        assert_eq!(r("time difference between london and tokyo"), "9 hours ahead");
        assert_eq!(r("time difference india and utc"), "5 hours 30 minutes behind");
        assert_eq!(r("time in tokyo"), "9:30 PM");
        assert_eq!(r("tokyo time"), "9:30 PM");
        assert_eq!(r("current time in new delhi"), "6:00 PM");
    }

    #[test]
    fn leaves_the_rest_alone() {
        for q in ["today", "now", "time", "monday", "5pm", "may", "3 weeks in days", "5 days to hours", "time machine", "12/25", "10 to 20", "firefox", "day", "2 + 2"] {
            assert!(at(q).is_none(), "{q}");
        }
    }
}
