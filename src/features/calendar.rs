//! My Schedule: the next two weeks from any calendar that publishes an iCalendar feed (Google's
//! "secret address", Fastmail, Nextcloud, Outlook) or a local `.ics` file. Feeds are fetched in
//! the background and cached, so the screen opens with the last known day already on it.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone, Weekday};
use rune::Tx;

use crate::app::{App, Built, Tone};
use crate::features::calc::zones::{self, Zone};
use crate::features::forms::{self, Field, FieldKind, Form, Target};
use crate::features::fuzzy;
use crate::icons;
use crate::model::*;
use crate::theme::Tint;
use crate::util;

/// How far ahead the schedule looks, and how stale a cached feed may be.
const DAYS_AHEAD: i64 = 14;
const FRESH_SECS: u64 = 15 * 60;

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub title: String,
    /// Seconds since the epoch.
    pub start: i64,
    pub end: i64,
    pub all_day: bool,
    pub location: String,
    pub description: String,
    pub url: String,
}

#[derive(Default)]
pub struct Schedule {
    pub events: Vec<Event>,
    pub loading: bool,
    /// When the feeds were last read, in seconds.
    pub read_at: u64,
    pub error: Option<String>,
}

// ---- Reading iCalendar ------------------------------------------------------------------

/// One content line: its name, its parameters, its value.
struct Line {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

impl Line {
    fn param(&self, key: &str) -> Option<&str> {
        self.params.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

fn lines(text: &str) -> Vec<Line> {
    // A line that starts with a space or a tab continues the one before it.
    let mut unfolded: Vec<String> = Vec::new();
    for raw in text.lines() {
        let raw = raw.trim_end_matches('\r');
        match raw.strip_prefix([' ', '\t']) {
            Some(rest) if !unfolded.is_empty() => unfolded.last_mut().unwrap().push_str(rest),
            _ => unfolded.push(raw.to_string()),
        }
    }
    unfolded.iter().filter_map(|l| {
        // The value starts at the first colon outside a quoted parameter.
        let mut quoted = false;
        let colon = l.char_indices().find(|(_, c)| {
            if *c == '"' { quoted = !quoted; }
            *c == ':' && !quoted
        })?.0;
        let (head, value) = (&l[..colon], &l[colon + 1..]);
        let mut parts = head.split(';');
        let name = parts.next()?.to_ascii_uppercase();
        let params = parts.filter_map(|p| p.split_once('=').map(|(k, v)| (k.to_ascii_uppercase(), v.trim_matches('"').to_string()))).collect();
        Some(Line { name, params, value: value.to_string() })
    }).collect()
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A moment as a calendar writes it: a wall-clock time and the zone it is told in.
#[derive(Clone, Debug)]
struct Stamp {
    wall: NaiveDateTime,
    /// `None` for UTC; a floating time carries the local zone.
    zone: Option<Zone>,
    all_day: bool,
}

impl Stamp {
    fn utc(&self) -> i64 {
        Self::wall_utc(self.wall, &self.zone)
    }
    fn wall_utc(wall: NaiveDateTime, zone: &Option<Zone>) -> i64 {
        let secs = wall.and_utc().timestamp();
        match zone {
            Some(zone) => zones::to_utc(zone, secs).unwrap_or(secs),
            None => secs,
        }
    }
}

fn zone_named(tzid: &str) -> Zone {
    let known = std::path::Path::new("/usr/share/zoneinfo").join(tzid).is_file();
    if known && !tzid.is_empty() { Zone { iana: tzid.to_string(), fixed: 0, label: tzid.to_string() } } else { zones::local() }
}

fn stamp(line: &Line) -> Option<Stamp> {
    let v = line.value.trim();
    if line.param("VALUE") == Some("DATE") || v.len() == 8 {
        let date = NaiveDate::parse_from_str(v.get(..8)?, "%Y%m%d").ok()?;
        return Some(Stamp { wall: date.and_time(NaiveTime::MIN), zone: Some(zones::local()), all_day: true });
    }
    let wall = NaiveDateTime::parse_from_str(v.get(..15)?, "%Y%m%dT%H%M%S").ok()?;
    let zone = if v.ends_with('Z') { None } else { Some(line.param("TZID").map(zone_named).unwrap_or_else(zones::local)) };
    Some(Stamp { wall, zone, all_day: false })
}

/// `PT1H30M`, `P1D`, `P1W` in seconds.
fn duration(text: &str) -> Option<i64> {
    let text = text.trim().trim_start_matches(['+', 'P']);
    let (mut total, mut number, mut time) = (0i64, String::new(), false);
    for c in text.chars() {
        match c {
            'T' => time = true,
            d if d.is_ascii_digit() => number.push(d),
            unit => {
                let n: i64 = number.parse().ok()?;
                number.clear();
                total += n * match (unit, time) {
                    ('W', _) => 604_800,
                    ('D', _) => 86_400,
                    ('H', true) => 3600,
                    ('M', true) => 60,
                    ('S', true) => 1,
                    _ => return None,
                };
            }
        }
    }
    Some(total)
}

#[derive(Default)]
struct Rule {
    freq: String,
    interval: i64,
    count: Option<usize>,
    until: Option<i64>,
    /// Weekdays, each with the ordinal a monthly rule gives it (`2TU`, `-1FR`); 0 for none.
    by_day: Vec<(i32, Weekday)>,
    by_month_day: Vec<i32>,
}

fn weekday(code: &str) -> Option<Weekday> {
    Some(match code {
        "MO" => Weekday::Mon, "TU" => Weekday::Tue, "WE" => Weekday::Wed, "TH" => Weekday::Thu,
        "FR" => Weekday::Fri, "SA" => Weekday::Sat, "SU" => Weekday::Sun,
        _ => return None,
    })
}

fn rule(text: &str) -> Rule {
    let mut r = Rule { interval: 1, ..Default::default() };
    for (key, value) in text.split(';').filter_map(|p| p.split_once('=')) {
        match key.to_ascii_uppercase().as_str() {
            "FREQ" => r.freq = value.to_ascii_uppercase(),
            "INTERVAL" => r.interval = value.parse().unwrap_or(1).max(1),
            "COUNT" => r.count = value.parse().ok(),
            "UNTIL" => r.until = stamp(&Line { name: String::new(), params: Vec::new(), value: value.to_string() }).map(|s| if s.all_day { s.utc() + 86_399 } else { s.utc() }),
            "BYDAY" => r.by_day = value.split(',').filter_map(|d| {
                let d = d.trim();
                let at = d.len().checked_sub(2)?;
                Some((d[..at].parse().unwrap_or(0), weekday(&d[at..].to_ascii_uppercase())?))
            }).collect(),
            "BYMONTHDAY" => r.by_month_day = value.split(',').filter_map(|d| d.trim().parse().ok()).collect(),
            _ => {}
        }
    }
    r
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let next = if month == 12 { NaiveDate::from_ymd_opt(year + 1, 1, 1) } else { NaiveDate::from_ymd_opt(year, month + 1, 1) };
    next.and_then(|n| n.pred_opt()).map(|d| d.day()).unwrap_or(28)
}

/// The dates a monthly rule picks in one month.
fn month_dates(r: &Rule, year: i32, month: u32, default_day: u32) -> Vec<NaiveDate> {
    let last = days_in_month(year, month);
    let mut out = Vec::new();
    if !r.by_day.is_empty() {
        for (ordinal, day) in &r.by_day {
            let all: Vec<NaiveDate> = (1..=last).filter_map(|d| NaiveDate::from_ymd_opt(year, month, d)).filter(|d| d.weekday() == *day).collect();
            match *ordinal {
                0 => out.extend(all),
                n if n > 0 => out.extend(all.get(n as usize - 1)),
                n => out.extend(all.len().checked_sub((-n) as usize).and_then(|i| all.get(i))),
            }
        }
    } else {
        let days: Vec<i32> = if r.by_month_day.is_empty() { vec![default_day as i32] } else { r.by_month_day.clone() };
        for d in days {
            let day = if d > 0 { d } else { last as i32 + d + 1 };
            if day >= 1 {
                out.extend(NaiveDate::from_ymd_opt(year, month, day as u32));
            }
        }
    }
    out.sort();
    out
}

/// Every start a rule gives from its first, as wall-clock times, up to `limit` (a UTC instant).
fn occurrences(first: &Stamp, r: &Rule, limit: i64) -> Vec<NaiveDateTime> {
    let (date, time) = (first.wall.date(), first.wall.time());
    let mut out = Vec::new();
    let push = |d: NaiveDate, out: &mut Vec<NaiveDateTime>| -> bool {
        let wall = d.and_time(time);
        if d < date {
            return true;
        }
        let utc = Stamp::wall_utc(wall, &first.zone);
        if utc > limit || r.until.is_some_and(|u| utc > u) || r.count.is_some_and(|c| out.len() >= c) {
            return false;
        }
        out.push(wall);
        true
    };
    // A bound on the walk, for a rule that started long ago or never ends.
    for step in 0..6000i64 {
        let k = step * r.interval;
        let more = match r.freq.as_str() {
            "DAILY" => push(date + Duration::days(k), &mut out),
            "WEEKLY" => {
                let monday = date - Duration::days(date.weekday().num_days_from_monday() as i64) + Duration::weeks(k);
                let mut days: Vec<Weekday> = if r.by_day.is_empty() { vec![date.weekday()] } else { r.by_day.iter().map(|(_, d)| *d).collect() };
                days.sort_by_key(|d| d.num_days_from_monday());
                days.iter().all(|d| push(monday + Duration::days(d.num_days_from_monday() as i64), &mut out))
            }
            "MONTHLY" => {
                let months = date.year() as i64 * 12 + date.month0() as i64 + k;
                let (year, month) = (months.div_euclid(12) as i32, months.rem_euclid(12) as u32 + 1);
                month_dates(r, year, month, date.day()).into_iter().all(|d| push(d, &mut out))
            }
            "YEARLY" => match NaiveDate::from_ymd_opt(date.year() + k as i32, date.month(), date.day()) {
                Some(d) => push(d, &mut out),
                None => true,
            },
            _ => {
                push(date, &mut out);
                false
            }
        };
        if !more {
            break;
        }
    }
    out
}

/// The events of one feed that fall between two instants, repeats expanded.
pub fn parse(text: &str, from: i64, to: i64) -> Vec<Event> {
    struct Raw {
        uid: String,
        start: Stamp,
        end: Option<i64>,
        span: Option<i64>,
        rule: Option<Rule>,
        excluded: HashSet<i64>,
        /// The instance of a repeating event this one replaces.
        replaces: Option<i64>,
        cancelled: bool,
        event: Event,
    }
    let mut raws: Vec<Raw> = Vec::new();
    let mut current: Option<Vec<Line>> = None;
    let mut nested = 0;
    for line in lines(text) {
        match (line.name.as_str(), line.value.to_ascii_uppercase().as_str()) {
            ("BEGIN", "VEVENT") => current = Some(Vec::new()),
            // An alarm inside an event has its own DESCRIPTION and TRIGGER.
            ("BEGIN", _) if current.is_some() => nested += 1,
            ("END", "VEVENT") => {
                let Some(props) = current.take() else { continue };
                let get = |name: &str| props.iter().find(|l| l.name == name);
                let Some(start) = get("DTSTART").and_then(stamp) else { continue };
                let text_of = |name: &str| get(name).map(|l| unescape(&l.value)).unwrap_or_default();
                let excluded = props.iter().filter(|l| l.name == "EXDATE").flat_map(|l| {
                    l.value.split(',').filter_map(|v| stamp(&Line { name: String::new(), params: l.params.clone(), value: v.to_string() })).map(|s| s.utc()).collect::<Vec<_>>()
                }).collect();
                raws.push(Raw {
                    uid: text_of("UID"),
                    end: get("DTEND").and_then(stamp).map(|s| s.utc()),
                    span: get("DURATION").and_then(|l| duration(&l.value)),
                    rule: get("RRULE").map(|l| rule(&l.value)),
                    excluded,
                    replaces: get("RECURRENCE-ID").and_then(stamp).map(|s| s.utc()),
                    cancelled: text_of("STATUS").eq_ignore_ascii_case("CANCELLED"),
                    event: Event {
                        title: Some(text_of("SUMMARY").trim().to_string()).filter(|t| !t.is_empty()).unwrap_or_else(|| "Untitled".into()),
                        start: 0, end: 0, all_day: start.all_day, location: text_of("LOCATION").trim().to_string(),
                        description: text_of("DESCRIPTION").trim().to_string(), url: text_of("URL").trim().to_string(),
                    },
                    start,
                });
            }
            ("END", _) if nested > 0 => nested -= 1,
            _ if nested == 0 => {
                if let Some(props) = &mut current {
                    props.push(line);
                }
            }
            _ => {}
        }
    }
    // An instance edited on its own replaces what the rule would have put there.
    let replaced: HashSet<(String, i64)> = raws.iter().filter_map(|r| Some((r.uid.clone(), r.replaces?))).collect();
    let mut out = Vec::new();
    for raw in &raws {
        if raw.cancelled {
            continue;
        }
        let first = raw.start.utc();
        let length = raw.end.map(|e| e - first).or(raw.span).unwrap_or(if raw.start.all_day { 86_400 } else { 0 }).max(0);
        let starts: Vec<i64> = match &raw.rule {
            Some(rule) if raw.replaces.is_none() => occurrences(&raw.start, rule, to).into_iter().map(|w| Stamp::wall_utc(w, &raw.start.zone))
                .filter(|s| !raw.excluded.contains(s) && !replaced.contains(&(raw.uid.clone(), *s))).collect(),
            _ => vec![first],
        };
        for start in starts.into_iter().filter(|s| *s + length.max(1) > from && *s < to) {
            out.push(Event { start, end: start + length, ..raw.event.clone() });
        }
    }
    out.sort_by(|a, b| a.start.cmp(&b.start).then(b.all_day.cmp(&a.all_day)).then(a.title.cmp(&b.title)));
    out.dedup();
    out
}

// ---- Fetching ---------------------------------------------------------------------------

fn cache_file(source: &str) -> PathBuf {
    let dir = util::data_dir().join("calendar");
    let _ = std::fs::create_dir_all(&dir);
    // FNV-1a: a stable name for a URL that may hold a secret token.
    let hash = source.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    dir.join(format!("{hash:016x}.ics"))
}

fn is_url(source: &str) -> bool {
    source.starts_with("http://") || source.starts_with("https://") || source.starts_with("webcal://")
}

/// A feed's text: a local file as it is, a URL from its cache unless that has gone stale.
fn read(source: &str, force: bool) -> Result<String, String> {
    if !is_url(source) {
        let path = if let Some(rest) = source.strip_prefix("~/") { util::home().join(rest) } else { PathBuf::from(source) };
        return std::fs::read_to_string(&path).map_err(|_| format!("Could not read {}", util::tilde(&path)));
    }
    let cache = cache_file(source);
    let age = std::fs::metadata(&cache).ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).map(|d| d.as_secs());
    if !force && age.is_some_and(|a| a < FRESH_SECS) && let Ok(text) = std::fs::read_to_string(&cache) {
        return Ok(text);
    }
    let url = source.replacen("webcal://", "https://", 1);
    match crate::ext::net::get(&url).filter(|t| t.contains("BEGIN:VCALENDAR")) {
        Some(text) => {
            let _ = std::fs::write(&cache, &text);
            Ok(text)
        }
        // Offline: yesterday's copy is better than an empty day.
        None => std::fs::read_to_string(&cache).map_err(|_| "Could not reach a calendar".to_string()),
    }
}

fn window() -> (i64, i64) {
    let today = Local::now().date_naive().and_time(NaiveTime::MIN);
    let from = Local.from_local_datetime(&today).earliest().map(|t| t.timestamp()).unwrap_or_else(|| util::now_secs() as i64);
    (from, from + (DAYS_AHEAD + 1) * 86_400)
}

/// Reads every calendar off the UI thread and hands the palette the result.
pub fn refresh(app: &mut App, force: bool) {
    let sources = app.settings.calendars.clone();
    if sources.is_empty() {
        app.schedule = Schedule::default();
        return;
    }
    if app.schedule.loading || (!force && util::now_secs().saturating_sub(app.schedule.read_at) < 60) {
        return;
    }
    let Some(me) = app.me.clone() else { return };
    app.schedule.loading = true;
    std::thread::spawn(move || {
        let (from, to) = window();
        let (mut events, mut error) = (Vec::new(), None);
        for source in &sources {
            match read(source, force) {
                Ok(text) => events.extend(parse(&text, from, to)),
                Err(e) => error = Some(e),
            }
        }
        events.sort_by(|a, b| a.start.cmp(&b.start).then(b.all_day.cmp(&a.all_day)));
        me.update(move |s: &mut App, _| {
            // A first load lands on the first event, not on the row that was there while it was empty.
            let first = s.schedule.events.is_empty();
            s.schedule = Schedule { events, loading: false, read_at: util::now_secs(), error };
            match s.top().route {
                Route::Schedule if first => {
                    s.refresh();
                    s.top_mut().sel = 0;
                }
                Route::Schedule | Route::Root => s.refresh_keeping(),
                _ => {}
            }
        });
    });
}

// ---- The screen -------------------------------------------------------------------------

fn local(secs: i64) -> DateTime<Local> {
    Local.timestamp_opt(secs, 0).earliest().unwrap_or_else(Local::now)
}

fn clock(secs: i64) -> String {
    local(secs).format("%H:%M").to_string()
}

fn day_title(date: NaiveDate, today: NaiveDate) -> String {
    match (date - today).num_days() {
        0 => "Today".into(),
        1 => "Tomorrow".into(),
        _ => date.format("%A, %-d %B").to_string(),
    }
}

/// The first link in an event that opens a call.
pub fn meeting_link(event: &Event) -> Option<String> {
    const HOSTS: [&str; 8] = ["meet.google.com/", "zoom.us/j/", "zoom.us/my/", "teams.microsoft.com/", "teams.live.com/", "webex.com/", "whereby.com/", "meet.jit.si/"];
    [&event.url, &event.location, &event.description].into_iter().flat_map(|text| {
        text.split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\'' | '(' | ')' | ','))
            .filter(|w| w.starts_with("https://") || w.starts_with("http://")).map(|w| w.trim_end_matches(['.', ';']).to_string()).collect::<Vec<_>>()
    }).find(|link| HOSTS.iter().any(|h| link.contains(h)))
}

/// How soon an event is, for the row's quiet label: "Now", "in 20m", "in 3h".
fn soon(event: &Event, now: i64) -> String {
    if event.all_day {
        return String::new();
    }
    let until = event.start - now;
    if until <= 0 && event.end > now { "Now".into() }
    else if until <= 0 { String::new() }
    else if until < 3600 { format!("in {}m", (until + 59) / 60) }
    else if until < 6 * 3600 { format!("in {}h", until / 3600) }
    else { String::new() }
}

fn when(event: &Event) -> String {
    if event.all_day {
        "All day".into()
    } else if event.end > event.start {
        format!("{} – {}", clock(event.start), clock(event.end))
    } else {
        clock(event.start)
    }
}

pub fn row(app: &App, event: &Event, index: usize, now: i64) -> Item {
    let link = meeting_link(event);
    let live = !event.all_day && event.start <= now && event.end > now;
    let mut item = Item::new(format!("event:{}:{index}", event.start), event.title.clone(),
        IconSrc::Badge(if link.is_some() { &icons::CAMERA } else { &icons::CLOCK }, if live { Tint::Green } else if event.all_day { Tint::Graphite } else { Tint::Red }))
        .subtitle(when(event)).trailing(&soon(event, now));
    if let Some(link) = &link {
        item = item.action(Action::new("Join Meeting", &icons::CAMERA, Act::OpenUrl(link.clone())));
    }
    if !event.url.is_empty() && link.as_deref() != Some(event.url.as_str()) {
        item = item.action(Action::new("Open Event", &icons::ARROW_UP_RIGHT, Act::OpenUrl(event.url.clone())));
    }
    let date = local(event.start).format("%A, %-d %B").to_string();
    let summary = format!("{}\n{date}, {}{}", event.title, when(event), if event.location.is_empty() { String::new() } else { format!("\n{}", event.location) });
    item = item.action(Action::new("Copy Event", &icons::COPY, Act::Copy(summary)).keys("ctrl+shift+c"))
        .action(Action::new(app.paste_target(), &icons::CLIPBOARD, Act::Paste(format!("{} ({date}, {})", event.title, when(event)))))
        .action(Action::new("Refresh Calendars", &icons::REFRESH, Act::Calendar(CalOp::Refresh)).keys("ctrl+r").section(""))
        .action(Action::new("Add Calendar…", &icons::PLUS, Act::Calendar(CalOp::Add)).keys("ctrl+shift+n"));
    if !event.location.is_empty() && !event.location.starts_with("http") {
        item.accessories.push(Accessory { icon: Some(&icons::PIN), text: util::truncate(&event.location, 28), tag: None });
    }
    let mut detail = Detail { heading: Some("Event".into()), ..Default::default() };
    if !event.description.is_empty() {
        detail.markdown = Some(util::truncate(&event.description, 3000));
    }
    detail.fields.push(("Date".into(), date));
    detail.fields.push(("Time".into(), when(event)));
    if !event.location.is_empty() {
        detail.fields.push(("Where".into(), event.location.clone()));
    }
    if let Some(link) = link {
        detail.fields.push(("Call".into(), link.trim_start_matches("https://").to_string()));
    }
    item.detail(detail)
}

/// What is on now or starts within the next few hours: the launcher shows it before anything is typed.
pub fn up_next(app: &App) -> Vec<Item> {
    let now = util::now_secs() as i64;
    app.schedule.events.iter().enumerate()
        .filter(|(_, e)| !e.all_day && e.end > now && e.start - now < 3 * 3600).take(2)
        .map(|(i, e)| {
            let mut item = row(app, e, i, now);
            item.id = format!("next:{}", item.id);
            item.detail = None;
            item
        }).collect()
}

pub fn build(app: &App, query: &str) -> Built {
    let q = query.trim();
    let now = util::now_secs() as i64;
    let today = Local::now().date_naive();
    let mut sections = Sections::default();
    let mut day: Option<(NaiveDate, Vec<Item>)> = None;
    for (i, event) in app.schedule.events.iter().enumerate() {
        // Yesterday's meeting is not on the schedule; today's finished ones stay for the record.
        if event.end <= now && local(event.end - 1).date_naive() < today {
            continue;
        }
        if !q.is_empty() && fuzzy::score_item(q, &event.title, &format!("{} {}", event.location, util::truncate(&event.description, 300))).is_none() {
            continue;
        }
        let date = local(event.start).date_naive().max(today);
        if day.as_ref().is_some_and(|(d, _)| *d != date) && let Some((d, items)) = day.take() {
            sections.push(&day_title(d, today), items);
        }
        day.get_or_insert_with(|| (date, Vec::new())).1.push(row(app, event, i, now));
    }
    if let Some((d, items)) = day {
        sections.push(&day_title(d, today), items);
    }
    let any = !sections.0.is_empty();
    let mut manage = vec![Item::new("cal:add", "Add Calendar…", IconSrc::Badge(&icons::PLUS, Tint::Red)).trailing("Command")
        .action(Action::new("Add Calendar", &icons::PLUS, Act::Calendar(CalOp::Add)))];
    for source in &app.settings.calendars {
        let name = if is_url(source) { crate::features::quicklinks::host(&source.replacen("webcal://", "https://", 1)).unwrap_or("Calendar feed").to_string() } else { util::tilde(std::path::Path::new(source)) };
        manage.push(Item::new(format!("cal:{source}"), name, IconSrc::Badge(&icons::LINK, Tint::Graphite)).trailing("Calendar")
            .action(Action::new("Refresh Calendars", &icons::REFRESH, Act::Calendar(CalOp::Refresh)))
            .action(Action::new("Remove Calendar", &icons::TRASH, Act::Confirm(Box::new(Confirm {
                title: "Remove Calendar?".into(), message: "Its events leave the schedule. The calendar itself is untouched.".into(), icon: &icons::TRASH,
                confirm: "Remove".into(), destructive: true, act: Act::Calendar(CalOp::Remove(source.clone())),
            }))).keys("ctrl+x").destructive()));
    }
    if q.is_empty() || !any {
        sections.push("Calendars", manage);
    }
    let empty = if app.settings.calendars.is_empty() {
        ("No Calendars Yet", "Add a calendar's iCal address to see your day here.")
    } else if let Some(error) = &app.schedule.error {
        ("Calendar Unavailable", error.as_str())
    } else {
        ("Nothing Scheduled", "The next two weeks are clear.")
    };
    Built::list(sections, "Search your schedule...").loading(app.schedule.loading).empty(empty.0, empty.1).keep()
}

fn form() -> Form {
    Form::new("Add Calendar", "Add Calendar", vec![
        Field::new("source", "Address", FieldKind::Text).placeholder("https://calendar.google.com/calendar/ical/…/basic.ics").required()
            .info("An iCal (.ics) address, or the path of an .ics file. In Google Calendar: Settings, your calendar, \"Secret address in iCal format\"."),
    ], Target::Calendar)
}

pub fn perform(app: &mut App, op: CalOp, tx: &mut Tx) {
    match op {
        CalOp::Add => forms::open(app, form(), tx),
        CalOp::Refresh => {
            refresh(app, true);
            app.refresh_keeping();
        }
        CalOp::Remove(source) => {
            app.settings.calendars.retain(|c| *c != source);
            app.settings.save();
            let _ = std::fs::remove_file(cache_file(&source));
            app.schedule.read_at = 0;
            refresh(app, false);
            app.refresh();
            app.toast("Calendar Removed", Tone::Success);
        }
    }
}

pub fn save(app: &mut App, tx: &mut Tx) {
    let Some(form) = &mut app.form else { return };
    let source = form.get("source").trim().to_string();
    let exists = is_url(&source) || std::path::Path::new(&source.replacen("~/", &format!("{}/", util::home().display()), 1)).is_file();
    if !exists {
        if let Some(field) = form.fields.iter_mut().find(|f| f.id == "source") {
            field.error = "Not a web address or an existing file.".into();
        }
        return;
    }
    if !app.settings.calendars.contains(&source) {
        app.settings.calendars.push(source);
        app.settings.save();
    }
    app.pop(tx);
    refresh(app, true);
    app.toast("Calendar Added", Tone::Success);
}

/// Feeds whose secret addresses should not be shown whole.
#[allow(dead_code)]
pub fn sources(app: &App) -> HashMap<String, PathBuf> {
    app.settings.calendars.iter().map(|s| (s.clone(), cache_file(s))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(text: &str) -> i64 {
        NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").unwrap().and_utc().timestamp()
    }

    const FEED: &str = "BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nUID:a\r\nDTSTART:20261110T090000Z\r\nDTEND:20261110T093000Z\r\nSUMMARY:Stand-up\\, daily\r\nRRULE:FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR\r\nEXDATE:20261112T090000Z\r\nLOCATION:https://meet.google.com/abc-defg-hij\r\nBEGIN:VALARM\r\nDESCRIPTION:Reminder\r\nEND:VALARM\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:a\r\nRECURRENCE-ID:20261113T090000Z\r\nDTSTART:20261113T110000Z\r\nDTEND:20261113T113000Z\r\nSUMMARY:Stand-up (moved)\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:b\r\nDTSTART;VALUE=DATE:20261111\r\nSUMMARY:Holi\r\n day\r\nEND:VEVENT\r\nBEGIN:VEVENT\r\nUID:c\r\nDTSTART:20261111T150000Z\r\nDURATION:PT45M\r\nSUMMARY:Gone\r\nSTATUS:CANCELLED\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn weekly_with_exceptions() {
        let events = parse(FEED, utc("2026-11-09 00:00"), utc("2026-11-16 00:00"));
        let standups: Vec<i64> = events.iter().filter(|e| e.title == "Stand-up, daily").map(|e| e.start).collect();
        // Tuesday to Friday, less the excluded Thursday and the Friday that was moved.
        assert_eq!(standups, vec![utc("2026-11-10 09:00"), utc("2026-11-11 09:00")]);
        let moved = events.iter().find(|e| e.title == "Stand-up (moved)").unwrap();
        assert_eq!((moved.start, moved.end), (utc("2026-11-13 11:00"), utc("2026-11-13 11:30")));
        assert!(events.iter().any(|e| e.title == "Holiday" && e.all_day));
        assert!(!events.iter().any(|e| e.title == "Gone"));
        assert_eq!(meeting_link(&events.iter().find(|e| e.title == "Stand-up, daily").unwrap().clone()).as_deref(), Some("https://meet.google.com/abc-defg-hij"));
    }

    #[test]
    fn rules() {
        let first = Stamp { wall: NaiveDateTime::parse_from_str("2026-01-13 10:00", "%Y-%m-%d %H:%M").unwrap(), zone: None, all_day: false };
        let days = |text: &str, limit: &str| occurrences(&first, &rule(text), utc(limit)).iter().map(|d| d.format("%m-%d").to_string()).collect::<Vec<_>>();
        assert_eq!(days("FREQ=DAILY;INTERVAL=2;COUNT=3", "2027-01-01 00:00"), ["01-13", "01-15", "01-17"]);
        assert_eq!(days("FREQ=MONTHLY;BYDAY=2TU", "2026-04-01 00:00"), ["01-13", "02-10", "03-10"]);
        assert_eq!(days("FREQ=MONTHLY;BYDAY=-1FR;COUNT=2", "2027-01-01 00:00"), ["01-30", "02-27"]);
        assert_eq!(days("FREQ=WEEKLY;INTERVAL=2;UNTIL=20260215T000000Z", "2027-01-01 00:00"), ["01-13", "01-27", "02-10"]);
        assert_eq!(days("FREQ=YEARLY", "2028-06-01 00:00"), ["01-13", "01-13", "01-13"]);
        assert_eq!(duration("PT1H30M"), Some(5400));
        assert_eq!(duration("P1D"), Some(86_400));
    }
}
