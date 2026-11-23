//! What a pack declares, collected by running `init.luau` once: abilities with their schemas,
//! verbs, stores, surfaces, boards and triggers. Search, the grants screen, the CLI and the type
//! definitions all read this, never the VM.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::error::{PackError, Result};
use super::types::Type;

/// Whether an ability touches the world, and whether that can be taken back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Read,
    Effect,
    Final,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "read" => Kind::Read,
            "effect" => Kind::Effect,
            "final" => Kind::Final,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Kind::Read => "read",
            Kind::Effect => "effect",
            Kind::Final => "final",
        }
    }
}

pub const FACES: &[&str] = &["row", "panel", "tile", "line", "icon", "json"];
pub const EVENTS: &[&str] = &["clipboard.changed", "window.focused", "workspace.changed", "file.changed", "calendar.soon", "battery.low", "network.changed"];
pub const SURFACES: &[&str] = &["panel", "window", "overlay", "widget"];
pub const PLACES: &[&str] = &["top-left", "top", "top-right", "left", "center", "right", "bottom-left", "bottom", "bottom-right", "full"];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ability {
    pub title: String,
    pub description: String,
    pub icon: String,
    pub tint: String,
    pub kind: Kind,
    pub input: Type,
    pub output: Option<Type>,
    pub examples: Vec<String>,
    /// An effect the AI may call as a tool. Reads always are.
    pub ai: bool,
    /// Let the AI fill the input from what was typed, when no example matches.
    pub slots: bool,
    /// A final ability asks first, unless this is false or the user turned it off.
    pub confirm: bool,
    /// No row of its own in root search.
    pub hidden: bool,
    pub keywords: Vec<String>,
    pub every: String,
    pub at: String,
    pub on: Vec<String>,
    /// How long a result stays fresh for surfaces that load it: `30m`.
    pub refresh: String,
    /// A global hotkey the pack suggests, as `super+alt+p`. Only a person binds it, in Settings.
    pub hotkey: String,
    pub has_run: bool,
    pub has_query: bool,
    pub has_undo: bool,
    pub faces: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Verb {
    pub on: String,
    pub title: String,
    pub icon: String,
    pub kind: Kind,
    pub has_undo: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StoreDecl {
    #[serde(rename = "type")]
    pub ty: Option<Type>,
    pub keep: String,
    pub persist: bool,
    pub initial: Value,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Surface {
    pub surface: String,
    pub title: String,
    pub icon: String,
    pub place: String,
    /// An overlay's input: `none` lets clicks through.
    pub input: String,
    pub size: String,
    pub look: String,
    pub refresh: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tile {
    pub address: String,
    pub args: Value,
    /// Input fields filled from earlier tiles' outputs, by tile number from 1: `{ events = 1 }`.
    /// A board can read itself: "summarise my day" takes the calendar tile and the PR tile.
    pub inputs: BTreeMap<String, usize>,
    pub size: String,
    pub refresh: String,
    pub title: String,
    pub face: String,
}

impl Tile {
    /// Columns and rows on the board's four-column grid.
    pub fn span(&self) -> (usize, usize) {
        let (w, h) = self.size.split_once('x').unwrap_or(("1", "1"));
        (w.parse().unwrap_or(1).clamp(1, 4), h.parse().unwrap_or(1).clamp(1, 4))
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Board {
    pub title: String,
    pub look: String,
    pub icon: String,
    pub tiles: Vec<Tile>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Trigger {
    pub every: String,
    pub at: String,
    pub on: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Index {
    pub title: String,
    pub look: String,
    pub abilities: BTreeMap<String, Ability>,
    pub verbs: BTreeMap<String, Verb>,
    pub stores: BTreeMap<String, StoreDecl>,
    pub surfaces: BTreeMap<String, Surface>,
    pub boards: BTreeMap<String, Board>,
    pub triggers: BTreeMap<String, Trigger>,
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).unwrap_or("").to_string()
}

fn flag(v: &Value, key: &str) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn texts(v: &Value, key: &str) -> Vec<String> {
    v.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default()
}

/// A section of the description: an object, or the empty array an empty Luau table becomes.
fn entries<'a>(v: &'a Value, key: &str) -> Vec<(&'a String, &'a Value)> {
    v.get(key).and_then(Value::as_object).map(|m| m.iter().collect()).unwrap_or_default()
}

fn bad(at: String, message: String) -> PackError {
    PackError::new("E0202", message).at(at).learn("echospot dev docs abilities")
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !name.starts_with(|c: char| c.is_ascii_digit())
}

/// Whether `event` is one a trigger may name: `clipboard.changed`, `file.changed:~/x/*.md`.
pub fn valid_event(event: &str) -> bool {
    let name = event.split_once(':').map_or(event, |(n, _)| n);
    EVENTS.contains(&name) && (name == "file.changed") == event.contains(':')
}

fn check_when(at: &str, every: &str, when: &str, on: &[String]) -> Result<()> {
    if !every.is_empty() && super::triggers::parse_every(every).is_none() {
        return Err(bad(at.into(), format!("every = {every:?} isn't a duration; write \"30s\", \"15m\", \"2h\" or \"1d\"")));
    }
    if !when.is_empty() && super::triggers::parse_at(when).is_none() {
        return Err(bad(at.into(), format!("at = {when:?} isn't a time; write \"09:00\", \"09:00 weekdays\" or \"18:30 mon,wed\"")));
    }
    for event in on {
        if !valid_event(event) {
            let name = event.split_once(':').map_or(event.as_str(), |(n, _)| n);
            let hint = if name == "file.changed" { " (file.changed needs a glob: \"file.changed:~/Notes/*.md\")" } else { "" };
            return Err(bad(at.into(), format!("no event {event:?}{hint}")).nearest(name, EVENTS.iter().copied()));
        }
    }
    Ok(())
}

impl Index {
    /// Reads what `__describe` returned, checking each declaration.
    pub fn from_describe(v: &Value) -> Result<Self> {
        let mut index = Index { title: text(v, "title"), look: text(v, "look"), ..Default::default() };
        if !index.look.is_empty() && !super::looks::NAMES.contains(&index.look.as_str()) {
            return Err(bad("init.luau".into(), format!("no Look {:?}", index.look)).nearest(&index.look, super::looks::NAMES.iter().copied()));
        }
        for (name, a) in entries(v, "abilities") {
            let at = format!("abilities.{name}");
            if !valid_name(name) {
                return Err(bad(at, format!("{name:?} isn't a name: use letters, digits and underscores")));
            }
            let kind = text(a, "kind");
            let Some(kind) = Kind::parse(&kind) else {
                return Err(bad(format!("{at}.kind"), format!("kind = {kind:?} must be \"read\", \"effect\" or \"final\"")).nearest(&kind, ["read", "effect", "final"]));
            };
            let input = match a.get("input").filter(|i| !i.is_null()) {
                Some(i) => Type::from_json(i).map_err(|e| bad(format!("{at}.input"), e))?,
                None => Type::of("record"),
            };
            if input.kind != "record" {
                return Err(bad(format!("{at}.input"), "input must be a table of named fields: input = { repo = t.text() }".into()));
            }
            let output = match a.get("output").filter(|o| !o.is_null()) {
                Some(o) => Some(Type::from_json(o).map_err(|e| bad(format!("{at}.output"), e))?),
                None => None,
            };
            let faces = texts(a, "faces");
            if let Some(face) = faces.iter().find(|f| !FACES.contains(&f.as_str())) {
                return Err(bad(format!("{at}.faces"), format!("no face {face:?}")).nearest(face, FACES.iter().copied()));
            }
            let ability = Ability {
                title: Some(text(a, "title")).filter(|t| !t.is_empty()).unwrap_or_else(|| name.clone()),
                description: text(a, "description"), icon: text(a, "icon"), tint: text(a, "tint"), kind, input, output,
                examples: texts(a, "examples"), ai: flag(a, "ai"), slots: flag(a, "slots"),
                confirm: a.get("confirm").and_then(Value::as_bool).unwrap_or(true), hidden: flag(a, "hidden"), keywords: texts(a, "keywords"),
                every: text(a, "every"), at: text(a, "at"), on: texts(a, "on"), refresh: text(a, "refresh"), hotkey: text(a, "hotkey").to_lowercase(),
                has_run: flag(a, "has_run"), has_query: flag(a, "has_query"), has_undo: flag(a, "has_undo"), faces,
            };
            if !ability.hotkey.is_empty() && crate::features::bindings::hypr_chord(&ability.hotkey).is_none() {
                return Err(bad(format!("{at}.hotkey"), format!("hotkey = {:?} isn't a chord: modifiers and one key, as \"super+alt+p\"", ability.hotkey)));
            }
            if !ability.has_run && !ability.has_query && !ability.faces.iter().any(|f| f == "panel") {
                return Err(bad(at, "an ability needs a run function, a query function or a panel face".into()));
            }
            if !ability.tint.is_empty() && !super::manifest::TINTS.contains(&ability.tint.as_str()) {
                return Err(bad(format!("{at}.tint"), format!("tint {:?} isn't a palette tint", ability.tint)).nearest(&ability.tint, super::manifest::TINTS.iter().copied()));
            }
            if !ability.refresh.is_empty() && super::triggers::parse_every(&ability.refresh).is_none() {
                return Err(bad(format!("{at}.refresh"), format!("refresh = {:?} isn't a duration", ability.refresh)));
            }
            check_when(&at, &ability.every, &ability.at, &ability.on)?;
            index.abilities.insert(name.clone(), ability);
        }
        for (name, verb) in entries(v, "verbs") {
            let at = format!("verbs.{name}");
            let on = text(verb, "on");
            if on.is_empty() {
                return Err(bad(at, "a verb needs `on`: the type it acts on, such as on = \"url\"".into()));
            }
            let kind = Kind::parse(&text(verb, "kind")).unwrap_or(Kind::Effect);
            index.verbs.insert(name.clone(), Verb { title: Some(text(verb, "title")).filter(|t| !t.is_empty()).unwrap_or_else(|| name.clone()), on, icon: text(verb, "icon"), kind, has_undo: flag(verb, "has_undo") });
        }
        for (name, store) in entries(v, "stores") {
            let at = format!("stores.{name}");
            let ty = match store.get("type").filter(|t| !t.is_null()) {
                Some(t) => Some(Type::from_json(t).map_err(|e| bad(format!("{at}.type"), e))?),
                None => None,
            };
            let keep = text(store, "keep");
            if !keep.is_empty() && super::triggers::parse_every(&keep).is_none() {
                return Err(bad(format!("{at}.keep"), format!("keep = {keep:?} isn't a duration; write \"30d\" or \"12h\"")));
            }
            index.stores.insert(name.clone(), StoreDecl { ty, keep, persist: flag(store, "persist"), initial: store.get("initial").cloned().unwrap_or(Value::Null) });
        }
        for (name, s) in entries(v, "surfaces") {
            let at = format!("surfaces.{name}");
            let surface = Surface {
                surface: text(s, "surface"), title: Some(text(s, "title")).filter(|t| !t.is_empty()).unwrap_or_else(|| name.clone()), icon: text(s, "icon"),
                place: text(s, "place"), input: text(s, "input"), size: text(s, "size"), look: text(s, "look"), refresh: text(s, "refresh"),
            };
            if !flag(s, "has_render") {
                return Err(bad(at, "a surface needs render = function(view, cx) … end".into()));
            }
            if !surface.place.is_empty() && !PLACES.contains(&surface.place.as_str()) {
                return Err(bad(format!("{at}.place"), format!("no place {:?}", surface.place)).nearest(&surface.place, PLACES.iter().copied()));
            }
            if !surface.look.is_empty() && !super::looks::NAMES.contains(&surface.look.as_str()) {
                return Err(bad(format!("{at}.look"), format!("no Look {:?}", surface.look)).nearest(&surface.look, super::looks::NAMES.iter().copied()));
            }
            index.surfaces.insert(name.clone(), surface);
        }
        for (name, b) in entries(v, "boards") {
            let at = format!("boards.{name}");
            let mut board = Board { title: Some(text(b, "title")).filter(|t| !t.is_empty()).unwrap_or_else(|| name.clone()), look: text(b, "look"), icon: text(b, "icon"), tiles: Vec::new() };
            if !board.look.is_empty() && !super::looks::NAMES.contains(&board.look.as_str()) {
                return Err(bad(format!("{at}.look"), format!("no Look {:?}", board.look)).nearest(&board.look, super::looks::NAMES.iter().copied()));
            }
            for (i, tile) in b.get("tiles").and_then(Value::as_array).into_iter().flatten().enumerate() {
                let address = text(tile, "address");
                if !address.contains('.') {
                    return Err(bad(format!("{at}.tiles[{}]", i + 1), "a tile starts with an address: { \"calendar.next\", size = \"2x1\" }".into()));
                }
                let size = Some(text(tile, "size")).filter(|s| !s.is_empty()).unwrap_or_else(|| "1x1".into());
                let fits = size.split_once('x').is_some_and(|(w, h)| matches!(w, "1" | "2" | "3" | "4") && matches!(h, "1" | "2" | "3" | "4"));
                if !fits {
                    return Err(bad(format!("{at}.tiles[{}].size", i + 1), format!("size = {size:?} must be columns x rows on the four-column grid, such as \"2x1\"")));
                }
                let mut inputs = BTreeMap::new();
                for (field, from) in tile.get("inputs").and_then(Value::as_object).into_iter().flatten() {
                    match from.as_u64().map(|n| n as usize) {
                        Some(n) if n >= 1 && n <= i => { inputs.insert(field.clone(), n); }
                        _ => return Err(bad(format!("{at}.tiles[{}].inputs.{field}", i + 1), format!("inputs.{field} = {from} must name an earlier tile by its number, 1 to {i}: a tile reads only what is above it"))),
                    }
                }
                board.tiles.push(Tile { address, args: tile.get("args").cloned().unwrap_or(Value::Null), inputs, size, refresh: text(tile, "refresh"), title: text(tile, "title"), face: text(tile, "face") });
            }
            index.boards.insert(name.clone(), board);
        }
        for (name, t) in entries(v, "triggers") {
            let at = format!("triggers.{name}");
            let trigger = Trigger { every: text(t, "every"), at: text(t, "at"), on: texts(t, "on") };
            if !flag(t, "has_run") {
                return Err(bad(at, "a trigger needs run = function(event, cx) … end".into()));
            }
            if trigger.every.is_empty() && trigger.at.is_empty() && trigger.on.is_empty() {
                return Err(bad(at, "a trigger needs `every`, `at` or `on`".into()));
            }
            check_when(&at, &trigger.every, &trigger.at, &trigger.on)?;
            index.triggers.insert(name.clone(), trigger);
        }
        Ok(index)
    }

    /// Whether anything here runs without being asked: what `background = true` allows.
    pub fn runs_in_background(&self) -> bool {
        !self.triggers.is_empty() || self.abilities.values().any(|a| !a.every.is_empty() || !a.at.is_empty() || !a.on.is_empty())
    }
}
