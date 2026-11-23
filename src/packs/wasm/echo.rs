//! `echo`: a Rust pack's side of EchoSpot (API-DESIGN.md §20). `echospot dev new --rust` and
//! `echospot dev check` write this file; your code is `src/lib.rs`, which defines `pub fn pack()`.
//!
//! ```ignore
//! mod echo;
//! use echo::{Ability, Cx, Pack, Result, t};
//! use serde_json::{Value, json};
//!
//! pub fn pack() -> Pack {
//!     Pack::new("Primes").ability("nth", Ability::read("Nth Prime")
//!         .input([("n", t::number())])
//!         .output(t::number())
//!         .examples(["nth prime 1000"])
//!         .run(|input, _cx| Ok(json!(nth(input["n"].as_f64().unwrap_or(1.0) as u64)))))
//! }
//! ```
//!
//! The pack is headless: abilities, verbs, stores and triggers. Its UI is the default faces for
//! each output type, or a Luau `init.luau` beside `Cargo.toml` that draws surfaces.
#![allow(dead_code)]

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

mod bindings {
    wit_bindgen::generate!({ world: "pack", path: "wit" });
}

use bindings::echospot::pack::host;
use bindings::{Context, Guest};

// ---- Errors ------------------------------------------------------------------------------

/// An error EchoSpot shows and an agent can act on: a code, a message, and a command to learn more.
#[derive(Clone, Debug)]
pub struct Error {
    pub code: String,
    pub message: String,
    pub learn: String,
}

impl Error {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Error { code: code.into(), message: message.into(), learn: String::new() }
    }
    pub fn learn(mut self, how: impl Into<String>) -> Self {
        self.learn = how.into();
        self
    }
    fn json(&self) -> String {
        json!({ "code": self.code, "message": self.message, "learn": self.learn }).to_string()
    }
    fn parse(text: String) -> Self {
        match serde_json::from_str::<Value>(&text) {
            Ok(v) if v.get("message").is_some() => Error {
                code: v["code"].as_str().unwrap_or("E0300").into(),
                message: v["message"].as_str().unwrap_or("").into(),
                learn: v["learn"].as_str().unwrap_or("").into(),
            },
            _ => Error::new("E0300", text),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Error {}

/// Any message is an error; `?` works on anything that prints.
impl From<String> for Error {
    fn from(message: String) -> Self {
        Error::new("E0300", message)
    }
}

impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Error::new("E0300", message)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::new("E0302", e.to_string())
    }
}

pub type Result<T = Value> = std::result::Result<T, Error>;

// ---- The context -------------------------------------------------------------------------

/// The call your code runs under. Everything it does to the world goes through here.
pub struct Cx {
    pub pack: String,
    dry: bool,
}

impl Cx {
    /// Calls an ability and waits for it: `cx.call("calc.convert", json!({ "amount": 5, "from": "usd", "to": "eur" }))`.
    /// Addresses of other packs need `calls = [...]` in pack.toml.
    pub fn call(&self, address: &str, input: Value) -> Result {
        let answer = host::call(address, &input.to_string()).map_err(Error::parse)?;
        Ok(serde_json::from_str(&answer).unwrap_or(Value::Null))
    }

    /// Calls an ability without failing: the error comes back as a value to look at.
    pub fn try_call(&self, address: &str, input: Value) -> std::result::Result<Value, Error> {
        self.call(address, input)
    }

    /// One of this pack's stores now.
    pub fn store(&self, name: &str) -> Result {
        self.call(&format!("{}.stores.{name}", self.pack), json!({}))
    }

    /// One of this pack's stores as it was at `time` (seconds since 1970).
    pub fn store_at(&self, name: &str, time: f64) -> Result {
        self.call(&format!("{}.stores.{name}", self.pack), json!({ "at": time }))
    }

    /// Writes one of this pack's own stores; a surface that reads it draws again.
    pub fn put(&self, name: &str, value: Value) -> Result<()> {
        host::put(name, &value.to_string()).map_err(Error::parse)
    }

    /// True while `echospot dev run --dry` or a test is stubbing effects.
    pub fn dry(&self) -> bool {
        self.dry
    }

    /// A line in `echospot dev logs`.
    pub fn log(&self, message: impl AsRef<str>) {
        host::log("info", message.as_ref());
    }

    pub fn warn(&self, message: impl AsRef<str>) {
        host::log("warn", message.as_ref());
    }
}

// ---- Types -------------------------------------------------------------------------------

/// Schemas, as `@echo/types` makes them in Luau: `t::text()`, `t::list(t::url())`,
/// `t::record("pr", [("title", t::text())])`. The host checks inputs and outputs against them.
pub mod t {
    use serde_json::{Map, Value, json};

    #[derive(Clone, Debug)]
    pub struct Type(pub(crate) Value);

    impl Type {
        fn of(kind: &str) -> Self {
            Type(json!({ "type": kind }))
        }
        fn set(mut self, key: &str, value: Value) -> Self {
            self.0[key] = value;
            self
        }
        /// May be left out.
        pub fn optional(self) -> Self {
            self.set("optional", json!(true))
        }
        /// What it is when left out.
        pub fn default(self, value: impl Into<Value>) -> Self {
            self.set("default", value.into())
        }
        /// What a form shows beside the field.
        pub fn describe(self, label: &str) -> Self {
            self.set("label", json!(label))
        }
    }

    macro_rules! core {
        ($($name:ident = $kind:literal),* $(,)?) => { $(pub fn $name() -> Type { Type::of($kind) })* };
    }

    core! {
        text = "text", markdown = "markdown", code = "code", number = "number", bool = "bool", quantity = "quantity",
        money = "money", currency = "currency", percent = "percent", date = "date", time = "time", datetime = "datetime",
        duration = "duration", url = "url", file = "file", folder = "folder", image = "image", color = "color", app = "app",
        window = "window", event = "event", person = "person", place = "place", table = "table", series = "series", any = "any",
    }

    pub fn list(of: Type) -> Type {
        Type::of("list").set("of", of.0)
    }

    /// A record with a name verbs and Send to… match on; `""` for none.
    pub fn record<'a>(name: &str, fields: impl IntoIterator<Item = (&'a str, Type)>) -> Type {
        let fields: Map<String, Value> = fields.into_iter().map(|(k, t)| (k.to_string(), t.0)).collect();
        Type::of("record").set("name", json!(name)).set("fields", Value::Object(fields))
    }

    pub fn enumeration<'a>(values: impl IntoIterator<Item = &'a str>) -> Type {
        Type::of("enum").set("values", json!(values.into_iter().collect::<Vec<_>>()))
    }
}

// ---- Declarations ------------------------------------------------------------------------

type RunFn = Box<dyn Fn(Value, &Cx) -> Result>;
type UndoFn = Box<dyn Fn(Value, Value, &Cx) -> Result>;

fn texts<'a>(list: impl IntoIterator<Item = &'a str>) -> Value {
    json!(list.into_iter().collect::<Vec<_>>())
}

/// A typed function with an address: `<pack>.<name>`.
pub struct Ability {
    decl: Map<String, Value>,
    run: Option<RunFn>,
    undo: Option<UndoFn>,
}

impl Ability {
    fn kind(kind: &str, title: &str) -> Self {
        let mut decl = Map::new();
        decl.insert("kind".into(), json!(kind));
        decl.insert("title".into(), json!(title));
        Ability { decl, run: None, undo: None }
    }
    /// No effect on the world. It may only call reads.
    pub fn read(title: &str) -> Self {
        Self::kind("read", title)
    }
    /// Changes something and can be taken back: give it an `undo`, or call only effects that have one.
    pub fn effect(title: &str) -> Self {
        Self::kind("effect", title)
    }
    /// Can't be undone. EchoSpot asks the person first.
    pub fn final_(title: &str) -> Self {
        Self::kind("final", title)
    }
    fn with(mut self, key: &str, value: Value) -> Self {
        self.decl.insert(key.into(), value);
        self
    }
    pub fn input<'a>(self, fields: impl IntoIterator<Item = (&'a str, t::Type)>) -> Self {
        let fields: Map<String, Value> = fields.into_iter().map(|(k, ty)| (k.to_string(), ty.0)).collect();
        self.with("input", json!({ "type": "record", "fields": fields }))
    }
    pub fn output(self, ty: t::Type) -> Self {
        self.with("output", ty.0)
    }
    pub fn description(self, text: &str) -> Self {
        self.with("description", json!(text))
    }
    /// A file in `icons/`, drawn to the icon spec: `icons/prime.svg`.
    pub fn icon(self, path: &str) -> Self {
        self.with("icon", json!(path))
    }
    pub fn tint(self, name: &str) -> Self {
        self.with("tint", json!(name))
    }
    /// What a person might type for it in root search; each is tested.
    pub fn examples<'a>(self, list: impl IntoIterator<Item = &'a str>) -> Self {
        self.with("examples", texts(list))
    }
    pub fn keywords<'a>(self, list: impl IntoIterator<Item = &'a str>) -> Self {
        self.with("keywords", texts(list))
    }
    /// An effect EchoSpot's AI and MCP may call as a tool. Reads always may.
    pub fn ai(self, yes: bool) -> Self {
        self.with("ai", json!(yes))
    }
    /// A final ability asks first unless this is false.
    pub fn confirm(self, yes: bool) -> Self {
        self.with("confirm", json!(yes))
    }
    pub fn hidden(self, yes: bool) -> Self {
        self.with("hidden", json!(yes))
    }
    /// A global hotkey to suggest, as `"super+alt+p"`. Settings offers it; only a person binds it.
    pub fn hotkey(self, chord: &str) -> Self {
        self.with("hotkey", json!(chord))
    }
    /// How long a result stays fresh for a surface that loads it: `"30m"`.
    pub fn refresh(self, every: &str) -> Self {
        self.with("refresh", json!(every))
    }
    /// Runs on a schedule: `"15m"`. Needs `background = true` in pack.toml.
    pub fn every(self, every: &str) -> Self {
        self.with("every", json!(every))
    }
    /// Runs at a time: `"09:00 weekdays"`. Needs `background = true` in pack.toml.
    pub fn at(self, when: &str) -> Self {
        self.with("at", json!(when))
    }
    /// Runs on events: `["clipboard.changed"]`. Needs `background = true` in pack.toml.
    pub fn on<'a>(self, events: impl IntoIterator<Item = &'a str>) -> Self {
        self.with("on", texts(events))
    }
    pub fn run(mut self, f: impl Fn(Value, &Cx) -> Result + 'static) -> Self {
        self.run = Some(Box::new(f));
        self
    }
    /// Takes the effect back, given what `run` was handed and what it returned.
    pub fn undo(mut self, f: impl Fn(Value, Value, &Cx) -> Result + 'static) -> Self {
        self.undo = Some(Box::new(f));
        self
    }
    fn describe(&self) -> Value {
        let mut d = self.decl.clone();
        d.insert("has_run".into(), json!(self.run.is_some()));
        d.insert("has_undo".into(), json!(self.undo.is_some()));
        Value::Object(d)
    }
}

/// An action on a type, offered wherever a value of that type appears.
pub struct Verb {
    decl: Map<String, Value>,
    run: Option<RunFn>,
    undo: Option<UndoFn>,
}

impl Verb {
    /// `Verb::new("url", "Shorten URL")`.
    pub fn new(on: &str, title: &str) -> Self {
        let mut decl = Map::new();
        decl.insert("on".into(), json!(on));
        decl.insert("title".into(), json!(title));
        decl.insert("kind".into(), json!("effect"));
        Verb { decl, run: None, undo: None }
    }
    pub fn kind(mut self, kind: &str) -> Self {
        self.decl.insert("kind".into(), json!(kind));
        self
    }
    pub fn icon(mut self, path: &str) -> Self {
        self.decl.insert("icon".into(), json!(path));
        self
    }
    pub fn run(mut self, f: impl Fn(Value, &Cx) -> Result + 'static) -> Self {
        self.run = Some(Box::new(f));
        self
    }
    pub fn undo(mut self, f: impl Fn(Value, Value, &Cx) -> Result + 'static) -> Self {
        self.undo = Some(Box::new(f));
        self
    }
    fn describe(&self) -> Value {
        let mut d = self.decl.clone();
        d.insert("has_undo".into(), json!(self.undo.is_some()));
        Value::Object(d)
    }
}

/// Live, typed data with history. Surfaces that read it draw again when it changes.
pub struct Store(Map<String, Value>);

impl Store {
    pub fn new(ty: t::Type) -> Self {
        let mut d = Map::new();
        d.insert("type".into(), ty.0);
        Store(d)
    }
    /// How long past versions are kept: `"30d"`.
    pub fn keep(mut self, how_long: &str) -> Self {
        self.0.insert("keep".into(), json!(how_long));
        self
    }
    /// Kept across restarts.
    pub fn persist(mut self, yes: bool) -> Self {
        self.0.insert("persist".into(), json!(yes));
        self
    }
    pub fn initial(mut self, value: Value) -> Self {
        self.0.insert("initial".into(), value);
        self
    }
}

/// Code that runs without being asked: on a schedule or an event.
pub struct Trigger {
    decl: Map<String, Value>,
    run: Option<RunFn>,
}

impl Trigger {
    pub fn every(every: &str) -> Self {
        let mut decl = Map::new();
        decl.insert("every".into(), json!(every));
        Trigger { decl, run: None }
    }
    pub fn at(when: &str) -> Self {
        let mut decl = Map::new();
        decl.insert("at".into(), json!(when));
        Trigger { decl, run: None }
    }
    pub fn on<'a>(events: impl IntoIterator<Item = &'a str>) -> Self {
        let mut decl = Map::new();
        decl.insert("on".into(), texts(events));
        Trigger { decl, run: None }
    }
    pub fn run(mut self, f: impl Fn(Value, &Cx) -> Result + 'static) -> Self {
        self.run = Some(Box::new(f));
        self
    }
}

/// Everything the pack declares.
pub struct Pack {
    title: String,
    abilities: BTreeMap<String, Ability>,
    verbs: BTreeMap<String, Verb>,
    stores: BTreeMap<String, Store>,
    triggers: BTreeMap<String, Trigger>,
}

impl Pack {
    pub fn new(title: &str) -> Self {
        Pack { title: title.into(), abilities: BTreeMap::new(), verbs: BTreeMap::new(), stores: BTreeMap::new(), triggers: BTreeMap::new() }
    }
    pub fn ability(mut self, name: &str, a: Ability) -> Self {
        self.abilities.insert(name.into(), a);
        self
    }
    pub fn verb(mut self, name: &str, v: Verb) -> Self {
        self.verbs.insert(name.into(), v);
        self
    }
    pub fn store(mut self, name: &str, s: Store) -> Self {
        self.stores.insert(name.into(), s);
        self
    }
    pub fn trigger(mut self, name: &str, tr: Trigger) -> Self {
        self.triggers.insert(name.into(), tr);
        self
    }

    fn describe(&self) -> Value {
        let section = |m: Map<String, Value>| Value::Object(m);
        json!({
            "title": self.title,
            "abilities": section(self.abilities.iter().map(|(n, a)| (n.clone(), a.describe())).collect()),
            "verbs": section(self.verbs.iter().map(|(n, v)| (n.clone(), v.describe())).collect()),
            "stores": section(self.stores.iter().map(|(n, s)| (n.clone(), Value::Object(s.0.clone()))).collect()),
            "triggers": section(self.triggers.iter().map(|(n, tr)| {
                let mut d = tr.decl.clone();
                d.insert("has_run".into(), json!(tr.run.is_some()));
                (n.clone(), Value::Object(d))
            }).collect()),
        })
    }

    fn run(&self, cx: &Cx, entry: &str, name: &str, input: Value, output: Value) -> Result {
        let missing = |what: &str| Error::new("E0300", format!("{}.{name} has no {what}", cx.pack));
        match entry {
            "invoke" => (self.abilities.get(name).and_then(|a| a.run.as_ref()).ok_or_else(|| missing("run"))?)(input, cx),
            "undo" => match (self.abilities.get(name), self.verbs.get(name)) {
                (Some(a), _) if a.undo.is_some() => (a.undo.as_ref().expect("checked"))(input, output, cx),
                (_, Some(v)) if v.undo.is_some() => (v.undo.as_ref().expect("checked"))(input, output, cx),
                _ => Err(missing("undo")),
            },
            "verb" => (self.verbs.get(name).and_then(|v| v.run.as_ref()).ok_or_else(|| missing("verb run"))?)(input, cx),
            "trigger" => (self.triggers.get(name).and_then(|tr| tr.run.as_ref()).ok_or_else(|| missing("trigger run"))?)(input, cx),
            other => Err(Error::new("E0300", format!("no entry {other:?}"))),
        }
    }
}

// ---- The component -----------------------------------------------------------------------

struct Component;

/// A panic is a bug in the pack: its message goes to `echospot dev logs` before the call stops.
fn catch_panics() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        std::panic::set_hook(Box::new(|info| host::log("error", &format!("panicked: {info}"))));
    });
}

impl Guest for Component {
    fn describe() -> String {
        catch_panics();
        crate::pack().describe().to_string()
    }

    fn run(cx: Context, entry: String, name: String, input: String, output: String) -> std::result::Result<String, String> {
        catch_panics();
        let cx = Cx { pack: cx.pack, dry: cx.dry };
        let input = serde_json::from_str(&input).unwrap_or(Value::Null);
        let output = serde_json::from_str(&output).unwrap_or(Value::Null);
        match crate::pack().run(&cx, &entry, &name, input, output) {
            Ok(value) => Ok(value.to_string()),
            Err(e) => Err(e.json()),
        }
    }
}

bindings::export!(Component with_types_in bindings);
