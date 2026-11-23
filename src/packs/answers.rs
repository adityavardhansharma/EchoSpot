//! Answers become commands. EchoSpot's AI can answer with a live surface instead of text: a
//! board of tiles, each an ability's output drawn by its type. "Keep" turns that answer into a
//! pack. It is safe because the surface is only data naming abilities that already exist and
//! are already granted: no new code runs, and the kept pack asks for its `calls` like any other.

use std::path::PathBuf;

use serde_json::{Value, json};

use super::builtins::Gate;
use super::error::{PackError, Result};
use super::index::{Kind, Tile};
use super::manifest;
use super::runtime::Runtime;

/// The tool the chat's model is given.
pub const TOOL: &str = "echospot__show_surface";
pub const ABOUT: &str = "Answer with a live surface instead of text: a board of tiles, each one an ability's output drawn by EchoSpot as a list, a number, a chart or a table by its type. Use it when the answer is something the user would keep looking at (a dashboard, a status, a list that changes). Each tile names a read ability by its address, like calendar.next or github.prs, with its arguments.";

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string", "description": "A short name for the surface" },
            "tiles": { "type": "array", "items": { "type": "object", "properties": {
                "address": { "type": "string", "description": "The ability, as <pack>.<name>" },
                "args": { "type": "object", "description": "Its input" },
                "inputs": { "type": "object", "description": "Input fields filled from earlier tiles' outputs, by tile number from 1: { \"events\": 1 }", "additionalProperties": { "type": "integer" } },
                "size": { "type": "string", "description": "Columns by rows on a four-column grid: 1x1, 2x1, 2x2, 4x2" },
                "title": { "type": "string" },
            }, "required": ["address"] } },
        },
        "required": ["title", "tiles"],
    })
}

/// A model may spell an address as its tool name: `github__prs` is `github.prs`.
fn address_of(tile: &Value) -> String {
    let named = tile.get("address").and_then(Value::as_str).unwrap_or("").trim();
    if named.contains('.') { named.to_string() } else { named.replacen("__", ".", 1) }
}

fn tiles(spec: &Value) -> Vec<Tile> {
    spec.get("tiles").and_then(Value::as_array).into_iter().flatten().enumerate().map(|(i, t)| Tile {
        address: address_of(t), args: t.get("args").cloned().unwrap_or(Value::Null),
        // An earlier tile's output into this one's input field, by number from 1, as a board file says it.
        inputs: t.get("inputs").and_then(Value::as_object).into_iter().flatten()
            .filter_map(|(k, v)| v.as_u64().map(|n| n as usize).filter(|n| *n >= 1 && *n <= i).map(|n| (k.clone(), n))).collect(),
        size: t.get("size").and_then(Value::as_str).filter(|s| s.split_once('x').is_some_and(|(w, h)| w.parse::<u8>().is_ok() && h.parse::<u8>().is_ok())).unwrap_or("2x1").to_string(),
        title: t.get("title").and_then(Value::as_str).unwrap_or("").to_string(), ..Default::default()
    }).collect()
}

fn kind_of(rt: &Runtime, address: &str) -> Option<(Kind, bool)> {
    let (pack, name) = address.split_once('.')?;
    match rt.packs.get(pack).and_then(|p| p.ability(name)) {
        Some(a) => Some((a.kind, true)),
        None => rt.specs().into_iter().find(|s| s.address == address).map(|s| (s.kind, s.gate != Gate::Free)),
    }
}

/// Whether a surface can be shown: it has tiles, and each is a read that exists.
pub fn check(rt: &Runtime, spec: &Value) -> Result<()> {
    let list = tiles(spec);
    if list.is_empty() {
        return Err(PackError::new("E0301", "a surface needs at least one tile: { address = \"calendar.next\" }"));
    }
    for tile in &list {
        match kind_of(rt, &tile.address) {
            None => {
                let known: Vec<String> = rt.addresses().into_iter().map(|a| a.0).collect();
                return Err(PackError::new("E0300", format!("no ability at {:?}", tile.address)).nearest(&tile.address, known.iter().map(String::as_str)));
            }
            Some((kind, _)) if kind != Kind::Read => return Err(PackError::new("E0310", format!("{} changes something, so it can't be a tile: a surface only shows", tile.address))),
            _ => {}
        }
    }
    Ok(())
}

/// The surface as blocks.
pub fn board(rt: &mut Runtime, spec: &Value, state: &Value) -> Result<Value> {
    check(rt, spec)?;
    let title = spec.get("title").and_then(Value::as_str).unwrap_or("Answer");
    Ok(super::surface::tiles(rt, title, "", &tiles(spec), state))
}

fn luau(v: &Value, depth: usize) -> String {
    let pad = "\t".repeat(depth);
    match v {
        Value::Null => "nil".into(),
        Value::String(s) => format!("{s:?}"),
        Value::Array(list) => format!("{{ {} }}", list.iter().map(|x| luau(x, depth)).collect::<Vec<_>>().join(", ")),
        Value::Object(map) if map.is_empty() => "{}".into(),
        Value::Object(map) => {
            let fields: Vec<String> = map.iter().map(|(k, x)| {
                let key = if k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !k.starts_with(|c: char| c.is_ascii_digit()) { k.clone() } else { format!("[{k:?}]") };
                format!("{pad}\t{key} = {},\n", luau(x, depth + 1))
            }).collect();
            format!("{{\n{}{pad}}}", fields.concat())
        }
        other => other.to_string(),
    }
}

/// An id for a title: `My Morning` is `my-morning`, and never one that is taken.
fn id_for(title: &str) -> String {
    let mut id: String = title.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    while id.contains("--") {
        id = id.replace("--", "-");
    }
    let mut id = id.trim_matches('-').to_string();
    if !id.starts_with(|c: char| c.is_ascii_lowercase()) {
        id = format!("kept-{id}");
    }
    let id = id.trim_matches('-').chars().take(32).collect::<String>().trim_matches('-').to_string();
    let base = if manifest::valid_id(&id) && !manifest::RESERVED.contains(&id.as_str()) { id } else { "kept".to_string() };
    let dir = super::packs_dir();
    (1..).map(|n| if n == 1 { base.clone() } else { format!("{base}-{n}") }).find(|c| !dir.join(c).exists()).expect("some number is free")
}

/// The files a kept surface is: a pack whose one board lists the same tiles.
pub fn files(rt: &Runtime, id: &str, spec: &Value) -> Vec<(String, String)> {
    let title = spec.get("title").and_then(Value::as_str).filter(|t| !t.trim().is_empty()).unwrap_or("Kept Answer");
    let list = tiles(spec);
    let mut calls: Vec<String> = list.iter().filter(|t| kind_of(rt, &t.address).is_some_and(|(_, gated)| gated)).map(|t| t.address.clone()).collect();
    calls.sort();
    calls.dedup();
    let permissions = if calls.is_empty() { String::new() } else { format!("\n[permissions]\ncalls = [{}]\n", calls.iter().map(|c| format!("{c:?}")).collect::<Vec<_>>().join(", ")) };
    let mut rows = String::new();
    for tile in &list {
        let mut parts = vec![format!("{:?}", tile.address)];
        if tile.args.as_object().is_some_and(|m| !m.is_empty()) {
            parts.push(format!("args = {}", luau(&tile.args, 4)));
        }
        parts.push(format!("size = {:?}", tile.size));
        if !tile.title.is_empty() {
            parts.push(format!("title = {:?}", tile.title));
        }
        rows.push_str(&format!("\t\t\t\t{{ {} }},\n", parts.join(", ")));
    }
    let init = format!("--!strict\n-- Kept from an AI answer. A board is tiles on a four-column grid; each tile is an ability's\n-- `tile` face. Edit the list, or give an ability a face of its own.\nlocal echo = require(\"@echo\")\n\nreturn echo.pack {{\n\tboards = {{\n\t\tmain = echo.board {{\n\t\t\ttitle = {title:?},\n\t\t\ticon = \"icons/board.svg\",\n\t\t\ttiles = {{\n{rows}\t\t\t}},\n\t\t}},\n\t}},\n}}\n");
    vec![
        ("pack.toml".into(), format!("id = \"{id}\"\nname = {title:?}\napi = {}\ntint = \"violet\"\n{permissions}", super::API)),
        ("init.luau".into(), init),
        ("icons/board.svg".into(), format!("{}\n", super::icon::TEMPLATE)),
        ("INTENT.md".into(), format!("# {title}\n\n## What was asked for\n\nKept from an answer EchoSpot's AI gave as a live surface.\n\n## Decisions\n\n- One board, `main`, with the tiles the answer showed.\n\n## Known limits\n\n- Tiles use each ability's default face.\n")),
        (".luaurc".into(), super::defs::LUAURC.to_string()),
    ]
}

/// Writes the surface into the user's packs as a pack of its own. Returns its id and folder.
pub fn keep(rt: &Runtime, spec: &Value) -> Result<(String, PathBuf)> {
    check(rt, spec)?;
    let id = id_for(spec.get("title").and_then(Value::as_str).unwrap_or(""));
    let dir = super::packs_dir().join(&id);
    for (name, text) in files(rt, &id, spec) {
        let path = dir.join(&name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| PackError::new("E0200", format!("couldn't make {}: {e}", parent.display())))?;
        }
        std::fs::write(&path, text).map_err(|e| PackError::new("E0200", format!("couldn't write {}: {e}", path.display())))?;
    }
    Ok((id, dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_is_checked_and_becomes_a_pack_that_loads() {
        let rt = Runtime::ephemeral();
        let spec = json!({ "title": "My Morning", "tiles": [
            { "address": "calc__eval", "args": { "expression": "6*7" }, "size": "1x1", "title": "Answer" },
            { "address": "calendar.next" },
        ] });
        check(&rt, &spec).unwrap();
        assert_eq!(check(&rt, &json!({ "title": "x", "tiles": [] })).unwrap_err().code, "E0301");
        assert_eq!(check(&rt, &json!({ "tiles": [{ "address": "calc.evall" }] })).unwrap_err().nearest[0], "calc.eval");
        assert_eq!(check(&rt, &json!({ "tiles": [{ "address": "clipboard.copy" }] })).unwrap_err().code, "E0310");

        let files = files(&rt, "my-morning", &spec);
        let toml = &files.iter().find(|(n, _)| n == "pack.toml").unwrap().1;
        let manifest = manifest::Manifest::parse(toml).unwrap();
        // A free built-in needs no grant; the calendar does.
        assert_eq!((manifest.id.as_str(), manifest.permissions.calls.clone()), ("my-morning", vec!["calendar.next".to_string()]));
        let init = &files.iter().find(|(n, _)| n == "init.luau").unwrap().1;
        assert!(init.contains("{ \"calc.eval\", args = {\n\t\t\t\t\texpression = \"6*7\",\n\t\t\t\t}, size = \"1x1\", title = \"Answer\" },"), "{init}");

        // What was written is a pack: it loads, and its board has the tiles.
        let dir = std::env::temp_dir().join(format!("echospot-kept-{}", std::process::id())).join("my-morning");
        for (name, text) in &files {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        let mut rt = Runtime::ephemeral();
        let id = rt.load(&dir, true).unwrap();
        let pack = &rt.packs[&id];
        assert!(pack.error.is_none(), "{:?}", pack.error);
        assert_eq!(pack.index.boards["main"].tiles.len(), 2);
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }
}
