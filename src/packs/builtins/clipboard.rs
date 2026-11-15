//! `clipboard.*`: the clipboard and its history. Reading needs `clipboard = "read"`, writing
//! `clipboard = "write"`.

use serde_json::{Value, json};

use super::{Done, Gate, Run, Spec, num, text};
use crate::features::clipboard::{self, Clipboard, Kind as ClipKind};
use crate::packs::index::Kind;
use crate::util;

fn current() -> String {
    util::output("wl-paste", &["--no-newline"]).unwrap_or_default()
}

fn entry(e: &clipboard::Entry) -> Value {
    let kind = match e.kind {
        ClipKind::Text => "text",
        ClipKind::Link => "url",
        ClipKind::Color => "color",
        ClipKind::Image => "image",
        ClipKind::File => "file",
    };
    let text = if e.kind == ClipKind::Image { clipboard::image_path(&e.text).display().to_string() } else { e.text.clone() };
    json!({ "id": e.id.to_string(), "kind": kind, "text": text, "time": e.time, "pinned": e.pinned, "source": e.source })
}

pub fn register(out: &mut Vec<Spec>) {
    out.push(Spec::new("clipboard.history", Kind::Read, Gate::Clipboard("read"), "The clipboard's history, newest first", "limit:number=50, query:text?", "list<record>", |_, i| {
        let (limit, query) = (num(i, "limit").unwrap_or(50.0).clamp(1.0, 1000.0) as usize, text(i, "query").to_lowercase());
        let entries = Clipboard::load().entries;
        Run::value(Value::Array(entries.iter().filter(|e| query.is_empty() || e.text.to_lowercase().contains(&query)).take(limit).map(entry).collect()))
    }));
    out.push(Spec::new("clipboard.read", Kind::Read, Gate::Clipboard("read"), "What is on the clipboard now", "", "text", |_, _| Run::value(Value::String(current()))));
    out.push(Spec::new("clipboard.copy", Kind::Effect, Gate::Clipboard("write"), "Put text on the clipboard", "text:text", "bool", |_, i| {
        let before = current();
        clipboard::copy_text(text(i, "text"));
        // Undo puts back what was there.
        Run::Now(Ok(Done { value: json!(true), undo: Some(("clipboard.copy".into(), json!({ "text": before }))), ..Default::default() }))
    }));
    out.push(Spec::new("clipboard.paste", Kind::Effect, Gate::Clipboard("write"), "Put text on the clipboard and paste it into the focused window", "text:text", "bool", |_, i| {
        let before = current();
        clipboard::copy_text(text(i, "text"));
        let pasted = util::has_program("wtype") && util::sh_detached("sleep 0.18; wtype -M ctrl v -m ctrl");
        Run::Now(Ok(Done { value: json!(pasted), undo: Some(("clipboard.copy".into(), json!({ "text": before }))), ..Default::default() }))
    }));
}
