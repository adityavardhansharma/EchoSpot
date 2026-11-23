//! The Rust tier (API-DESIGN.md §20): Rust compiled to WebAssembly components, run with wasmtime.
//!
//! A Rust pack is a pack folder with a `Cargo.toml`. EchoSpot builds it for `wasm32-wasip2`,
//! compiles the component to native code once and keeps that, and runs each call in a fresh
//! instance on a worker thread. The component reaches the world only through `echospot:pack/host`,
//! whose `call` is the broker, so a Rust pack has exactly the grants, audit, undo and dry runs a
//! Luau pack has. Its WASI is bare: no files, no network, no environment, no arguments.
//!
//! Rust packs are headless. They declare abilities, verbs, stores and triggers in the same JSON a
//! Luau pack's `__describe` returns; their UI is the default faces, or an `init.luau` beside them.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use serde_json::{Value, json};

use super::error::{PackError, Result};

/// What a Rust pack is built from: the interface, written into `wit/` before each build.
pub const WIT: &str = include_str!("echospot.wit");
/// The pack's side of it, written into `src/echo.rs` before each build.
pub const ECHO_RS: &str = include_str!("echo.rs");
/// The target a pack is built for: a component, with the standard library working.
pub const TARGET: &str = "wasm32-wasip2";
/// Compute one entry may spend, in wasmtime fuel: about ten seconds of work. Waiting on an
/// ability the pack called costs nothing.
pub const FUEL: u64 = 20_000_000_000;
/// The budget in force: [`FUEL`], or less in a test.
pub static FUEL_NOW: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(FUEL);

fn fuel() -> u64 {
    FUEL_NOW.load(std::sync::atomic::Ordering::Relaxed)
}

/// A call's heap, as a Luau pack's.
pub const HEAP: usize = 64 << 20;
/// What a call may print before the rest is dropped.
const STDOUT_CAP: usize = 64 << 10;

/// Whether a pack folder is a Rust pack.
pub fn is_rust(dir: &Path) -> bool {
    dir.join("Cargo.toml").is_file()
}

/// Whether this EchoSpot was built able to run Rust packs.
pub const ENABLED: bool = cfg!(feature = "wasm");

/// The crate's name as cargo writes its artifact: `my-pack` becomes `my_pack.wasm`.
fn crate_name(dir: &Path) -> Result<String> {
    let text = std::fs::read_to_string(dir.join("Cargo.toml")).map_err(|e| PackError::new("E0200", format!("can't read Cargo.toml: {e}")).at("Cargo.toml"))?;
    let doc: toml::Value = text.parse().map_err(|e| PackError::new("E0201", format!("Cargo.toml isn't valid TOML: {e}")).at("Cargo.toml"))?;
    let name = doc.get("package").and_then(|p| p.get("name")).and_then(|n| n.as_str())
        .ok_or_else(|| PackError::new("E0201", "Cargo.toml has no [package] name").at("Cargo.toml"))?;
    let lib = doc.get("lib").and_then(|l| l.get("crate-type")).and_then(|c| c.as_array());
    if !lib.is_some_and(|types| types.iter().any(|t| t.as_str() == Some("cdylib"))) {
        return Err(PackError::new("E0201", "a Rust pack is a component: Cargo.toml needs [lib] crate-type = [\"cdylib\"]").at("Cargo.toml").learn("echospot dev docs rust"));
    }
    Ok(name.replace('-', "_"))
}

/// Where builds go: the pack's own `target/`, or `ECHOSPOT_RUST_TARGET` shared by every pack.
pub fn target_dir(dir: &Path) -> PathBuf {
    std::env::var_os("ECHOSPOT_RUST_TARGET").map(PathBuf::from).unwrap_or_else(|| dir.join("target"))
}

/// The component a build wrote.
pub fn artifact(dir: &Path) -> Result<PathBuf> {
    Ok(target_dir(dir).join(TARGET).join("release").join(format!("{}.wasm", crate_name(dir)?)))
}

/// Where a build records the sources it was built from.
fn built_stamp_path(dir: &Path) -> PathBuf {
    dir.join("target").join("echospot-built")
}

/// A hash of what a build reads: the manifest, the lock file, `src/` and `wit/`.
pub fn stamp(dir: &Path) -> u64 {
    fn walk(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
            let child = if rel.is_empty() { name.to_string() } else { format!("{rel}/{name}") };
            if path.is_dir() {
                walk(&path, &child, out);
            } else {
                out.push((child, path));
            }
        }
    }
    let mut files = vec![("Cargo.toml".to_string(), dir.join("Cargo.toml")), ("Cargo.lock".to_string(), dir.join("Cargo.lock"))];
    for sub in ["src", "wit"] {
        walk(&dir.join(sub), sub, &mut files);
    }
    super::vm::hash(files.iter().flat_map(|(rel, path)| [rel.clone().into_bytes(), std::fs::read(path).unwrap_or_default()]))
}

/// Whether the component on disk was built from the sources there now.
pub fn up_to_date(dir: &Path) -> bool {
    let built = std::fs::read_to_string(built_stamp_path(dir)).ok().and_then(|s| s.trim().parse::<u64>().ok());
    built == Some(stamp(dir)) && artifact(dir).is_ok_and(|a| a.is_file())
}

/// Writes the interface and `src/echo.rs` as this EchoSpot has them. Returns the files it changed.
pub fn sync_sdk(dir: &Path) -> Result<Vec<String>> {
    let mut wrote = Vec::new();
    for (rel, text) in [("wit/echospot.wit", WIT), ("src/echo.rs", ECHO_RS)] {
        let path = dir.join(rel);
        if std::fs::read_to_string(&path).is_ok_and(|t| t == text) {
            continue;
        }
        std::fs::create_dir_all(path.parent().expect("has a folder")).and_then(|()| std::fs::write(&path, text))
            .map_err(|e| PackError::new("E0200", format!("couldn't write {rel}: {e}")))?;
        wrote.push(rel.to_string());
    }
    Ok(wrote)
}

/// How a build is going, for the `checking` row and `echospot dev check`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    /// Crates compiled so far.
    pub done: usize,
    /// What it is compiling now.
    pub now: String,
}

impl Progress {
    pub fn label(&self) -> String {
        if self.now.is_empty() { "Building".into() } else { format!("Building · {} ({} done)", self.now, self.done) }
    }
}

/// The first error cargo printed, as a pack error that points at the line.
fn compile_error(message: &Value, dir: &Path) -> PackError {
    let msg = &message["message"];
    let text = msg["message"].as_str().unwrap_or("it didn't compile").to_string();
    let mut e = PackError::new("E0206", text).learn("echospot dev docs rust");
    if let Some(span) = msg["spans"].as_array().and_then(|s| s.iter().find(|s| s["is_primary"].as_bool() == Some(true)).or(s.first())) {
        let file = span["file_name"].as_str().unwrap_or("");
        let file = Path::new(file).strip_prefix(dir).map(|p| p.display().to_string()).unwrap_or_else(|_| file.to_string());
        e = e.at(format!("{file}:{}:{}", span["line_start"].as_u64().unwrap_or(0), span["column_start"].as_u64().unwrap_or(0)));
    }
    if let Some(rendered) = msg["rendered"].as_str() {
        e.message = format!("{}\n{}", e.message, rendered.trim_end());
    }
    e
}

/// Builds a Rust pack and returns its component. Blocking: the app runs it on a thread.
pub fn build(dir: &Path, on_progress: &dyn Fn(&Progress)) -> Result<PathBuf> {
    use std::io::{BufRead, Read};
    sync_sdk(dir)?;
    let artifact = artifact(dir)?;
    let mut child = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()))
        .args(["build", "--release", "--target", TARGET, "--message-format=json", "--target-dir"])
        .arg(target_dir(dir))
        .current_dir(dir)
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn()
        .map_err(|e| PackError::new("E0206", format!("couldn't run cargo: {e}")).learn("install Rust from https://rustup.rs, then: rustup target add wasm32-wasip2"))?;
    let stderr = child.stderr.take().expect("piped");
    let errors = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = std::io::BufReader::new(stderr).read_to_string(&mut text);
        text
    });
    let mut progress = Progress::default();
    let mut first_error: Option<PackError> = None;
    for line in std::io::BufReader::new(child.stdout.take().expect("piped")).lines().map_while(std::result::Result::ok) {
        let Ok(message) = serde_json::from_str::<Value>(&line) else { continue };
        match message["reason"].as_str() {
            Some("compiler-artifact") => {
                progress.done += 1;
                progress.now = message["target"]["name"].as_str().unwrap_or("").to_string();
                on_progress(&progress);
            }
            Some("compiler-message") if message["message"]["level"] == "error" && first_error.is_none() => {
                first_error = Some(compile_error(&message, dir));
            }
            _ => {}
        }
    }
    let status = child.wait().map_err(|e| PackError::new("E0206", format!("cargo stopped: {e}")))?;
    let stderr = errors.join().unwrap_or_default();
    if !status.success() {
        if let Some(e) = first_error {
            return Err(e);
        }
        if stderr.contains("target may not be installed") || stderr.contains(&format!("can't find crate for `core`")) {
            return Err(PackError::new("E0206", format!("Rust can't build for {TARGET} yet")).learn(format!("rustup target add {TARGET}")));
        }
        let last = stderr.lines().filter(|l| l.contains("error")).last().unwrap_or(stderr.lines().last().unwrap_or("cargo failed")).trim().to_string();
        return Err(PackError::new("E0206", last).learn("echospot dev docs rust"));
    }
    if !artifact.is_file() {
        return Err(PackError::new("E0206", format!("cargo finished but wrote no {}", artifact.display())).learn("echospot dev docs rust"));
    }
    let stamp_path = built_stamp_path(dir);
    let _ = std::fs::create_dir_all(stamp_path.parent().expect("has a folder"));
    let _ = std::fs::write(stamp_path, stamp(dir).to_string());
    Ok(artifact)
}

/// What a running call may ask of the host. Each closure runs on the worker thread the call is
/// on; the runtime answers on its own thread.
pub struct Calls {
    pub call: Box<dyn Fn(&str, Value) -> Result<Value> + Send>,
    pub put: Box<dyn Fn(&str, Value) -> Result<()> + Send>,
    pub log: Box<dyn Fn(&'static str, String) + Send>,
}

impl Calls {
    /// For reading declarations: nothing is reachable.
    pub fn none() -> Self {
        let refuse = || PackError::new("E0300", "nothing can be called while a pack is only being read");
        Calls { call: Box::new(move |_, _| Err(refuse())), put: Box::new(move |_, _| Err(refuse())), log: Box::new(|_, _| {}) }
    }
}

#[cfg(feature = "wasm")]
mod engine {
    use std::path::Path;
    use std::sync::{Arc, OnceLock};

    use serde_json::Value;
    use wasmtime::component::{Component, HasSelf, Linker, ResourceTable};
    use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};
    use wasmtime_wasi::p2::pipe::MemoryOutputPipe;
    use wasmtime_wasi::{WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

    use super::{Calls, HEAP, STDOUT_CAP, fuel};
    use crate::packs::error::{PackError, Result};

    mod bindings {
        wasmtime::component::bindgen!({ world: "pack", path: "src/packs/wasm/echospot.wit" });
    }
    use bindings::{Context, Pack as World};

    pub struct State {
        wasi: WasiCtx,
        table: ResourceTable,
        limits: StoreLimits,
        calls: Calls,
    }

    impl WasiView for State {
        fn ctx(&mut self) -> WasiCtxView<'_> {
            WasiCtxView { ctx: &mut self.wasi, table: &mut self.table }
        }
    }

    impl bindings::echospot::pack::host::Host for State {
        fn call(&mut self, address: String, input: String) -> std::result::Result<String, String> {
            let input = serde_json::from_str(&input).unwrap_or(Value::Null);
            (self.calls.call)(&address, input).map(|v| v.to_string()).map_err(|e| e.json().to_string())
        }
        fn put(&mut self, store: String, value: String) -> std::result::Result<(), String> {
            let value = serde_json::from_str(&value).unwrap_or(Value::Null);
            (self.calls.put)(&store, value).map_err(|e| e.json().to_string())
        }
        fn log(&mut self, level: String, message: String) {
            let level = match level.as_str() {
                "warn" => "warn",
                "error" => "error",
                _ => "info",
            };
            (self.calls.log)(level, message);
        }
    }

    fn engine() -> &'static Engine {
        static ENGINE: OnceLock<Engine> = OnceLock::new();
        ENGINE.get_or_init(|| {
            let mut config = Config::new();
            config.consume_fuel(true);
            Engine::new(&config).expect("wasmtime's configuration is fixed")
        })
    }

    fn linker() -> &'static Linker<State> {
        static LINKER: OnceLock<Linker<State>> = OnceLock::new();
        LINKER.get_or_init(|| {
            let mut linker = Linker::new(engine());
            wasmtime_wasi::p2::add_to_linker_sync(&mut linker).expect("WASI links once");
            World::add_to_linker::<State, HasSelf<State>>(&mut linker, |s| s).expect("the host interface links once");
            linker
        })
    }

    /// A pack's component, compiled to native code.
    pub struct Module {
        component: Component,
        pub compiled_ms: f64,
        pub cached: bool,
    }

    impl Module {
        /// Compiles a component, or reads what an earlier compile of the same bytes kept.
        pub fn load(wasm: &Path) -> Result<Arc<Module>> {
            let started = std::time::Instant::now();
            let bytes = std::fs::read(wasm).map_err(|e| PackError::new("E0200", format!("can't read {}: {e}", wasm.display())))?;
            let engine = engine();
            let compat = {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                engine.precompile_compatibility_hash().hash(&mut h);
                h.finish()
            };
            let key = crate::packs::vm::hash([bytes.as_slice(), &compat.to_le_bytes()[..]]);
            let cache = crate::packs::state_dir().join("wasm").join(format!("{key:016x}.cwasm"));
            // Safety: the file is one this process wrote with `serialize`, for this engine's
            // configuration and wasmtime version (both are in its name's hash).
            if cache.is_file() && let Ok(component) = unsafe { Component::deserialize_file(engine, &cache) } {
                return Ok(Arc::new(Module { component, compiled_ms: started.elapsed().as_secs_f64() * 1e3, cached: true }));
            }
            let component = Component::new(engine, &bytes).map_err(|e| PackError::new("E0207", format!("not a component EchoSpot can run: {e:#}")).learn("echospot dev docs rust"))?;
            if let Ok(native) = component.serialize() {
                let _ = std::fs::create_dir_all(cache.parent().expect("has a folder"));
                let _ = std::fs::write(&cache, native);
            }
            Ok(Arc::new(Module { component, compiled_ms: started.elapsed().as_secs_f64() * 1e3, cached: false }))
        }

        fn instantiate(&self, calls: Calls) -> Result<(Store<State>, World, MemoryOutputPipe)> {
            let stdout = MemoryOutputPipe::new(STDOUT_CAP);
            // No preopened folders, no environment, no arguments, no sockets: the broker is the only way out.
            let wasi = WasiCtxBuilder::new().stdout(stdout.clone()).stderr(stdout.clone())
                .allow_tcp(false).allow_udp(false).allow_ip_name_lookup(false)
                .socket_addr_check(|_, _| Box::pin(async { false }))
                .build();
            let limits = StoreLimitsBuilder::new().memory_size(HEAP).instances(4).build();
            let mut store = Store::new(engine(), State { wasi, table: ResourceTable::new(), limits, calls });
            store.limiter(|s| &mut s.limits);
            store.set_fuel(fuel()).expect("fuel is on");
            let world = World::instantiate(&mut store, &self.component, linker()).map_err(|e| trap(e, "starting"))?;
            Ok((store, world, stdout))
        }

        /// What the pack declares.
        pub fn describe(&self) -> Result<Value> {
            let (mut store, world, _) = self.instantiate(Calls::none())?;
            let text = world.call_describe(&mut store).map_err(|e| trap(e, "describe"))?;
            serde_json::from_str(&text).map_err(|e| PackError::new("E0202", format!("describe() didn't return JSON: {e}")))
        }

        /// Runs one entry in a fresh instance. What it printed goes to the log, a line at a time.
        pub fn run(&self, pack: &str, dry: bool, entry: &str, name: &str, input: &Value, output: &Value, calls: Calls) -> Result<Value> {
            let (mut store, world, stdout) = self.instantiate(calls)?;
            let cx = Context { pack: pack.to_string(), dry };
            let answer = world.call_run(&mut store, &cx, entry, name, &input.to_string(), &output.to_string());
            let printed = String::from_utf8_lossy(&stdout.contents()).to_string();
            for line in printed.lines().filter(|l| !l.trim().is_empty()) {
                (store.data().calls.log)("info", line.to_string());
            }
            let used = fuel() - store.get_fuel().unwrap_or(0);
            match answer {
                Ok(Ok(text)) => Ok(serde_json::from_str(&text).unwrap_or(Value::Null)),
                Ok(Err(error)) => Err(match serde_json::from_str::<Value>(&error) {
                    Ok(v) if v.get("code").is_some() => PackError::from_json(&v),
                    _ => PackError::new("E0300", error),
                }),
                Err(_) if used >= fuel() => Err(PackError::new("E0209", format!("{pack}.{name} used its whole compute budget and was stopped")).learn("echospot dev docs rust")),
                Err(e) => Err(trap(e, &format!("{entry} {name}"))),
            }
        }
    }

    /// A trap: a panic (its message is already in the log), running out of memory, a bug.
    fn trap(e: wasmtime::Error, during: &str) -> PackError {
        let text = format!("{e:#}");
        let why = if text.contains("unreachable") { "it panicked; the message is in echospot dev logs".to_string() } else { text };
        PackError::new("E0208", format!("the pack's code stopped during {during}: {why}")).learn("echospot dev logs")
    }

    pub fn warm() {
        let _ = linker();
    }
}

#[cfg(feature = "wasm")]
pub use engine::Module;

/// Without the `wasm` feature a Rust pack loads as broken, with a reason.
#[cfg(not(feature = "wasm"))]
pub struct Module;

#[cfg(not(feature = "wasm"))]
impl Module {
    pub fn load(_: &Path) -> Result<std::sync::Arc<Module>> {
        Err(PackError::new("E0207", "this EchoSpot was built without Rust packs (the wasm feature)"))
    }
    pub fn describe(&self) -> Result<Value> {
        unreachable!()
    }
    pub fn run(&self, _: &str, _: bool, _: &str, _: &str, _: &Value, _: &Value, _: Calls) -> Result<Value> {
        unreachable!()
    }
}

/// A Rust name for an address or a field: `calc.convert` is `calc_convert`, `type` is `r#type`.
fn rust_ident(name: &str) -> String {
    let mut out: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    const KEYWORDS: &[&str] = &["as", "async", "await", "box", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern", "false", "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub", "ref", "return", "static", "struct", "super", "trait", "true", "type", "unsafe", "use", "where", "while", "yield"];
    if KEYWORDS.contains(&out.as_str()) { format!("r#{out}") } else { out }
}

/// The Rust parameter a field of a schema takes.
fn rust_param(t: &super::types::Type) -> &'static str {
    let base = match t.kind.as_str() {
        "number" | "percent" | "duration" => "f64",
        "bool" => "bool",
        "text" | "markdown" | "code" | "currency" | "date" | "time" | "datetime" | "url" | "file" | "folder" | "image" | "color" | "app" | "enum" => "&str",
        _ => "Value",
    };
    match (t.optional || t.default.is_some(), base) {
        (true, "f64") => "Option<f64>",
        (true, "bool") => "Option<bool>",
        (true, "&str") => "Option<&str>",
        (true, _) => "Option<Value>",
        (false, b) => b,
    }
}

/// `src/calls.rs`: a typed function for every address a Rust pack may call, from the same
/// declarations the broker checks calls against. Built-ins are typed field by field; another
/// pack's ability, named in `calls`, takes and gives JSON. It depends only on `pack.toml` and
/// EchoSpot's own abilities, so it changes only when one of those does.
pub fn calls_rs(manifest: &super::manifest::Manifest, specs: &[super::builtins::Spec]) -> String {
    use super::builtins::Gate;
    let perms = &manifest.permissions;
    let mut out = String::from("//! Every address this pack may call, as a function. Written by EchoSpot from pack.toml and
//! its own declarations before each build; don't edit it. Not here? It needs `calls = [\"…\"]`
//! in pack.toml, or it doesn't exist: `echospot dev api search <words>`.
#![allow(dead_code, clippy::too_many_arguments)]

use serde_json::{Map, Value, json};

use crate::echo::{Cx, Result};
");
    let mut seen = std::collections::BTreeSet::new();
    let mut specs: Vec<&super::builtins::Spec> = specs.iter().filter(|spec| match spec.gate {
        Gate::Free | Gate::Files => true,
        Gate::Calls => perms.calls.contains(&spec.address),
        Gate::Clipboard(level) => perms.clipboard == "read" || perms.clipboard == level,
        Gate::Net => !perms.net.is_empty(),
        Gate::Run => !perms.run.is_empty(),
        Gate::Ai => perms.ai.is_some(),
    }).collect();
    specs.sort_by(|a, b| a.address.cmp(&b.address));
    for spec in specs {
        let name = rust_ident(&spec.address);
        if !seen.insert(name.clone()) {
            continue;
        }
        let fields: Vec<(&String, &super::types::Type)> = spec.input.fields.iter().collect();
        let params: String = fields.iter().map(|(k, t)| format!(", {}: {}", rust_ident(k), rust_param(t))).collect();
        let sig = format!("{}({}) → {}", spec.address, fields.iter().map(|(k, t)| format!("{k}: {}", t.label())).collect::<Vec<_>>().join(", "), spec.output.label());
        out.push_str(&format!("\n/// {}\n///\n/// `{sig}`, {}\npub fn {name}(cx: &Cx{params}) -> Result {{\n    let mut input = Map::new();\n", spec.title, spec.kind.name()));
        for (k, t) in &fields {
            let id = rust_ident(k);
            if rust_param(t).starts_with("Option") {
                out.push_str(&format!("    if let Some(v) = {id} {{\n        input.insert({k:?}.into(), json!(v));\n    }}\n"));
            } else {
                out.push_str(&format!("    input.insert({k:?}.into(), json!({id}));\n"));
            }
        }
        out.push_str(&format!("    cx.call({:?}, Value::Object(input))\n}}\n", spec.address));
    }
    for address in perms.calls.iter().filter(|a| !specs_has(a)) {
        let name = rust_ident(address);
        if !seen.insert(name.clone()) {
            continue;
        }
        out.push_str(&format!("\n/// `{address}`, another pack's ability: `echospot dev api show {address}` for what it takes.\npub fn {name}(cx: &Cx, input: Value) -> Result {{\n    cx.call({address:?}, input)\n}}\n"));
    }
    out
}

fn specs_has(address: &str) -> bool {
    super::builtins::find(address).is_some() || super::builtins::find_dynamic(address).is_some()
}

/// Writes `src/calls.rs` when it differs. Returns whether it did.
pub fn write_calls(dir: &Path, text: &str) -> bool {
    let path = dir.join("src").join("calls.rs");
    if std::fs::read_to_string(&path).is_ok_and(|t| t == text) {
        return false;
    }
    std::fs::create_dir_all(dir.join("src")).and_then(|()| std::fs::write(&path, text)).is_ok()
}

/// Whether `rustup` has the target installed, for `echospot dev check` and the docs.
pub fn toolchain() -> std::result::Result<(), String> {
    static FOUND: OnceLock<std::result::Result<(), String>> = OnceLock::new();
    FOUND.get_or_init(|| {
        let sysroot = Command::new("rustc").args(["--print", "sysroot"]).output().map_err(|_| "Rust isn't installed: https://rustup.rs".to_string())?;
        let sysroot = String::from_utf8_lossy(&sysroot.stdout).trim().to_string();
        if Path::new(&sysroot).join("lib/rustlib").join(TARGET).is_dir() {
            Ok(())
        } else {
            Err(format!("rustup target add {TARGET}"))
        }
    }).clone()
}

/// A declaration in a Rust pack's JSON, read the way a Luau one is.
pub fn index(described: &Value) -> Result<super::index::Index> {
    let index = super::index::Index::from_describe(described).map_err(|mut e| {
        e.at = format!("src/lib.rs ({})", if e.at.is_empty() { "pack()" } else { &e.at });
        e
    })?;
    if !index.surfaces.is_empty() || !index.boards.is_empty() {
        return Err(PackError::new("E0202", "a Rust pack is headless: draw surfaces in an init.luau beside Cargo.toml").learn("echospot dev docs rust"));
    }
    Ok(index)
}

/// A value to describe the tier with in `dev docs`.
pub fn about() -> Value {
    json!({ "target": TARGET, "enabled": ENABLED, "heap": HEAP, "fuel": FUEL })
}

#[cfg(all(test, feature = "wasm"))]
mod tests;
