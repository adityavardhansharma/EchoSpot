//! The runtime: the packs that are loaded, and the broker every ability call passes through.
//!
//! A call is checked (does the address exist, may this caller use it, does a read stay a read,
//! does the input fit), then run, then logged, and what would take it back is recorded. There is
//! no second path: a pack's `cx:call`, a surface's `cx:load`, the palette, the command line, the
//! AI and MCP all arrive at [`Runtime::start`].

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rune::{Tx, Waker};
use rune_luau::{Completer, Data, Script};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::audit;
use super::builtins::{self, Ctx, Done, Gate, Run, Spec};
use super::error::{PackError, Result};
use super::grants::Grants;
use super::index::{Ability, Index, Kind};
use super::json::{from_data, summary, to_data};
use super::manifest::{Ask, Manifest, Permissions};
use super::shared::{Access, Request, Resource, Scene, Shared};
use super::types::Type;
use super::vm;

/// A render may run this long before the watchdog stops it; `dev logs` warns past [`RENDER_WARN`].
pub const RENDER_LIMIT: Duration = Duration::from_millis(50);
pub const RENDER_WARN: Duration = Duration::from_millis(8);
/// A pack's share of one keystroke in root search.
pub const QUERY_LIMIT: Duration = Duration::from_millis(8);
/// How long an Undo is offered in a toast.
pub const UNDO_TOAST_SECS: f64 = 5.0;

/// Where a pack is in its life, and so what its rows show.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    /// `pack.toml` exists, the entry is missing: an agent is still writing it.
    Drafting,
    /// Compiling and reading declarations.
    Checking,
    /// Compiled; some of what it asks for hasn't been granted.
    NeedsGrant,
    Ready,
    /// The newest version failed. The last good one, if there was one, keeps running.
    Broken,
}

impl State {
    pub fn name(self) -> &'static str {
        match self {
            State::Drafting => "drafting",
            State::Checking => "checking",
            State::NeedsGrant => "needs-grant",
            State::Ready => "ready",
            State::Broken => "broken",
        }
    }
    /// What a row says beside the pack's name.
    pub fn label(self) -> &'static str {
        match self {
            State::Drafting => "Being built",
            State::Checking => "Checking",
            State::NeedsGrant => "Review permissions",
            State::Ready => "",
            State::Broken => "Broken",
        }
    }
}

pub struct Pack {
    pub id: String,
    pub dir: PathBuf,
    pub manifest: Manifest,
    pub state: State,
    /// Why it is broken, or what the newest version got wrong while the last good one runs.
    pub error: Option<PackError>,
    pub index: Index,
    /// The VM; started the first time something needs it.
    pub script: Option<Script>,
    /// Ships inside EchoSpot: its grants are implied.
    pub system: bool,
    /// What its files hashed to when the index was read.
    pub stamp: u64,
    /// How long the last load took.
    pub load_ms: f64,
    /// Whether what runs is older than what is on disk.
    pub stale: bool,
    /// A Rust pack's component (API-DESIGN.md §20), compiled to native code.
    pub wasm: Option<Arc<super::wasm::Module>>,
    /// What the component declares: these names run in it, not in Luau.
    pub wasm_index: Index,
    /// A build in progress: what a row says while it runs.
    pub building: Option<String>,
}

impl Pack {
    pub fn name(&self) -> &str {
        self.manifest.display_name()
    }
    pub fn ability(&self, name: &str) -> Option<&Ability> {
        self.index.abilities.get(name)
    }
    /// What a row says beside the pack's name: a build's progress, or its state.
    pub fn status(&self) -> String {
        match &self.building {
            Some(progress) if self.state != State::Ready => progress.clone(),
            _ => self.state.label().to_string(),
        }
    }
    /// Whether this entry (`__invoke`, `__verb`…) of `name` runs in the Rust component.
    fn in_wasm(&self, entry: &str, name: &str) -> bool {
        self.wasm.is_some() && match entry {
            "__invoke" => self.wasm_index.abilities.contains_key(name),
            "__verb" => self.wasm_index.verbs.contains_key(name),
            "__trigger" => self.wasm_index.triggers.contains_key(name),
            "__undo" => self.wasm_index.abilities.contains_key(name) || self.wasm_index.verbs.contains_key(name),
            _ => false,
        }
    }
}

/// Who started a call.
#[derive(Clone, Debug, PartialEq)]
pub enum Origin {
    /// A person: the palette, a hotkey, `echospot call`.
    User,
    /// EchoSpot's own AI chat.
    Ai,
    /// An outside agent over MCP.
    Mcp,
    /// A schedule or an event, on behalf of a pack.
    Trigger(String),
    /// Code running under another call.
    Frame(u64),
    /// A surface of this pack loading data: always a read.
    Load(String),
}

pub enum Responder {
    /// Keep the result for [`Runtime::take`].
    Keep,
    Luau(Completer),
    /// Settle the resource with this key.
    Resource(String),
    /// A worker thread is waiting: an AI tool loop calling an ability.
    Channel(Sender<Result<Value>>),
    Drop,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum UndoStep {
    /// Another call takes it back.
    Call { address: String, input: Value },
    /// The pack's own `undo(input, output, cx)`.
    Ability { pack: String, name: String, input: Value, output: Value, verb: bool },
}

/// One thing a person did that changed something: a row in Timeline.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Effect {
    pub id: u64,
    pub t: f64,
    pub address: String,
    pub title: String,
    pub summary: String,
    /// Run in reverse to take it back; empty when it can't be.
    pub steps: Vec<UndoStep>,
    pub undone: bool,
    #[serde(rename = "final")]
    pub is_final: bool,
}

struct Frame {
    /// The pack whose code runs under this token; empty for a built-in.
    pack: String,
    address: String,
    title: String,
    kind: Kind,
    /// Under a read: nothing below may be an effect.
    read_ctx: bool,
    /// The read that put this frame under `read_ctx`, for the error.
    read_by: String,
    dry: bool,
    parent: Option<u64>,
    respond: Option<Responder>,
    started: Instant,
    input: Value,
    steps: Vec<UndoStep>,
    usd: f64,
    /// Who the audit log says called: a pack, or `user`, `ai`, `mcp`.
    actor: String,
    /// Whether the audit log takes it.
    logged: bool,
    output: Option<Type>,
    /// A pack ability with its own `undo`.
    own_undo: Option<(String, bool)>,
    /// The pack a built-in is running for: what it calls, it calls with that pack's grants.
    on_behalf: String,
    /// An effect with no undo of its own: it may not call a final.
    no_undo: bool,
    /// A person started it, so it becomes a Timeline row.
    root_user: bool,
    spec_gate: Option<Gate>,
}

enum WorkerMsg {
    Done(u64, Result<Done>),
    Partial(u64, Value),
    /// A built-in's worker calls an ability under its own call: `ai.agent` using a tool.
    Call(u64, String, Value, Sender<Result<Value>>),
    /// A Rust pack's call writes one of its stores.
    Put(u64, String, Value, Sender<Result<()>>),
    /// A Rust pack's call printed or logged something.
    Log(String, &'static str, String),
    /// A Rust pack's build moved on.
    Building(PathBuf, String),
    /// A Rust pack's build finished.
    Built(PathBuf, Result<PathBuf>),
}

/// A final call waiting for a person's yes.
enum Pending {
    Ability { owner: String, name: String, input: Value },
    Spec { spec: Spec, ctx: Box<Ctx>, input: Value },
    /// `files.pick`: the person is choosing a path for this pack.
    Pick { pack: String },
}

/// When something that runs unasked last ran, and how it went.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Ran {
    pub t: f64,
    pub ok: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
}

/// Something the app should act on.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// `cx:push`, `cx:replace`, `cx:pop`.
    Nav { pack: String, op: String, address: String, args: Value },
    Toast { text: String, intent: String },
    Hud { text: String },
    /// An effect finished and can be taken back.
    Undoable { id: u64, title: String },
    /// A pack's state or declarations changed.
    PacksChanged,
    /// A final call waits for a yes: answer with [`Runtime::answer_confirm`].
    Confirm { token: u64, address: String, title: String, summary: String },
    /// A pack asked the person to pick a path: answer with [`Runtime::answer_pick`].
    Pick { token: u64, pack: String, title: String, folder: bool },
}

pub struct Runtime {
    pub shared: Rc<Shared>,
    pub packs: BTreeMap<String, Pack>,
    pub grants: Grants,
    frames: HashMap<u64, Frame>,
    done: HashMap<u64, Result<Value>>,
    worker_tx: Sender<WorkerMsg>,
    worker_rx: Receiver<WorkerMsg>,
    waker: Arc<Mutex<Option<Waker>>>,
    pub events: Vec<Event>,
    /// Effects a dry run stubbed, in order.
    pub dry_log: Vec<Value>,
    pub timeline: VecDeque<Effect>,
    /// Recorded HTTP responses, for a test.
    pub fixtures: Option<PathBuf>,
    pub record: bool,
    /// Paths the user picked for a pack this session.
    pub picked: HashMap<String, Vec<PathBuf>>,
    /// Stores nothing on disk: a test, `echospot dev`.
    pub ephemeral: bool,
    next_effect: u64,
    /// The stores version surfaces last saw, to notice writes.
    seen_stores: u64,
    /// Ask before a final call a person started. The app does; the command line and tests don't.
    pub confirm_finals: bool,
    pending: HashMap<u64, Pending>,
    /// Schedules and triggers by `<pack>.<name>`: when each last ran.
    pub runs: BTreeMap<String, Ran>,
    /// Calls a schedule or an event started, to note how each ended.
    unasked: HashMap<u64, String>,
    /// What each address's signature read as when last loaded, to notice a change.
    signatures: HashMap<String, String>,
    /// What each pack ability's input was when last loaded, to tell a change that breaks a caller.
    inputs: HashMap<String, Type>,
    /// Rust packs being built on a thread.
    builds: std::collections::HashSet<PathBuf>,
    /// Build Rust packs before `load` returns, telling this how it goes: the command line and
    /// tests. The app builds on a thread and shows the progress on the pack's row.
    pub build_inline: Option<Box<dyn Fn(&super::wasm::Progress)>>,
}

/// A pack with both tiers: what `init.luau` declares and what the Rust component does. A name
/// is one or the other, never both.
fn merge(mut luau: Index, rust: &Index) -> Result<Index> {
    fn take<T: Clone>(into: &mut BTreeMap<String, T>, from: &BTreeMap<String, T>, what: &str) -> Result<()> {
        for (name, item) in from {
            if into.insert(name.clone(), item.clone()).is_some() {
                return Err(PackError::new("E0202", format!("{what} {name:?} is declared in both init.luau and src/lib.rs; keep one")).learn("echospot dev docs rust"));
            }
        }
        Ok(())
    }
    take(&mut luau.abilities, &rust.abilities, "the ability")?;
    take(&mut luau.verbs, &rust.verbs, "the verb")?;
    take(&mut luau.stores, &rust.stores, "the store")?;
    take(&mut luau.triggers, &rust.triggers, "the trigger")?;
    if luau.title.is_empty() {
        luau.title = rust.title.clone();
    }
    Ok(luau)
}

/// Why a caller written for input `old` can't call with input `new`: a field it never had to give
/// is now required, or a field takes a type the old one's values aren't.
fn breaking(old: &Type, new: &Type) -> Option<String> {
    for (name, t) in &new.fields {
        let required = !t.optional && t.default.is_none();
        match old.fields.get(name) {
            None if required => return Some(format!("it now needs `{name}`")),
            Some(was) if !t.accepts(was) => return Some(format!("`{name}` is now {} where it was {}", t.label(), was.label())),
            Some(was) if required && (was.optional || was.default.is_some()) => return Some(format!("`{name}` can no longer be left out")),
            _ => {}
        }
    }
    None
}

fn timeline_path() -> PathBuf {
    super::state_dir().join("timeline.json")
}

/// Where pack prints, warnings and errors are kept for `echospot dev logs`.
pub fn log_path() -> PathBuf {
    super::state_dir().join("logs.jsonl")
}

fn runs_path() -> PathBuf {
    super::state_dir().join("triggers.json")
}

impl Runtime {
    pub fn new() -> Self {
        let (worker_tx, worker_rx) = channel();
        let timeline: VecDeque<Effect> = std::fs::read_to_string(timeline_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        let shared = Shared::default();
        *shared.log_file.borrow_mut() = Some(log_path());
        Self {
            shared: Rc::new(shared), packs: BTreeMap::new(), grants: Grants::load(), frames: HashMap::new(), done: HashMap::new(),
            worker_tx, worker_rx, waker: Arc::default(), events: Vec::new(), dry_log: Vec::new(),
            next_effect: timeline.iter().map(|e| e.id).max().unwrap_or(0) + 1, timeline,
            fixtures: None, record: false, picked: HashMap::new(), ephemeral: false, seen_stores: 0,
            confirm_finals: false, pending: HashMap::new(), unasked: HashMap::new(), signatures: HashMap::new(),
            builds: Default::default(), build_inline: None, inputs: HashMap::new(),
            runs: std::fs::read_to_string(runs_path()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default(),
        }
    }

    /// The runtime a command-line call runs in: the real grants, which it can read and never
    /// write, and no window to ask a person anything in.
    pub fn for_cli() -> Self {
        let mut rt = Self::new();
        rt.grants = Grants::load().read_only();
        rt.build_inline = Some(Box::new(|p| {
            use std::io::{IsTerminal, Write};
            if std::io::stderr().is_terminal() {
                // An empty progress means the build ended: the line is cleared.
                let label = if *p == super::wasm::Progress::default() { String::new() } else { p.label() };
                eprint!("\r\x1b[K{label}");
                let _ = std::io::stderr().flush();
            }
        }));
        rt
    }

    /// A runtime that leaves nothing behind: in-memory grants, no timeline file.
    pub fn ephemeral() -> Self {
        let mut rt = Self::new();
        rt.grants = Grants::in_memory();
        rt.timeline.clear();
        rt.runs.clear();
        *rt.shared.log_file.borrow_mut() = None;
        rt.ephemeral = true;
        rt.build_inline = Some(Box::new(|_| {}));
        rt
    }

    pub fn set_waker(&mut self, waker: Waker) {
        *self.waker.lock().unwrap_or_else(|e| e.into_inner()) = Some(waker.clone());
        for pack in self.packs.values() {
            if let Some(script) = &pack.script {
                script.set_waker(waker.clone());
            }
        }
    }

    fn wake(&self) {
        if let Some(waker) = self.waker.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            waker();
        }
    }

    // ---- Loading --------------------------------------------------------------------------

    /// Loads every pack folder under [`super::packs_dir`].
    pub fn load_all(&mut self) {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(super::packs_dir()).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.join("pack.toml").is_file()).collect();
        dirs.sort();
        let seen: Vec<String> = dirs.iter().filter_map(|d| self.load(d, false)).collect();
        let gone: Vec<String> = self.packs.iter().filter(|(id, p)| !p.system && !seen.contains(id)).map(|(id, _)| id.clone()).collect();
        for id in gone {
            self.unload(&id);
        }
        // What ships inside EchoSpot, unless a fork in the user's packs took its id.
        for dir in super::system::unpack() {
            let taken = dir.file_name().and_then(|n| n.to_str()).is_some_and(|id| seen.iter().any(|s| s == id));
            if !taken {
                self.load(&dir, false);
            }
        }
        self.check_dependencies();
    }

    /// Marks a pack broken when an address it names no longer exists, and warns the packs that
    /// depend on an ability whose signature changed.
    pub fn check_dependencies(&mut self) {
        let known: std::collections::HashSet<String> = self.addresses().into_iter().map(|a| a.0).collect();
        let mut signatures = HashMap::new();
        for (address, signature, _, _) in self.addresses() {
            signatures.insert(address, signature);
        }
        let changed: Vec<String> = signatures.iter().filter(|(a, s)| self.signatures.get(*a).is_some_and(|old| old != *s)).map(|(a, _)| a.clone()).collect();
        let inputs: HashMap<String, Type> = self.packs.values().flat_map(|p| p.index.abilities.iter().map(move |(n, a)| (format!("{}.{n}", p.id), a.input.clone()))).collect();
        let mut broken: Vec<(String, PackError)> = Vec::new();
        for pack in self.packs.values().filter(|p| matches!(p.state, State::Ready | State::NeedsGrant)) {
            for used in super::graph::uses(pack) {
                let exists = known.contains(&used.address) || self.shared.stores.borrow().exists(&used.address.replacen(".stores.", ".", 1)) || used.address.starts_with("raycast.");
                if !exists && used.literal {
                    let e = PackError::new("E0300", format!("{} calls {}, which no longer exists", pack.id, used.address))
                        .at(used.at.clone()).nearest(&used.address, known.iter().map(String::as_str)).learn(format!("echospot dev api search {}", used.address.replace('.', " ")));
                    broken.push((pack.id.clone(), e));
                    break;
                }
                if changed.contains(&used.address) {
                    // A change a caller written for the old input can't meet breaks it; anything else is a warning.
                    let breaks = match (self.inputs.get(&used.address), inputs.get(&used.address)) {
                        (Some(old), Some(new)) => breaking(old, new),
                        _ => None,
                    };
                    match breaks {
                        Some(why) if used.literal => {
                            let e = PackError::new("E0303", format!("{} changed in a way {} can't meet: {why}; it is now {}", used.address, pack.id, signatures[&used.address]))
                                .at(used.at.clone()).learn(format!("echospot dev api show {}", used.address));
                            broken.push((pack.id.clone(), e));
                            break;
                        }
                        _ => self.shared.log(&pack.id, "warn", format!("{} changed its signature to {}; {} uses it at {}", used.address, signatures[&used.address], pack.id, used.at)),
                    }
                }
            }
        }
        for (id, error) in broken {
            self.shared.log(&id, "error", error.to_string());
            if let Some(pack) = self.packs.get_mut(&id) {
                pack.state = State::Broken;
                pack.error = Some(error);
            }
        }
        self.signatures = signatures;
        self.inputs = inputs;
    }

    pub fn unload(&mut self, id: &str) {
        if self.packs.remove(id).is_some() {
            self.shared.stores.borrow_mut().retain(id, &[]);
            self.shared.access.borrow_mut().remove(id);
            self.shared.invalidate(id);
            self.events.push(Event::PacksChanged);
        }
    }

    /// What the grants and the manifest together allow, recorded where the VM's host functions
    /// can see it.
    fn settle_access(&mut self, id: &str) {
        let Some(pack) = self.packs.get(id) else { return };
        let perms = self.effective(pack);
        self.shared.access.borrow_mut().insert(id.to_string(), Access { calls: perms.calls.iter().cloned().collect(), clipboard: perms.clipboard.clone(), system: pack.system });
    }

    /// The permissions a pack both declared and was granted.
    pub fn effective(&self, pack: &Pack) -> Permissions {
        let p = &pack.manifest.permissions;
        if pack.system {
            return p.clone();
        }
        let has = |ask: Ask| self.grants.has(&pack.id, &ask);
        Permissions {
            net: p.net.iter().filter(|h| has(Ask::new("net", h.to_lowercase()))).cloned().collect(),
            secrets: p.secrets.iter().filter(|(name, hosts)| has(Ask::new("secret", format!("{name} → {}", hosts.join(", "))))).map(|(k, v)| (k.clone(), v.clone())).collect(),
            clipboard: if has(Ask::new("clipboard", &p.clipboard)) { p.clipboard.clone() } else { String::new() },
            files: p.files.iter().filter(|g| has(Ask::new("files", g.as_str()))).cloned().collect(),
            run: p.run.iter().filter(|r| has(Ask::new("run", std::iter::once(r.bin.clone()).chain(r.args.iter().cloned()).collect::<Vec<_>>().join(" ")))).cloned().collect(),
            ai: p.ai.clone().filter(|ai| has(Ask::new("ai", format!("{:.2}", ai.budget_usd_month)))),
            calls: p.calls.iter().filter(|c| has(Ask::new("calls", c.as_str()))).cloned().collect(),
            background: p.background && has(Ask::new("background", "on")),
        }
    }

    /// Checks what a description declares against the pack it came from.
    fn check_index(manifest: &Manifest, index: &Index) -> Result<()> {
        if index.runs_in_background() && !manifest.permissions.background {
            return Err(PackError::new("E0202", "the pack has schedules or event triggers, so pack.toml needs background = true under [permissions]").at("pack.toml").learn("echospot dev docs triggers"));
        }
        Ok(())
    }

    /// Loads or reloads the pack in `dir`. A version that fails leaves the last good one running.
    /// With `eager` the VM starts now; otherwise a cached index is enough until something runs.
    pub fn load(&mut self, dir: &Path, eager: bool) -> Option<String> {
        let started = Instant::now();
        let manifest = match Manifest::load(dir) {
            Ok(m) => m,
            Err(error) => {
                // No valid identity: the folder's name stands in so the failure has a row.
                let id = dir.file_name()?.to_string_lossy().to_string();
                match self.packs.get_mut(&id) {
                    Some(pack) => {
                        pack.state = State::Broken;
                        pack.error = Some(error);
                    }
                    None => {
                        let manifest = Manifest { id: id.clone(), ..Default::default() };
                        self.packs.insert(id.clone(), Pack { id: id.clone(), dir: dir.into(), manifest, state: State::Broken, error: Some(error), index: Index::default(), script: None, system: false, stamp: 0, load_ms: 0.0, stale: false, wasm: None, wasm_index: Index::default(), building: None });
                    }
                }
                self.events.push(Event::PacksChanged);
                return Some(id);
            }
        };
        let id = manifest.id.clone();
        // Only what EchoSpot itself unpacked is a system pack. A fork is the user's: it asks for its grants.
        let system = dir.starts_with(super::system::dir());
        let rust = super::wasm::is_rust(dir);
        let luau = dir.join("init.luau").is_file();
        // The Rust tier: the component, built if its sources are newer than it.
        let mut component = None;
        if rust {
            match self.component(&id, dir, &manifest, system) {
                Ok(Some(found)) => component = Some(found),
                // A build is running on a thread; the pack loads again when it ends.
                Ok(None) => return Some(id),
                Err(error) => {
                    self.fail(&id, dir, manifest, error, system);
                    return Some(id);
                }
            }
        }
        if !luau && !rust {
            self.packs.insert(id.clone(), Pack { id: id.clone(), dir: dir.into(), manifest, state: State::Drafting, error: None, index: Index::default(), script: None, system, stamp: 0, load_ms: 0.0, stale: false, wasm: None, wasm_index: Index::default(), building: None });
            self.events.push(Event::PacksChanged);
            return Some(id);
        }
        let read = vm::sources(dir).map_err(|e| PackError::new("E0200", format!("can't read the pack's files: {e}")).at(dir.display().to_string()));
        let (sources, mut stamp) = match read {
            Ok(x) => x,
            Err(error) => {
                self.fail(&id, dir, manifest, error, system);
                return Some(id);
            }
        };
        if let Some((_, _, rust_stamp)) = &component {
            stamp = stamp.wrapping_mul(1_099_511_628_211) ^ rust_stamp;
        }
        if self.packs.get(&id).is_some_and(|p| p.stamp == stamp && p.error.is_none() && p.dir == dir && (p.script.is_some() || !eager || !luau)) {
            // Nothing changed on disk; the grants may have.
            self.settle_state(&id);
            return Some(id);
        }
        // The index cache: a pack that hasn't changed declares what it did, with no VM started.
        if !eager && !rust && !self.packs.contains_key(&id)
            && let Some(index) = super::cache::read(&id, stamp) {
            self.install(id.clone(), dir, manifest, index, None, None, system, stamp, started);
            return Some(id);
        }
        // A scratch VM reads the new version first, so a mistake never replaces what is running.
        let luau_index = if luau {
            let probe = vm::start(&id, sources.clone(), &Rc::new(Shared::default()));
            let described = vm::describe(&probe).and_then(|v| Index::from_describe(&v));
            drop(probe);
            described
        } else {
            Ok(Index::default())
        };
        let described = luau_index
            .and_then(|index| match &component {
                Some((_, wasm_index, _)) => merge(index, wasm_index),
                None => Ok(index),
            })
            .and_then(|index| Self::check_index(&manifest, &index).map(|()| index));
        let index = match described {
            Ok(index) => index,
            Err(error) => {
                self.fail(&id, dir, manifest, error, system);
                return Some(id);
            }
        };
        let script = match self.packs.get_mut(&id).and_then(|p| p.script.take()).filter(|_| luau) {
            Some(script) => match script.reload(sources) {
                Ok(_) => Some(script),
                Err(e) => {
                    self.packs.get_mut(&id).expect("just read").script = Some(script);
                    self.fail(&id, dir, manifest, vm::script_error(&e), system);
                    return Some(id);
                }
            },
            None if eager && luau => Some(self.spawn_vm(&id, sources)),
            None => None,
        };
        if !self.ephemeral && !rust {
            super::cache::write(&id, stamp, &index);
        }
        self.install(id.clone(), dir, manifest, index, script, component.map(|(m, i, _)| (m, i)), system, stamp, started);
        Some(id)
    }

    /// A Rust pack's component and what it declares, or `None` while a build runs on a thread.
    fn component(&mut self, id: &str, dir: &Path, manifest: &Manifest, system: bool) -> Result<Option<(Arc<super::wasm::Module>, Index, u64)>> {
        use super::wasm;
        if !wasm::ENABLED {
            return Err(PackError::new("E0207", "this EchoSpot was built without Rust packs (the wasm feature)"));
        }
        // What the pack may call, as Rust functions, before the build reads them.
        wasm::write_calls(dir, &wasm::calls_rs(manifest, &self.specs()));
        if !wasm::up_to_date(dir) {
            if let Some(report) = &self.build_inline {
                let started = Instant::now();
                wasm::build(dir, &|p| report(p))?;
                report(&wasm::Progress::default());
                self.shared.log(id, "info", format!("built in {:.1} s", started.elapsed().as_secs_f64()));
            } else {
                self.start_build(id, dir, manifest, system);
                return Ok(None);
            }
        }
        let module = wasm::Module::load(&wasm::artifact(dir)?)?;
        let index = wasm::index(&module.describe()?)?;
        Ok(Some((module, index, wasm::stamp(dir))))
    }

    /// Builds a Rust pack on a thread. Until it ends the last good version runs, or the pack's
    /// row shows the build's progress.
    fn start_build(&mut self, id: &str, dir: &Path, manifest: &Manifest, system: bool) {
        if !self.builds.insert(dir.to_path_buf()) {
            return;
        }
        match self.packs.get_mut(id) {
            Some(pack) => pack.building = Some("Building".into()),
            None => {
                self.packs.insert(id.to_string(), Pack {
                    id: id.into(), dir: dir.into(), manifest: manifest.clone(), state: State::Checking, error: None, index: Index::default(), script: None, system,
                    stamp: 0, load_ms: 0.0, stale: false, wasm: None, wasm_index: Index::default(), building: Some("Building".into()),
                });
            }
        }
        self.events.push(Event::PacksChanged);
        let (tx, waker, dir) = (self.worker_tx.clone(), self.waker.clone(), dir.to_path_buf());
        let wake = move || {
            if let Some(waker) = waker.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                waker();
            }
        };
        let _ = std::thread::Builder::new().name("echospot-pack-build".into()).spawn(move || {
            let result = super::wasm::build(&dir, &|p| {
                let _ = tx.send(WorkerMsg::Building(dir.clone(), p.label()));
                wake();
            });
            let _ = tx.send(WorkerMsg::Built(dir, result));
            wake();
        });
    }

    fn spawn_vm(&self, id: &str, sources: rune_luau::Sources) -> Script {
        let script = vm::start(id, sources, &self.shared);
        if let Some(waker) = self.waker.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            script.set_waker(waker);
        }
        script
    }

    #[allow(clippy::too_many_arguments)]
    fn install(&mut self, id: String, dir: &Path, manifest: Manifest, index: Index, script: Option<Script>, component: Option<(Arc<super::wasm::Module>, Index)>, system: bool, stamp: u64, started: Instant) {
        let (wasm, wasm_index) = match component {
            Some((m, i)) => (Some(m), i),
            None => (None, Index::default()),
        };
        {
            let mut stores = self.shared.stores.borrow_mut();
            for (name, decl) in &index.stores {
                let mut decl = decl.clone();
                decl.persist &= !self.ephemeral;
                stores.declare(&id, name, &decl);
            }
            stores.retain(&id, &index.stores.keys().collect::<Vec<_>>());
        }
        let load_ms = started.elapsed().as_secs_f64() * 1e3;
        self.packs.insert(id.clone(), Pack { id: id.clone(), dir: dir.into(), manifest, state: State::Ready, error: None, index, script, system, stamp, load_ms, stale: false, wasm, wasm_index, building: None });
        self.settle_state(&id);
        self.shared.invalidate(&id);
        self.shared.log(&id, "info", format!("loaded in {load_ms:.1} ms"));
        self.events.push(Event::PacksChanged);
    }

    fn fail(&mut self, id: &str, dir: &Path, manifest: Manifest, error: PackError, system: bool) {
        self.shared.log(id, "error", error.to_string());
        match self.packs.get_mut(id) {
            // The last good version keeps running, with its own manifest and declarations.
            Some(pack) if pack.state != State::Drafting => {
                pack.state = State::Broken;
                pack.error = Some(error);
                pack.stale = true;
            }
            _ => {
                self.packs.insert(id.to_string(), Pack { id: id.into(), dir: dir.into(), manifest, state: State::Broken, error: Some(error), index: Index::default(), script: None, system, stamp: 0, load_ms: 0.0, stale: false, wasm: None, wasm_index: Index::default(), building: None });
            }
        }
        self.events.push(Event::PacksChanged);
    }

    /// `ready` or `needs-grant`, from what is granted now.
    pub fn settle_state(&mut self, id: &str) {
        let Some(pack) = self.packs.get(id) else { return };
        if matches!(pack.state, State::Ready | State::NeedsGrant) {
            let missing = !pack.system && !self.grants.missing(id, &pack.manifest.permissions).is_empty();
            let state = if missing { State::NeedsGrant } else { State::Ready };
            self.packs.get_mut(id).expect("just read").state = state;
        }
        self.settle_access(id);
    }

    /// Records a person's yes to everything the pack asks for. Only EchoSpot's own window calls
    /// this, from a key or a click there.
    pub fn grant_all(&mut self, id: &str) {
        let Some(pack) = self.packs.get(id) else { return };
        let asks = pack.manifest.permissions.asks();
        self.grants.grant(id, asks);
        self.settle_state(id);
        self.shared.invalidate(id);
        self.events.push(Event::PacksChanged);
    }

    pub fn revoke(&mut self, id: &str, ask: Option<&Ask>) {
        match ask {
            Some(ask) => self.grants.revoke(id, ask),
            None => self.grants.revoke_all(id),
        }
        self.settle_state(id);
        self.shared.invalidate(id);
        self.events.push(Event::PacksChanged);
    }

    /// Starts the pack's VM if it isn't running.
    pub fn ensure_vm(&mut self, id: &str) -> Result<()> {
        let Some(pack) = self.packs.get(id) else { return Err(PackError::new("E0300", format!("no pack {id:?}"))) };
        if pack.script.is_some() {
            return Ok(());
        }
        if pack.state == State::Drafting {
            return Err(PackError::new("E0203", format!("{id} is still being built: it has no init.luau yet")));
        }
        if !pack.dir.join("init.luau").is_file() {
            let why = if pack.building.is_some() { "is still being built" } else { "is a Rust pack with no init.luau, so it has no Luau to run" };
            return Err(PackError::new("E0203", format!("{id} {why}")).learn("echospot dev docs rust"));
        }
        if pack.index.abilities.is_empty() && pack.index.surfaces.is_empty() && pack.error.is_some() {
            return Err(pack.error.clone().expect("checked"));
        }
        let (sources, _) = vm::sources(&pack.dir).map_err(|e| PackError::new("E0200", format!("can't read the pack's files: {e}")))?;
        let script = self.spawn_vm(id, sources);
        if let Some(e) = script.error() {
            return Err(vm::script_error(&e));
        }
        self.packs.get_mut(id).expect("just read").script = Some(script);
        Ok(())
    }

    // ---- Addresses ------------------------------------------------------------------------

    /// Every address there is, with its signature: pack abilities, then built-ins.
    pub fn addresses(&self) -> Vec<(String, String, Kind, String)> {
        let mut out: Vec<(String, String, Kind, String)> = self.packs.values().flat_map(|p| {
            p.index.abilities.iter().filter(|(_, a)| a.has_run).map(move |(name, a)| {
                let input = a.input.fields.iter().map(|(k, t)| format!("{k}: {}", t.label())).collect::<Vec<_>>().join(", ");
                (format!("{}.{name}", p.id), format!("({input}) → {}", a.output.as_ref().map(|t| t.label()).unwrap_or_else(|| "any".into())), a.kind, a.title.clone())
            })
        }).collect();
        out.extend(self.specs().into_iter().map(|s| {
            let sig = s.signature();
            (s.address.clone(), sig[s.address.len()..].to_string(), s.kind, s.title)
        }));
        out
    }

    /// Every built-in, the ones made from the user's scripts, snippets and quicklinks included.
    pub fn specs(&self) -> Vec<Spec> {
        let mut specs = builtins::all();
        specs.extend(builtins::dynamic());
        specs
    }

    fn spec(&self, address: &str) -> Option<Spec> {
        builtins::find(address).or_else(|| builtins::find_dynamic(address))
    }

    fn unknown(&self, address: &str) -> PackError {
        let known: Vec<String> = self.addresses().into_iter().map(|a| a.0).collect();
        PackError::new("E0300", format!("no ability at {address:?}")).nearest(address, known.iter().map(String::as_str)).learn(format!("echospot dev api search {}", address.replace('.', " ")))
    }

    // ---- The broker -----------------------------------------------------------------------

    /// Starts a call and returns its token. The result arrives through `respond`; a call that
    /// can't start answers with its error at once.
    pub fn start(&mut self, origin: Origin, address: &str, input: Value, respond: Responder, dry: bool) -> u64 {
        let token = self.shared.token();
        if let Err(error) = self.begin(token, &origin, address, input, respond, dry) {
            // `begin` puts the frame in before anything can fail past the checks, so the
            // responder is still there to answer.
            match self.frames.contains_key(&token) {
                true => self.finish(token, Err(error)),
                false => self.shared.log("", "error", format!("{address}: {error}")),
            }
        }
        token
    }

    fn begin(&mut self, token: u64, origin: &Origin, address: &str, input: Value, respond: Responder, dry: bool) -> Result<()> {
        // Who is asking, and what they carry down.
        let (caller, parent, read_ctx, read_by, mut dry, actor, parent_no_undo) = match origin {
            Origin::Frame(parent) => match self.frames.get(parent) {
                Some(f) => {
                    // A built-in's own calls are made for the pack that called it, never as the user.
                    let pack = if f.pack.is_empty() { f.on_behalf.clone() } else { f.pack.clone() };
                    let host_only = pack.is_empty() && f.spec_gate.is_some() && f.actor != "user";
                    if host_only {
                        return Err(PackError::new("E0400", format!("{} can't call abilities for {}", f.address, f.actor)));
                    }
                    (Some(pack.clone()), Some(*parent), f.read_ctx, f.read_by.clone(), f.dry || dry, if pack.is_empty() { f.actor.clone() } else { pack }, f.no_undo.then(|| f.address.clone()))
                }
                None => (None, None, false, String::new(), dry, "user".into(), None),
            },
            // With no pack, the host is drawing for the person: a board tile, a default face.
            Origin::Load(pack) if pack.is_empty() => (None, None, true, "a board tile or a default face".to_string(), dry, "user".into(), None),
            Origin::Load(pack) => (Some(pack.clone()), None, true, format!("a surface of {pack}"), dry, pack.clone(), None),
            Origin::Trigger(pack) => (Some(pack.clone()), None, false, String::new(), dry, pack.clone(), None),
            Origin::User => (None, None, false, String::new(), dry, "user".into(), None),
            Origin::Ai => (None, None, false, String::new(), dry, "ai".into(), None),
            Origin::Mcp => (None, None, false, String::new(), dry, "mcp".into(), None),
        };
        dry |= self.shared.scene.borrow().is_some();
        let root_user = matches!(origin, Origin::User) || parent.and_then(|p| self.frames.get(&p)).is_some_and(|f| f.root_user && f.parent.is_none() && f.address.starts_with('@'));
        let mut frame = Frame {
            pack: String::new(), address: address.to_string(), title: address.to_string(), kind: Kind::Read, read_ctx, read_by, dry, parent, respond: Some(respond),
            started: Instant::now(), input: Value::Null, steps: Vec::new(), usd: 0.0, actor, logged: false, output: None, own_undo: None, no_undo: false,
            root_user, spec_gate: None, on_behalf: String::new(),
        };
        // A scene's fixture answers before anything real is reached.
        let fixture = self.shared.scene.borrow().as_ref().and_then(|s| s.load.get(address).cloned().map(Ok).or_else(|| s.errors.get(address).map(|m| Err(m.as_str().unwrap_or("failed").to_string()))));
        if let Some(fixture) = fixture {
            self.frames.insert(token, frame);
            self.finish(token, fixture.map(|value| Done { value, ..Default::default() }).map_err(|m| PackError::new("E0203", m)));
            return Ok(());
        }

        // `<pack>.stores.<name>`: another pack's store, read as an ability.
        let mut parts = address.splitn(3, '.');
        let (ns, second, third) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next());
        let store_key = (second == "stores").then_some(third).flatten().map(|name| format!("{ns}.{name}")).filter(|key| self.shared.stores.borrow().exists(key));
        let ability = self.packs.get(ns).filter(|_| third.is_none()).and_then(|p| p.ability(second).cloned().map(|a| (p.id.clone(), a)));
        let spec = if ability.is_none() && store_key.is_none() { self.spec(address) } else { None };
        let kind = match (&ability, &spec) {
            (Some((_, a)), _) => a.kind,
            (_, Some(s)) => s.kind,
            _ => Kind::Read,
        };
        frame.kind = kind;
        self.frames.insert(token, frame);
        if ability.is_none() && spec.is_none() && store_key.is_none() {
            return Err(self.unknown(address));
        }

        // A read stays a read, all the way down.
        if read_ctx && kind != Kind::Read {
            let by = &self.frames[&token].read_by;
            return Err(PackError::new("E0310", format!("{by} is a read, so it can't call {address}, which is {} {}", if kind == Kind::Effect { "an" } else { "a" }, kind.name()))
                .learn("echospot dev docs kinds"));
        }
        if kind == Kind::Final && let Some(effect) = &parent_no_undo {
            return Err(PackError::new("E0311", format!("{effect} is an effect with no undo, so it can't call {address}, which is final; give {effect} an undo or declare it kind = \"final\"")).learn("echospot dev docs kinds"));
        }
        // An effect with no undo of its own may call only effects that have one, or that leave
        // nothing lasting (§6.2): otherwise what it did couldn't be taken back.
        if kind == Kind::Effect && let Some(effect) = &parent_no_undo {
            let covered = match (&ability, &spec) {
                (Some((_, a)), _) => a.has_undo,
                (_, Some(s)) => builtins::effect_is_covered(&s.address),
                _ => true,
            };
            if !covered {
                return Err(PackError::new("E0311", format!("{effect} is an effect with no undo, so it can call only effects that can be undone; {address} can't be. Give {effect} an undo, or declare it kind = \"final\"")).learn("echospot dev docs kinds"));
            }
        }
        // AI and MCP get every read, and only the effects a pack marked for them.
        if matches!(origin, Origin::Ai | Origin::Mcp) {
            let allowed = match (&ability, &spec) {
                (Some((_, a)), _) => a.kind == Kind::Read || a.ai,
                (_, Some(s)) => matches!(s.gate, Gate::Free | Gate::Calls | Gate::Clipboard(_)) && (s.kind == Kind::Read || s.gate == Gate::Free),
                _ => true,
            };
            if !allowed {
                return Err(PackError::new("E0400", format!("{address} isn't open to AI tools: reads are, and effects marked ai = true")).learn("echospot dev docs ai"));
            }
        }

        // May this pack call it? The callee's own grants are checked when it acts.
        let caller_pack = caller.as_deref().filter(|c| !c.is_empty());
        let mut ctx = Ctx { user: caller_pack.is_none(), fixtures: self.fixtures.clone(), record: self.record, ..Default::default() };
        if let Some(caller) = caller_pack {
            let Some(pack) = self.packs.get(caller) else { return Err(PackError::new("E0300", format!("no pack {caller:?}"))) };
            let perms = self.effective(pack);
            let declared = &pack.manifest.permissions;
            let learn = "echospot dev grants";
            let need_call = |what: &str| -> Result<()> {
                if pack.system || perms.calls.iter().any(|c| c == what) {
                    Ok(())
                } else if declared.calls.iter().any(|c| c == what) {
                    Err(PackError::new("E0400", format!("{caller} declared {what} but it hasn't been granted yet: open EchoSpot and choose Review permissions")).learn(learn))
                } else {
                    Err(PackError::new("E0401", format!("{caller} can't call {what}: add it to calls in pack.toml, calls = [\"{what}\"]")).learn(learn))
                }
            };
            if let Some((owner, _)) = &ability {
                if owner != caller {
                    need_call(address)?;
                }
            } else if store_key.is_some() {
                if ns != caller {
                    need_call(address)?;
                }
            } else if let Some(spec) = &spec {
                match spec.gate {
                    Gate::Free | Gate::Net | Gate::Run | Gate::Files | Gate::Ai => {}
                    Gate::Calls => need_call(address)?,
                    Gate::Clipboard(level) => {
                        let ok = pack.system || perms.clipboard == "read" || perms.clipboard == level;
                        if !ok {
                            let state = if declared.clipboard == "read" || declared.clipboard == level { "declared but not granted yet" } else { "not declared" };
                            return Err(PackError::new("E0400", format!("{address} needs clipboard = \"{level}\" in pack.toml ({state})")).learn(learn));
                        }
                    }
                }
            }
            ctx.pack = caller.to_string();
            ctx.data_dir = super::data_dir(caller);
            // What the user picked for this pack, this session and before.
            ctx.picked = self.picked.get(caller).cloned().unwrap_or_default();
            ctx.picked.extend(self.grants.held(caller).into_iter().filter(|a| a.kind == "picked").map(|a| PathBuf::from(a.what)));
            ctx.budget_left = perms.ai.as_ref().map_or(0.0, |ai| ai.budget_usd_month - audit::spend_this_month(caller));
            ctx.perms = perms;
        }

        if let Some(key) = store_key {
            let value = self.shared.stores.borrow().get(&key, input.get("at").and_then(Value::as_f64));
            self.finish(token, Done::value(value));
            return Ok(());
        }

        if let Some((owner, a)) = ability {
            let input = a.input.coerce(input).map_err(|e| PackError::new("E0301", format!("{address}: {e}")).learn(format!("echospot dev api show {address}")))?;
            let frame = self.frames.get_mut(&token).expect("inserted above");
            frame.pack = owner.clone();
            frame.title = a.title.clone();
            frame.input = input.clone();
            frame.output = a.output.clone();
            frame.read_ctx |= kind == Kind::Read;
            if kind == Kind::Read && frame.read_by.is_empty() {
                frame.read_by = address.to_string();
            }
            frame.own_undo = a.has_undo.then(|| (second.to_string(), false));
            frame.no_undo = kind == Kind::Effect && !a.has_undo;
            // A read a pack makes of itself, or a person makes, isn't worth a log line.
            frame.logged = kind != Kind::Read || caller_pack.is_some_and(|c| c != owner);
            if frame.dry {
                self.shared.dry.borrow_mut().insert(token);
            }
            if !a.has_run {
                return Err(PackError::new("E0300", format!("{address} has no run function: it only draws a panel")));
            }
            if a.confirm && self.must_ask(token, address, kind) {
                self.ask(token, address, Pending::Ability { owner, name: second.to_string(), input });
                return Ok(());
            }
            return self.enter(&owner, token, "__invoke", vec![Data::String(second.to_string()), to_data(&input)]);
        }

        let spec = spec.expect("one of the three");
        let input = spec.input.coerce(input).map_err(|e| PackError::new("E0301", format!("{address}: {e}")).learn(format!("echospot dev api show {address}")))?;
        {
            let frame = self.frames.get_mut(&token).expect("inserted above");
            frame.title = spec.title.clone();
            frame.input = input.clone();
            frame.logged = !(kind == Kind::Read && spec.gate == Gate::Free);
            frame.spec_gate = Some(spec.gate);
            frame.on_behalf = caller_pack.unwrap_or("").to_string();
        }
        // What a worker uses to call an ability under this call: an AI tool loop.
        {
            let (tx, waker) = (self.worker_tx.clone(), self.waker.clone());
            ctx.caller = Some(builtins::Caller(Arc::new(move |address: &str, input: Value| {
                let (reply, answer) = channel();
                tx.send(WorkerMsg::Call(token, address.to_string(), input, reply)).map_err(|_| PackError::new("E0900", "EchoSpot is shutting down"))?;
                if let Some(waker) = waker.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                    waker();
                }
                answer.recv_timeout(Duration::from_secs(120)).unwrap_or_else(|_| Err(PackError::new("E0900", format!("{address} gave no answer in 120 s"))))
            })));
        }
        // A dry run stops here for anything that would change the world.
        if self.frames[&token].dry && kind != Kind::Read {
            self.dry_log.push(json!({ "address": address, "kind": kind.name(), "input": input, "by": self.frames[&token].actor }));
            self.finish(token, Done::value(Value::Null));
            return Ok(());
        }
        // Timeline is the runtime's own: what was done, and taking it back.
        match address {
            "timeline.list" => {
                let limit = input.get("limit").and_then(Value::as_u64).unwrap_or(50) as usize;
                let rows: Vec<Value> = self.timeline.iter().rev().take(limit).map(|e| {
                    let mut row = serde_json::to_value(e).unwrap_or(Value::Null);
                    row["can_undo"] = json!(!e.undone && !e.is_final && !e.steps.is_empty());
                    row
                }).collect();
                self.finish(token, Done::value(Value::Array(rows)));
                return Ok(());
            }
            "timeline.undo" => {
                let id = input.get("id").and_then(Value::as_u64).unwrap_or(0);
                let result = self.undo(id).map(|_| Done { value: json!(true), ..Default::default() });
                self.finish(token, result);
                return Ok(());
            }
            "files.pick" if self.confirm_finals => {
                let pack = self.frames[&token].on_behalf.clone();
                let title = Some(input.get("title").and_then(Value::as_str).unwrap_or("")).filter(|t| !t.is_empty()).unwrap_or("Choose a File").to_string();
                self.events.push(Event::Pick { token, pack: pack.clone(), title, folder: input.get("folder").and_then(Value::as_bool).unwrap_or(false) });
                self.pending.insert(token, Pending::Pick { pack });
                return Ok(());
            }
            "echo.describe" => {
                let wanted = input.get("address").and_then(Value::as_str).unwrap_or("").to_string();
                let result = super::api::describe(self, &wanted).ok_or_else(|| self.unknown(&wanted)).map(|value| Done { value, ..Default::default() });
                self.finish(token, result);
                return Ok(());
            }
            "echo.search" => {
                let limit = input.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;
                let found: Vec<Value> = super::api::search(self, input.get("query").and_then(Value::as_str).unwrap_or("")).into_iter().filter(|hit| hit["what"] == "ability").take(limit).collect();
                self.finish(token, Done::value(Value::Array(found)));
                return Ok(());
            }
            _ => {}
        }
        if self.must_ask(token, address, kind) {
            self.ask(token, address, Pending::Spec { spec, ctx: Box::new(ctx), input });
            return Ok(());
        }
        self.run_spec(token, &spec, &ctx, &input)
    }

    /// Whether a final call must wait for a person: one a person's action led to, in the app.
    fn must_ask(&self, token: u64, address: &str, kind: Kind) -> bool {
        if !self.confirm_finals || kind != Kind::Final || self.grants.no_confirm.contains(address) {
            return false;
        }
        let mut at = token;
        loop {
            let Some(frame) = self.frames.get(&at) else { return false };
            if frame.dry {
                return false;
            }
            match frame.parent {
                Some(parent) => at = parent,
                None => return frame.root_user || frame.address.starts_with('@'),
            }
        }
    }

    fn ask(&mut self, token: u64, address: &str, pending: Pending) {
        let frame = &self.frames[&token];
        self.events.push(Event::Confirm { token, address: address.to_string(), title: frame.title.clone(), summary: summary(&frame.input, 160) });
        self.pending.insert(token, pending);
    }

    /// A person's answer to [`Event::Confirm`]. `never_again` stops asking for this address.
    pub fn answer_confirm(&mut self, token: u64, yes: bool, never_again: bool) {
        let Some(pending) = self.pending.remove(&token) else { return };
        if !yes {
            return self.finish(token, Err(PackError::new("E0900", "cancelled: it wasn't confirmed")));
        }
        if never_again && let Some(address) = self.frames.get(&token).map(|f| f.address.clone()) {
            self.grants.set_confirm(&address, false);
        }
        let started = match pending {
            Pending::Pick { .. } => Ok(()),
            Pending::Ability { owner, name, input } => self.enter(&owner, token, "__invoke", vec![Data::String(name), to_data(&input)]),
            Pending::Spec { spec, ctx, input } => self.run_spec(token, &spec, &ctx, &input),
        };
        if let Err(e) = started {
            self.finish(token, Err(e));
        }
    }

    /// What the person picked for [`Event::Pick`], or nothing. The choice is the permission:
    /// that path, for that pack, from now on.
    pub fn answer_pick(&mut self, token: u64, path: Option<PathBuf>) {
        let Some(Pending::Pick { pack }) = self.pending.remove(&token) else { return };
        let Some(path) = path.and_then(|p| p.canonicalize().ok()) else { return self.finish(token, Done::value(Value::Null)) };
        if !pack.is_empty() {
            self.picked.entry(pack.clone()).or_default().push(path.clone());
            self.grants.grant(&pack, [Ask::new("picked", path.display().to_string())]);
        }
        self.finish(token, Done::value(Value::String(path.display().to_string())));
    }

    fn run_spec(&mut self, token: u64, spec: &Spec, ctx: &Ctx, input: &Value) -> Result<()> {
        let ctx = ctx.clone();
        match (spec.run)(&ctx, input) {
            Run::Now(result) => self.finish(token, result),
            Run::Later(job) => {
                let (tx, partial, waker, wake_partial) = (self.worker_tx.clone(), self.worker_tx.clone(), self.waker.clone(), self.waker.clone());
                let wake = |w: &Arc<Mutex<Option<Waker>>>| {
                    if let Some(waker) = w.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
                        waker();
                    }
                };
                let spawned = std::thread::Builder::new().name("echospot-pack-call".into()).spawn(move || {
                    let emit = move |value: Value| {
                        let _ = partial.send(WorkerMsg::Partial(token, value));
                        wake(&wake_partial);
                    };
                    let result = job(&emit);
                    let _ = tx.send(WorkerMsg::Done(token, result));
                    wake(&waker);
                });
                if spawned.is_err() {
                    return Err(PackError::new("E0900", "couldn't start a worker for the call"));
                }
            }
        }
        Ok(())
    }

    /// Enters a pack's VM at one of `@echo`'s entry points, as a task that may wait.
    fn enter(&mut self, pack: &str, token: u64, entry: &str, args: Vec<Data>) -> Result<()> {
        let name = match args.first() {
            Some(Data::String(name)) => name.to_string(),
            _ => String::new(),
        };
        if self.packs.get(pack).is_some_and(|p| p.in_wasm(entry, &name)) {
            return self.enter_wasm(pack, token, entry, &name, args);
        }
        self.ensure_vm(pack)?;
        self.shared.owners.borrow_mut().insert(token, pack.to_string());
        let script = self.packs.get(pack).and_then(|p| p.script.as_ref()).expect("ensure_vm");
        let mut all = vec![Data::Integer(token as i64)];
        all.extend(args);
        script.call(entry, all).map_err(|e| vm::script_error(&e))
    }

    /// A call came to its end: check the output, record the undo, write the log, answer.
    fn finish(&mut self, token: u64, result: Result<Done>) {
        let Some(mut frame) = self.frames.remove(&token) else { return };
        self.shared.dry.borrow_mut().remove(&token);
        self.shared.owners.borrow_mut().remove(&token);
        let ms = frame.started.elapsed().as_millis() as u64;
        let mut host = String::new();
        let mut result = result.map(|done| {
            frame.usd += done.usd;
            host = done.host;
            if let Some((address, input)) = done.undo {
                frame.steps.push(UndoStep::Call { address, input });
            }
            done.value
        });
        if let (Ok(value), Some(ty)) = (&result, &frame.output) {
            match ty.coerce(value.clone()) {
                Ok(fitted) => result = Ok(fitted),
                Err(e) => result = Err(PackError::new("E0302", format!("{} returned something that isn't its declared output: {e}", frame.address)).learn(format!("echospot dev api show {}", frame.address))),
            }
        }
        // An ability's own undo covers everything it did.
        if let (Ok(value), Some((name, verb))) = (&result, frame.own_undo.take())
            && frame.kind != Kind::Read && !frame.dry {
            frame.steps = vec![UndoStep::Ability { pack: frame.pack.clone(), name, input: frame.input.clone(), output: value.clone(), verb }];
        }
        if frame.logged && !frame.address.starts_with('@') {
            let bytes = result.as_ref().map_or(0, |v| v.to_string().len());
            let entry = audit::Entry {
                t: audit::now(), pack: frame.actor.clone(), address: frame.address.clone(), kind: frame.kind.name().into(), args: summary(&frame.input, 120),
                ok: result.is_ok(), error: result.as_ref().err().map(|e| format!("{}: {}", e.code, e.message)).unwrap_or_default(), bytes, ms, usd: frame.usd, host, dry: frame.dry,
            };
            if !self.ephemeral {
                audit::append(&entry);
            }
        }
        if let Err(e) = &result {
            self.shared.log(if frame.pack.is_empty() { &frame.actor } else { &frame.pack }, "error", format!("{}: {e}", frame.address));
        }
        self.pending.remove(&token);
        // What a person just did may have changed what the pack's surfaces loaded.
        if frame.parent.is_none() && !frame.pack.is_empty() && (frame.address.starts_with('@') || frame.kind != Kind::Read) {
            self.shared.invalidate(&frame.pack);
        }
        if let Some(key) = self.unasked.remove(&token) {
            self.runs.insert(key, Ran { t: audit::now(), ok: result.is_ok(), error: result.as_ref().err().map(|e| e.message.clone()).unwrap_or_default() });
            if !self.ephemeral && let Ok(text) = serde_json::to_string(&self.runs) {
                let _ = std::fs::write(runs_path(), text);
            }
        }
        match frame.parent.and_then(|p| self.frames.get_mut(&p)) {
            Some(parent) => {
                parent.steps.append(&mut frame.steps);
                parent.usd += frame.usd;
            }
            None => self.record_effect(&frame, result.is_ok()),
        }
        match frame.respond.take() {
            Some(Responder::Keep) => {
                self.done.insert(token, result);
            }
            Some(Responder::Luau(done)) => {
                let answer = match result {
                    Ok(value) => json!({ "ok": true, "value": value }),
                    Err(e) => json!({ "ok": false, "error": e.json() }),
                };
                done.complete(to_data(&answer));
            }
            Some(Responder::Channel(reply)) => {
                let _ = reply.send(result);
            }
            Some(Responder::Resource(key)) => {
                let now = self.shared.clock();
                let ttl = self.ttl_of(&frame.address);
                let mut resources = self.shared.resources.borrow_mut();
                if let Some(slot) = resources.get_mut(&key) {
                    *slot = match result {
                        Ok(value) => Resource { state: "ok", value, error: None, at: now, ty: frame.output.clone(), ttl },
                        Err(e) => Resource { state: "error", value: Value::Null, error: Some(e), at: now, ty: None, ttl: 0.0 },
                    };
                }
                drop(resources);
                self.shared.bump();
            }
            Some(Responder::Drop) | None => {}
        }
    }

    fn ttl_of(&self, address: &str) -> f64 {
        let (pack, name) = address.split_once('.').unwrap_or((address, ""));
        self.packs.get(pack).and_then(|p| p.ability(name)).and_then(|a| super::triggers::parse_every(&a.refresh)).unwrap_or(0.0)
    }

    /// A person's action that changed something becomes a Timeline row, with its way back.
    fn record_effect(&mut self, frame: &Frame, ok: bool) {
        if !ok || frame.dry || !frame.root_user || frame.address == "timeline.undo" {
            return;
        }
        let changed = frame.kind != Kind::Read || !frame.steps.is_empty();
        if !changed {
            return;
        }
        let effect = Effect {
            id: self.next_effect, t: audit::now(), address: frame.address.clone(), title: frame.title.clone(), summary: summary(&frame.input, 80),
            steps: if frame.kind == Kind::Final { Vec::new() } else { frame.steps.clone() }, undone: false, is_final: frame.kind == Kind::Final,
        };
        self.next_effect += 1;
        if !effect.steps.is_empty() {
            self.events.push(Event::Undoable { id: effect.id, title: effect.title.clone() });
        }
        self.timeline.push_back(effect);
        while self.timeline.len() > 200 {
            self.timeline.pop_front();
        }
        self.save_timeline();
    }

    fn save_timeline(&self) {
        if !self.ephemeral && let Ok(text) = serde_json::to_string(&self.timeline) {
            let _ = std::fs::write(timeline_path(), text);
        }
    }

    /// Takes an effect back: its steps, newest first. Returns the tokens of the calls made.
    pub fn undo(&mut self, id: u64) -> Result<Vec<u64>> {
        let Some(effect) = self.timeline.iter_mut().find(|e| e.id == id) else { return Err(PackError::new("E0300", "that effect is no longer in Timeline")) };
        if effect.undone {
            return Err(PackError::new("E0311", format!("{} was already undone", effect.title)));
        }
        if effect.steps.is_empty() {
            return Err(PackError::new("E0311", format!("{} can't be undone", effect.title)));
        }
        effect.undone = true;
        let steps: Vec<UndoStep> = effect.steps.iter().rev().cloned().collect();
        self.save_timeline();
        let mut tokens = Vec::new();
        for step in steps {
            match step {
                UndoStep::Call { address, input } => tokens.push(self.start_internal(&address, input)),
                UndoStep::Ability { pack, name, input, output, verb } => {
                    let token = self.shared.token();
                    self.frames.insert(token, Self::host_frame(&pack, format!("@undo {pack}.{name}"), Kind::Effect, false));
                    if let Err(e) = self.enter(&pack, token, "__undo", vec![Data::String(name), to_data(&input), to_data(&output), Data::Bool(verb)]) {
                        self.finish(token, Err(e));
                    }
                    tokens.push(token);
                }
            }
        }
        Ok(tokens)
    }

    /// A call the host makes for itself: an undo step. It leaves no Timeline row of its own.
    fn start_internal(&mut self, address: &str, input: Value) -> u64 {
        let token = self.start(Origin::User, address, input, Responder::Keep, false);
        if let Some(frame) = self.frames.get_mut(&token) {
            frame.root_user = false;
        }
        token
    }

    /// A frame for pack code the host enters directly: a handler, a verb, a trigger, an undo.
    fn host_frame(pack: &str, address: String, kind: Kind, root_user: bool) -> Frame {
        Frame {
            pack: pack.to_string(), title: address.trim_start_matches('@').to_string(), address, kind, read_ctx: kind == Kind::Read, read_by: String::new(), dry: false, parent: None,
            respond: Some(Responder::Keep), started: Instant::now(), input: Value::Null, steps: Vec::new(), usd: 0.0, actor: "user".into(), logged: false,
            output: None, own_undo: None, no_undo: false, root_user, spec_gate: None, on_behalf: String::new(),
        }
    }

    /// Runs a function a drawn surface holds: an action's `run`, a form's `submit`.
    pub fn handle(&mut self, pack: &str, target: &str, handler: u64, payload: Value, dry: bool) -> u64 {
        let token = self.shared.token();
        let mut frame = Self::host_frame(pack, format!("@action {pack}"), Kind::Effect, true);
        frame.dry = dry || self.shared.scene.borrow().is_some();
        if frame.dry {
            self.shared.dry.borrow_mut().insert(token);
        }
        self.frames.insert(token, frame);
        if let Err(e) = self.enter(pack, token, "__handle", vec![Data::String(target.to_string()), Data::Integer(handler as i64), to_data(&payload)]) {
            self.finish(token, Err(e));
        }
        token
    }

    /// Runs a pack's verb on a value.
    pub fn verb(&mut self, pack: &str, name: &str, value: Value, dry: bool) -> u64 {
        let token = self.shared.token();
        let Some(verb) = self.packs.get(pack).and_then(|p| p.index.verbs.get(name)).cloned() else {
            self.done.insert(token, Err(PackError::new("E0300", format!("{pack} has no verb {name}"))));
            return token;
        };
        let mut frame = Self::host_frame(pack, format!("{pack}.verbs.{name}"), verb.kind, true);
        frame.title = verb.title.clone();
        frame.input = value.clone();
        frame.logged = verb.kind != Kind::Read;
        frame.own_undo = verb.has_undo.then(|| (name.to_string(), true));
        frame.no_undo = verb.kind == Kind::Effect && !verb.has_undo;
        frame.dry = dry;
        if dry {
            self.shared.dry.borrow_mut().insert(token);
        }
        self.frames.insert(token, frame);
        if let Err(e) = self.enter(pack, token, "__verb", vec![Data::String(name.to_string()), to_data(&value)]) {
            self.finish(token, Err(e));
        }
        token
    }

    /// Runs a pack's trigger: a schedule came due or an event happened.
    pub fn trigger(&mut self, pack: &str, name: &str, event: Value) -> u64 {
        let token = self.shared.token();
        let mut frame = Self::host_frame(pack, format!("{pack}.triggers.{name}"), Kind::Effect, false);
        frame.actor = pack.to_string();
        frame.respond = Some(Responder::Drop);
        self.frames.insert(token, frame);
        self.unasked.insert(token, format!("{pack}.triggers.{name}"));
        if let Err(e) = self.enter(pack, token, "__trigger", vec![Data::String(name.to_string()), to_data(&event)]) {
            self.finish(token, Err(e));
        }
        token
    }

    // ---- Drawing --------------------------------------------------------------------------

    /// Draws a target of a pack (`ability:<name>:<face>`, `surface:<name>`) and returns its
    /// blocks. It can't wait and it can't change anything: data comes through `cx:load`.
    pub fn render(&mut self, pack: &str, target: &str, args: &Value, state: &Value, out: &Value) -> Result<Value> {
        self.ensure_vm(pack)?;
        let token = self.shared.token();
        self.shared.owners.borrow_mut().insert(token, pack.to_string());
        let script = self.packs.get(pack).and_then(|p| p.script.as_ref()).expect("ensure_vm");
        let started = Instant::now();
        let args = [Data::Integer(token as i64), Data::String(target.to_string()), to_data(args), to_data(state), to_data(out)];
        let built = script.with_time_limit(RENDER_LIMIT, || script.query("__render", args));
        self.shared.owners.borrow_mut().remove(&token);
        let took = started.elapsed();
        if took > RENDER_WARN {
            self.shared.log(pack, "warn", format!("{target} took {:.1} ms to draw; the budget is {} ms and it is stopped at {} ms", took.as_secs_f64() * 1e3, RENDER_WARN.as_millis(), RENDER_LIMIT.as_millis()));
        }
        built.map(|d| from_data(&d)).map_err(|e| vm::script_error(&e))
    }

    /// What an address returns for these arguments, as a surface the host draws needs it: at
    /// once from the cache, pending while it loads. `fresh_for` makes a board tile load again
    /// after that many seconds. Only reads load this way; anything else is an action.
    pub fn resource(&mut self, address: &str, input: &Value, fresh_for: Option<f64>) -> Resource {
        if let Some(scene) = self.shared.scene.borrow().as_ref() {
            if let Some(value) = scene.load.get(address) {
                return Resource::ok(value.clone(), None);
            }
            if let Some(message) = scene.errors.get(address) {
                return Resource { state: "error", error: Some(PackError::new("E0203", message.as_str().unwrap_or("failed"))), ..Resource::pending() };
            }
            if scene.pending.contains(address) {
                return Resource::pending();
            }
        }
        let key = Shared::resource_key("", address, input);
        let now = self.shared.clock();
        let found = self.shared.resources.borrow().get(&key).cloned();
        let ttl = |r: &Resource| match (fresh_for, r.ttl > 0.0) {
            (Some(tile), true) => tile.min(r.ttl),
            (Some(tile), false) => tile,
            (None, _) => r.ttl,
        };
        match found {
            Some(r) if r.state == "pending" || ttl(&r) <= 0.0 || now - r.at < ttl(&r) => r,
            stale => {
                let mut pending = stale.unwrap_or_else(Resource::pending);
                pending.at = now;
                if pending.state != "ok" {
                    pending = Resource::pending();
                    pending.at = now;
                }
                self.shared.resources.borrow_mut().insert(key.clone(), pending.clone());
                self.start(Origin::Load(String::new()), address, input.clone(), Responder::Resource(key), false);
                // A built-in that answers at once has already settled it.
                self.shared.resources.borrow().get(&Shared::resource_key("", address, input)).cloned().unwrap_or(pending)
            }
        }
    }

    /// Every `query(text, cx)` of a pack, within its share of a keystroke.
    pub fn query(&mut self, pack: &str, text: &str) -> Vec<(String, Value)> {
        let has = self.packs.get(pack).is_some_and(|p| p.state == State::Ready && p.index.abilities.values().any(|a| a.has_query));
        if !has || self.ensure_vm(pack).is_err() {
            return Vec::new();
        }
        let token = self.shared.token();
        let script = self.packs.get(pack).and_then(|p| p.script.as_ref()).expect("ensure_vm");
        let started = Instant::now();
        let found = script.with_time_limit(QUERY_LIMIT, || script.query("__query", [Data::Integer(token as i64), Data::String(text.to_string())]));
        if started.elapsed() > QUERY_LIMIT {
            self.shared.log(pack, "warn", format!("query took {:.1} ms; a pack has {} ms of each keystroke", started.elapsed().as_secs_f64() * 1e3, QUERY_LIMIT.as_millis()));
        }
        let Ok(found) = found else { return Vec::new() };
        from_data(&found).as_array().into_iter().flatten().filter_map(|f| Some((f.get("ability")?.as_str()?.to_string(), f.get("value")?.clone()))).collect()
    }

    // ---- The loop -------------------------------------------------------------------------

    fn handle_request(&mut self, request: Request) {
        match request {
            Request::Call { pack, token, address, input, done } => {
                // A token names a call of this pack, or nothing at all.
                let owned = self.shared.owners.borrow().get(&token).is_some_and(|owner| *owner == pack);
                if !owned {
                    let e = PackError::new("E0900", "that call has already finished");
                    return done.complete(to_data(&json!({ "ok": false, "error": e.json() })));
                }
                // A handler, a verb or a trigger has a frame; a draw has none and can only load.
                let origin = if self.frames.contains_key(&token) { Origin::Frame(token) } else { Origin::Load(pack) };
                self.start(origin, &address, input, Responder::Luau(done), false);
            }
            Request::Finish { pack, token, ok, value } => {
                if self.frames.get(&token).is_none_or(|f| f.pack != pack) {
                    return;
                }
                let result = if ok {
                    Done::value(value)
                } else if value.get("code").is_some() {
                    Err(PackError::from_json(&value))
                } else {
                    Err(PackError::luau(value.get("message").and_then(Value::as_str).unwrap_or("the ability failed")))
                };
                self.finish(token, result);
            }
            Request::Load { pack, key, address, input } => {
                self.start(Origin::Load(pack), &address, input, Responder::Resource(key), false);
            }
            Request::Nav { pack, op, address, args, .. } => match op.as_str() {
                "toast" => self.events.push(Event::Toast { text: address, intent: args.get("intent").and_then(Value::as_str).unwrap_or("").into() }),
                "hud" => self.events.push(Event::Hud { text: address }),
                _ => self.events.push(Event::Nav { pack, op, address, args }),
            },
        }
    }

    /// Runs everything that is ready: queued requests, finished workers, tasks whose wait is
    /// over. Returns whether anything a surface shows may have changed.
    pub fn pump(&mut self, tx: &mut Tx) -> bool {
        let before = (self.shared.version.get(), self.events.len(), self.done.len());
        loop {
            let mut any = false;
            loop {
                let Some(request) = self.shared.take() else { break };
                any = true;
                self.handle_request(request);
            }
            while let Ok(msg) = self.worker_rx.try_recv() {
                any = true;
                match msg {
                    WorkerMsg::Done(token, result) => self.finish(token, result),
                    WorkerMsg::Partial(token, value) => {
                        if let Some(Frame { respond: Some(Responder::Resource(key)), .. }) = self.frames.get(&token) {
                            if let Some(slot) = self.shared.resources.borrow_mut().get_mut(key) {
                                slot.value = value;
                                slot.state = "ok";
                            }
                            self.shared.bump();
                        }
                    }
                }
            }
            let now = self.shared.clock();
            for pack in self.packs.values() {
                if let Some(script) = &pack.script {
                    any |= script.poll(now, tx);
                }
            }
            if !any && self.shared.requests.borrow().is_empty() {
                break;
            }
        }
        let stores = self.shared.stores.borrow().version;
        if stores != self.seen_stores {
            self.seen_stores = stores;
            self.shared.bump();
        }
        before != (self.shared.version.get(), self.events.len(), self.done.len())
    }

    /// Whether any call is still running.
    pub fn busy(&self) -> bool {
        !self.frames.is_empty()
    }

    /// The result of a call started with [`Responder::Keep`], once it has one.
    pub fn take(&mut self, token: u64) -> Option<Result<Value>> {
        self.done.remove(&token)
    }

    /// Pumps until the call answers. For the command line and tests: the app never blocks.
    pub fn wait(&mut self, token: u64, timeout: Duration) -> Result<Value> {
        let deadline = Instant::now() + timeout;
        let mut tx = Tx::default();
        loop {
            self.pump(&mut tx);
            if let Some(result) = self.take(token) {
                return result;
            }
            if Instant::now() > deadline {
                self.frames.remove(&token);
                return Err(PackError::new("E0900", format!("no answer after {} s", timeout.as_secs())));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Calls an ability as a person would and waits for it.
    pub fn call(&mut self, address: &str, input: Value, dry: bool) -> Result<Value> {
        let token = self.start(Origin::User, address, input, Responder::Keep, dry);
        self.wait(token, Duration::from_secs(120))
    }

    /// Pumps until nothing is running: every load a draw started has settled.
    pub fn settle(&mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        let mut tx = Tx::default();
        loop {
            self.pump(&mut tx);
            if !self.busy() || Instant::now() > deadline {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    // ---- Running unasked -----------------------------------------------------------------

    /// Everything that runs on a schedule or an event, as `(key, pack, name, is_trigger, every, at, on)`.
    pub fn unasked(&self) -> Vec<super::triggers::Entry> {
        let mut out = Vec::new();
        for pack in self.packs.values().filter(|p| p.state == State::Ready && (p.system || self.effective(p).background)) {
            for (name, t) in &pack.index.triggers {
                out.push(super::triggers::Entry { key: format!("{}.triggers.{name}", pack.id), pack: pack.id.clone(), name: name.clone(), trigger: true, every: t.every.clone(), at: t.at.clone(), on: t.on.clone() });
            }
            for (name, a) in pack.index.abilities.iter().filter(|(_, a)| a.has_run && (!a.every.is_empty() || !a.at.is_empty() || !a.on.is_empty())) {
                out.push(super::triggers::Entry { key: format!("{}.{name}", pack.id), pack: pack.id.clone(), name: name.clone(), trigger: false, every: a.every.clone(), at: a.at.clone(), on: a.on.clone() });
            }
        }
        out
    }

    fn run_unasked(&mut self, entry: &super::triggers::Entry, event: Value) -> u64 {
        // Marked now, so a slow run isn't started twice.
        self.runs.insert(entry.key.clone(), Ran { t: audit::now(), ok: true, error: String::new() });
        if entry.trigger {
            return self.trigger(&entry.pack, &entry.name, event);
        }
        let token = self.start(Origin::Trigger(entry.pack.clone()), &entry.key, json!({}), Responder::Drop, false);
        if self.frames.contains_key(&token) {
            self.unasked.insert(token, entry.key.clone());
        }
        token
    }

    /// Starts every schedule that has come due at `now` (seconds since 1970). Returns how many.
    pub fn tick(&mut self, now: f64) -> usize {
        let due: Vec<super::triggers::Entry> = self.unasked().into_iter().filter(|e| {
            let last = self.runs.get(&e.key).map_or(0.0, |r| r.t);
            super::triggers::due(e, last, now)
        }).collect();
        for entry in &due {
            self.run_unasked(entry, json!({ "event": "schedule", "time": now }));
        }
        due.len()
    }

    /// Something happened: runs every trigger and ability that listens for it. `detail` rides
    /// along (the path for `file.changed`). Returns how many started.
    pub fn emit(&mut self, event: &str, detail: Value) -> usize {
        let path = detail.get("path").and_then(Value::as_str).unwrap_or("").to_string();
        let hit: Vec<super::triggers::Entry> = self.unasked().into_iter().filter(|e| e.on.iter().any(|on| super::triggers::listens(on, event, &path))).collect();
        for entry in &hit {
            let mut payload = json!({ "event": event, "time": audit::now() });
            if let (Some(map), Some(extra)) = (payload.as_object_mut(), detail.as_object()) {
                map.extend(extra.clone());
            }
            self.run_unasked(entry, payload);
        }
        hit.len()
    }

    pub fn set_scene(&mut self, scene: Option<Scene>) {
        *self.shared.scene.borrow_mut() = scene;
        self.shared.resources.borrow_mut().clear();
        self.shared.bump();
    }
}
