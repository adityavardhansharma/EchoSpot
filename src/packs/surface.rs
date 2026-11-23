//! Surfaces as the host draws them: any target of a pack, to blocks. A pack's own face or
//! surface is asked of its VM; a face the pack didn't write comes from the output's type; a
//! board is its tiles on the four-column grid.

use serde_json::{Value, json};

use super::error::{PackError, Result};
use super::faces;
use super::index::Kind;
use super::runtime::Runtime;
use super::shared::Resource;
use super::triggers::parse_every;

/// The side of the narrowest board tile, in points: one column of four across the palette.
pub const TILE: f64 = 168.0;
/// The room between tiles.
pub const TILE_GAP: f64 = 8.0;

/// What stands in while something loads: a skeleton the size of what is coming.
pub fn pending(title: &str, face: &str) -> Value {
    json!({ "_ui": "Loading", "state": "pending", "title": title, "face": face })
}

pub fn failed(title: &str, error: &PackError) -> Value {
    json!({ "_ui": "Empty", "state": "error", "title": title, "error": error.message, "code": error.code, "learn": error.learn })
}

/// One face of an ability, loading its output through a resource.
pub fn face(rt: &mut Runtime, address: &str, face: &str, args: &Value, state: &Value, fresh_for: Option<f64>) -> Result<Value> {
    let (pack, name) = address.split_once('.').unwrap_or((address, ""));
    let ability = rt.packs.get(pack).and_then(|p| p.ability(name)).cloned();
    let title = faces::title_of(rt, address);
    // A pack's own panel draws from the view and loads what it needs itself.
    if face == "panel" && ability.as_ref().is_some_and(|a| a.faces.iter().any(|f| f == "panel")) {
        return rt.render(pack, &format!("ability:{name}:panel"), args, state, &Value::Null);
    }
    let kind = match &ability {
        Some(a) => a.kind,
        None => rt.specs().into_iter().find(|s| s.address == address).map(|s| s.kind).ok_or_else(|| PackError::new("E0300", format!("no ability at {address:?}")))?,
    };
    if kind != Kind::Read {
        // Drawing never changes anything: an effect is run by a person, and then shows its result.
        return Ok(json!({ "_ui": "Empty", "title": title, "hint": format!("{} — press Return to run it", if kind == Kind::Final { "This can't be undone" } else { "This changes something" }), "run": address }));
    }
    let resource: Resource = rt.resource(address, args, fresh_for);
    match resource.state {
        "ok" => {
            if face == "json" {
                return Ok(json!({ "_ui": "code", "source": serde_json::to_string_pretty(&resource.value).unwrap_or_default(), "language": "json" }));
            }
            Ok(faces::of_value(rt, address, face, args, state, &resource.value)?.unwrap_or_else(|| json!({ "_ui": "Empty", "title": title, "hint": format!("Nothing to show as a {face}") })))
        }
        "error" => Ok(failed(&title, resource.error.as_ref().unwrap_or(&PackError::new("E0203", "failed")))),
        _ => Ok(pending(&title, face)),
    }
}

/// A board: each tile is an ability's `tile` face, loaded on its own and refreshed on its own.
pub fn board(rt: &mut Runtime, pack: &str, name: &str, state: &Value) -> Result<Value> {
    let Some(board) = rt.packs.get(pack).and_then(|p| p.index.boards.get(name)).cloned() else {
        return Err(PackError::new("E0300", format!("{pack} has no board {name:?}")));
    };
    Ok(tiles(rt, &board.title, &board.look, &board.tiles, state))
}

/// Tiles as a Board block, whoever listed them: a pack's board file, or an AI answer.
pub fn tiles(rt: &mut Runtime, title: &str, look: &str, list: &[super::index::Tile], state: &Value) -> Value {
    let mut tiles = Vec::new();
    // Each tile's input as it was loaded, so a later tile can read what an earlier one loaded.
    let mut loaded: Vec<Value> = Vec::new();
    for tile in list {
        let mut args = if tile.args.is_null() { json!({}) } else { tile.args.clone() };
        let face_name = if tile.face.is_empty() { "tile" } else { &tile.face };
        let (w, h) = tile.span();
        let tile_state = json!({ "size": [w as f64 * TILE + (w as f64 - 1.0) * TILE_GAP, h as f64 * TILE + (h as f64 - 1.0) * TILE_GAP], "dark": state.get("dark") });
        // What earlier tiles loaded fills this one's inputs; until they have, it waits with them.
        let mut waiting: Option<Value> = None;
        let fields = input_of(rt, &tile.address);
        for (field, from) in &tile.inputs {
            let Some(source) = list.get(from - 1) else { continue };
            let source_args = loaded.get(from - 1).cloned().unwrap_or_else(|| json!({}));
            let r = rt.resource(&source.address, &source_args, parse_every(&source.refresh));
            match r.state {
                "ok" => {
                    // A text field takes the value as it reads: a list of events becomes their lines.
                    let text_field = fields.as_ref().and_then(|t| t.fields.get(field)).is_some_and(|t| matches!(t.kind.as_str(), "text" | "markdown"));
                    args[field.as_str()] = if text_field && !r.value.is_string() { json!(as_text(&r.value, r.ty.as_ref())) } else { r.value.clone() };
                }
                "error" => {
                    let e = PackError::new("E0203", format!("needs tile {from} ({}), which failed: {}", source.address, r.error.map(|e| e.message).unwrap_or_default()));
                    waiting = Some(failed(&tile.address, &e));
                }
                _ => waiting = waiting.or_else(|| Some(pending(&tile.address, face_name))),
            }
        }
        loaded.push(args.clone());
        // One tile failing is that tile's problem, not the board's.
        let content = match waiting {
            Some(content) => content,
            None => match face(rt, &tile.address, face_name, &args, &tile_state, parse_every(&tile.refresh)) {
                Ok(content) => content,
                Err(e) => failed(&tile.address, &e),
            },
        };
        let own = faces::title_of(rt, &tile.address);
        let title = if tile.title.is_empty() { own.clone() } else { tile.title.clone() };
        // A tile that names itself is called that, wherever its face would have put the ability's name.
        let mut content = content;
        if title != own {
            for key in ["heading", "caption", "title"] {
                if content.get(key).and_then(Value::as_str) == Some(own.as_str()) {
                    content[key] = json!(title);
                }
            }
        }
        tiles.push(json!({ "size": tile.size, "title": title, "address": tile.address, "args": args, "content": content }));
    }
    json!({ "_ui": "Board", "title": title, "look": look, "tiles": tiles })
}

/// A value as text a reader (or a model) can take in whole: a list a line per item, a record a
/// line per field, anything else as it shows.
fn as_text(value: &Value, ty: Option<&super::types::Type>) -> String {
    match value {
        Value::Array(items) => {
            let of = ty.and_then(|t| t.of.as_deref());
            items.iter().map(|v| format!("- {}", as_text(v, of).replace('\n', "\n  "))).collect::<Vec<_>>().join("\n")
        }
        Value::Object(map) if ty.is_none_or(|t| t.kind == "record" || t.kind == "any") => {
            map.iter().map(|(k, v)| format!("{k}: {}", super::blocks::show(v, ty.and_then(|t| t.fields.get(k))))).collect::<Vec<_>>().join("\n")
        }
        other => super::blocks::show(other, ty),
    }
}

/// The input an address takes: a pack's ability, or a built-in's.
fn input_of(rt: &Runtime, address: &str) -> Option<super::types::Type> {
    let (pack, name) = address.split_once('.')?;
    rt.packs.get(pack).and_then(|p| p.ability(name)).map(|a| a.input.clone())
        .or_else(|| super::builtins::find(address).or_else(|| super::builtins::find_dynamic(address)).map(|s| s.input))
}

/// Draws any target: `ability:<name>:<face>`, `surface:<name>`, `board:<name>`.
pub fn render(rt: &mut Runtime, pack: &str, target: &str, args: &Value, state: &Value) -> Result<Value> {
    let mut parts = target.splitn(3, ':');
    let (kind, name, face_name) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or("panel"));
    match kind {
        "ability" => face(rt, &format!("{pack}.{name}"), face_name, args, state, None),
        "board" => board(rt, pack, name, state),
        "surface" => rt.render(pack, target, args, state, &Value::Null),
        _ => Err(PackError::new("E0300", format!("can't draw {target:?}: a target is ability:<name>:<face>, surface:<name> or board:<name>"))),
    }
}

/// The Look a target is drawn under: its own, its pack's, or the one handed in.
pub fn look_of(rt: &Runtime, pack: &str, target: &str, blocks: &Value, fallback: &str) -> &'static super::looks::Look {
    let mut parts = target.splitn(3, ':');
    let (kind, name) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let p = rt.packs.get(pack);
    let declared = match kind {
        "surface" => p.and_then(|p| p.index.surfaces.get(name)).map(|s| s.look.clone()),
        "board" => p.and_then(|p| p.index.boards.get(name)).map(|b| b.look.clone()),
        _ => None,
    };
    let named = [blocks.get("look").and_then(Value::as_str).map(String::from), declared, p.map(|p| p.index.look.clone())].into_iter().flatten().find(|l| !l.is_empty());
    super::looks::get(named.as_deref().unwrap_or(fallback))
}
