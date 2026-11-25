//! Packs in the palette. The runtime knows nothing about windows; this is where its abilities
//! become rows, its blocks become screens, and what it asks of a person (a grant, a yes before a
//! final call, a path) is asked in EchoSpot's own window.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use rune::{AppHandle, Tx};
use serde_json::{Map, Value, json};

use crate::app::{App, Built, Page, Tone};
use crate::features::forms::{self, Field, FieldKind, Form, Target};
use crate::features::fuzzy;
use crate::icons;
use crate::model::*;
use crate::theme::Tint;
use crate::util;

use super::error::PackError;
use super::index::Kind;
use super::looks::{self, Look};
use super::runtime::{Event, Origin, Responder, Runtime, State};
use super::types::{self, Type};
use super::{audit, blocks, examples, faces, inbox, links, scenes, surface, trust, verbs};

/// The pack an AI answer's surface is drawn under: no folder, no code, only tiles.
pub const ANSWER: &str = "@answer";

static MAIN: OnceLock<AppHandle<App>> = OnceLock::new();

pub fn attach(handle: AppHandle<App>) {
    let _ = MAIN.set(handle);
}

/// Runs a pack action from anywhere a block is drawn: the palette, an overlay, a window.
pub fn dispatch(op: PackOp) {
    if let Some(main) = MAIN.get() {
        main.update(move |s, tx| s.perform(Act::Pack(op), tx));
    }
}

/// A pack's window was closed from inside it.
pub fn dispatch_closed() {
    if let Some(main) = MAIN.get() {
        main.update(|s, _| super::slots::window_closed(s));
    }
}

/// What a pack screen last drew, kept for the view and for point and say.
pub struct Drawn {
    pub pack: String,
    pub target: String,
    pub args: Value,
    pub blocks: Value,
    pub look: &'static Look,
    /// The host draws the blocks itself, rather than as the palette's rows.
    pub custom: bool,
    pub error: Option<PackError>,
}

/// What a call a person started is waited on for.
struct Waiting {
    title: String,
    /// Say how it went; a handler says so itself.
    announce: bool,
    pack: String,
}

/// A form the palette shows for a pack: an ability's input, a `ui.Form`, a path, a report.
#[derive(Clone, Debug)]
pub enum PackForm {
    Ability { address: String, input: Type, args: Value },
    Handler { pack: String, target: String, handler: u64, schema: Type },
    Pick { token: u64 },
    Report { depth: usize },
}

pub struct Host {
    /// What has been typed into pack inputs on the palette's screens, by key.
    pub inputs: HashMap<String, rune::TextEdit>,
    /// Living icons by address: what each ability's `icon` face last said (§14.4).
    pub living: HashMap<String, crate::model::Living>,
    pub rt: Runtime,
    /// By the screen's place in the stack.
    pub drawn: RefCell<HashMap<usize, Drawn>>,
    /// Root-search answers for what is typed now.
    pub answers: Vec<Item>,
    /// The value Send to… is offering around.
    pub sending: Option<(Value, Type)>,
    waiting: HashMap<u64, Waiting>,
    /// An effect that can still be taken back with a key, and until when.
    pub undo: Option<(u64, f64)>,
    icons: RefCell<HashMap<String, IconSrc>>,
    /// The rows of the top screen whose effects run.
    pub fx_rows: RefCell<Vec<usize>>,
    /// The newest error each pack showed, for "Copy Error for Agent".
    pub errors: HashMap<String, PackError>,
    /// What the packs folder looked like when last read.
    stamp: u64,
    /// Surfaces showing outside the palette, as `<pack>.<name>`.
    pub shown: Vec<String>,
    /// When each last drew, to honour its `refresh`.
    drew: HashMap<String, f64>,
    pub headless: bool,
    /// The size a scene asks to be drawn at: a tile, not the whole palette.
    pub scene_size: Option<(f64, f64)>,
    last_tick: f64,
    /// What the event sources last saw.
    seen: Seen,
}

#[derive(Default)]
struct Seen {
    battery_low: bool,
    network: String,
    files: HashMap<String, u64>,
    soon: Vec<String>,
}

fn shown_path() -> PathBuf {
    super::state_dir().join("surfaces.json")
}

impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}

impl Host {
    pub fn new() -> Self {
        let mut rt = Runtime::new();
        rt.confirm_finals = true;
        Self {
            rt, inputs: HashMap::new(), living: HashMap::new(), drawn: RefCell::default(), answers: Vec::new(), sending: None, waiting: HashMap::new(), undo: None,
            icons: RefCell::default(), fx_rows: RefCell::default(), errors: HashMap::new(), stamp: 0, shown: util::read_json(&shown_path()), drew: HashMap::new(),
            headless: false, scene_size: None, last_tick: 0.0, seen: Seen::default(),
        }
    }

    /// Reads the packs folder again when anything in it changed.
    pub fn reload(&mut self, force: bool) -> bool {
        let stamp = folder_stamp(&super::packs_dir());
        if !force && stamp == self.stamp {
            return false;
        }
        self.stamp = stamp;
        self.rt.load_all();
        self.icons.borrow_mut().clear();
        self.refresh_living();
        true
    }

    fn pack_dir(&self, pack: &str) -> PathBuf {
        self.rt.packs.get(pack).map(|p| p.dir.clone()).unwrap_or_default()
    }

    /// An icon a pack names: a file of its own on its tint, or one of EchoSpot's glyphs.
    pub fn icon(&self, pack: &str, name: &str, tint: &str) -> IconSrc {
        let p = self.rt.packs.get(pack);
        let tint_name = if tint.is_empty() { p.map(|p| p.manifest.tint.as_str()).unwrap_or("") } else { tint };
        let tint = looks::tint(tint_name).unwrap_or(Tint::Graphite);
        let key = format!("{pack}\n{name}\n{tint_name}");
        if let Some(found) = self.icons.borrow().get(&key) {
            return found.clone();
        }
        let made = match name {
            "" => IconSrc::Badge(&icons::BOX, tint),
            file if file.ends_with(".svg") => p.and_then(|p| std::fs::read_to_string(p.dir.join(file)).ok()).and_then(|svg| tile_svg(pack, &svg, tint))
                .map(IconSrc::App).unwrap_or(IconSrc::Badge(&icons::BOX, tint)),
            file if file.contains('/') || file.contains('.') => IconSrc::File(p.map(|p| p.dir.join(file)).unwrap_or_else(|| PathBuf::from(file))),
            glyph => IconSrc::Badge(icons::by_name(&glyph.replace('-', "_")).unwrap_or(&icons::BOX), tint),
        };
        self.icons.borrow_mut().insert(key, made.clone());
        made
    }
}

/// The newest change under a folder, two levels down: enough to see a pack's file saved.
fn folder_stamp(dir: &Path) -> u64 {
    fn walk(dir: &Path, depth: usize, acc: &mut u64) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') || name == "inbox" || name == "target" {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos() as u64);
            *acc = acc.wrapping_mul(1_099_511_628_211).wrapping_add(modified ^ meta.len());
            if meta.is_dir() && depth < 2 {
                walk(&entry.path(), depth + 1, acc);
            }
        }
    }
    let mut acc = 14_695_981_039_346_656_037;
    walk(dir, 0, &mut acc);
    acc
}

/// A pack's icon on its tile, as one SVG the image cache draws whole. `None` when the file isn't
/// an SVG at all.
fn tile_svg(pack: &str, svg: &str, tint: Tint) -> Option<PathBuf> {
    let open = svg.find("<svg")?;
    let start = open + svg[open..].find('>')? + 1;
    let end = svg.rfind("</svg>")?;
    let inner = svg.get(start..end)?.replace("currentColor", "#ffffff");
    let (top, bottom) = tint.hex();
    let composed = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 36 36\"><defs><linearGradient id=\"g\" x1=\"0\" y1=\"0\" x2=\"0\" y2=\"1\"><stop offset=\"0\" stop-color=\"#{top:06x}\"/><stop offset=\"1\" stop-color=\"#{bottom:06x}\"/></linearGradient></defs>\
         <rect x=\"1.5\" y=\"1.5\" width=\"33\" height=\"33\" rx=\"9.2\" fill=\"url(#g)\"/><rect x=\"2\" y=\"2\" width=\"32\" height=\"32\" rx=\"8.8\" fill=\"none\" stroke=\"#ffffff\" stroke-opacity=\"0.22\"/>\
         <g transform=\"translate(7.8 7.8) scale(0.85)\" fill=\"none\" stroke=\"#ffffff\" stroke-width=\"1.9\" stroke-linecap=\"round\" stroke-linejoin=\"round\">{inner}</g></svg>");
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for b in composed.bytes() {
        hash = (hash ^ b as u64).wrapping_mul(1_099_511_628_211);
    }
    let dir = super::state_dir().join("icons");
    std::fs::create_dir_all(&dir).ok()?;
    let path = dir.join(format!("{}-{hash:016x}.svg", pack.replace(|c: char| !c.is_ascii_alphanumeric(), "_")));
    if !path.exists() {
        std::fs::write(&path, composed).ok()?;
    }
    Some(path)
}

fn op(title: impl Into<String>, icon: &'static rune::Icon, op: PackOp) -> Action {
    Action::new(title, icon, Act::Pack(op))
}

fn glyph_named(name: &str) -> &'static rune::Icon {
    icons::by_name(&name.replace('-', "_")).unwrap_or(&icons::BOLT)
}

fn tone(intent: &str) -> Tone {
    match intent {
        "success" => Tone::Success,
        "urgent" | "danger" | "error" => Tone::Danger,
        "loading" => Tone::Progress,
        _ => Tone::Neutral,
    }
}

/// A value as text a person would paste.
fn plain_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Object(_) | Value::Array(_) => {
            let ty = types::infer(value);
            if matches!(ty.kind.as_str(), "record" | "list" | "table" | "any") { serde_json::to_string_pretty(value).unwrap_or_default() } else { types::display(value, &ty) }
        }
        other => blocks::show(other, None),
    }
}

/// The path or link a value names, when it names one.
fn locator(value: &Value) -> Option<String> {
    match value {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Object(m) => ["url", "path", "file", "link", "href"].iter().find_map(|k| m.get(*k).and_then(Value::as_str)).map(String::from),
        _ => None,
    }
}

// ---- Rows in root search -------------------------------------------------------------------

/// What each pack puts in the launcher: its abilities, boards and surfaces when it is ready, and
/// one row saying where it stands when it isn't.
pub fn rows(app: &App) -> Vec<(Item, String)> {
    let host = &app.packs;
    let mut out = Vec::new();
    for pack in host.rt.packs.values() {
        let name = pack.name().to_string();
        let folder = Action::new("Open Pack Folder", &icons::FOLDER, Act::OpenPath(pack.dir.clone()));
        let manage = op("Manage Packs", &icons::PUZZLE, PackOp::Manage);
        let tint = looks::tint(&pack.manifest.tint).unwrap_or(Tint::Graphite);
        let status = |title: String, subtitle: &str, glyph: &'static rune::Icon, first: Action| {
            let mut item = Item::new(format!("pack:{}", pack.id), title, IconSrc::Badge(glyph, tint)).subtitle(subtitle).trailing("Pack").action(first);
            item.actions.extend([folder.clone(), manage.clone()]);
            item
        };
        let usable = !pack.index.abilities.is_empty() || !pack.index.boards.is_empty() || !pack.index.surfaces.is_empty();
        match pack.state {
            State::Drafting | State::Checking => {
                let mut item = status(name.clone(), &pack.status(), &icons::REFRESH, folder.clone());
                item.intent = "loading".into();
                out.push((item, format!("{} pack", pack.id)));
                continue;
            }
            State::NeedsGrant => {
                let held = !host.rt.grants.held(&pack.id).is_empty();
                let label = if held { "Review new permissions" } else { "Review permissions" };
                let mut item = status(name.clone(), label, &icons::EYE, op("Review Permissions", &icons::EYE, PackOp::Review(pack.id.clone())));
                item.intent = "new".into();
                out.push((item, format!("{} pack permissions grant", pack.id)));
                // An update that asks for more keeps running with what it already had.
                if !held {
                    continue;
                }
            }
            State::Broken => {
                let message = pack.error.as_ref().map(|e| e.message.clone()).unwrap_or_default();
                let mut item = status(format!("{name} is broken"), &util::truncate(&message, 90), &icons::X, op("Copy Error for Agent", &icons::COPY, PackOp::CopyError(pack.id.clone())));
                item.id = format!("pack:{}:broken", pack.id);
                item.intent = "warning".into();
                out.push((item, format!("{} pack error broken", pack.id)));
                // The last good version keeps running.
                if !usable {
                    continue;
                }
            }
            State::Ready => {}
        }
        for (ability, a) in pack.index.abilities.iter().filter(|(_, a)| !a.hidden && (a.has_run || a.faces.iter().any(|f| f == "panel"))) {
            let address = format!("{}.{ability}", pack.id);
            let verb = match a.kind {
                _ if a.faces.iter().any(|f| f == "panel") => "Open",
                Kind::Read => "Show",
                _ => "Run",
            };
            let mut item = Item::new(format!("pack:{address}"), a.title.clone(), host.icon(&pack.id, &a.icon, &a.tint)).trailing(&name)
                .living(host.living.get(&address).cloned())
                .action(op(format!("{verb} {}", a.title), &icons::ARROW_UP_RIGHT, PackOp::Invoke { address: address.clone(), args: json!({}) }))
                .action(Action::new("Copy Link", &icons::LINK, Act::Copy(links::make(&address, &json!({})))).keys("ctrl+shift+c"));
            if a.kind == Kind::Final {
                let asking = a.confirm && !host.rt.grants.no_confirm.contains(&address);
                item.actions.push(op(if asking { "Stop Asking Before It Runs" } else { "Ask Before It Runs" }, &icons::EYE, PackOp::Confirming { address: address.clone(), ask: !asking }));
            }
            item.actions.extend([folder.clone().section(""), manage.clone()]);
            if !a.description.is_empty() {
                item.subtitle = Some(a.description.clone());
            }
            out.push((item, format!("{} {} {} {}", a.keywords.join(" "), a.examples.join(" "), pack.id, name)));
        }
        for (board, b) in &pack.index.boards {
            // A board shows in the palette, in its own window, or on the desktop (§11).
            let (in_window, on_desktop) = (host.shown.contains(&format!("{}.board:{board}:window", pack.id)), host.shown.contains(&format!("{}.board:{board}:widget", pack.id)));
            let mut item = Item::new(format!("pack:{}:board:{board}", pack.id), b.title.clone(), host.icon(&pack.id, if b.icon.is_empty() { "grid" } else { &b.icon }, "")).trailing("Board")
                .action(op("Open Board", &icons::ARROW_UP_RIGHT, PackOp::Open { pack: pack.id.clone(), target: format!("board:{board}"), args: json!({}) }))
                .action(op(if in_window { "Close Board Window" } else { "Open in Window" }, &icons::WINDOW, PackOp::Board { pack: pack.id.clone(), name: board.clone(), window: true }))
                .action(op(if on_desktop { "Remove from Desktop" } else { "Show on Desktop" }, &icons::DISPLAY, PackOp::Board { pack: pack.id.clone(), name: board.clone(), window: false }))
                .action(folder.clone().section(""));
            if in_window || on_desktop {
                item.accessories.push(Accessory { icon: None, text: "Showing".into(), tag: Some(Tint::Green.stops().0) });
            }
            out.push((item, format!("board dashboard {} {name}", pack.id)));
        }
        for (surface, s) in &pack.index.surfaces {
            let kind = if s.surface.is_empty() { "panel" } else { s.surface.as_str() };
            let showing = host.shown.contains(&format!("{}.{surface}", pack.id));
            let first = match kind {
                "panel" => op("Open", &icons::ARROW_UP_RIGHT, PackOp::Open { pack: pack.id.clone(), target: format!("surface:{surface}"), args: json!({}) }),
                _ => op(if showing { "Hide" } else { "Show" }, &icons::EYE, PackOp::Surface { pack: pack.id.clone(), name: surface.clone() }),
            };
            let label = crate::util::capitalize(kind);
            let mut item = Item::new(format!("pack:{}:surface:{surface}", pack.id), s.title.clone(), host.icon(&pack.id, if s.icon.is_empty() { "window" } else { &s.icon }, "")).trailing(&label)
                .action(first).action(folder.clone().section(""));
            if showing {
                item.accessories.push(Accessory { icon: None, text: "Showing".into(), tag: Some(Tint::Green.stops().0) });
            }
            out.push((item, format!("{kind} {} {name}", pack.id)));
        }
    }
    let manage = Item::new("cmd:packs", "Manage Packs", IconSrc::Badge(&icons::PUZZLE, Tint::Orange)).trailing("Command")
        .action(op("Open Command", &icons::ARROW_UP_RIGHT, PackOp::Manage));
    out.push((manage, "packs abilities permissions grants agents luau".into()));
    out
}

/// Values a person pinned, for the launcher's empty state.
pub fn pinned(app: &App) -> Vec<Item> {
    verbs::pins().into_iter().enumerate().map(|(i, pin)| {
        let shown = blocks::show(&pin.value, Some(&pin.ty));
        let mut item = Item::new(format!("pin:{i}"), if pin.title.is_empty() { shown.clone() } else { pin.title.clone() }, IconSrc::Badge(&icons::STAR, Tint::Yellow)).trailing("Pinned");
        if !pin.title.is_empty() && pin.title != shown {
            item.subtitle = Some(util::truncate(&shown, 80));
        }
        item.actions = verb_actions(&app.packs.rt, &pin.title, &pin.value, &pin.ty);
        item
    }).collect()
}

/// A `Row` block (a `row` face) as a launcher row.
fn answer_item(host: &Host, address: &str, args: &Value, block: &Value, value: &Value) -> Item {
    let (pack, name) = address.split_once('.').unwrap_or((address, ""));
    let ability = host.rt.packs.get(pack).and_then(|p| p.ability(name));
    let icon = host.icon(pack, ability.map_or("", |a| a.icon.as_str()), ability.map_or("", |a| a.tint.as_str()));
    let title = blocks::show(block.get("title").unwrap_or(&Value::Null), None);
    let mut item = Item::new(format!("answer:{address}"), title, icon).trailing(host.rt.packs.get(pack).map_or(pack, |p| p.name()));
    let subtitle = blocks::text(block, "subtitle");
    if !subtitle.is_empty() {
        item.subtitle = Some(subtitle.to_string());
    }
    if let Some(text) = block.get("accessory").map(|a| blocks::show(a, None)).filter(|a| !a.is_empty() && a != "null") {
        item.accessories.push(Accessory { icon: None, text, tag: None });
    }
    item.intent = blocks::text(block, "intent").to_string();
    let shown = block.get("value").unwrap_or(value);
    let ty = block.get("type").and_then(|t| Type::from_json(t).ok()).filter(|t| !t.kind.is_empty() && t.kind != "any").unwrap_or_else(|| types::infer(shown));
    item.actions = verb_actions(&host.rt, &item.title, shown, &ty);
    let title = ability.map(|a| a.title.clone()).unwrap_or_else(|| address.to_string());
    item.actions.insert(1.min(item.actions.len()), op(format!("Open {title}"), &icons::ARROW_UP_RIGHT, PackOp::Invoke { address: address.to_string(), args: args.clone() }).keys("ctrl+enter"));
    item
}

impl Host {
    /// Asks each ability with an `icon` face what its icon should show now: a count, progress,
    /// a pulse. Its output loads as a resource, so this never waits; it is asked again when
    /// something a surface shows may have changed.
    pub fn refresh_living(&mut self) -> bool {
        let mut found = HashMap::new();
        let wanted: Vec<(String, String)> = self.rt.packs.values().filter(|p| p.state == State::Ready).flat_map(|p| {
            p.index.abilities.iter().filter(|(_, a)| a.has_run && a.kind == Kind::Read && a.faces.iter().any(|f| f == "icon"))
                .map(|(name, a)| (format!("{}.{name}", p.id), a.refresh.clone())).collect::<Vec<_>>()
        }).collect();
        for (address, refresh) in wanted {
            let r = self.rt.resource(&address, &json!({}), super::triggers::parse_every(&refresh));
            if r.state != "ok" {
                if let Some(before) = self.living.get(&address) {
                    found.insert(address, before.clone());
                }
                continue;
            }
            let Ok(Some(face)) = faces::of_value(&mut self.rt, &address, "icon", &json!({}), &json!({}), &r.value) else { continue };
            let living = crate::model::Living {
                badge: face.get("badge").filter(|b| !b.is_null() && *b != &json!(0) && *b != &json!("")).map(|b| b.as_str().map(String::from).unwrap_or_else(|| b.to_string())),
                progress: face.get("progress").and_then(Value::as_f64).map(|p| p.clamp(0.0, 1.0) as f32),
                pulse: face.get("pulse").and_then(Value::as_bool).unwrap_or(false),
            };
            found.insert(address, living);
        }
        let changed = found != self.living;
        self.living = found;
        changed
    }

    /// Answers to what is typed, each pack inside its share of the keystroke. A row keeps its
    /// place once shown: answers come in the packs' own order, never by how fast they arrived.
    pub fn answer(&mut self, text: &str) {
        self.answers.clear();
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let started = std::time::Instant::now();
        let budget = std::time::Duration::from_millis(16);
        let ready: Vec<String> = self.rt.packs.values().filter(|p| p.state == State::Ready).map(|p| p.id.clone()).collect();
        for pack in ready {
            if started.elapsed() > budget {
                break;
            }
            for (ability, value) in self.rt.query(&pack, text) {
                let address = format!("{pack}.{ability}");
                if let Ok(Some(block)) = faces::of_value(&mut self.rt, &address, "row", &json!({}), &json!({ "query": text }), &value) {
                    self.answers.push(answer_item(self, &address, &json!({}), &block, &value));
                }
            }
            // Natural input, read through each ability's examples: the answer loads as a resource.
            let parsed: Vec<(String, Value)> = self.rt.packs.get(&pack).into_iter().flat_map(|p| p.index.abilities.iter())
                .filter(|(_, a)| a.has_run && a.kind == Kind::Read && !a.examples.is_empty())
                .filter_map(|(name, a)| examples::parse(a, text).map(|args| (format!("{pack}.{name}"), args))).collect();
            for (address, args) in parsed {
                if self.answers.iter().any(|i| i.id == format!("answer:{address}")) {
                    continue;
                }
                let resource = self.rt.resource(&address, &args, None);
                let title = faces::title_of(&self.rt, &address);
                match resource.state {
                    "ok" => {
                        if let Ok(Some(block)) = faces::of_value(&mut self.rt, &address, "row", &args, &json!({ "query": text }), &resource.value) {
                            self.answers.push(answer_item(self, &address, &args, &block, &resource.value));
                        }
                    }
                    "error" => {}
                    _ => {
                        let mut item = answer_item(self, &address, &args, &json!({ "title": title, "subtitle": "Working…" }), &Value::Null);
                        item.actions = vec![op(format!("Open {title}"), &icons::ARROW_UP_RIGHT, PackOp::Invoke { address: address.clone(), args: args.clone() })];
                        item.intent = "loading".into();
                        self.answers.push(item);
                    }
                }
            }
        }
    }
}

// ---- Verbs and actions ---------------------------------------------------------------------

/// The verbs a value brings: its type's own, with a record's typed fields ahead of them.
pub fn verb_actions(rt: &Runtime, title: &str, value: &Value, ty: &Type) -> Vec<Action> {
    let recent = verbs::recent();
    let mut out = Vec::new();
    if ty.kind == "record" || value.is_object() && ty.fields.is_empty() {
        let inferred = types::infer(value);
        let fields = if ty.fields.is_empty() { &inferred.fields } else { &ty.fields };
        for (field, field_ty) in fields.iter().filter(|(_, t)| matches!(t.kind.as_str(), "url" | "file" | "folder" | "image" | "app")) {
            let Some(inner) = value.get(field).filter(|v| !v.is_null()) else { continue };
            let label = match field_ty.kind.as_str() {
                "url" => "Open in Browser".to_string(),
                kind => format!("Open {}", capitalise(kind)),
            };
            out.push(op(label, &icons::ARROW_UP_RIGHT, PackOp::Verb { id: "open".into(), value: inner.clone() }));
            if field_ty.kind == "url" {
                out.push(Action::new("Copy Link", &icons::LINK, Act::Copy(plain_text(inner))).keys("ctrl+shift+c"));
            }
        }
        // A typed field brings its packs' verbs along (§7.1): a PR's url offers "Shorten URL".
        let typed: Vec<(&String, &Type)> = fields.iter().filter(|(_, t)| !matches!(t.kind.as_str(), "record" | "list" | "any" | "")).collect();
        for (field, field_ty) in &typed {
            let Some(inner) = value.get(*field).filter(|v| !v.is_null()) else { continue };
            let same_kind = typed.iter().filter(|(_, t)| t.kind == field_ty.kind).count() > 1;
            for offer in verbs::offers(rt, field_ty, &recent).into_iter().filter(|o| !o.pack.is_empty()) {
                let title = if same_kind { format!("{} ({field})", offer.title) } else { offer.title.clone() };
                let mut action = op(title, glyph_named(&offer.icon), PackOp::Verb { id: offer.id.clone(), value: inner.clone() });
                action.destructive = offer.kind == Kind::Final;
                out.push(action);
            }
        }
    }
    for offer in verbs::offers(rt, ty, &recent) {
        let act = match offer.id.as_str() {
            "send" => PackOp::SendTo { value: value.clone(), ty: serde_json::to_value(ty).unwrap_or(Value::Null) },
            "pin" => PackOp::Verb { id: format!("pin\n{title}"), value: value.clone() },
            id => PackOp::Verb { id: id.to_string(), value: value.clone() },
        };
        let mut action = op(offer.title.clone(), glyph_named(&offer.icon), act);
        action.keys = match offer.id.as_str() {
            "copy" if !out.iter().any(|a: &Action| a.keys == "ctrl+shift+c") => "ctrl+shift+c".into(),
            "send" => "ctrl+shift+s".into(),
            "pin" => "ctrl+shift+p".into(),
            _ => String::new(),
        };
        action.destructive = offer.kind == Kind::Final;
        out.push(action);
    }
    out
}

/// What packs offer for a value shown outside a pack (clipboard history, a note): their verbs
/// for its type, and Send to… (§7.2). EchoSpot's own actions for it are already on the row.
pub fn pack_verbs(rt: &Runtime, value: &Value, kind: &str) -> Vec<Action> {
    let ty = Type::of(kind);
    let mut out: Vec<Action> = verbs::offers(rt, &ty, &verbs::recent()).into_iter().filter(|o| !o.pack.is_empty()).map(|offer| {
        let mut action = op(offer.title.clone(), glyph_named(&offer.icon), PackOp::Verb { id: offer.id.clone(), value: value.clone() });
        action.destructive = offer.kind == Kind::Final;
        action
    }).collect();
    out.push(op("Send to…", &icons::ARROW_RIGHT, PackOp::SendTo { value: value.clone(), ty: serde_json::to_value(&ty).unwrap_or(Value::Null) }).keys("ctrl+shift+s"));
    if let Some(first) = out.first_mut() {
        *first = first.clone().section("");
    }
    out
}

fn capitalise(s: &str) -> String {
    crate::util::capitalize(s)
}

/// One action a pack put on a row or a block.
fn pack_action(pack: &str, target: &str, a: &Value, payload: &Value) -> Option<Action> {
    let title = a.get("title").and_then(Value::as_str)?;
    let handler = a.get("run").and_then(|r| r.get("_handler")).or_else(|| a.get("_handler")).and_then(Value::as_u64);
    let text = |key: &str| a.get(key).and_then(Value::as_str);
    let act = if let Some(handler) = handler {
        Act::Pack(PackOp::Handler { pack: pack.into(), target: target.into(), handler, payload: payload.clone() })
    } else if let Some(address) = text("call") {
        Act::Pack(PackOp::Invoke { address: address.into(), args: a.get("args").cloned().unwrap_or_else(|| json!({})) })
    } else if let Some(url) = text("open") {
        Act::OpenUrl(url.into())
    } else if let Some(copy) = text("copy") {
        Act::Copy(copy.into())
    } else if let Some(push) = text("push") {
        Act::Pack(PackOp::Open { pack: pack.into(), target: push.into(), args: a.get("args").cloned().unwrap_or_else(|| json!({})) })
    } else {
        return None;
    };
    let mut action = Action::new(title, glyph_named(text("icon").unwrap_or("bolt")), act);
    action.keys = text("keys").unwrap_or("").to_string();
    action.destructive = text("kind") == Some("final") || a.get("destructive").and_then(Value::as_bool).unwrap_or(false);
    Some(action)
}

fn actions_of(pack: &str, target: &str, list: Option<&Value>, payload: &Value) -> Vec<Action> {
    list.and_then(Value::as_array).into_iter().flatten().filter_map(|a| pack_action(pack, target, a, payload)).collect()
}

/// What every pack screen offers last: a way to tell the agent what is wrong with it.
fn report_action() -> Action {
    op("Report to Agent…", &icons::PENCIL, PackOp::Report).keys("ctrl+.").section("")
}

// ---- A pack screen -------------------------------------------------------------------------

fn accessory(value: Option<&Value>) -> Option<Accessory> {
    match value? {
        Value::Null => None,
        Value::Object(m) => {
            let text = m.get("text").or_else(|| m.get("title")).map(|t| blocks::show(t, None)).unwrap_or_default();
            let tag = m.get("tint").or_else(|| m.get("tag")).and_then(Value::as_str).and_then(looks::tint).map(|t| t.stops().0);
            let icon = m.get("icon").and_then(Value::as_str).and_then(|n| icons::by_name(&n.replace('-', "_")));
            (!text.is_empty() || icon.is_some()).then_some(Accessory { icon, text, tag })
        }
        other => Some(Accessory { icon: None, text: blocks::show(other, None), tag: None }).filter(|a| !a.text.is_empty()),
    }
}

/// A block a row shows beside the list, as the palette's own preview pane.
fn detail_of(block: &Value) -> Option<Detail> {
    let fields = |list: &[Value]| -> Vec<(String, String)> {
        list.iter().filter_map(|f| match f {
            Value::Array(pair) => Some((blocks::show(pair.first()?, None), blocks::show(pair.get(1)?, None))),
            Value::Object(m) => Some((blocks::show(m.get("label")?, None), blocks::show(m.get("value")?, None))),
            _ => None,
        }).collect()
    };
    Some(match blocks::kind(block) {
        "Detail" => Detail {
            markdown: Some(blocks::text(block, "markdown").to_string()).filter(|m| !m.is_empty()),
            fields: fields(blocks::items(block, "fields")),
            image: block.get("image").and_then(Value::as_str).map(PathBuf::from),
            ..Default::default()
        },
        "KeyValue" => Detail { fields: fields(blocks::items(block, "items")), heading: Some(blocks::text(block, "heading").to_string()).filter(|h| !h.is_empty()), ..Default::default() },
        "markdown" => Detail { markdown: Some(blocks::text(block, "source").to_string()), ..Default::default() },
        "code" => Detail { text: Some(blocks::text(block, "source").to_string()), ..Default::default() },
        "text" => Detail { markdown: Some(blocks::text(block, "text").to_string()), ..Default::default() },
        "image" => Detail { image: Some(PathBuf::from(blocks::text(block, "source"))), ..Default::default() },
        "" => return None,
        _ => Detail { text: Some(blocks::tree(block)), ..Default::default() },
    })
}

/// The palette's own rows for a List, a Grid, a Gallery or a Timeline.
fn list_items(host: &Host, drawn: &Drawn, root: &Value, query: &str) -> (Sections, bool) {
    let (pack, target) = (drawn.pack.as_str(), drawn.target.as_str());
    let kind = blocks::kind(root);
    let list_ty = root.get("type").and_then(|t| Type::from_json(t).ok());
    let row_ty = list_ty.as_ref().and_then(|t| t.of.as_deref().cloned());
    // A surface that reads the query filters for itself; otherwise the palette does.
    let filtering = root.get("filter").and_then(Value::as_bool).unwrap_or(true);
    let q = query.trim();
    let mut sections = Sections::default();
    let mut current: (String, Vec<Item>) = (String::new(), Vec::new());
    let mut split = false;
    // A row with no picture of its own wears the icon of the ability it came from.
    let fallback = match target.splitn(3, ':').collect::<Vec<_>>().as_slice() {
        ["ability", name, ..] => host.rt.packs.get(pack).and_then(|p| p.ability(name)).filter(|a| !a.icon.is_empty()).map(|a| host.icon(pack, &a.icon, &a.tint)),
        _ => None,
    };
    let grid = matches!(kind, "Grid" | "Gallery");
    for row in blocks::items(root, "rows") {
        let title = blocks::show(row.get("title").unwrap_or(&Value::Null), None);
        let subtitle = row.get("subtitle").filter(|s| !s.is_null()).map(|s| blocks::show(s, None)).unwrap_or_default();
        let keywords = row.get("keywords").map(|k| match k {
            Value::Array(list) => list.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" "),
            other => blocks::show(other, None),
        }).unwrap_or_default();
        if filtering && !q.is_empty() && fuzzy::score_item(q, &title, &format!("{subtitle} {keywords}")).is_none() {
            continue;
        }
        let value = row.get("value").cloned().unwrap_or(Value::Null);
        let icon = match (row.get("image").and_then(Value::as_str), row.get("icon").and_then(Value::as_str)) {
            (Some(image), _) if !image.is_empty() => IconSrc::File(resolve_path(&host.pack_dir(pack), image)),
            (_, Some(name)) if name.chars().count() <= 2 && !name.is_ascii() => IconSrc::Emoji(name.into()),
            (_, Some(name)) if !name.is_empty() => host.icon(pack, name, blocks::text(row, "tint")),
            _ if kind == "Timeline" => IconSrc::Badge(&icons::CLOCK, Tint::Graphite),
            _ if grid => IconSrc::Glyph(&icons::BOX),
            _ => fallback.clone().unwrap_or(IconSrc::None),
        };
        let mut item = Item::new(format!("pk:{}", blocks::text(row, "id")), title.clone(), icon);
        if !subtitle.is_empty() {
            item.subtitle = Some(subtitle);
        }
        item.intent = blocks::text(row, "intent").to_string();
        if let Some(badge) = accessory(row.get("badge")) {
            item.accessories.push(Accessory { tag: badge.tag.or(Some(Tint::Blue.stops().0)), ..badge });
        }
        item.accessories.extend(accessory(row.get("accessory")));
        if let Some(time) = row.get("time").filter(|t| !t.is_null()) {
            item.trailing = match time.as_f64() {
                Some(secs) => util::ago(util::now_secs().saturating_sub(secs as u64)),
                None => blocks::show(time, None),
            };
        }
        let label = looks::get(drawn.look.name).intent(&item.intent).label;
        if !label.is_empty() && item.accessories.is_empty() && matches!(item.intent.as_str(), "urgent" | "live" | "new" | "warning") {
            item.accessories.push(Accessory { icon: None, text: label.into(), tag: drawn.look.intent(&item.intent).tint.map(|t| t.stops().0) });
        }
        item.actions = actions_of(pack, target, row.get("actions"), &value);
        let ty = row_ty.clone().filter(|t| !t.kind.is_empty() && t.kind != "any").unwrap_or_else(|| types::infer(&value));
        if !value.is_null() {
            item.actions.extend(verb_actions(&host.rt, &title, &value, &ty));
        }
        item.actions.push(report_action());
        if let Some(detail) = row.get("detail").and_then(detail_of) {
            item.detail = Some(detail);
            split = true;
        }
        let section = blocks::text(row, "section").to_string();
        if section != current.0 {
            let (name, items) = std::mem::replace(&mut current, (section, Vec::new()));
            sections.push(&name, items);
        }
        current.1.push(item);
    }
    let heading = if current.0.is_empty() { blocks::text(root, "heading").to_string() } else { current.0 };
    sections.push(&heading, current.1);
    (sections, split)
}

/// A path a pack names: its own folder's when it isn't absolute, `~` as home.
pub fn resolve_path(dir: &Path, path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        util::home().join(rest)
    } else if path.starts_with('/') || path.contains("://") {
        PathBuf::from(path.trim_start_matches("file://"))
    } else {
        dir.join(path)
    }
}

/// Draws the pack screen at `depth` and hands the palette what it shows.
pub fn build(app: &mut App, depth: usize, pack: &str, target: &str, args: &Value, query: &str, filter: usize) -> Built {
    let dark = app.settings.dark(app.system_dark.get());
    let host = &mut app.packs;
    let size = host.scene_size.unwrap_or((crate::theme::size::PANEL_W as f64, (crate::theme::size::PANEL_H - crate::theme::size::HEADER_BAND - crate::theme::size::BOTTOM_BAR_H) as f64));
    let state = json!({ "query": query, "dark": dark, "size": [size.0, size.1], "tab": filter });
    let title = match target.splitn(3, ':').collect::<Vec<_>>().as_slice() {
        ["ability", name, ..] => faces::title_of(&host.rt, &format!("{pack}.{name}")),
        ["board", name] => host.rt.packs.get(pack).and_then(|p| p.index.boards.get(*name)).map(|b| b.title.clone()).unwrap_or_default(),
        ["surface", name] => host.rt.packs.get(pack).and_then(|p| p.index.surfaces.get(*name)).map(|s| s.title.clone()).unwrap_or_default(),
        _ => args.get("title").and_then(Value::as_str).unwrap_or("").to_string(),
    };
    let rendered = if pack == ANSWER { super::answers::board(&mut host.rt, args, &state) } else { surface::render(&mut host.rt, pack, target, args, &state) };
    let (mut root, error) = match rendered {
        Ok(blocks) => (blocks, None),
        Err(e) => (surface::failed(&title, &e), Some(e)),
    };
    if let Some(e) = &error {
        host.errors.insert(pack.to_string(), e.clone());
    }
    let look = surface::look_of(&host.rt, pack, target, &root, &app.settings.pack_look);
    // Tabs become the header's filter: one view at a time, switched as every other screen is.
    let mut filters: Vec<String> = Vec::new();
    if blocks::kind(&root) == "Tabs" {
        let tabs = blocks::items(&root, "tabs");
        filters = tabs.iter().map(|t| blocks::text(t, "title").to_string()).collect();
        root = tabs.get(filter.min(tabs.len().saturating_sub(1))).and_then(|t| t.get("content")).cloned().unwrap_or(Value::Null);
    }
    let kind = blocks::kind(&root).to_string();
    let block_state = blocks::text(&root, "state").to_string();
    let rows = blocks::items(&root, "rows").len();
    let listy = matches!(kind.as_str(), "List" | "Grid" | "Gallery" | "Timeline") && block_state != "error" && !(block_state == "pending" && rows == 0);
    let placeholder = Some(blocks::text(&root, "placeholder")).filter(|p| !p.is_empty()).map(String::from).unwrap_or_else(|| if title.is_empty() { "Search…".into() } else { format!("Search {title}…") });
    let mut drawn = Drawn { pack: pack.into(), target: target.into(), args: args.clone(), blocks: root.clone(), look, custom: !listy, error };
    let mut built = if listy {
        let (sections, split) = list_items(host, &drawn, &root, query);
        let (empty_title, empty_hint) = blocks::title_hint(root.get("empty"));
        let layout = match kind.as_str() {
            "Grid" => Layout::Grid { columns: root.get("columns").and_then(Value::as_u64).unwrap_or(5).clamp(2, 8) as usize },
            "Gallery" => Layout::Grid { columns: 3 },
            _ if split => Layout::Split,
            _ => Layout::List,
        };
        let built = Built::list(sections, &placeholder).layout(layout).keep()
            .empty(if empty_title.is_empty() { "No Results" } else { &empty_title }, &empty_hint).loading(block_state == "pending");
        if built.items.is_empty() {
            // Nothing to select: one unseen row still carries the screen's own actions.
            drawn.custom = true;
            drawn.blocks = json!({ "_ui": "Empty", "title": built.empty.0, "hint": built.empty.1, "icon": root.get("icon") });
        }
        built
    } else {
        Built::list(Sections::default(), &placeholder).keep().loading(block_state == "pending")
    };
    if drawn.custom {
        let mut carrier = Item::new("pk:screen", title.clone(), IconSrc::None);
        let value = root.get("value").cloned().unwrap_or(Value::Null);
        carrier.actions = actions_of(pack, target, root.get("actions"), &value);
        if let Some(address) = root.get("run").and_then(Value::as_str) {
            carrier.actions.push(op(format!("Run {title}"), &icons::BOLT, PackOp::Run { address: address.into(), input: args.clone() }));
        }
        if drawn.error.is_some() || block_state == "error" {
            carrier.actions.push(op("Try Again", &icons::REFRESH, PackOp::Reload));
            carrier.actions.push(op("Copy Error for Agent", &icons::COPY, PackOp::CopyError(pack.into())));
        }
        if !value.is_null() {
            let ty = root.get("type").and_then(|t| Type::from_json(t).ok()).filter(|t| !t.kind.is_empty() && t.kind != "any").unwrap_or_else(|| types::infer(&value));
            carrier.actions.extend(verb_actions(&host.rt, &title, &value, &ty));
        }
        if pack == ANSWER {
            carrier.actions.insert(0, op("Keep as a Pack", &icons::STAR, PackOp::Keep));
        } else {
            carrier.actions.push(report_action());
        }
        built.items = vec![carrier];
        built.rows = vec![Row::Item(0)];
    }
    built.filters = filters;
    host.drawn.borrow_mut().insert(depth, drawn);
    built
}

// ---- Screens of the host's own --------------------------------------------------------------

fn pack_summary(host: &Host, id: &str) -> String {
    let Some(pack) = host.rt.packs.get(id) else { return String::new() };
    let diff = host.rt.grants.diff(id, &pack.manifest.permissions);
    let mut md = format!("## {}\n\n`{}` · {}{}\n\n", pack.name(), pack.id, pack.state.name(), if pack.system { " · ships with EchoSpot" } else { "" });
    if let Some(e) = &pack.error {
        md.push_str(&format!("**{}** {}\n\n", e.code, e.message));
        if !e.at.is_empty() {
            md.push_str(&format!("at `{}`\n\n", e.at));
        }
    }
    let list = |title: &str, asks: &[super::manifest::Ask]| if asks.is_empty() { String::new() } else { format!("**{title}**\n\n{}\n\n", asks.iter().map(|a| format!("- {}", a.sentence())).collect::<Vec<_>>().join("\n")) };
    if pack.system {
        md.push_str("What it asks for is implied: it is part of EchoSpot.\n\n");
    } else {
        md.push_str(&list("Not granted yet", &diff.new));
        md.push_str(&list("Granted", &diff.held));
        if diff.new.is_empty() && diff.held.is_empty() {
            md.push_str("It asks for nothing.\n\n");
        }
    }
    let facts = trust::labels(id, &pack.manifest.permissions, &pack.index, &audit::read());
    if !facts.is_empty() {
        md.push_str(&format!("**What EchoSpot has seen**\n\n{}\n\n", facts.iter().map(|f| format!("- {f}")).collect::<Vec<_>>().join("\n")));
    }
    let unasked: Vec<String> = host.rt.unasked().into_iter().filter(|e| e.pack == id).map(|e| {
        let ran = host.rt.runs.get(&e.key).map(|r| format!("last ran {}{}", util::ago(util::now_secs().saturating_sub(r.t as u64)).to_lowercase(), if r.ok { String::new() } else { format!(", failed: {}", r.error) })).unwrap_or_else(|| "hasn't run yet".into());
        format!("- `{}` {} · {ran}", e.name, e.when())
    }).collect();
    if !unasked.is_empty() {
        md.push_str(&format!("**Runs unasked**\n\n{}\n\n", unasked.join("\n")));
    }
    if let Some(script) = &pack.script {
        md.push_str(&format!("Loaded in {:.1} ms · {} of memory\n\n", pack.load_ms, util::human_size(script.memory() as u64)));
        // What its code has cost since it loaded: every entry into the VM, and the time inside.
        let m = script.metrics();
        if m.vm_calls > 0 {
            let total = m.vm_time.as_secs_f64() * 1e3;
            md.push_str(&format!("{} entr{} into its code, {total:.1} ms in all ({:.2} ms each) · {} draw{}{}\n\n",
                m.vm_calls, if m.vm_calls == 1 { "y" } else { "ies" }, total / m.vm_calls as f64, m.renders, if m.renders == 1 { "" } else { "s" },
                m.last_reload.map(|_| format!(" · reloaded {} time{}", m.reloads, if m.reloads == 1 { "" } else { "s" })).unwrap_or_default()));
        }
    }
    md.push_str(&format!("`{}`\n", util::tilde(&pack.dir)));
    md
}

fn manage_actions(host: &Host, id: &str) -> Vec<Action> {
    let Some(pack) = host.rt.packs.get(id) else { return Vec::new() };
    let mut out = Vec::new();
    if pack.state == State::NeedsGrant {
        out.push(op("Review Permissions", &icons::EYE, PackOp::Review(id.into())));
    }
    if pack.error.is_some() || host.errors.contains_key(id) {
        out.push(op("Copy Error for Agent", &icons::COPY, PackOp::CopyError(id.into())));
    }
    out.push(Action::new("Open Pack Folder", &icons::FOLDER, Act::OpenPath(pack.dir.clone())));
    out.push(op("Reload Packs", &icons::REFRESH, PackOp::Reload).keys("ctrl+r"));
    if !pack.system {
        out.push(op("Permissions…", &icons::EYE, PackOp::Review(id.into())));
        if !host.rt.grants.held(id).is_empty() {
            out.push(Action::new("Revoke All Permissions", &icons::X, Act::Confirm(Box::new(Confirm {
                title: format!("Revoke {}'s permissions?", pack.name()), message: "It stops until you review them again.".into(), icon: &icons::X, confirm: "Revoke".into(), destructive: true,
                act: Act::Pack(PackOp::Revoke(id.into())),
            }))).destructive().section(""));
        }
    }
    out
}

/// Every pack, with where it stands.
pub fn manage(app: &App, query: &str) -> Built {
    let host = &app.packs;
    let q = query.trim();
    let (mut yours, mut system) = (Vec::new(), Vec::new());
    for pack in host.rt.packs.values().filter(|p| q.is_empty() || fuzzy::score_item(q, p.name(), &p.id).is_some()) {
        let tint = looks::tint(&pack.manifest.tint).unwrap_or(Tint::Graphite);
        let icon = pack.index.abilities.values().find(|a| !a.icon.is_empty()).map(|a| host.icon(&pack.id, &a.icon, &a.tint)).unwrap_or(IconSrc::Badge(&icons::BOX, tint));
        let n = pack.index.abilities.len();
        let mut item = Item::new(format!("packs:{}", pack.id), pack.name().to_string(), icon)
            .subtitle(if pack.state == State::Ready { format!("{n} abilit{}", if n == 1 { "y" } else { "ies" }) } else { pack.status() });
        let spent = audit::spend_this_month(&pack.id);
        if spent > 0.0 {
            item.accessories.push(Accessory { icon: Some(&icons::AI), text: format!("${spent:.2}"), tag: None });
        }
        item.intent = match pack.state {
            State::Broken => "warning",
            State::NeedsGrant => "new",
            State::Drafting | State::Checking => "loading",
            State::Ready => "",
        }.into();
        item.detail = Some(Detail { markdown: Some(pack_summary(host, &pack.id)), ..Default::default() });
        item.actions = manage_actions(host, &pack.id);
        if pack.system { system.push(item) } else { yours.push(item) }
    }
    let mut sections = Sections::default();
    sections.push("Your Packs", yours);
    sections.push("Part of EchoSpot", system);
    Built::list(sections, "Search packs…").layout(Layout::Split).keep()
        .empty("No Packs Yet", "Ask your coding agent: “make me a command that…”")
}

/// What a pack asks for, in sentences, with one way to say yes.
pub fn grants(app: &App, id: &str) -> Built {
    let host = &app.packs;
    let Some(pack) = host.rt.packs.get(id) else { return Built::list(Sections::default(), "").empty("No Such Pack", "") };
    let diff = host.rt.grants.diff(id, &pack.manifest.permissions);
    let glyph = |kind: &str| match kind {
        "net" => &icons::LINK,
        "secret" => &icons::EYE,
        "clipboard" => &icons::CLIPBOARD,
        "files" | "picked" => &icons::FOLDER,
        "run" => &icons::BOLT,
        "ai" => &icons::AI,
        "calls" => &icons::PUZZLE,
        _ => &icons::CLOCK,
    };
    let grant = op(format!("Grant to {}", pack.name()), &icons::CHECK, PackOp::Grant(id.into()));
    let revoke = Action::new("Revoke All", &icons::X, Act::Pack(PackOp::Revoke(id.into()))).destructive();
    let row = |ask: &super::manifest::Ask, i: usize, granted: bool| {
        let mut item = Item::new(format!("grant:{}:{i}", ask.kind), ask.sentence(), IconSrc::Badge(glyph(&ask.kind), if granted { Tint::Green } else { Tint::Orange }));
        item.actions = if granted { vec![revoke.clone()] } else { vec![grant.clone(), Action::new("Not Now", &icons::X, Act::Pop)] };
        item
    };
    let mut sections = Sections::default();
    sections.push(&format!("{} asks to", pack.name()), diff.new.iter().enumerate().map(|(i, a)| row(a, i, false)).collect());
    sections.push("Already granted", diff.held.iter().enumerate().map(|(i, a)| row(a, i + 100, true)).collect());
    let facts: Vec<Item> = trust::labels(id, &pack.manifest.permissions, &pack.index, &audit::read()).into_iter().enumerate()
        .map(|(i, fact)| Item::new(format!("fact:{i}"), fact, IconSrc::Badge(&icons::CHECK, Tint::Graphite)).action(Action::new("Back", &icons::CHEVRON_LEFT, Act::Pop))).collect();
    sections.push("What EchoSpot has seen", facts);
    Built::list(sections, &format!("{} permissions", pack.name())).keep()
        .empty("It Asks for Nothing", "This pack runs without any permission")
}

/// Send to…: every ability that takes the value in hand.
pub fn send(app: &App, query: &str) -> Built {
    let host = &app.packs;
    let Some((value, ty)) = &host.sending else { return Built::list(Sections::default(), "Send to…") };
    let q = query.trim();
    let items: Vec<Item> = verbs::send_targets(&host.rt, ty).into_iter().filter(|t| q.is_empty() || fuzzy::score_item(q, &t.title, &t.address).is_some()).map(|t| {
        let (pack, name) = t.address.split_once('.').unwrap_or((t.address.as_str(), ""));
        let ability = host.rt.packs.get(pack).and_then(|p| p.ability(name));
        let icon = match ability {
            Some(a) => host.icon(pack, &a.icon, &a.tint),
            None => IconSrc::Badge(&icons::ARROW_RIGHT, Tint::Graphite),
        };
        let mut item = Item::new(format!("send:{}", t.address), t.title.clone(), icon).subtitle(t.address.clone())
            .trailing(match t.kind { Kind::Read => "", Kind::Effect => "Changes something", Kind::Final => "Can't be undone" })
            .action(op(format!("Send to {}", t.title), &icons::ARROW_RIGHT, PackOp::Send { address: t.address.clone(), slot: t.slot.clone(), value: value.clone(), ready: t.ready }));
        if !t.ready {
            item.accessories.push(Accessory { icon: None, text: "Asks for more".into(), tag: None });
        }
        item
    }).collect();
    let mut sections = Sections::default();
    sections.push(&format!("Send {} to", util::truncate(&blocks::show(value, Some(ty)), 40)), items);
    Built::list(sections, "Send to…").empty("Nothing Takes This", &format!("No ability has an input for a {}", ty.label()))
}

// ---- Doing things --------------------------------------------------------------------------

struct Signature {
    title: String,
    kind: Kind,
    input: Type,
    panel: bool,
}

fn signature(rt: &Runtime, address: &str) -> Option<Signature> {
    let (pack, name) = address.split_once('.')?;
    match rt.packs.get(pack).and_then(|p| p.ability(name)) {
        Some(a) => Some(Signature { title: a.title.clone(), kind: a.kind, input: a.input.clone(), panel: a.faces.iter().any(|f| f == "panel") }),
        None => rt.specs().into_iter().find(|s| s.address == address).map(|s| Signature { title: s.title, kind: s.kind, input: s.input, panel: false }),
    }
}

fn form_fields(schema: &Type, args: &Value) -> Vec<Field> {
    schema.fields.iter().map(|(name, ty)| {
        let label = if ty.label.is_empty() { capitalise(&name.replace('_', " ")) } else { ty.label.clone() };
        let held = args.get(name).or(ty.default.as_ref()).filter(|v| !v.is_null());
        let text = held.map(plain_text).unwrap_or_default();
        let mut field = match ty.kind.as_str() {
            "bool" => {
                let mut f = Field::new(name, "", FieldKind::Checkbox(label.clone()));
                f.checked = held.and_then(Value::as_bool).unwrap_or(false);
                f
            }
            "enum" => Field::new(name, &label, FieldKind::Dropdown(ty.values.iter().map(|v| (v.clone(), capitalise(v))).collect())).value(&text),
            "markdown" | "code" => Field::new(name, &label, FieldKind::Area).value(&text),
            kind => Field::new(name, &label, FieldKind::Text).value(&text).placeholder(match kind {
                "text" | "any" | "" => "",
                "money" => "50 usd",
                "quantity" => "5 km",
                "date" => "2026-01-31",
                "time" => "09:30",
                "duration" => "15m",
                other => other,
            }),
        };
        field.required = !ty.optional && ty.default.is_none() && ty.kind != "bool";
        field
    }).collect()
}

/// What a form holds, as the schema's types. Marks the field that doesn't fit.
fn form_values(form: &mut Form, schema: &Type, base: &Value) -> Option<Value> {
    let mut out: Map<String, Value> = base.as_object().cloned().unwrap_or_default();
    let mut ok = true;
    for field in &mut form.fields {
        let Some(ty) = schema.fields.get(&field.id) else { continue };
        let text = field.text();
        let value = match ty.kind.as_str() {
            "bool" => Value::Bool(field.checked),
            _ if text.trim().is_empty() => {
                out.remove(&field.id);
                continue;
            }
            "number" | "percent" => match text.trim().trim_end_matches('%').replace(',', "").parse::<f64>() {
                Ok(n) => json!(n),
                Err(_) => {
                    field.error = "A number".into();
                    ok = false;
                    continue;
                }
            },
            "list" | "record" | "table" | "series" => serde_json::from_str(&text).unwrap_or(Value::String(text)),
            _ => Value::String(text),
        };
        match ty.coerce(value) {
            Ok(v) => {
                out.insert(field.id.clone(), v);
            }
            Err(why) => {
                field.error = why;
                ok = false;
            }
        }
    }
    ok.then_some(Value::Object(out))
}

/// A person picked an ability.
fn invoke(app: &mut App, address: &str, args: Value, tx: &mut Tx) {
    let (pack, name) = address.split_once('.').unwrap_or((address, ""));
    match app.packs.rt.packs.get(pack).map(|p| p.state) {
        Some(State::NeedsGrant) if app.packs.rt.grants.held(pack).is_empty() => return perform(app, PackOp::Review(pack.to_string()), tx),
        Some(State::Drafting | State::Checking) => return app.toast("Still being built", Tone::Progress),
        _ => {}
    }
    let Some(sig) = signature(&app.packs.rt, address) else { return app.toast(format!("No ability at {address}"), Tone::Danger) };
    let missing = sig.input.fields.iter().any(|(k, t)| !t.optional && t.default.is_none() && args.get(k).is_none_or(Value::is_null));
    if missing {
        let fields = form_fields(&sig.input, &args);
        let submit = match sig.kind { Kind::Read => "Show", _ => "Run" };
        let form = Form::new(&sig.title, submit, fields, Target::Pack(Box::new(PackForm::Ability { address: address.into(), input: sig.input, args })));
        return forms::open(app, form, tx);
    }
    let args = sig.input.coerce(args.clone()).unwrap_or(args);
    if sig.panel || sig.kind == Kind::Read {
        open(app, pack, &format!("ability:{name}:panel"), args, tx);
    } else {
        run(app, address, args, true);
    }
}

/// Shows a target of a pack. A surface that is a form becomes the palette's own form.
fn open(app: &mut App, pack: &str, target: &str, args: Value, tx: &mut Tx) {
    if target.starts_with("surface:") && pack != ANSWER {
        let state = json!({ "query": "", "dark": app.settings.dark(app.system_dark.get()) });
        if let Ok(block) = surface::render(&mut app.packs.rt, pack, target, &args, &state)
            && blocks::kind(&block) == "Form" {
            return open_form(app, pack, target, &block, tx);
        }
    }
    app.usage.record(&format!("pack:{pack}:{target}"));
    app.push(Route::Pack { pack: pack.into(), target: target.into(), args }, tx);
}

fn open_form(app: &mut App, pack: &str, target: &str, block: &Value, tx: &mut Tx) {
    let title = Some(blocks::text(block, "title")).filter(|t| !t.is_empty()).unwrap_or("Form").to_string();
    let submit = block.get("submit_title").and_then(Value::as_str).unwrap_or("Submit").to_string();
    let values = block.get("values").cloned().unwrap_or_else(|| json!({}));
    if let Some(address) = block.get("ability").and_then(Value::as_str) {
        let Some(sig) = signature(&app.packs.rt, address) else { return app.toast(format!("No ability at {address}"), Tone::Danger) };
        let form = Form::new(&title, &submit, form_fields(&sig.input, &values), Target::Pack(Box::new(PackForm::Ability { address: address.into(), input: sig.input, args: values })));
        return forms::open(app, form, tx);
    }
    let schema = Type::from_json(&json!({ "type": "record", "fields": block.get("fields").cloned().unwrap_or_else(|| json!({})) })).unwrap_or_default();
    let Some(handler) = block.get("submit").and_then(|s| s.get("_handler")).and_then(Value::as_u64) else { return app.toast("The form has no submit function", Tone::Danger) };
    let form = Form::new(&title, &submit, form_fields(&schema, &values), Target::Pack(Box::new(PackForm::Handler { pack: pack.into(), target: target.into(), handler, schema })));
    forms::open(app, form, tx);
}

/// The palette's form was submitted for a pack.
pub fn submit_form(app: &mut App, form: PackForm, tx: &mut Tx) {
    match form {
        PackForm::Ability { address, input, args } => {
            let Some(values) = app.form.as_mut().and_then(|f| form_values(f, &input, &args)) else { return };
            app.pop(tx);
            invoke(app, &address, values, tx);
        }
        PackForm::Handler { pack, target, handler, schema } => {
            let Some(values) = app.form.as_mut().and_then(|f| form_values(f, &schema, &json!({}))) else { return };
            app.pop(tx);
            let token = app.packs.rt.handle(&pack, &target, handler, values, false);
            app.packs.waiting.insert(token, Waiting { title: String::new(), announce: false, pack });
        }
        PackForm::Pick { token } => {
            let typed = app.form.as_ref().map(|f| f.get("path")).unwrap_or_default();
            let path = resolve_path(&util::home(), typed.trim());
            if !path.exists() {
                if let Some(field) = app.form.as_mut().and_then(|f| f.fields.first_mut()) {
                    field.error = "Nothing is there".into();
                }
                return;
            }
            app.pop(tx);
            app.packs.rt.answer_pick(token, Some(path));
        }
        PackForm::Report { depth } => {
            let note = app.form.as_ref().map(|f| f.get("note")).unwrap_or_default();
            app.pop(tx);
            report(app, depth, note.trim());
        }
    }
}

/// A form for a pack was dismissed: a pick with no answer still has to answer.
pub fn cancel_form(app: &mut App, form: &PackForm) {
    if let PackForm::Pick { token } = form {
        app.packs.rt.answer_pick(*token, None);
    }
}

/// Starts an ability for a person and waits for it without blocking a frame.
fn run(app: &mut App, address: &str, input: Value, announce: bool) {
    let title = faces::title_of(&app.packs.rt, address);
    let token = app.packs.rt.start(Origin::User, address, input, Responder::Keep, false);
    let pack = address.split('.').next().unwrap_or("").to_string();
    app.packs.waiting.insert(token, Waiting { title: title.clone(), announce, pack });
    if announce {
        app.toast_progress(format!("{title}…"));
    }
}

fn builtin_verb(app: &mut App, id: &str, value: Value, tx: &mut Tx) {
    let (id, title) = id.split_once('\n').unwrap_or((id, ""));
    verbs::note_used(id);
    match id {
        "open" => match locator(&value) {
            Some(link) if link.contains("://") || link.starts_with("mailto:") => app.perform(Act::OpenUrl(link), tx),
            Some(path) => app.perform(Act::OpenPath(resolve_path(&util::home(), &path)), tx),
            None => app.toast("Nothing to open", Tone::Neutral),
        },
        "copy" => app.perform(Act::Copy(plain_text(&value)), tx),
        "paste" => app.perform(Act::Paste(plain_text(&value)), tx),
        "reveal" => match locator(&value) {
            Some(path) => app.perform(Act::Reveal(resolve_path(&util::home(), &path)), tx),
            None => app.toast("Nothing to show", Tone::Neutral),
        },
        "quick-look" => {
            let ty = types::infer(&value);
            let markdown = match (&value, ty.kind.as_str()) {
                (Value::String(s), "image") => format!("![]({s})"),
                (Value::String(s), _) => s.clone(),
                (_, "event") => format!("# {}\n\n{}", blocks::show(&value, Some(&ty)), faces::until(&value)),
                _ => format!("```json\n{}\n```", serde_json::to_string_pretty(&value).unwrap_or_default()),
            };
            let actions = vec![Action::new("Copy to Clipboard", &icons::COPY, Act::Copy(plain_text(&value)))];
            app.open_page(Page { title: if title.is_empty() { "Quick Look".into() } else { title.into() }, markdown, actions }, tx);
        }
        "pin" => {
            let ty = types::infer(&value);
            let pinned = verbs::toggle_pin(title, &value, &ty);
            app.toast(if pinned { "Pinned" } else { "Unpinned" }, Tone::Success);
            app.refresh_keeping();
        }
        other => match other.split_once(".verbs.") {
            Some((pack, name)) => {
                let title = app.packs.rt.packs.get(pack).and_then(|p| p.index.verbs.get(name)).map(|v| v.title.clone()).unwrap_or_default();
                let token = app.packs.rt.verb(pack, name, value, false);
                app.packs.waiting.insert(token, Waiting { title: title.clone(), announce: true, pack: pack.into() });
                app.toast_progress(format!("{title}…"));
            }
            None => app.toast(format!("No verb {other}"), Tone::Danger),
        },
    }
}

/// Point and say: what is on screen, exactly, and one sentence about it.
fn report(app: &mut App, depth: usize, note: &str) {
    let query = app.screens.get(depth).map(|s| s.query.text.clone()).unwrap_or_default();
    let selected = app.screens.get(depth).map_or(0, |s| s.sel);
    let saved = {
        let drawn = app.packs.drawn.borrow();
        let Some(d) = drawn.get(&depth) else { return };
        let dir = app.packs.pack_dir(&d.pack);
        if dir.as_os_str().is_empty() {
            return;
        }
        let error = d.error.as_ref().or(app.packs.errors.get(&d.pack)).map(|e| e.to_string()).unwrap_or_default();
        // The data behind what was drawn, so the same state can be drawn again with new code.
        let (mut loads, mut failed, mut stores) = (serde_json::Map::new(), serde_json::Map::new(), serde_json::Map::new());
        let prefix = format!("{}>", d.pack);
        for (key, r) in app.packs.rt.shared.resources.borrow().iter().filter(|(k, _)| k.starts_with(&prefix)) {
            let address = key[prefix.len()..].split('|').next().unwrap_or("").to_string();
            match r.state {
                "ok" => { loads.insert(address, r.value.clone()); }
                "error" => { failed.insert(address, json!(r.error.as_ref().map(|e| e.message.clone()).unwrap_or_default())); }
                _ => {}
            }
        }
        if let Some(p) = app.packs.rt.packs.get(&d.pack) {
            let held = app.packs.rt.shared.stores.borrow();
            for name in p.index.stores.keys() {
                stores.insert(name.clone(), held.get(&format!("{}.{name}", d.pack), None));
            }
        }
        let report = inbox::Report {
            t: audit::now(), pack: d.pack.clone(), target: d.target.clone(), note: note.to_string(), element: inbox::element(&d.blocks, selected),
            args: d.args.clone(), query, tree: blocks::tree(&d.blocks), blocks: d.blocks.clone(), error, loads, failed, stores, ..Default::default()
        };
        // The palette as it stands, when the screen can be read.
        let shot = std::env::temp_dir().join(format!("echospot-report-{}.png", std::process::id()));
        let _ = std::fs::remove_file(&shot);
        if let Some((x, y)) = crate::backdrop::centered_origin(crate::theme::size::PANEL_W, crate::theme::size::PANEL_H) {
            let geometry = format!("{},{} {}x{}", x.round(), y.round(), crate::theme::size::PANEL_W, crate::theme::size::PANEL_H);
            let _ = std::process::Command::new("grim").args(["-g", &geometry]).arg(&shot).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
        }
        let saved = inbox::save(&dir, report, Some(&shot));
        let _ = std::fs::remove_file(&shot);
        saved
    };
    match saved {
        Ok(_) => app.toast("Noted for the agent: echospot dev inbox", Tone::Success),
        Err(e) => app.toast(format!("Couldn't save the report: {e}"), Tone::Danger),
    }
}

/// The error an agent needs, as it would read it from `echospot dev check --json`.
fn error_for_agent(host: &Host, pack: &str) -> Option<String> {
    let p = host.rt.packs.get(pack);
    let error = p.and_then(|p| p.error.as_ref()).or(host.errors.get(pack))?;
    let dir = p.map(|p| p.dir.display().to_string()).unwrap_or_default();
    Some(format!("The EchoSpot pack `{pack}` ({dir}) has an error:\n\n{}\n\nFix it, then run `echospot dev check {pack}`.", serde_json::to_string_pretty(&error.json()).unwrap_or_default()))
}

pub fn perform(app: &mut App, op: PackOp, tx: &mut Tx) {
    match op {
        PackOp::Invoke { address, args } => invoke(app, &address, args, tx),
        PackOp::Run { address, input } => run(app, &address, input, true),
        PackOp::Open { pack, target, args } => {
            let target = if target.contains(':') { target } else { scenes::target(&app.packs.rt, &pack, &target).unwrap_or(target) };
            open(app, &pack, &target, args, tx);
        }
        PackOp::Handler { pack, target, handler, payload } => {
            app.menu = None;
            let token = app.packs.rt.handle(&pack, &target, handler, payload, false);
            app.packs.waiting.insert(token, Waiting { title: String::new(), announce: false, pack });
        }
        PackOp::Verb { id, value } => builtin_verb(app, &id, value, tx),
        PackOp::SendTo { value, ty } => {
            let ty = Type::from_json(&ty).ok().filter(|t| !t.kind.is_empty()).unwrap_or_else(|| types::infer(&value));
            verbs::note_used("send");
            app.packs.sending = Some((value, ty));
            app.push(Route::PackSend, tx);
        }
        PackOp::Send { address, slot, value, .. } => {
            app.pop(tx);
            let mut args = Map::new();
            args.insert(slot, value);
            invoke(app, &address, Value::Object(args), tx);
        }
        PackOp::Review(id) => app.push(Route::PackGrants(id), tx),
        PackOp::Grant(id) => {
            let name = app.packs.rt.packs.get(&id).map(|p| p.name().to_string()).unwrap_or_default();
            app.packs.rt.grant_all(&id);
            if matches!(app.top().route, Route::PackGrants(_)) {
                app.pop(tx);
            }
            app.toast(format!("{name} can run now"), Tone::Success);
            app.refresh();
        }
        PackOp::Revoke(id) => {
            app.packs.rt.revoke(&id, None);
            app.toast("Permissions revoked", Tone::Success);
            app.refresh_keeping();
        }
        PackOp::CopyError(id) => match error_for_agent(&app.packs, &id) {
            Some(text) => {
                crate::features::clipboard::copy_text(&text);
                app.toast("Error copied: paste it to your agent", Tone::Success);
            }
            None => app.toast("No error to copy", Tone::Neutral),
        },
        PackOp::Undo(id) => {
            app.packs.undo = None;
            match app.packs.rt.undo(id) {
                Ok(tokens) => {
                    for token in tokens {
                        app.packs.waiting.insert(token, Waiting { title: String::new(), announce: false, pack: String::new() });
                    }
                    app.toast("Undone", Tone::Success);
                }
                Err(e) => app.toast(e.message, Tone::Danger),
            }
        }
        PackOp::Manage => app.push(Route::Packs, tx),
        PackOp::Surface { pack, name } => super::slots::toggle(app, &pack, &name),
        PackOp::Board { pack, name, window } => super::slots::toggle_board(app, &pack, &name, window),
        PackOp::Report => {
            let depth = app.screens.len() - 1;
            if !matches!(app.top().route, Route::Pack { .. }) || !app.packs.drawn.borrow().contains_key(&depth) {
                return app.toast("Point at a pack's screen to report it", Tone::Neutral);
            }
            app.menu = None;
            let fields = vec![Field::new("note", "What should change?", FieldKind::Area).placeholder("show the repo name here").required()];
            forms::open(app, Form::new("Report to Agent", "Save Report", fields, Target::Pack(Box::new(PackForm::Report { depth }))), tx);
        }
        PackOp::Keep => {
            let spec = match &app.top().route {
                Route::Pack { pack, args, .. } if pack == ANSWER => args.clone(),
                _ => return,
            };
            match super::answers::keep(&app.packs.rt, &spec) {
                Ok((id, _)) => {
                    app.packs.reload(true);
                    app.pop(tx);
                    app.toast(format!("Kept as the pack “{id}”"), Tone::Success);
                    if app.packs.rt.packs.get(&id).is_some_and(|p| p.state == State::NeedsGrant) {
                        perform(app, PackOp::Review(id), tx);
                    }
                }
                Err(e) => app.toast(e.message, Tone::Danger),
            }
        }
        PackOp::Confirming { address, ask } => {
            app.packs.rt.grants.set_confirm(&address, ask);
            app.toast(if ask { "It will ask first" } else { "It won't ask again" }, Tone::Success);
            app.refresh_keeping();
        }
        PackOp::Reload => {
            app.packs.rt.shared.resources.borrow_mut().clear();
            app.packs.reload(true);
            app.refresh_keeping();
        }
    }
}

/// A link the system handed EchoSpot: `echospot://github.prs?repo=rune`.
pub fn open_link(app: &mut App, link: &str, tx: &mut Tx) -> bool {
    let Some((address, args)) = links::parse(link) else { return false };
    invoke(app, &address, args, tx);
    true
}

// ---- The loop ------------------------------------------------------------------------------

fn pack_route(route: &Route) -> bool {
    matches!(route, Route::Pack { .. } | Route::Packs | Route::PackGrants(_) | Route::PackSend | Route::Root)
}

/// Runs what is ready and acts on what the runtime asks of the app. True when a frame is due.
pub fn poll(app: &mut App, tx: &mut Tx) -> bool {
    let mut changed = app.packs.rt.pump(tx);
    if changed {
        changed |= app.packs.refresh_living();
    }
    // Data a pack brought (a load settling, a store written) changes what a pack screen shows.
    // Rune lands changes no input caused at once; on a pack screen they move, as §13.5 asks.
    if changed && app.visible && matches!(app.top().route, Route::Pack { .. }) {
        tx.animated();
    }
    let events = std::mem::take(&mut app.packs.rt.events);
    let mut reloaded = false;
    for event in events {
        changed = true;
        match event {
            Event::Nav { pack, op, address, args } => {
                let target = match address.split_once('.') {
                    Some((other, name)) if signature(&app.packs.rt, &address).is_some() => Some((other.to_string(), format!("ability:{name}:panel"))),
                    _ => scenes::target(&app.packs.rt, &pack, &address).ok().map(|t| (pack.clone(), t)),
                };
                match (op.as_str(), target) {
                    ("pop", _) => app.pop(tx),
                    ("push", Some((pack, target))) => open(app, &pack, &target, args, tx),
                    ("replace", Some((pack, target))) => {
                        if matches!(app.top().route, Route::Pack { .. }) {
                            app.screens.pop();
                        }
                        open(app, &pack, &target, args, tx);
                    }
                    _ => app.toast(format!("{pack} has nothing named {address}"), Tone::Danger),
                }
            }
            Event::Toast { text, intent } => {
                if app.visible { app.toast(text, tone(&intent)) } else { crate::hud::show(&text, tone(&intent)) }
            }
            Event::Hud { text } => crate::hud::show(&text, Tone::Neutral),
            Event::Undoable { id, title } => {
                app.packs.undo = Some((id, app.clock + 5.0));
                let text = format!("{title} · Undo with Ctrl Z");
                if app.visible {
                    app.toast = Some(crate::app::Toast { text, tone: Tone::Success, until: app.clock + 5.0 });
                    tx.wake_after(std::time::Duration::from_secs(5));
                } else {
                    crate::hud::show(&title, Tone::Success);
                }
            }
            Event::PacksChanged => reloaded = true,
            Event::Confirm { token, title, summary, .. } => {
                app.ask(Confirm {
                    title: format!("{title}?"), message: if summary.is_empty() { "This can't be undone.".into() } else { format!("{summary}\nThis can't be undone.") },
                    icon: &icons::BOLT, confirm: "Do It".into(), destructive: true, act: Act::None,
                }, crate::app::Asker::Pack(token));
            }
            Event::Pick { token, title, folder, .. } => {
                let label = if folder { "Folder" } else { "File" };
                let fields = vec![
                    Field::new("path", label, FieldKind::Text).placeholder("~/Documents/…").required(),
                    Field::new("about", "", FieldKind::Description).value("The pack can use this one path from now on, and nothing beside it."),
                ];
                forms::open(app, Form::new(if title.is_empty() { "Choose a Path" } else { &title }, "Allow", fields, Target::Pack(Box::new(PackForm::Pick { token }))), tx);
            }
        }
    }
    if reloaded {
        app.packs.icons.borrow_mut().clear();
    }
    // Calls a person started: say how each went.
    let finished: Vec<u64> = app.packs.waiting.keys().copied().collect();
    for token in finished {
        let Some(result) = app.packs.rt.take(token) else { continue };
        let Some(waiting) = app.packs.waiting.remove(&token) else { continue };
        changed = true;
        match result {
            Ok(value) => {
                // An undo toast, when the effect left one, says more than "Done".
                let undoable = app.packs.undo.is_some_and(|(_, until)| until > app.clock + 4.9);
                if waiting.announce && !undoable {
                    let shown = plain_text(&value);
                    if value.is_null() || shown.is_empty() {
                        app.toast(format!("{} · Done", waiting.title), Tone::Success);
                    } else if shown.chars().count() <= 70 && !shown.contains('\n') {
                        app.toast(shown, Tone::Success);
                    } else {
                        app.toast = None;
                        let ty = types::infer(&value);
                        let markdown = if value.is_string() { shown.clone() } else { format!("```json\n{shown}\n```") };
                        let mut actions = verb_actions(&app.packs.rt, &waiting.title, &value, &ty);
                        actions.retain(|a| !matches!(&a.act, Act::Pack(PackOp::Verb { id, .. }) if id == "quick-look"));
                        app.open_page(Page { title: waiting.title.clone(), markdown, actions }, tx);
                    }
                } else if app.toast.as_ref().is_some_and(|t| t.tone == Tone::Progress) {
                    app.toast = None;
                }
            }
            Err(e) if e.message.starts_with("cancelled") => app.toast = None,
            Err(e) => {
                app.toast(util::truncate(&e.message, 110), Tone::Danger);
                if !waiting.pack.is_empty() {
                    app.packs.errors.insert(waiting.pack, e);
                }
            }
        }
    }
    if app.packs.undo.is_some_and(|(_, until)| app.clock >= until) {
        app.packs.undo = None;
    }
    if changed && pack_route(&app.top().route) && app.form.is_none() {
        app.refresh_keeping();
    }
    if changed {
        super::slots::sync(app);
    }
    changed
}

/// Once a second: files saved by an agent, schedules come due, things that happened.
pub fn tick(app: &mut App) -> bool {
    let now = audit::now();
    let mut changed = app.packs.reload(false);
    if now - app.packs.last_tick >= 15.0 {
        app.packs.last_tick = now;
        app.packs.rt.tick(now);
        changed |= app.packs.watch(now);
    }
    changed |= super::slots::due(app, now);
    if changed && app.visible && pack_route(&app.top().route) && app.form.is_none() {
        app.refresh_keeping();
    }
    changed
}

impl Host {
    /// Whether the row at `index` of the top screen may run its effect: the first few that ask
    /// do, and the rest hold still, so a list of urgent things isn't a wall of fire.
    pub fn fx_room(&self, index: usize) -> bool {
        self.fx_rows.borrow().contains(&index)
    }

    pub fn drew(&self, key: &str) -> f64 {
        self.drew.get(key).copied().unwrap_or(0.0)
    }
    pub fn mark_drew(&mut self, key: &str, now: f64) {
        self.drew.insert(key.to_string(), now);
    }

    /// Something happened outside EchoSpot: packs that listen for it run.
    pub fn emit(&mut self, event: &str, detail: Value) {
        self.rt.emit(event, detail);
    }

    /// The events with no signal of their own, read by looking: battery, network, files, calendar.
    fn watch(&mut self, now: f64) -> bool {
        let listening: Vec<String> = self.rt.unasked().into_iter().flat_map(|e| e.on).collect();
        if listening.is_empty() {
            return false;
        }
        let mut any = false;
        let hears = |name: &str| listening.iter().any(|on| on == name || on.starts_with(&format!("{name}:")));
        if hears("battery.low") {
            let battery = super::stores::battery();
            let low = battery.get("percent").and_then(Value::as_f64).is_some_and(|p| p <= 20.0) && battery.get("charging").and_then(Value::as_bool) != Some(true);
            if low && !self.seen.battery_low {
                any |= self.rt.emit("battery.low", battery) > 0;
            }
            self.seen.battery_low = low;
        }
        if hears("network.changed") {
            let network = super::stores::network();
            let key = network.to_string();
            if !self.seen.network.is_empty() && key != self.seen.network {
                any |= self.rt.emit("network.changed", network) > 0;
            }
            self.seen.network = key;
        }
        for on in listening.iter().filter_map(|on| on.strip_prefix("file.changed:")) {
            let pattern = resolve_path(&util::home(), on).display().to_string();
            for path in glob(&pattern) {
                let stamp = std::fs::metadata(&path).ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64);
                let key = path.display().to_string();
                match self.seen.files.insert(key.clone(), stamp) {
                    Some(before) if before != stamp => any |= self.rt.emit("file.changed", json!({ "path": key })) > 0,
                    _ => {}
                }
            }
        }
        if hears("calendar.soon") {
            let next = super::stores::builtin("calendar");
            for event in next.as_array().into_iter().flatten() {
                let starts = event.get("starts").or_else(|| event.get("start")).and_then(Value::as_f64).unwrap_or(0.0);
                let key = format!("{}@{starts}", event.get("title").and_then(Value::as_str).unwrap_or(""));
                if starts > now && starts - now <= 600.0 && !self.seen.soon.contains(&key) {
                    self.seen.soon.push(key);
                    // The oldest are forgotten; truncating would forget the one just noted.
                    if self.seen.soon.len() > 64 {
                        let extra = self.seen.soon.len() - 64;
                        self.seen.soon.drain(..extra);
                    }
                    any |= self.rt.emit("calendar.soon", event.clone()) > 0;
                }
            }
        }
        any
    }
}

/// Files a glob names, without leaving the folder its fixed part names. `**` reaches four
/// levels down; that is enough for a project and bounded for a home folder.
fn glob(pattern: &str) -> Vec<PathBuf> {
    let fixed: PathBuf = Path::new(pattern).components().take_while(|c| !c.as_os_str().to_string_lossy().contains(['*', '?'])).collect();
    if fixed.as_os_str() == pattern {
        return vec![fixed];
    }
    fn walk(dir: &Path, depth: usize, pattern: &str, out: &mut Vec<PathBuf>) {
        if out.len() > 2000 {
            return;
        }
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if path.is_dir() {
                if depth < 4 {
                    walk(&path, depth + 1, pattern, out);
                }
            } else if super::builtins::fs::glob(pattern, &path.display().to_string()) {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(&fixed, 0, pattern, &mut out);
    out
}

/// Keys the host answers on a pack's screen, before the palette's own.
pub fn key(app: &mut App, ev: &rune::KeyEvent, tx: &mut Tx) -> bool {
    use rune::Key;
    match &ev.key {
        Key::Command('z') if !ev.mods.shift => match app.packs.undo {
            Some((id, _)) => {
                perform(app, PackOp::Undo(id), tx);
                true
            }
            None => false,
        },
        Key::Command('.') if matches!(app.top().route, Route::Pack { .. }) => {
            perform(app, PackOp::Report, tx);
            true
        }
        Key::Command('r') if matches!(app.top().route, Route::Pack { .. } | Route::Packs) => {
            perform(app, PackOp::Reload, tx);
            true
        }
        // Tab on a value: Send to…
        Key::Tab if !ev.mods.cmd && !ev.mods.shift && matches!(app.top().route, Route::Pack { .. }) && app.top().filters.is_empty() => {
            let send = app.top().selected().and_then(|i| i.actions.iter().find(|a| matches!(a.act, Act::Pack(PackOp::SendTo { .. })))).map(|a| a.act.clone());
            match send {
                Some(act) => {
                    app.perform(act, tx);
                    true
                }
                None => false,
            }
        }
        _ => false,
    }
}

/// Every pack ability the chat may use as a tool, and the way a worker thread calls one: on the
/// UI thread, through the broker, as the AI.
pub fn toolbox(app: &App) -> crate::features::ai_wire::Toolbox {
    use crate::features::mcp::Tool;
    let listed = super::mcp::tools(&app.packs.rt);
    let mut tools: Vec<Tool> = listed.iter().map(|(wire, address, about, schema)| Tool { wire: wire.clone(), server: "echospot".into(), name: address.clone(), description: about.clone(), schema: schema.clone() }).collect();
    tools.push(Tool { wire: super::answers::TOOL.into(), server: "echospot".into(), name: super::answers::TOOL.into(), description: super::answers::ABOUT.into(), schema: super::answers::schema() });
    let addresses: HashMap<String, String> = listed.into_iter().map(|(wire, address, ..)| (wire, address)).collect();
    let Some(main) = MAIN.get().cloned() else { return Default::default() };
    let call = move |wire: &str, input: &Value| -> Result<String, String> {
        let (reply, answer) = std::sync::mpsc::channel();
        if wire == super::answers::TOOL {
            let spec = input.clone();
            main.update(move |s, tx| {
                let _ = reply.send(super::answers::check(&s.packs.rt, &spec).map(|()| {
                    s.perform(Act::Pack(PackOp::Open { pack: ANSWER.into(), target: "board:answer".into(), args: spec }), tx);
                    json!("Shown to the user as a live surface. They can keep it as a pack.")
                }));
            });
        } else {
            let Some(address) = addresses.get(wire).cloned() else { return Err(format!("There is no tool named {wire}.")) };
            let input = input.clone();
            main.update(move |s, _| {
                s.packs.rt.start(Origin::Ai, &address, input, Responder::Channel(reply), false);
            });
        }
        match answer.recv_timeout(std::time::Duration::from_secs(120)) {
            Ok(Ok(value)) => Ok(plain_text(&value)),
            Ok(Err(e)) => Err(e.to_string()),
            Err(_) => Err("The ability didn't answer in time.".into()),
        }
    };
    crate::features::ai_wire::Toolbox { tools, call: Some(std::sync::Arc::new(call)) }
}

/// What is happening on the desktop, heard from Hyprland as it happens: the window that took
/// focus, the workspace that came up. Packs that listen for either run.
pub fn listen(handle: AppHandle<App>) {
    use std::io::{BufRead, BufReader};
    let Some(signature) = std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE") else { return };
    let socket = util::runtime_dir().join("hypr").join(signature).join(".socket2.sock");
    std::thread::Builder::new().name("echospot-pack-events".into()).spawn(move || loop {
        if let Ok(stream) = std::os::unix::net::UnixStream::connect(&socket) {
            for line in BufReader::new(stream).lines().map_while(Result::ok) {
                let Some((event, data)) = line.split_once(">>") else { continue };
                let (name, detail) = match event {
                    "activewindow" => {
                        let (class, title) = data.split_once(',').unwrap_or((data, ""));
                        // The palette taking focus is not news to a pack.
                        if class == crate::APP_ID || class.is_empty() {
                            continue;
                        }
                        ("window.focused", json!({ "class": class, "title": title }))
                    }
                    "workspace" => ("workspace.changed", json!({ "name": data })),
                    _ => continue,
                };
                handle.update_if(move |s: &mut App, _| {
                    s.packs.emit(name, detail);
                    false
                });
            }
        }
        // Hyprland restarted, or isn't up yet.
        std::thread::sleep(std::time::Duration::from_secs(5));
    }).ok();
}

/// `echospot dev render`: this process draws one surface of one pack, in one scene, and nothing
/// of the user's is touched: the grants are read, never written, and a scene stands in for
/// every load.
pub fn snap(app: &mut App, spec: &Value) {
    let dir = PathBuf::from(spec.get("dir").and_then(Value::as_str).unwrap_or(""));
    let target = spec.get("target").and_then(Value::as_str).unwrap_or("").to_string();
    let name = spec.get("scene").and_then(Value::as_str).unwrap_or("live");
    let mut rt = Runtime::for_cli();
    rt.load_all();
    let Some(id) = rt.load(&dir, true) else { return };
    rt.check_dependencies();
    let scene = match spec.get("scene_data").filter(|d| d.is_object()) {
        Some(data) => Some(super::shared::Scene::from_json(name, data)),
        None => scenes::stress(&mut rt, &id, &target).into_iter().find(|s| s.name == name),
    };
    app.packs.rt = rt;
    app.packs.headless = true;
    let mut args = json!({});
    if let Some(scene) = &scene {
        if let Some(dark) = scene.dark {
            app.settings.appearance = if dark { "dark" } else { "light" }.into();
        }
        if !scene.args.is_null() {
            args = scene.args.clone();
        }
        app.packs.scene_size = scene.size;
    }
    let (query, live) = (scene.as_ref().map(|s| s.query.clone()).unwrap_or_default(), scene.is_none());
    app.packs.rt.set_scene(scene);
    app.screens.truncate(1);
    app.screens.push(crate::app::Screen::new(Route::Pack { pack: id, target, args }));
    app.top_mut().query.replace_text(&query);
    app.visible = true;
    app.refresh();
    if live {
        // The real abilities run: draw again until everything a draw loads has settled.
        for _ in 0..6 {
            app.packs.rt.settle(std::time::Duration::from_secs(20));
            let before = app.packs.drawn.borrow().get(&1).map(|d| d.blocks.clone());
            app.refresh();
            if before == app.packs.drawn.borrow().get(&1).map(|d| d.blocks.clone()) {
                break;
            }
        }
    }
}

// ---- Settings ------------------------------------------------------------------------------

/// One pack as the Settings window's Packs pane shows it.
#[derive(Clone, Debug, Default)]
pub struct PackView {
    pub id: String,
    pub name: String,
    /// Where it stands: "Ready", "Review permissions", "Broken"…
    pub status: String,
    pub system: bool,
    /// Spent on AI this month, and the monthly budget it asks for.
    pub spent: f64,
    pub budget: Option<f64>,
    /// What a person granted it, one revocable line each.
    pub held: Vec<super::manifest::Ask>,
    /// What runs without being asked: its name, its schedule, when it last ran.
    pub runs: Vec<(String, String, String)>,
    /// Load time, memory and time inside its code.
    pub cost: String,
    pub facts: Vec<String>,
}

/// What the Settings window's Packs pane draws, read from the palette's runtime.
#[derive(Clone, Debug, Default)]
pub struct Snapshot {
    pub packs: Vec<PackView>,
    /// Secret names with the hosts each may go to. The values never leave the host.
    pub secrets: Vec<(String, Vec<String>)>,
    /// The newest audit lines first.
    pub audit: Vec<audit::Entry>,
}

pub fn snapshot(host: &Host) -> Snapshot {
    let log = audit::read();
    let unasked = host.rt.unasked();
    let packs = host.rt.packs.values().map(|p| {
        let runs = unasked.iter().filter(|e| e.pack == p.id).map(|e| {
            let last = host.rt.runs.get(&e.key).map(|r| format!("{}{}", util::ago(util::now_secs().saturating_sub(r.t as u64)), if r.ok { String::new() } else { format!(" · failed: {}", r.error) })).unwrap_or_else(|| "Hasn't run yet".into());
            (e.name.clone(), e.when(), last)
        }).collect();
        let cost = match &p.script {
            Some(script) => {
                let m = script.metrics();
                format!("Loaded in {:.1} ms · {} · {} entries, {:.1} ms in its code", p.load_ms, util::human_size(script.memory() as u64), m.vm_calls, m.vm_time.as_secs_f64() * 1e3)
            }
            None if p.wasm.is_some() => format!("Loaded in {:.1} ms · Rust, each call in a fresh instance", p.load_ms),
            None => format!("Loaded in {:.1} ms · not started", p.load_ms),
        };
        PackView {
            id: p.id.clone(), name: p.name().to_string(), status: if p.state == State::Ready { "Ready".into() } else { p.status() }, system: p.system,
            spent: audit::spend_this_month(&p.id), budget: p.manifest.permissions.ai.as_ref().map(|a| a.budget_usd_month),
            held: host.rt.grants.held(&p.id), runs, cost,
            facts: trust::labels(&p.id, &p.manifest.permissions, &p.index, &log),
        }
    }).collect();
    Snapshot { packs, secrets: super::secrets::names(), audit: log.into_iter().rev().take(80).collect() }
}

impl super::draw::PackInputs for crate::app::App {
    fn pack_input(&mut self, key: &str, initial: &str) -> &mut rune::TextEdit {
        self.packs.inputs.entry(key.to_string()).or_insert_with(|| rune::TextEdit::new(initial))
    }
}
