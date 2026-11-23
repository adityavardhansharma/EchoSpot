//! What a pack's VM and the runtime both reach. Host functions run inside a VM entry, so they
//! only read caches here and queue requests; the runtime acts on the queue between entries.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::time::Instant;

use rune_luau::Completer;
use serde_json::{Map, Value, json};

use super::error::PackError;
use super::stores::Stores;
use super::types::Type;

pub enum Request {
    /// `cx:call`: a task in `pack` is waiting on `done`.
    Call { pack: String, token: u64, address: String, input: Value, done: Completer },
    /// A pack entry finished: an ability's `run`, an undo, a handler.
    Finish { pack: String, token: u64, ok: bool, value: Value },
    /// `cx:load` missed the cache.
    Load { pack: String, key: String, address: String, input: Value },
    /// `cx:push`, `cx:pop`, `cx:replace`, `cx:toast`, `cx:hud`.
    Nav { pack: String, token: u64, op: String, address: String, args: Value },
}

/// What `cx:load` hands a surface.
#[derive(Clone, Debug)]
pub struct Resource {
    /// `pending`, `ok` or `error`.
    pub state: &'static str,
    pub value: Value,
    pub error: Option<PackError>,
    /// When it settled, on the runtime's clock.
    pub at: f64,
    pub ty: Option<Type>,
    /// Seconds it stays fresh; zero is until something invalidates it.
    pub ttl: f64,
}

impl Resource {
    pub fn pending() -> Self {
        Self { state: "pending", value: Value::Null, error: None, at: 0.0, ty: None, ttl: 0.0 }
    }
    pub fn ok(value: Value, ty: Option<Type>) -> Self {
        Self { state: "ok", value, error: None, at: 0.0, ty, ttl: 0.0 }
    }
    pub fn json(&self) -> Value {
        json!({
            "_resource": true, "state": self.state, "value": self.value,
            "error": self.error.as_ref().map(|e| e.message.clone()),
            "type": self.ty.as_ref().and_then(|t| serde_json::to_value(t).ok()),
        })
    }
}

/// A fixture state: what abilities return and what stores hold while a scene is drawn or a
/// test runs.
#[derive(Clone, Debug, Default)]
pub struct Scene {
    pub name: String,
    pub load: Map<String, Value>,
    pub stores: Map<String, Value>,
    /// Addresses that fail, with the message.
    pub errors: Map<String, Value>,
    /// Addresses that never settle: the loading state.
    pub pending: HashSet<String>,
    pub args: Value,
    pub query: String,
    pub dark: Option<bool>,
    /// The surface's size in points, for a narrow tile.
    pub size: Option<(f64, f64)>,
}

impl Scene {
    /// Reads what a scene file returned: `{ load = { [address] = value }, stores = { … }, errors = { … }, pending = { … } }`.
    pub fn from_json(name: &str, v: &Value) -> Self {
        let map = |k: &str| v.get(k).and_then(Value::as_object).cloned().unwrap_or_default();
        Scene {
            name: name.into(), load: map("load"), stores: map("stores"), errors: map("errors"),
            pending: v.get("pending").and_then(Value::as_array).map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default(),
            args: v.get("args").cloned().unwrap_or(Value::Null), query: v.get("query").and_then(Value::as_str).unwrap_or("").into(),
            dark: v.get("dark").and_then(Value::as_bool),
            size: v.get("size").and_then(Value::as_array).and_then(|a| Some((a.first()?.as_f64()?, a.get(1)?.as_f64()?))),
        }
    }
}

/// What a pack may read without a call: decided from its manifest and grants when it loads.
#[derive(Clone, Debug, Default)]
pub struct Access {
    /// `calls` entries that were granted.
    pub calls: BTreeSet<String>,
    /// `read`, `write` or empty.
    pub clipboard: String,
    /// A pack that ships inside EchoSpot.
    pub system: bool,
}

#[derive(Clone, Debug)]
pub struct LogLine {
    pub t: f64,
    pub pack: String,
    /// `info`, `warn` or `error`.
    pub level: &'static str,
    pub text: String,
}

pub struct Shared {
    pub requests: RefCell<VecDeque<Request>>,
    /// By `<caller pack>><address>|<input>`.
    pub resources: RefCell<HashMap<String, Resource>>,
    pub stores: RefCell<Stores>,
    pub scene: RefCell<Option<Scene>>,
    /// Tokens of calls whose effects are being stubbed.
    pub dry: RefCell<HashSet<u64>>,
    /// The pack whose code runs under each live token.
    pub owners: RefCell<HashMap<u64, String>>,
    pub access: RefCell<HashMap<String, Access>>,
    pub logs: RefCell<VecDeque<LogLine>>,
    /// Where log lines are also written, for `echospot dev logs` in another process to read.
    pub log_file: RefCell<Option<std::path::PathBuf>>,
    /// Moves whenever something a surface drew from changed: a resource settled, a store was written.
    pub version: Cell<u64>,
    tokens: Cell<u64>,
    started: Instant,
}

impl Default for Shared {
    fn default() -> Self {
        Self {
            requests: Default::default(), resources: Default::default(), stores: Default::default(), scene: Default::default(),
            dry: Default::default(), owners: Default::default(), access: Default::default(), logs: Default::default(), log_file: Default::default(),
            version: Cell::new(1), tokens: Cell::new(0), started: Instant::now(),
        }
    }
}

impl Shared {
    pub fn token(&self) -> u64 {
        self.tokens.set(self.tokens.get() + 1);
        self.tokens.get()
    }
    /// Seconds since the runtime started: the clock tasks wait on.
    pub fn clock(&self) -> f64 {
        self.started.elapsed().as_secs_f64()
    }
    pub fn bump(&self) {
        self.version.set(self.version.get().wrapping_add(1));
    }
    pub fn push(&self, request: Request) {
        self.requests.borrow_mut().push_back(request);
    }
    pub fn take(&self) -> Option<Request> {
        self.requests.borrow_mut().pop_front()
    }
    pub fn log(&self, pack: &str, level: &'static str, text: impl Into<String>) {
        let mut logs = self.logs.borrow_mut();
        if logs.len() >= 2000 {
            logs.pop_front();
        }
        let line = LogLine { t: super::audit::now(), pack: pack.into(), level, text: text.into() };
        if let Some(path) = self.log_file.borrow().as_ref() {
            use std::io::Write;
            // Kept small: past a few megabytes the older half goes.
            if std::fs::metadata(path).is_ok_and(|m| m.len() > 4 << 20)
                && let Ok(old) = std::fs::read_to_string(path) {
                let lines: Vec<&str> = old.lines().collect();
                let _ = std::fs::write(path, lines[lines.len() / 2..].join("\n") + "\n");
            }
            if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                let _ = writeln!(file, "{}", json!({ "t": line.t, "pack": line.pack, "level": line.level, "text": line.text }));
            }
        }
        logs.push_back(line);
    }
    /// Drops what surfaces loaded from `pack`'s abilities, so they load again.
    pub fn invalidate(&self, pack: &str) {
        let mark = format!(">{pack}.");
        self.resources.borrow_mut().retain(|key, _| !key.contains(&mark));
        self.bump();
    }
    pub fn resource_key(pack: &str, address: &str, input: &Value) -> String {
        format!("{pack}>{address}|{}", super::json::canonical(input))
    }

    /// Whether `pack` may read store `key` (`<pack>.<name>`, or a built-in's bare name).
    pub fn may_read_store(&self, pack: &str, key: &str) -> Result<(), PackError> {
        if key.strip_prefix(pack).is_some_and(|rest| rest.starts_with('.')) {
            return Ok(());
        }
        let access = self.access.borrow();
        let Some(access) = access.get(pack) else { return Ok(()) };
        if access.system {
            return Ok(());
        }
        let learn = "echospot dev grants";
        if let Some((_, ability)) = super::stores::BUILTIN.iter().find(|(name, _)| *name == key) {
            let allowed = ability.is_empty() || if *name_is_clipboard(key) { access.clipboard == "read" } else { access.calls.contains(*ability) };
            return if allowed { Ok(()) } else {
                let need = if key == "clipboard" { "clipboard = \"read\"".to_string() } else { format!("calls = [\"{ability}\"]") };
                Err(PackError::new("E0400", format!("{pack} can't read the {key} store: it needs {need} in pack.toml, granted")).learn(learn))
            };
        }
        let (owner, name) = key.split_once('.').unwrap_or((key, ""));
        let address = format!("{owner}.stores.{name}");
        if access.calls.contains(&address) { Ok(()) } else {
            Err(PackError::new("E0401", format!("{pack} can't read {key}: add calls = [\"{address}\"] to pack.toml")).learn(learn))
        }
    }
}

fn name_is_clipboard(key: &str) -> &'static bool {
    if key == "clipboard" { &true } else { &false }
}
