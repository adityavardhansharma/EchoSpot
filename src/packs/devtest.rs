//! `echospot dev test`: a pack's `tests/*.luau`, run against the real broker with the world
//! held still. Effects are stubbed and listed, abilities can be mocked, stores set, surfaces
//! drawn in scenes, and HTTP answers from `fixtures/http/` unless `--record` is capturing it.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use rune_luau::mlua::{self, Function, Lua, Table, Value as Lv};
use serde::Serialize;
use serde_json::{Value, json};

use super::error::PackError;
use super::json::{from_lua, to_lua};
use super::runtime::Runtime;
use super::shared::Scene;

#[derive(Clone, Debug, Default, Serialize)]
pub struct Case {
    pub file: String,
    pub name: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub error: String,
    pub ms: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Report {
    pub ok: bool,
    pub pack: String,
    pub passed: usize,
    pub failed: usize,
    pub cases: Vec<Case>,
}

fn raise(e: PackError) -> mlua::Error {
    mlua::Error::runtime(e.to_string())
}

fn fail(message: String) -> mlua::Error {
    mlua::Error::runtime(message)
}

/// The scene a test runs in: its mocks answer first, and everything else has effects stubbed.
fn scene_of(mocks: &Value) -> Scene {
    Scene::from_json("test", mocks)
}

/// Builds `t`, the table a test function is handed.
fn harness(lua: &Lua, rt: &Rc<RefCell<Runtime>>, pack: &str, dir: &Path) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    // What the test has mocked so far: `{ load = {}, errors = {}, stores = {} }`.
    let mocks = Rc::new(RefCell::new(json!({ "load": {}, "errors": {}, "stores": {} })));
    let apply = {
        let (rt, mocks) = (rt.clone(), mocks.clone());
        move || rt.borrow_mut().set_scene(Some(scene_of(&mocks.borrow())))
    };
    apply();

    let (r, p) = (rt.clone(), pack.to_string());
    t.set("call", lua.create_function(move |lua, (_, address, input): (Table, String, Option<Lv>)| {
        let address = if address.contains('.') { address } else { format!("{p}.{address}") };
        let out = r.borrow_mut().call(&address, input.map(|v| from_lua(&v)).unwrap_or(json!({})), false).map_err(raise)?;
        to_lua(lua, &out)
    })?)?;

    let (r, p) = (rt.clone(), pack.to_string());
    t.set("try", lua.create_function(move |lua, (_, address, input): (Table, String, Option<Lv>)| {
        let address = if address.contains('.') { address } else { format!("{p}.{address}") };
        match r.borrow_mut().call(&address, input.map(|v| from_lua(&v)).unwrap_or(json!({})), false) {
            Ok(out) => Ok((true, to_lua(lua, &out)?)),
            Err(e) => Ok((false, to_lua(lua, &e.json())?)),
        }
    })?)?;

    let (m, a) = (mocks.clone(), apply.clone());
    t.set("mock", lua.create_function(move |_, (_, address, value): (Table, String, Lv)| {
        m.borrow_mut()["load"][address.as_str()] = from_lua(&value);
        m.borrow_mut()["errors"].as_object_mut().map(|e| e.remove(&address));
        a();
        Ok(())
    })?)?;

    let (m, a) = (mocks.clone(), apply.clone());
    t.set("fail", lua.create_function(move |_, (_, address, message): (Table, String, Option<String>)| {
        m.borrow_mut()["errors"][address.as_str()] = json!(message.unwrap_or_else(|| "failed".into()));
        m.borrow_mut()["load"].as_object_mut().map(|e| e.remove(&address));
        a();
        Ok(())
    })?)?;

    let (m, a) = (mocks.clone(), apply.clone());
    t.set("store", lua.create_function(move |_, (_, name, value): (Table, String, Lv)| {
        m.borrow_mut()["stores"][name.as_str()] = from_lua(&value);
        a();
        Ok(())
    })?)?;

    // A surface, drawn. With a scene's name, in that scene; otherwise with the test's own mocks.
    let draw = {
        let (rt, pack, dir, mocks) = (rt.clone(), pack.to_string(), dir.to_path_buf(), mocks.clone());
        move |surface: String, scene: Option<String>| -> mlua::Result<Value> {
            let mut rt = rt.borrow_mut();
            let target = super::scenes::target(&rt, &pack, &surface).map_err(raise)?;
            let scene = match scene {
                Some(name) => {
                    let own = super::scenes::of_pack(&dir).into_iter().flatten().find(|s| s.name == name);
                    let found = match own {
                        Some(scene) => Some(scene),
                        None => super::scenes::stress(&mut rt, &pack, &target).into_iter().find(|s| s.name == name || s.name == format!("stress-{name}")),
                    };
                    found.ok_or_else(|| fail(format!("no scene {name:?}: write scenes/{name}.luau, or use one of the host's (empty, one, thousand, long-titles, missing-icons, slow-load, error, light, dark, narrow)")))?
                }
                None => scene_of(&mocks.borrow()),
            };
            let blocks = super::scenes::draw(&mut rt, &pack, &target, Some(scene)).map_err(raise);
            rt.set_scene(Some(scene_of(&mocks.borrow())));
            blocks
        }
    };
    let d = draw.clone();
    t.set("render", lua.create_function(move |lua, (_, surface, scene): (Table, String, Option<String>)| to_lua(lua, &d(surface, scene)?))?)?;
    let d = draw.clone();
    t.set("tree", lua.create_function(move |_, (_, surface, scene): (Table, String, Option<String>)| Ok(super::blocks::tree(&d(surface, scene)?)))?)?;

    let r = rt.clone();
    t.set("effects", lua.create_function(move |lua, _: Table| to_lua(lua, &Value::Array(r.borrow().dry_log.clone())))?)?;

    let r = rt.clone();
    t.set("logs", lua.create_function(move |_, _: Table| Ok(r.borrow().shared.logs.borrow().iter().map(|l| format!("{} {}", l.level, l.text)).collect::<Vec<_>>()))?)?;

    t.set("equal", lua.create_function(|_, (_, got, want, what): (Table, Lv, Lv, Option<String>)| {
        let (got, want) = (from_lua(&got), from_lua(&want));
        // An empty Luau table reads as an empty list either way.
        let same = got == want || got.as_array().is_some_and(Vec::is_empty) && want.as_object().is_some_and(|m| m.is_empty());
        if same { Ok(()) } else { Err(fail(format!("{}expected {want}, got {got}", what.map(|w| format!("{w}: ")).unwrap_or_default()))) }
    })?)?;
    t.set("ok", lua.create_function(|_, (_, value, what): (Table, Lv, Option<String>)| {
        if matches!(value, Lv::Nil | Lv::Boolean(false)) { Err(fail(what.unwrap_or_else(|| "expected something true".into()))) } else { Ok(()) }
    })?)?;
    let find: Function = lua.load("return function(text, pattern) return string.find(text, pattern) ~= nil end").eval()?;
    t.set("match", lua.create_function(move |_, (_, text, pattern, what): (Table, String, String, Option<String>)| {
        if find.call::<bool>((text.clone(), pattern.clone()))? { Ok(()) } else { Err(fail(format!("{}{pattern:?} isn't in:\n{text}", what.map(|w| format!("{w}: ")).unwrap_or_default()))) }
    })?)?;
    Ok(t)
}

/// A test's failure as one readable message: where it was raised, and why.
fn message(e: &mlua::Error) -> String {
    let text = e.to_string();
    let first = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().trim_start_matches("runtime error: ");
    let rest: Vec<&str> = text.lines().skip(1).take_while(|l| !l.starts_with("stack traceback")).collect();
    if rest.is_empty() { first.to_string() } else { format!("{first}\n{}", rest.join("\n")) }
}

/// Runs every test of the pack in `dir`. `record` lets HTTP reach the network and saves what
/// comes back as fixtures.
pub fn run(dir: &Path, record: bool, only: Option<&str>) -> Report {
    let mut rt = Runtime::ephemeral();
    // The pack's real grants, read and never written: a test can't do what a person hasn't allowed.
    rt.grants = super::grants::Grants::load().read_only();
    rt.fixtures = Some(dir.join("fixtures").join("http"));
    rt.record = record;
    if record {
        let _ = std::fs::create_dir_all(dir.join("fixtures").join("http"));
    }
    rt.load_all();
    let pack = rt.load(dir, true).unwrap_or_default();
    let mut report = Report { pack: pack.clone(), ..Default::default() };
    if let Some(error) = rt.packs.get(&pack).and_then(|p| p.error.clone()) {
        report.cases.push(Case { file: "init.luau".into(), name: "the pack loads".into(), ok: false, error: error.to_string(), ms: 0 });
        report.failed = 1;
        return report;
    }
    let rt = Rc::new(RefCell::new(rt));
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir.join("tests")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "luau")).collect();
    files.sort();
    for file in files {
        let name = file.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let lua = Lua::new();
        let _ = lua.sandbox(true);
        let loaded = std::fs::read_to_string(&file).map_err(|e| e.to_string()).and_then(|source| lua.load(&source).set_name(format!("=tests/{name}")).eval::<Lv>().map_err(|e| message(&e)));
        let cases = match loaded {
            Ok(Lv::Table(cases)) => cases,
            Ok(_) => {
                report.cases.push(Case { file: name.clone(), name: "the file loads".into(), ok: false, error: "a test file returns a table of functions: return { [\"it works\"] = function(t) … end }".into(), ms: 0 });
                continue;
            }
            Err(error) => {
                report.cases.push(Case { file: name.clone(), name: "the file loads".into(), ok: false, error, ms: 0 });
                continue;
            }
        };
        let mut named: Vec<(String, Function)> = cases.pairs::<String, Function>().flatten().collect();
        named.sort_by(|a, b| a.0.cmp(&b.0));
        for (case, function) in named {
            if only.is_some_and(|o| !case.contains(o) && !name.contains(o)) {
                continue;
            }
            // Each test starts clean: no mocks, no stubbed effects from the one before.
            rt.borrow_mut().dry_log.clear();
            let started = Instant::now();
            let outcome = harness(&lua, &rt, &pack, dir).and_then(|t| function.call::<()>(t));
            rt.borrow_mut().settle(Duration::from_secs(5));
            let ms = started.elapsed().as_millis() as u64;
            match outcome {
                Ok(()) => report.cases.push(Case { file: name.clone(), name: case, ok: true, error: String::new(), ms }),
                Err(e) => report.cases.push(Case { file: name.clone(), name: case, ok: false, error: message(&e), ms }),
            }
        }
    }
    rt.borrow_mut().set_scene(None);
    report.passed = report.cases.iter().filter(|c| c.ok).count();
    report.failed = report.cases.len() - report.passed;
    report.ok = report.failed == 0;
    report
}
