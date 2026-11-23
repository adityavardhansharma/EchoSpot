//! `system.*`: a notification, a link opened in the browser. Free to every pack.

use serde_json::json;

use super::{Gate, Run, Spec, missing, text};
use crate::packs::index::Kind;
use crate::util;

pub fn register(out: &mut Vec<Spec>) {
    out.push(Spec::new("system.notify", Kind::Effect, Gate::Free, "Show a desktop notification", "title:text, body:text?", "bool", |_, i| {
        let (title, body) = (text(i, "title"), text(i, "body"));
        if title.is_empty() {
            return Run::err(missing("title"));
        }
        // Arguments, never a shell line: nothing in the text can become a command.
        let sent = util::has_program("notify-send") && util::spawn_detached(&["notify-send".into(), "-a".into(), "EchoSpot".into(), "--".into(), title.into(), body.into()]);
        Run::value(json!(sent))
    }));
    out.push(Spec::new("system.open_url", Kind::Effect, Gate::Free, "Open a link in the default browser", "url:url", "bool", |_, i| {
        let url = text(i, "url");
        if !(url.starts_with("https://") || url.starts_with("http://") || url.starts_with("mailto:")) {
            return Run::err(crate::packs::error::PackError::new("E0301", format!("{url:?} isn't an http, https or mailto link")));
        }
        util::open_url(url);
        Run::value(json!(true))
    }));
    // The runtime answers these itself: it holds the timeline.
    out.push(Spec::new("timeline.list", Kind::Read, Gate::Calls, "What was done lately, newest first, and whether each can be undone", "limit:number=50", "list<record>", |_, _| Run::value(json!([]))));
    out.push(Spec::new("timeline.undo", Kind::Effect, Gate::Calls, "Take back one thing from the timeline, by its id", "id:number", "bool", |_, _| Run::value(json!(false))));
    // And these: it holds the registry.
    out.push(Spec::new("echo.describe", Kind::Read, Gate::Free, "An ability's title, kind, signature and JSON Schema", "address:text", "record", |_, _| Run::value(json!({}))));
    out.push(Spec::new("echo.search", Kind::Read, Gate::Free, "Find abilities by words in their address or title", "query:text, limit:number=20", "list<record>", |_, _| Run::value(json!([]))));
}
