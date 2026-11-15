//! Faces: how one ability's output looks on one surface. The output's type picks a default for
//! each of `row`, `panel`, `tile` and `line`; a pack overrides any of them in `faces`.

use serde_json::{Value, json};

use super::blocks;
use super::error::Result;
use super::runtime::Runtime;
use super::types::{self, Type};

/// The field of a record that names it, and the one that says more.
fn title_fields(ty: &Type, sample: Option<&Value>) -> (Option<String>, Option<String>) {
    let mut names: Vec<(String, String)> = ty.fields.iter().map(|(k, t)| (k.clone(), t.kind.clone())).collect();
    if names.is_empty() && let Some(Value::Object(m)) = sample {
        names = m.iter().map(|(k, v)| (k.clone(), types::infer(v).kind)).collect();
    }
    let texty = |kind: &str| matches!(kind, "text" | "markdown" | "enum" | "url" | "file" | "date" | "datetime" | "time" | "money" | "number" | "quantity" | "percent" | "duration");
    let title = ["title", "name", "label", "subject", "text", "summary", "id"].iter().find(|k| names.iter().any(|(n, _)| n == *k)).map(|k| k.to_string())
        .or_else(|| names.iter().find(|(_, kind)| kind == "text").map(|(n, _)| n.clone()))
        .or_else(|| names.first().map(|(n, _)| n.clone()));
    let subtitle = ["subtitle", "description", "repo", "author", "status", "url", "path", "location", "time", "date"].iter()
        .find(|k| names.iter().any(|(n, _)| n == *k) && title.as_deref() != Some(**k)).map(|k| k.to_string())
        .or_else(|| names.iter().find(|(n, kind)| Some(n) != title.as_ref() && texty(kind)).map(|(n, _)| n.clone()));
    (title, subtitle)
}

/// A list's items as the rows `ui.List` would have made.
fn rows(items: &[Value], of: &Type) -> Vec<Value> {
    let (title, subtitle) = title_fields(of, items.first());
    items.iter().enumerate().map(|(i, item)| {
        let field = |name: &Option<String>| name.as_ref().and_then(|n| item.get(n)).filter(|v| !v.is_null());
        let shown = |v: &Value, name: &Option<String>| blocks::show(v, name.as_ref().and_then(|n| of.fields.get(n)));
        let title_text = match (item, field(&title)) {
            (_, Some(v)) => shown(v, &title),
            (Value::Object(_), None) => format!("Item {}", i + 1),
            (other, None) => blocks::show(other, Some(of)),
        };
        let mut row = json!({ "id": item.get("id").map(|v| blocks::show(v, None)).unwrap_or_else(|| (i + 1).to_string()), "title": title_text, "value": item });
        if let Some(v) = field(&subtitle) {
            row["subtitle"] = json!(shown(v, &subtitle));
        }
        if let Some(icon) = item.get("icon").and_then(Value::as_str) {
            row["icon"] = json!(icon);
        }
        row
    }).collect()
}

/// When an event starts, as people say it: `in 12m`, `now`, `tomorrow 09:00`.
pub fn until(event: &Value) -> String {
    use chrono::{DateTime, Local};
    let start = event.get("starts").and_then(Value::as_i64).or_else(|| event.get("start").and_then(Value::as_str).and_then(|s| DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp()));
    let Some(start) = start else { return String::new() };
    let left = start - Local::now().timestamp();
    match left {
        i64::MIN..=-1 => "now".into(),
        0..=59 => "in under a minute".into(),
        60..=86_399 => format!("in {}", types::duration(left as f64)),
        _ => DateTime::from_timestamp(start, 0).map(|d| d.with_timezone(&Local).format("%a %H:%M").to_string()).unwrap_or_default(),
    }
}

fn series_points(v: &Value) -> Vec<f64> {
    v.as_array().into_iter().flatten().filter_map(|p| p.as_f64().or_else(|| p.get("v").and_then(Value::as_f64))).collect()
}

/// The default face for a value of a type. `None` where the design gives none: a table has
/// no row or line, an image no line.
pub fn default(face: &str, value: &Value, ty: &Type, title: &str) -> Option<Value> {
    let ty_json = serde_json::to_value(ty).unwrap_or(Value::Null);
    let shown = types::display(value, ty);
    if value.is_null() {
        return match face {
            "line" => Some(json!({ "_ui": "text", "text": "—" })),
            "row" => None,
            _ => Some(json!({ "_ui": "Empty", "title": title, "hint": "Nothing to show" })),
        };
    }
    let block = match (ty.kind.as_str(), face) {
        ("money" | "quantity" | "number" | "percent" | "duration", "row") => json!({ "_ui": "Row", "title": shown, "subtitle": title, "big": true, "value": value, "type": ty_json }),
        ("money" | "quantity" | "number" | "percent" | "duration", "panel" | "tile") => json!({ "_ui": "Stat", "value": value, "caption": title, "type": ty_json, "state": "ok" }),
        ("list", "row") => {
            let items = value.as_array()?;
            let of = ty.of.as_deref().cloned().unwrap_or_default();
            let top = rows(&items[..items.len().min(1)], &of).into_iter().next()?;
            json!({ "_ui": "Row", "title": top["title"], "subtitle": top.get("subtitle").cloned().unwrap_or_else(|| json!(title)), "value": top["value"], "type": serde_json::to_value(&of).unwrap_or(Value::Null), "accessory": shown })
        }
        ("list", "panel" | "tile") => {
            let items = value.as_array()?;
            let of = ty.of.as_deref().cloned().unwrap_or_default();
            // A tile is a glance: the top five.
            let shown_items = if face == "tile" { &items[..items.len().min(5)] } else { &items[..] };
            json!({ "_ui": "List", "state": "ok", "rows": rows(shown_items, &of), "type": ty_json, "heading": title, "density": if face == "tile" { "compact" } else { "regular" }, "empty": format!("No {}", title.to_lowercase()), "more": items.len().saturating_sub(shown_items.len()) })
        }
        ("list", "line") => json!({ "_ui": "text", "text": shown }),
        ("series", "row") => json!({ "_ui": "Row", "title": title, "sparkline": series_points(value), "accessory": series_points(value).last().map(|v| types::number(*v)) }),
        ("series", "panel" | "tile") => json!({ "_ui": "Chart", "series": value, "title": title, "state": "ok", "kind": "line" }),
        ("series", "line") => {
            let points = series_points(value);
            let (last, first) = (points.last().copied().unwrap_or(0.0), points.first().copied().unwrap_or(0.0));
            json!({ "_ui": "text", "text": format!("{} {}", types::number(last), if last > first { "↑" } else if last < first { "↓" } else { "→" }) })
        }
        ("event", "row") => json!({ "_ui": "Row", "title": shown, "subtitle": until(value), "value": value, "type": ty_json }),
        ("event", "panel") => json!({ "_ui": "Detail", "state": "ok", "markdown": format!("# {shown}\n\n{}", value.get("location").and_then(Value::as_str).unwrap_or("")), "fields": [["Starts", value.get("start")], ["Ends", value.get("end")], ["When", until(value)]] }),
        ("event", "tile") => json!({ "_ui": "Stat", "value": until(value), "caption": shown, "state": "ok", "intent": "live" }),
        ("event", "line") => json!({ "_ui": "text", "text": until(value) }),
        ("image", "row") => json!({ "_ui": "Row", "title": title, "image": value, "value": value, "type": ty_json }),
        ("image", "panel") => json!({ "_ui": "Gallery", "state": "ok", "rows": [{ "id": "1", "title": title, "image": value, "value": value }] }),
        ("image", "tile") => json!({ "_ui": "image", "source": value, "cover": true }),
        ("image", "line") => return None,
        ("table", "panel") => json!({ "_ui": "Table", "state": "ok", "columns": value.get("columns"), "rows": value.get("rows") }),
        ("table", "tile") => json!({ "_ui": "Table", "state": "ok", "columns": value.get("columns"), "rows": value.get("rows").and_then(Value::as_array).map(|r| r.iter().take(4).cloned().collect::<Vec<_>>()) }),
        ("table", _) => return None,
        ("record", "panel" | "tile") => {
            let items: Vec<Value> = value.as_object().into_iter().flatten().filter(|(_, v)| !v.is_object() && !v.is_array()).map(|(k, v)| json!({ "label": k, "value": blocks::show(v, ty.fields.get(k)) })).collect();
            json!({ "_ui": "KeyValue", "state": "ok", "items": items, "heading": title })
        }
        (_, "row") => json!({ "_ui": "Row", "title": shown, "subtitle": title, "value": value, "type": ty_json }),
        (_, "panel") => {
            let text = value.as_str().map(String::from).unwrap_or_else(|| serde_json::to_string_pretty(value).unwrap_or_default());
            let markdown = if matches!(ty.kind.as_str(), "code") || !value.is_string() { format!("```\n{text}\n```") } else { text };
            json!({ "_ui": "Detail", "state": "ok", "markdown": markdown })
        }
        (_, "tile") => json!({ "_ui": "text", "text": value.as_str().map(String::from).unwrap_or(shown), "clip": 6, "caption": title }),
        (_, "line") => json!({ "_ui": "text", "text": shown }),
        _ => return None,
    };
    Some(block)
}

/// The output type an address declares, wherever it lives.
pub fn output_type(rt: &Runtime, address: &str) -> Option<Type> {
    let (pack, name) = address.split_once('.')?;
    match rt.packs.get(pack).and_then(|p| p.ability(name)) {
        Some(a) => a.output.clone(),
        None => rt.specs().into_iter().find(|s| s.address == address).map(|s| s.output),
    }
}

pub fn title_of(rt: &Runtime, address: &str) -> String {
    let (pack, name) = address.split_once('.').unwrap_or((address, ""));
    match rt.packs.get(pack).and_then(|p| p.ability(name)) {
        Some(a) => a.title.clone(),
        None => rt.specs().into_iter().find(|s| s.address == address).map(|s| s.title).unwrap_or_else(|| address.to_string()),
    }
}

/// One face of an ability for an output already in hand: the pack's own when it wrote one, the
/// type's default otherwise.
pub fn of_value(rt: &mut Runtime, address: &str, face: &str, args: &Value, state: &Value, out: &Value) -> Result<Option<Value>> {
    let (pack, name) = address.split_once('.').unwrap_or((address, ""));
    let custom = rt.packs.get(pack).and_then(|p| p.ability(name)).is_some_and(|a| a.faces.iter().any(|f| f == face));
    if custom {
        return rt.render(pack, &format!("ability:{name}:{face}"), args, state, out).map(Some);
    }
    let ty = output_type(rt, address).filter(|t| t.kind != "any").unwrap_or_else(|| types::infer(out));
    Ok(default(face, out, &ty, &title_of(rt, address)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_types_pick_their_faces() {
        let money = json!({ "amount": 4170.5, "currency": "INR" });
        let row = default("row", &money, &Type::of("money"), "USD → INR").unwrap();
        assert_eq!((row["_ui"].as_str(), row["title"].as_str(), row["big"].as_bool()), (Some("Row"), Some("₹4,170.50"), Some(true)));
        assert_eq!(default("tile", &money, &Type::of("money"), "x").unwrap()["_ui"], json!("Stat"));
        assert_eq!(default("line", &money, &Type::of("money"), "x").unwrap()["text"], json!("₹4,170.50"));

        let pr = Type { name: "pr".into(), ..Type::record([("title", Type::of("text")), ("url", Type::of("url")), ("failing", Type::of("bool"))]) };
        let prs = json!([{ "title": "Fix CI", "url": "https://x.dev/1", "failing": true }, { "title": "Docs", "url": "https://x.dev/2" }, {}, {}, {}, {}, {}]);
        let panel = default("panel", &prs, &Type::list(pr.clone()), "Pull Requests").unwrap();
        assert_eq!((panel["rows"].as_array().unwrap().len(), panel["rows"][0]["title"].as_str(), panel["rows"][0]["subtitle"].as_str()), (7, Some("Fix CI"), Some("https://x.dev/1")));
        let tile = default("tile", &prs, &Type::list(pr.clone()), "Pull Requests").unwrap();
        assert_eq!((tile["rows"].as_array().unwrap().len(), tile["more"].as_u64()), (5, Some(2)));
        assert_eq!(default("row", &prs, &Type::list(pr), "Pull Requests").unwrap()["title"], json!("Fix CI"));
        assert_eq!(default("line", &prs, &Type::list(Type::of("any")), "x").unwrap()["text"], json!("7 items"));

        assert!(default("row", &json!({ "columns": [], "rows": [] }), &Type::of("table"), "t").is_none());
        assert!(default("line", &json!("/a.png"), &Type::of("image"), "t").is_none());
        assert_eq!(default("line", &json!([1, 2, 5]), &Type::of("series"), "t").unwrap()["text"], json!("5 ↑"));
        assert_eq!(default("panel", &json!("# Hi"), &Type::of("markdown"), "t").unwrap()["markdown"], json!("# Hi"));
        let soon = json!({ "title": "Stand-up", "starts": chrono::Local::now().timestamp() + 12 * 60 + 5 });
        assert_eq!(default("line", &soon, &Type::of("event"), "t").unwrap()["text"], json!("in 12m 05s"));
    }

    #[test]
    fn a_tree_reads_like_the_surface() {
        let prs = json!([{ "title": "Fix CI", "url": "https://x.dev/1" }]);
        let mut panel = default("panel", &prs, &Type::list(Type::of("record")), "Pull Requests").unwrap();
        panel["rows"][0]["intent"] = json!("urgent");
        panel["rows"][0]["actions"] = json!([{ "title": "Open" }, { "title": "Copy URL" }]);
        assert_eq!(blocks::tree(&panel), "List  density=regular  1 row\n  - Fix CI — https://x.dev/1  [urgent]  actions: Open, Copy URL\n");
        let empty = json!({ "_ui": "List", "state": "pending", "rows": [], "empty": "No open PRs" });
        assert_eq!(blocks::tree(&empty), "List  [pending]  0 rows  empty: \"No open PRs\"\n");
    }
}
