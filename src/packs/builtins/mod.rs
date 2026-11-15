//! Built-in abilities: what EchoSpot already does, as addresses with typed signatures. They live
//! in reserved packs (`calc`, `clipboard`, `http`…), and a pack reaches the world only through them.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};

use super::error::{PackError, Result};
use super::index::Kind;
use super::manifest::Permissions;
use super::types::Type;

mod calc;
mod clipboard;
mod desk;
mod http;
mod notes;
mod system;

/// What a pack needs before it may call a built-in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gate {
    /// No grant.
    Free,
    /// The address in the pack's `calls`.
    Calls,
    /// `clipboard = "write"` or `"read"`.
    Clipboard(&'static str),
    /// A declared host.
    Net,
    /// A declared program and argument template.
    Run,
    /// An AI budget with money left this month.
    Ai,
    /// The pack's data folder, a declared glob, or a path the user picked.
    Files,
}

impl Gate {
    pub fn label(self) -> &'static str {
        match self {
            Gate::Free => "free",
            Gate::Calls => "needs `calls`",
            Gate::Clipboard("read") => "needs clipboard = \"read\"",
            Gate::Clipboard(_) => "needs clipboard = \"write\"",
            Gate::Net => "declared hosts",
            Gate::Run => "declared programs",
            Gate::Ai => "AI budget",
            Gate::Files => "data folder, declared globs, picked paths",
        }
    }
}

/// What a built-in call came to.
#[derive(Clone, Debug, Default)]
pub struct Done {
    pub value: Value,
    /// The call that takes this one back: an address and its input.
    pub undo: Option<(String, Value)>,
    /// What it cost, for an AI call.
    pub usd: f64,
    /// The host a network call reached, for the audit log.
    pub host: String,
}

impl Done {
    pub fn value(value: Value) -> Result<Done> {
        Ok(Done { value, ..Default::default() })
    }
}

/// Sends a partial result while a job is still running: text as it streams.
pub type Emit = dyn Fn(Value) + Send + Sync;
pub type Job = Box<dyn FnOnce(&Emit) -> Result<Done> + Send>;

pub enum Run {
    Now(Result<Done>),
    /// Slow work (network, a process, AI): runs on a worker, off the UI thread.
    Later(Job),
}

impl Run {
    pub fn value(value: Value) -> Run {
        Run::Now(Done::value(value))
    }
    pub fn err(e: PackError) -> Run {
        Run::Now(Err(e))
    }
}

/// Who is calling, and what they may do.
#[derive(Clone, Debug, Default)]
pub struct Ctx {
    /// The pack whose code made the call; empty when a person did.
    pub pack: String,
    /// A person at the palette or the command line: nothing to check.
    pub user: bool,
    /// What the pack declared and was granted.
    pub perms: Permissions,
    pub data_dir: PathBuf,
    /// Recorded HTTP responses to answer from, in a test.
    pub fixtures: Option<PathBuf>,
    /// Capture real responses into `fixtures`.
    pub record: bool,
    /// Dollars of AI budget left this month.
    pub budget_left: f64,
    /// Paths the user picked for this pack.
    pub picked: Vec<PathBuf>,
}

type RunFn = dyn Fn(&Ctx, &Value) -> Run + Send + Sync;

#[derive(Clone)]
pub struct Spec {
    pub address: String,
    pub kind: Kind,
    pub gate: Gate,
    pub title: String,
    pub input: Type,
    pub output: Type,
    pub run: Arc<RunFn>,
}

impl Spec {
    pub fn new(address: impl Into<String>, kind: Kind, gate: Gate, title: impl Into<String>, input: &str, output: &str, run: impl Fn(&Ctx, &Value) -> Run + Send + Sync + 'static) -> Self {
        Self { address: address.into(), kind, gate, title: title.into(), input: sig(input), output: one(output), run: Arc::new(run) }
    }
    pub fn signature(&self) -> String {
        format!("{}({}) → {}", self.address, self.input.fields.iter().map(|(k, t)| format!("{k}: {}", t.label())).collect::<Vec<_>>().join(", "), self.output.label())
    }
}

/// One type from its short spelling: `text`, `number?`, `list<url>`, `one|two`.
fn one(spec: &str) -> Type {
    let spec = spec.trim();
    let (spec, optional) = match spec.strip_suffix('?') {
        Some(s) => (s, true),
        None => (spec, false),
    };
    let mut t = if let Some(inner) = spec.strip_prefix("list<").and_then(|s| s.strip_suffix('>')) {
        Type::list(one(inner))
    } else if spec.contains('|') {
        Type { kind: "enum".into(), values: spec.split('|').map(|s| s.trim().to_string()).collect(), ..Default::default() }
    } else if let Some((kind, default)) = spec.split_once('=') {
        Type { default: Some(serde_json::from_str(default).unwrap_or_else(|_| Value::String(default.into()))), ..Type::of(kind) }
    } else {
        Type::of(spec)
    };
    t.optional = optional;
    t
}

/// An input record from its short spelling: `amount:number, from:currency, note:text?`.
fn sig(spec: &str) -> Type {
    let mut t = Type::of("record");
    for field in spec.split(',').map(str::trim).filter(|f| !f.is_empty()) {
        let (name, ty) = field.split_once(':').unwrap_or((field, "text"));
        t.fields.insert(name.trim().to_string(), one(ty));
    }
    t
}

fn text<'a>(input: &'a Value, key: &str) -> &'a str {
    input.get(key).and_then(Value::as_str).unwrap_or("")
}

fn num(input: &Value, key: &str) -> Option<f64> {
    input.get(key).and_then(Value::as_f64)
}

/// Every built-in whose address is fixed.
pub fn all() -> Vec<Spec> {
    let mut out = Vec::new();
    calc::register(&mut out);
    clipboard::register(&mut out);
    notes::register(&mut out);
    desk::register(&mut out);
    http::register(&mut out);
    system::register(&mut out);
    out
}

/// The built-in at `address`.
pub fn find(address: &str) -> Option<Spec> {
    all().into_iter().find(|s| s.address == address)
}

/// Built-ins made from what the user already has: scripts, snippets, quicklinks, AI presets.
pub fn dynamic() -> Vec<Spec> {
    Vec::new()
}

pub fn find_dynamic(address: &str) -> Option<Spec> {
    dynamic().into_iter().find(|s| s.address == address)
}

/// Whether `address` is in a reserved pack: one a pack may never define.
pub fn reserved(address: &str) -> bool {
    address.split('.').next().is_some_and(|ns| super::manifest::RESERVED.contains(&ns))
}

/// Runs a quick built-in as the user and returns its value: what a built-in store reads.
pub fn call_now(address: &str, input: Value) -> Value {
    let Some(spec) = find(address) else { return Value::Null };
    let Ok(input) = spec.input.coerce(input) else { return Value::Null };
    match (spec.run)(&Ctx { user: true, ..Default::default() }, &input) {
        Run::Now(Ok(done)) => done.value,
        _ => Value::Null,
    }
}

fn missing(what: &str) -> PackError {
    PackError::new("E0301", format!("{what} is empty"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures_read_from_their_short_spelling() {
        let s = Spec::new("x.y", Kind::Read, Gate::Free, "X", "amount:number, to:currency?, tags:list<text>, mode:fast|slow, limit:number=20", "list<url>", |_, _| Run::value(json!(null)));
        assert_eq!(s.signature(), "x.y(amount: number, limit: number, mode: \"fast\" | \"slow\", tags: list<text>, to: currency?) → list<url>");
        assert_eq!(s.input.coerce(json!({ "amount": 1, "tags": [], "mode": "fast" })).unwrap()["limit"], json!(20));
    }

    #[test]
    fn every_builtin_is_in_a_reserved_pack() {
        for spec in all() {
            assert!(reserved(&spec.address), "{} isn't in a reserved pack", spec.address);
            assert!(spec.kind == Kind::Read || spec.gate != Gate::Calls || !spec.title.is_empty());
        }
    }
}
