//! `windows.*`, `files.search`, `calendar.*`: what is open, what is on disk, what is coming up.
//! A pack names each address it uses in `calls`.

use serde_json::{Value, json};

use super::{Done, Gate, Run, Spec, missing, num, text};
use crate::features::{calendar, files, windows};
use crate::packs::error::PackError;
use crate::packs::index::Kind;
use crate::settings::Settings;

fn window(c: &windows::Client) -> Value {
    json!({ "address": c.address, "title": c.title, "class": c.class, "workspace": c.workspace.name, "floating": c.floating })
}

/// A window's address as Hyprland writes it: nothing else reaches a dispatcher.
fn address(input: &Value) -> Result<String, PackError> {
    let address = text(input, "address");
    let hex = address.strip_prefix("0x").is_some_and(|h| !h.is_empty() && h.chars().all(|c| c.is_ascii_hexdigit()));
    if hex { Ok(address.to_string()) } else { Err(PackError::new("E0301", format!("{address:?} isn't a window address; windows.list gives them")).learn("echospot dev api show windows.list")) }
}

fn event(e: &calendar::Event) -> Value {
    use chrono::{Local, TimeZone};
    let iso = |secs: i64| Local.timestamp_opt(secs, 0).earliest().map(|d| d.to_rfc3339()).unwrap_or_default();
    json!({
        "title": e.title, "start": iso(e.start), "end": iso(e.end), "starts": e.start, "ends": e.end, "all_day": e.all_day,
        "location": e.location, "url": calendar::meeting_link(e).unwrap_or_else(|| e.url.clone()),
    })
}

/// Events from the user's calendars between two times, soonest first.
fn events(from: i64, to: i64) -> Vec<calendar::Event> {
    let mut events: Vec<calendar::Event> = Settings::load().calendars.iter().filter_map(|source| calendar::read(source, false).ok()).flat_map(|text| calendar::parse(&text, from, to)).collect();
    events.sort_by(|a, b| a.start.cmp(&b.start).then(b.all_day.cmp(&a.all_day)));
    events
}

pub fn register(out: &mut Vec<Spec>) {
    out.push(Spec::new("windows.list", Kind::Read, Gate::Calls, "Every open window", "", "list<window>", |_, _| {
        Run::value(Value::Array(windows::clients().iter().filter(|c| c.mapped && !c.hidden).map(window).collect()))
    }));
    out.push(Spec::new("windows.active", Kind::Read, Gate::Calls, "The focused window", "", "window?", |_, _| Run::value(windows::active().as_ref().map(window).unwrap_or(Value::Null))));
    out.push(Spec::new("windows.focus", Kind::Effect, Gate::Calls, "Focus a window", "address:text", "bool", |_, i| {
        let before = windows::active().map(|c| c.address);
        Run::Now(address(i).map(|a| {
            windows::focus(&a);
            Done { value: json!(true), undo: before.map(|b| ("windows.focus".to_string(), json!({ "address": b }))), ..Default::default() }
        }))
    }));
    out.push(Spec::new("windows.move", Kind::Effect, Gate::Calls, "Move a window to a workspace", "address:text, workspace:text", "bool", |_, i| {
        let workspace = text(i, "workspace").to_string();
        if workspace.is_empty() || !workspace.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':' | '+')) {
            return Run::err(PackError::new("E0301", format!("{workspace:?} isn't a workspace")));
        }
        Run::Now(address(i).map(|a| {
            let before = windows::clients().into_iter().find(|c| c.address == a).map(|c| c.workspace.name);
            let moved = windows::dispatch(&format!("hl.dsp.window.move({{ window = \"address:{a}\", workspace = \"{workspace}\" }})"), &["movetoworkspacesilent", &format!("{workspace},address:{a}")]);
            Done { value: json!(moved), undo: before.map(|b| ("windows.move".to_string(), json!({ "address": a, "workspace": b }))), ..Default::default() }
        }))
    }));
    out.push(Spec::new("files.search", Kind::Read, Gate::Calls, "Find files by name, as Search Files does", "query:text, limit:number=30", "list<file>", |_, i| {
        let (query, limit) = (text(i, "query").trim().to_string(), num(i, "limit").unwrap_or(30.0).clamp(1.0, 500.0) as usize);
        if query.is_empty() {
            return Run::err(missing("query"));
        }
        Run::Later(Box::new(move |_| {
            let hits = files::search(&query, &Settings::load().files);
            Done::value(Value::Array(hits.into_iter().take(limit).map(|p| Value::String(p.display().to_string())).collect()))
        }))
    }));
    out.push(Spec::new("calendar.next", Kind::Read, Gate::Calls, "The next event on the user's calendars", "", "event?", |_, _| {
        Run::Later(Box::new(|_| {
            let now = chrono::Local::now().timestamp();
            let (_, to) = calendar::window();
            Done::value(events(now, to).iter().find(|e| !e.all_day && e.end > now).map(event).unwrap_or(Value::Null))
        }))
    }));
    out.push(Spec::new("calendar.range", Kind::Read, Gate::Calls, "Events between two times (seconds since 1970); the next two weeks when left out", "from:number?, to:number?", "list<event>", |_, i| {
        let (from, to) = (num(i, "from"), num(i, "to"));
        Run::Later(Box::new(move |_| {
            let (start, end) = calendar::window();
            Done::value(Value::Array(events(from.map_or(start, |f| f as i64), to.map_or(end, |t| t as i64)).iter().map(event).collect()))
        }))
    }));
}
