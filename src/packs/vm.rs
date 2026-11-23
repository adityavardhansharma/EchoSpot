//! A pack's VM: its files as sources, the host modules it may require, and what its Luau side
//! (`@echo`) is given to reach the host with. The only way out of a VM is the table built here,
//! and every function in it either reads a cache or queues a request for the broker.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use rune_luau::mlua::{self, Function, Lua, MultiValue, Table, Value as Lv};
use rune_luau::{Data, Host, LogLevel, Script, ScriptOptions, Sources};
use serde_json::Value;

use super::error::PackError;
use super::json::{from_lua, to_lua};
use super::shared::{Request, Resource, Shared};

pub const ECHO: &str = include_str!("luau/echo.luau");
pub const TYPES: &str = include_str!("luau/types.luau");
pub const UI: &str = include_str!("luau/ui.luau");

/// The module the VM starts from: it loads `init.luau`, then hands it the boards found in
/// `boards/`.
const ENTRY: &str = "__pack.luau";

/// Folders that aren't part of the running pack.
const APART: &[&str] = &["tests", "scenes", "fixtures", "inbox", "icons", ".git", "target"];

/// Names `echo` answers to, for "did you mean".
pub const ECHO_NAMES: &[&str] = &[
    "pack", "ability", "verb", "store", "board", "trigger", "panel", "window", "overlay", "widget", "map", "filter", "find", "keys", "sort", "json",
    "now", "date", "api",
];

/// Names `cx` answers to.
pub const CX_NAMES: &[&str] = &["call", "try", "load", "store", "storeAt", "put", "push", "replace", "pop", "toast", "hud", "dry", "log", "pack", "data"];

/// The `@echo/ui` names, read from the module itself.
pub fn ui_names() -> Vec<String> {
    let lua = Lua::new();
    let run = || -> mlua::Result<Vec<String>> {
        let host = lua.create_table()?;
        host.set("unknown", lua.create_function(|_, _: MultiValue| Ok(String::new()))?)?;
        let ui: Table = lua.load(UI).set_name("=@echo/ui").call(host)?;
        ui.raw_get::<Vec<String>>("names")
    };
    run().unwrap_or_default().into_iter().filter(|n| !matches!(n.as_str(), "names" | "resolve" | "pick")).collect()
}

fn walk(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        let child = if rel.is_empty() { name.to_string() } else { format!("{rel}/{name}") };
        if path.is_dir() {
            if !(rel.is_empty() && APART.contains(&name)) && !name.starts_with('.') {
                walk(&path, &child, out);
            }
        } else if name.ends_with(".d.luau") || name == ".luaurc" {
            // Type definitions are for an editor, and the editor's aliases would shadow the
            // modules the host provides.
        } else if name.ends_with(".luau") {
            out.push((child, path));
        }
    }
}

/// FNV-1a over what was read: the stamp an index cache is keyed by.
pub fn hash(parts: impl IntoIterator<Item = impl AsRef<[u8]>>) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for part in parts {
        for b in part.as_ref().iter().chain(&[0xff]) {
            hash ^= *b as u64;
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    hash
}

/// A pack folder's Luau files as sources, and a stamp of everything that decides what it declares.
pub fn sources(dir: &Path) -> std::io::Result<(Sources, u64)> {
    let mut files = Vec::new();
    walk(dir, "", &mut files);
    let mut sources = Sources::new();
    let mut boards = Vec::new();
    let mut stamp = vec![std::fs::read(dir.join("pack.toml")).unwrap_or_default(), super::API.to_string().into_bytes(), ECHO.as_bytes().to_vec()];
    for (rel, path) in &files {
        let text = std::fs::read_to_string(path)?;
        stamp.push(rel.clone().into_bytes());
        stamp.push(text.clone().into_bytes());
        if let Some(name) = rel.strip_prefix("boards/").and_then(|n| n.strip_suffix(".luau")).filter(|n| !n.contains('/')) {
            boards.push(name.to_string());
        }
        sources.insert(rel, text);
    }
    let mut entry = String::from("local pack = require(\"./init\")\nif type(pack) == \"table\" and pack.__boards then\n\tpack.__boards({\n");
    for board in &boards {
        entry.push_str(&format!("\t\t[{board:?}] = require(\"./boards/{board}\"),\n"));
    }
    entry.push_str("\t})\nend\nreturn pack\n");
    sources.insert(ENTRY, entry);
    Ok((sources.entry(ENTRY), hash(stamp)))
}

fn raise(e: PackError) -> mlua::Error {
    mlua::Error::runtime(e.json().to_string())
}

/// Binds the host into a new VM for pack `id`.
fn bind(host: &Host<'_>, shared: &Rc<Shared>, id: &str) -> mlua::Result<()> {
    let lua = host.lua();
    let h = lua.create_table()?;
    h.set("id", id)?;
    h.set("api", super::API)?;
    h.set("data_dir", super::data_dir(id).display().to_string())?;
    h.set("json_encode", lua.create_function(|_, v: Lv| Ok(from_lua(&v).to_string()))?)?;
    h.set("json_decode", lua.create_function(|lua, text: String| match serde_json::from_str::<Value>(&text) {
        Ok(v) => to_lua(lua, &v),
        Err(e) => Err(mlua::Error::runtime(format!("not JSON: {e}"))),
    })?)?;
    h.set("now", lua.create_function(|_, ()| Ok(super::audit::now()))?)?;
    h.set("date", lua.create_function(|_, (format, at): (Option<String>, Option<f64>)| {
        use chrono::{Local, TimeZone};
        let when = at.and_then(|t| Local.timestamp_opt(t as i64, 0).single()).unwrap_or_else(Local::now);
        let mut out = String::new();
        // A bad format string is the pack's mistake, not a panic.
        Ok(match std::fmt::write(&mut out, format_args!("{}", when.format(format.as_deref().unwrap_or("%Y-%m-%d %H:%M")))) {
            Ok(()) => out,
            Err(_) => when.to_rfc3339(),
        })
    })?)?;
    h.set("unknown", lua.create_function(|_, (space, name): (String, String)| {
        let known: Vec<String> = match space.as_str() {
            "ui" => ui_names(),
            _ => ECHO_NAMES.iter().map(|s| s.to_string()).collect(),
        };
        let what = if space == "ui" { "component" } else { "function" };
        let learn = if space == "ui" { "echospot dev api show List".to_string() } else { "echospot dev docs echo".to_string() };
        let mut e = PackError::new("E0412", format!("{space} has no {what} '{name}'")).nearest(&name, known.iter().map(String::as_str)).learn(learn);
        if let Some(first) = e.nearest.first() {
            e.learn = format!("echospot dev api show {first}");
        }
        Ok(e.json().to_string())
    })?)?;

    let (s, pack) = (shared.clone(), id.to_string());
    h.set("call", host.async_function("call", move |_, args: MultiValue, done| {
        let mut args = args.into_iter();
        let token = args.next().and_then(|v| v.as_number().or(v.as_integer().map(|i| i as f64))).unwrap_or(0.0) as u64;
        let address = args.next().and_then(|v| v.as_string().map(|s| s.to_string_lossy())).unwrap_or_default();
        let input = args.next().map(|v| from_lua(&v)).unwrap_or(Value::Null);
        s.push(Request::Call { pack: pack.clone(), token, address, input, done });
        Ok(())
    })?)?;

    let (s, pack) = (shared.clone(), id.to_string());
    h.set("finish", lua.create_function(move |_, (token, ok, value): (f64, bool, Lv)| {
        s.push(Request::Finish { pack: pack.clone(), token: token as u64, ok, value: from_lua(&value) });
        Ok(())
    })?)?;

    let (s, pack) = (shared.clone(), id.to_string());
    h.set("load", lua.create_function(move |lua, (_token, _target, address, input): (f64, String, String, Lv)| {
        let input = from_lua(&input);
        // A scene answers first: fixtures in, the real thing never called.
        if let Some(scene) = s.scene.borrow().as_ref() {
            let fixed = if let Some(value) = scene.load.get(&address) {
                Some(Resource::ok(value.clone(), None))
            } else if let Some(message) = scene.errors.get(&address) {
                Some(Resource { state: "error", error: Some(PackError::new("E0203", message.as_str().unwrap_or("failed"))), ..Resource::pending() })
            } else if scene.pending.contains(&address) {
                Some(Resource::pending())
            } else {
                None
            };
            if let Some(resource) = fixed {
                return to_lua(lua, &resource.json());
            }
        }
        let key = Shared::resource_key(&pack, &address, &input);
        let now = s.clock();
        let found = s.resources.borrow().get(&key).cloned();
        let resource = match found {
            // A stale result is still shown while the fresh one loads: never blank.
            Some(r) if r.state == "pending" || r.ttl <= 0.0 || now - r.at < r.ttl => r,
            stale => {
                let mut pending = stale.unwrap_or_else(Resource::pending);
                pending.at = now;
                if pending.state != "ok" {
                    pending = Resource::pending();
                }
                s.resources.borrow_mut().insert(key.clone(), pending.clone());
                s.push(Request::Load { pack: pack.clone(), key, address, input });
                pending
            }
        };
        to_lua(lua, &resource.json())
    })?)?;

    let (s, pack) = (shared.clone(), id.to_string());
    h.set("store_get", lua.create_function(move |lua, (_token, _target, name, at): (f64, String, String, Option<f64>)| {
        if let Some(value) = s.scene.borrow().as_ref().and_then(|scene| scene.stores.get(&name)) {
            return to_lua(lua, value);
        }
        let builtin = super::stores::BUILTIN.iter().any(|(n, _)| *n == name);
        let key = if builtin || name.contains('.') && !s.stores.borrow().exists(&format!("{pack}.{name}")) { name.clone() } else { format!("{pack}.{name}") };
        s.may_read_store(&pack, &key).map_err(raise)?;
        if builtin {
            return to_lua(lua, &super::stores::builtin(&name));
        }
        if !s.stores.borrow().exists(&key) {
            let names = s.stores.borrow().names();
            return Err(raise(PackError::new("E0412", format!("no store {name:?}")).nearest(&key, names.iter().map(String::as_str)).learn("echospot dev docs stores")));
        }
        to_lua(lua, &s.stores.borrow().get(&key, at))
    })?)?;

    let (s, pack) = (shared.clone(), id.to_string());
    h.set("store_put", lua.create_function(move |_, (_token, name, value): (f64, String, Lv)| {
        // Only the owner writes: the key is always under this pack's id.
        let key = format!("{pack}.{name}");
        if !s.stores.borrow().exists(&key) {
            return Err(raise(PackError::new("E0412", format!("{pack} declares no store {name:?}; only a pack's own stores can be written")).learn("echospot dev docs stores")));
        }
        let put = s.stores.borrow_mut().put(&key, from_lua(&value), super::audit::now());
        put.map_err(|e| raise(PackError::new("E0302", format!("store {name}: {e}"))))?;
        s.invalidate(&pack);
        Ok(())
    })?)?;

    let (s, pack) = (shared.clone(), id.to_string());
    h.set("nav", lua.create_function(move |_, (token, op, address, args): (f64, String, String, Lv)| {
        s.push(Request::Nav { pack: pack.clone(), token: token as u64, op, address, args: from_lua(&args) });
        Ok(())
    })?)?;

    let s = shared.clone();
    h.set("is_dry", lua.create_function(move |_, token: f64| Ok(s.dry.borrow().contains(&(token as u64))))?)?;

    let core: Vec<&str> = super::types::CORE.iter().map(|(name, _)| *name).filter(|n| !matches!(*n, "list" | "record" | "enum" | "series")).collect();
    let types: Table = lua.load(TYPES).set_name("=@echo/types").call(core)?;
    let ui: Table = lua.load(UI).set_name("=@echo/ui").call(h.clone())?;
    let echo: Table = lua.load(ECHO).set_name("=@echo").call((h, types.clone(), ui.clone()))?;
    host.register_module("@echo", echo)?;
    host.register_module("@echo/types", types)?;
    host.register_module("@echo/ui", ui)?;
    Ok(())
}

/// Starts a VM for pack `id` from `sources`.
pub fn start(id: &str, sources: Sources, shared: &Rc<Shared>) -> Script {
    let (setup_shared, setup_id) = (shared.clone(), id.to_string());
    let (log_shared, log_id) = (shared.clone(), id.to_string());
    // A pack describes, it doesn't draw (§2.1): `@echo/ui` is its UI, and raw `@rune/ui` isn't there.
    let options = ScriptOptions::new()
        .modules(["@rune/signal"])
        .setup(move |host| bind(host, &setup_shared, &setup_id))
        .on_log(move |level, text| {
            let level = match level {
                LogLevel::Info => "info",
                LogLevel::Warn => "warn",
                LogLevel::Error => "error",
            };
            log_shared.log(&log_id, level, text);
        });
    Script::with_options(id, sources, options)
}

/// A script's failure as an error that teaches.
pub fn script_error(e: &rune_luau::ScriptError) -> PackError {
    use rune_luau::ErrorKind;
    let mut err = PackError::luau(&e.message);
    err.code = match e.kind {
        ErrorKind::Syntax => "E0200",
        ErrorKind::Timeout => "E0204",
        ErrorKind::Missing => "E0201",
        _ if err.code != "E0203" => err.code,
        _ => "E0203",
    };
    if err.learn.is_empty() {
        err.learn = match err.code {
            "E0200" => "echospot dev check".into(),
            "E0201" => "echospot dev docs packs".into(),
            "E0204" => "echospot dev docs budgets".into(),
            _ => "echospot dev logs".into(),
        };
    }
    if err.at.starts_with("__pack") {
        err.at = "init.luau".into();
    }
    err
}

/// Asks a running VM what its pack declares.
pub fn describe(script: &Script) -> Result<Value, PackError> {
    if let Some(e) = script.error() {
        return Err(script_error(&e));
    }
    match script.query("__describe", std::iter::empty::<Data>()) {
        Ok(data) => Ok(super::json::from_data(&data)),
        Err(e) if e.kind == rune_luau::ErrorKind::Missing => Err(PackError::new("E0201", "init.luau must end with `return echo.pack { … }`").at("init.luau").learn("echospot dev docs packs")),
        Err(e) => Err(script_error(&e)),
    }
}

/// A Luau function of the pack's entry, by name.
pub fn export(script: &Script, name: &str) -> Option<Function> {
    script.module().and_then(|m| m.raw_get::<Option<Function>>(name).ok().flatten())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::packs::index::Index;

    pub fn write(dir: &Path, files: &[(&str, &str)]) {
        for (name, text) in files {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
    }

    pub fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("echospot-pack-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    const CURRENCY: &str = r#"--!strict
local echo = require("@echo")
local t = require("@echo/types")

return echo.pack {
    abilities = {
        convert = echo.ability {
            title = "Convert Currency",
            icon = "icons/convert.svg",
            kind = "read",
            input = { amount = t.number(), from = t.currency(), to = t.currency() },
            output = t.money(),
            examples = { "50 usd to inr", "€20 in yen" },
            run = function(input, cx)
                return cx:call("calc.convert", input)
            end,
        },
    },
}
"#;

    #[test]
    fn the_design_recipe_declares_itself() {
        let dir = temp("describe");
        write(&dir, &[("pack.toml", "id = \"currency\"\napi = 1\n"), ("init.luau", CURRENCY), ("boards/morning.luau", "local echo = require(\"@echo\")\nreturn echo.board { title = \"Morning\", tiles = { { \"calendar.next\", size = \"2x1\" }, \"currency.convert\" } }"), ("tests/broken.luau", "this is not luau")]);
        let shared = Rc::new(Shared::default());
        let (src, stamp) = sources(&dir).unwrap();
        let script = start("currency", src, &shared);
        let index = Index::from_describe(&describe(&script).unwrap()).unwrap();
        let convert = &index.abilities["convert"];
        assert_eq!((convert.title.as_str(), convert.kind.name(), convert.input.fields.len()), ("Convert Currency", "read", 3));
        assert_eq!(convert.output.as_ref().unwrap().kind, "money");
        assert_eq!(index.boards["morning"].tiles[0].span(), (2, 1));
        assert_eq!(index.boards["morning"].tiles[1].address, "currency.convert");
        assert_eq!(sources(&dir).unwrap().1, stamp);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mistakes_name_what_was_meant() {
        let shared = Rc::new(Shared::default());
        let case = |name: &str, init: &str| {
            let dir = temp(name);
            write(&dir, &[("pack.toml", "id = \"x\"\napi = 1\n"), ("init.luau", init)]);
            let script = start("x", sources(&dir).unwrap().0, &shared);
            let out = describe(&script).and_then(|v| Index::from_describe(&v));
            let _ = std::fs::remove_dir_all(&dir);
            out.unwrap_err()
        };
        let e = case("syntax", "return echo.pack {");
        assert_eq!(e.code, "E0200", "{e}");
        let e = case("noreturn", "local echo = require(\"@echo\")\nreturn { }");
        assert_eq!(e.code, "E0201", "{e}");
        let e = case("component", "local ui = require(\"@echo/ui\")\nlocal x = ui.Table2\nreturn {}");
        assert_eq!((e.code, e.nearest.first().map(String::as_str), e.at.as_str()), ("E0412", Some("Table"), "init.luau:2"), "{e}");
        let e = case("kind", "local echo = require(\"@echo\")\nreturn echo.pack { abilities = { a = echo.ability { kind = \"write\", run = function() end } } }");
        assert_eq!((e.code, e.at.as_str()), ("E0202", "abilities.a.kind"), "{e}");
        let e = case("io", "local f = io.open(\"/etc/passwd\")\nreturn {}");
        assert_eq!(e.code, "E0203", "{e}");
    }
}
