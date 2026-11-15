//! Blocks: what `@echo/ui` returns. A surface is a tree of plain data naming components and
//! what they hold; the host draws it, prints it as text for an agent to read, and checks it.

use serde_json::Value;

use super::types::{self, Type};

/// Every component: its name, what it is for, and the props it reads.
pub const COMPONENTS: &[(&str, &str, &str)] = &[
    ("List", "Rows to pick from, with actions", "items, title, subtitle, icon, accessory, intent, section, detail, actions, empty, heading"),
    ("Grid", "Cells in columns", "items, title, image, icon, columns, actions, empty"),
    ("Detail", "A page of Markdown with fields beside it", "markdown, fields, image, actions"),
    ("Form", "Fields from a schema, and what to do with the values", "fields, ability, submit, title, values"),
    ("Stat", "One value that matters", "value, caption, unit, trend, history"),
    ("Chart", "A series as a line or bars", "series, kind, title"),
    ("Table", "Rows and columns", "columns, rows, value"),
    ("Timeline", "Things in time order", "items, title, subtitle, time"),
    ("Gallery", "Pictures, large", "items, image, title"),
    ("KeyValue", "Labels and their values", "items"),
    ("Progress", "How far along, from 0 to 1", "value, label"),
    ("Banner", "A message across the top", "title, text"),
    ("Empty", "Nothing here, and what to do about it", "title, hint, icon"),
    ("Tabs", "Views to switch between", "tabs"),
    ("Split", "Two panes side by side", "left, right"),
    ("Stream", "Text that arrives as it is written", "source"),
    ("Board", "Tiles on the four-column grid", "tiles"),
];

pub const PRIMITIVES: &[(&str, &str)] = &[
    ("row", "Children side by side"), ("col", "Children stacked"), ("box", "One child, with padding and a fill"), ("text", "A run of text"),
    ("spacer", "Takes the room that is left"), ("each", "One child per item"), ("icon", "A glyph by name"), ("image", "A picture from a file or a URL"),
    ("input", "A field the user types in"), ("fx", "An effect around a block"), ("glass", "A glass panel around children"),
    ("markdown", "Markdown, drawn"), ("code", "Code, monospaced"), ("chart", "A bare chart"),
];

pub const MODIFIERS: &[(&str, &str)] = &[
    ("look", "The art direction: glass, ink or ember"), ("intent", "What it means: urgent, live, loading, success, new, muted"),
    ("density", "compact, regular or roomy"), ("accessory", "Something small at the trailing edge"), ("fx", "A raw effect, counted against the budget"),
    ("empty", "What to say when there is nothing"), ("error", "What to say when loading failed"),
];

pub const INTENTS: &[&str] = &["urgent", "live", "loading", "success", "new", "muted", "warning"];

pub fn kind(block: &Value) -> &str {
    block.get("_ui").and_then(Value::as_str).unwrap_or("")
}

pub fn text<'a>(block: &'a Value, key: &str) -> &'a str {
    block.get(key).and_then(Value::as_str).unwrap_or("")
}

pub fn items<'a>(block: &'a Value, key: &str) -> &'a [Value] {
    block.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

/// A prop that may be a string or a small table saying the same thing: `empty = "No PRs"`,
/// `empty = { title = "No PRs", hint = "…" }`.
pub fn title_hint(v: Option<&Value>) -> (String, String) {
    match v {
        Some(Value::String(s)) => (s.clone(), String::new()),
        Some(Value::Object(m)) => (m.get("title").and_then(Value::as_str).unwrap_or("").into(), m.get("hint").or_else(|| m.get("text")).and_then(Value::as_str).unwrap_or("").into()),
        _ => (String::new(), String::new()),
    }
}

/// A value as a surface shows it, by its declared or apparent type.
pub fn show(v: &Value, ty: Option<&Type>) -> String {
    match ty {
        Some(t) => types::display(v, t),
        None => types::display(v, &types::infer(v)),
    }
}

/// The blocks directly inside a block.
pub fn children(block: &Value) -> Vec<&Value> {
    let mut out: Vec<&Value> = Vec::new();
    out.extend(items(block, "children"));
    for key in ["child", "left", "right", "content", "detail"] {
        if let Some(child) = block.get(key).filter(|c| c.get("_ui").is_some()) {
            out.push(child);
        }
    }
    for key in ["tabs", "tiles"] {
        out.extend(items(block, key).iter().filter_map(|t| t.get("content")).filter(|c| c.get("_ui").is_some()));
    }
    out.extend(items(block, "rows").iter().filter_map(|r| r.get("detail")).filter(|d| d.get("_ui").is_some()));
    out
}

/// Visits every block in a tree, depth first.
pub fn walk<'a>(block: &'a Value, visit: &mut dyn FnMut(&'a Value, usize)) {
    fn go<'a>(block: &'a Value, depth: usize, visit: &mut dyn FnMut(&'a Value, usize)) {
        if block.get("_ui").is_none() || depth > 64 {
            return;
        }
        visit(block, depth);
        for child in children(block) {
            go(child, depth + 1, visit);
        }
    }
    go(block, 0, visit);
}

fn clip(s: &str, max: usize) -> String {
    let s = s.replace('\n', " ");
    if s.chars().count() > max { format!("{}…", s.chars().take(max - 1).collect::<String>()) } else { s }
}

fn actions(v: Option<&Value>) -> String {
    let names: Vec<&str> = v.and_then(Value::as_array).into_iter().flatten().filter_map(|a| a.get("title").and_then(Value::as_str)).collect();
    if names.is_empty() { String::new() } else { format!("  actions: {}", names.join(", ")) }
}

/// One line for a block: its component and what matters about it.
fn line(block: &Value) -> String {
    let mut out = kind(block).to_string();
    let state = text(block, "state");
    if !state.is_empty() && state != "ok" {
        out.push_str(&format!("  [{state}]"));
        if let Some(error) = block.get("error").and_then(Value::as_str) {
            out.push_str(&format!(" {}", clip(error, 80)));
        }
    }
    for key in ["look", "intent", "density"] {
        let value = text(block, key);
        if !value.is_empty() {
            out.push_str(&format!("  {key}={value}"));
        }
    }
    let ty = block.get("type").and_then(|t| Type::from_json(t).ok());
    match kind(block) {
        "text" => out.push_str(&format!("  {:?}", clip(text(block, "text"), 100))),
        "icon" => out.push_str(&format!("  {}", text(block, "name"))),
        "image" => out.push_str(&format!("  {}", clip(text(block, "source"), 80))),
        "markdown" | "code" => out.push_str(&format!("  {:?}", clip(text(block, "source"), 100))),
        "fx" => out.push_str(&format!("  {}", block.get("effects").map(|e| e.to_string()).unwrap_or_default())),
        "List" | "Grid" | "Gallery" | "Timeline" => {
            let rows = items(block, "rows");
            out.push_str(&format!("  {} {}", rows.len(), if rows.len() == 1 { "row" } else { "rows" }));
            if rows.is_empty() {
                let (title, hint) = title_hint(block.get("empty"));
                out.push_str(&format!("  empty: {:?}", if title.is_empty() { "No Results".into() } else if hint.is_empty() { title } else { format!("{title} — {hint}") }));
            }
        }
        "Stat" => {
            out.push_str(&format!("  {}", show(block.get("value").unwrap_or(&Value::Null), ty.as_ref())));
            if !text(block, "caption").is_empty() {
                out.push_str(&format!("  ({})", text(block, "caption")));
            }
        }
        "Chart" | "chart" => out.push_str(&format!("  {} points  kind={}", items(block, "series").len(), Some(text(block, "kind")).filter(|k| !k.is_empty()).unwrap_or("line"))),
        "Table" => out.push_str(&format!("  {} columns, {} rows", items(block, "columns").len(), items(block, "rows").len())),
        "Detail" => out.push_str(&format!("  {:?}{}", clip(text(block, "markdown"), 100), actions(block.get("actions")))),
        "Progress" => out.push_str(&format!("  {:.0}%  {}", block.get("value").and_then(Value::as_f64).unwrap_or(0.0) * 100.0, text(block, "label"))),
        "Banner" | "Empty" => out.push_str(&format!("  {:?}  {}", text(block, "title"), clip(if text(block, "text").is_empty() { text(block, "hint") } else { text(block, "text") }, 80))),
        "Stream" => out.push_str(&format!("  {:?}", clip(text(block, "text"), 100))),
        "Form" => {
            let fields: Vec<String> = block.get("fields").and_then(Value::as_object).map(|m| m.keys().cloned().collect()).unwrap_or_default();
            out.push_str(&format!("  fields: {}{}", fields.join(", "), if text(block, "ability").is_empty() { String::new() } else { format!("  → {}", text(block, "ability")) }));
        }
        "Board" => out.push_str(&format!("  {} tiles", items(block, "tiles").len())),
        "Tabs" => out.push_str(&format!("  {}", items(block, "tabs").iter().map(|t| text(t, "title")).collect::<Vec<_>>().join(" | "))),
        _ => {}
    }
    out
}

/// A surface as indented text: what an agent reads to know what it built.
pub fn tree(root: &Value) -> String {
    fn go(block: &Value, depth: usize, out: &mut String) {
        let pad = "  ".repeat(depth);
        if block.get("_ui").is_none() {
            if !block.is_null() {
                out.push_str(&format!("{pad}{}\n", clip(&block.to_string(), 120)));
            }
            return;
        }
        out.push_str(&format!("{pad}{}\n", line(block)));
        match kind(block) {
            "List" | "Grid" | "Gallery" | "Timeline" => {
                let rows = items(block, "rows");
                let mut section = "";
                for row in rows.iter().take(12) {
                    if text(row, "section") != section {
                        section = text(row, "section");
                        out.push_str(&format!("{pad}  § {section}\n"));
                    }
                    let mut l = format!("{pad}  - {}", clip(&show(row.get("title").unwrap_or(&Value::Null), None), 70));
                    if let Some(sub) = row.get("subtitle").filter(|s| !s.is_null()) {
                        l.push_str(&format!(" — {}", clip(&show(sub, None), 60)));
                    }
                    if let Some(acc) = row.get("accessory").filter(|s| !s.is_null()) {
                        l.push_str(&format!("  ({})", clip(&show(acc, None), 30)));
                    }
                    if !text(row, "intent").is_empty() {
                        l.push_str(&format!("  [{}]", text(row, "intent")));
                    }
                    l.push_str(&actions(row.get("actions")));
                    out.push_str(&l);
                    out.push('\n');
                    if let Some(detail) = row.get("detail").filter(|d| d.get("_ui").is_some()) {
                        go(detail, depth + 2, out);
                    }
                }
                if rows.len() > 12 {
                    out.push_str(&format!("{pad}  … {} more\n", rows.len() - 12));
                }
            }
            "KeyValue" => {
                for item in items(block, "items") {
                    out.push_str(&format!("{pad}  {}: {}\n", text(item, "label"), clip(&show(item.get("value").unwrap_or(&Value::Null), None), 80)));
                }
            }
            "Table" => {
                let columns: Vec<String> = items(block, "columns").iter().map(|c| show(c, None)).collect();
                out.push_str(&format!("{pad}  | {} |\n", columns.join(" | ")));
                for row in items(block, "rows").iter().take(8) {
                    let cells: Vec<String> = row.as_array().into_iter().flatten().map(|c| clip(&show(c, None), 24)).collect();
                    out.push_str(&format!("{pad}  | {} |\n", cells.join(" | ")));
                }
            }
            "Detail" => {
                for field in items(block, "fields") {
                    let (label, value) = match field {
                        Value::Array(pair) => (pair.first().map(|l| show(l, None)).unwrap_or_default(), pair.get(1).map(|v| show(v, None)).unwrap_or_default()),
                        other => (text(other, "label").to_string(), show(other.get("value").unwrap_or(&Value::Null), None)),
                    };
                    out.push_str(&format!("{pad}  {label}: {}\n", clip(&value, 80)));
                }
            }
            "Board" => {
                for tile in items(block, "tiles") {
                    out.push_str(&format!("{pad}  tile {} {}\n", Some(text(tile, "size")).filter(|s| !s.is_empty()).unwrap_or("1x1"), text(tile, "title")));
                    if let Some(content) = tile.get("content") {
                        go(content, depth + 2, out);
                    }
                }
                return;
            }
            "Tabs" => {
                for tab in items(block, "tabs") {
                    out.push_str(&format!("{pad}  tab {:?}\n", text(tab, "title")));
                    if let Some(content) = tab.get("content") {
                        go(content, depth + 2, out);
                    }
                }
                return;
            }
            _ => {}
        }
        for child in children(block) {
            if !matches!(kind(block), "List" | "Grid" | "Gallery" | "Timeline") {
                go(child, depth + 1, out);
            }
        }
    }
    let mut out = String::new();
    go(root, 0, &mut out);
    out
}
