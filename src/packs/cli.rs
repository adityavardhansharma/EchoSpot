//! The command line a coding agent works through: `echospot dev …`, and `echospot call`, `bar`,
//! `fork`, `agent install`. Every command prints for a person, or with `--json` one JSON value
//! for a program. Nothing here can grant a permission: grants are read, never written.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use super::error::{PackError, Result};
use super::runtime::{Runtime, State};
use super::{api, audit, check, defs, design, devtest, docs, icon, inbox, scenes, skill, surface, system, template};

/// What a command came to: what to print, and whether it worked.
pub struct Outcome {
    pub ok: bool,
    pub text: String,
    pub json: Value,
}

impl Outcome {
    fn ok(text: impl Into<String>, json: Value) -> Self {
        Self { ok: true, text: text.into(), json }
    }
    fn fail(e: &PackError) -> Self {
        Self { ok: false, text: format!("error: {e}"), json: json!({ "ok": false, "error": e.json() }) }
    }
}

/// Arguments, apart from the flags among them.
struct Args {
    words: Vec<String>,
    flags: Vec<(String, Option<String>)>,
}

/// Flags that take the next word as their value.
const VALUED: &[&str] = &["--json", "--from", "--scene", "--out", "--pack", "--inbox", "--only", "--sheet", "--interval", "--agents-md", "--resolve", "-n", "--face"];

impl Args {
    fn parse(args: &[String]) -> Self {
        let (mut words, mut flags) = (Vec::new(), Vec::new());
        let mut i = 0;
        while i < args.len() {
            let arg = &args[i];
            if let Some((name, value)) = arg.split_once('=').filter(|(n, _)| n.starts_with("--")) {
                flags.push((name.to_string(), Some(value.to_string())));
            } else if arg.starts_with('-') && arg.len() > 1 && arg.parse::<f64>().is_err() {
                // `--json` alone asks for JSON output; followed by a value, it carries an input.
                let takes = VALUED.contains(&arg.as_str()) && args.get(i + 1).is_some_and(|next| arg != "--json" || next.trim_start().starts_with(['{', '[']));
                if takes {
                    flags.push((arg.clone(), args.get(i + 1).cloned()));
                    i += 1;
                } else {
                    flags.push((arg.clone(), None));
                }
            } else {
                words.push(arg.clone());
            }
            i += 1;
        }
        Self { words, flags }
    }
    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|(n, _)| n == name)
    }
    fn value(&self, name: &str) -> Option<&str> {
        self.flags.iter().find(|(n, v)| n == name && v.is_some()).and_then(|(_, v)| v.as_deref())
    }
    fn word(&self, at: usize) -> &str {
        self.words.get(at).map(String::as_str).unwrap_or("")
    }
    /// The JSON an input flag carries: `--json '{"amount": 50}'`.
    fn input(&self) -> Result<Value> {
        match self.value("--json") {
            Some(text) => serde_json::from_str(text).map_err(|e| PackError::new("E0301", format!("--json isn't JSON: {e}")).learn("echospot dev run <address> --json '{\"key\": \"value\"}'")),
            None => Ok(json!({})),
        }
    }
    fn wants_json(&self) -> bool {
        self.flags.iter().any(|(n, v)| n == "--json" && v.is_none()) || self.has("--json-out")
    }
}

/// The pack a command is about: the one named, or the one the shell is standing in.
fn pack_dir(named: &str) -> Result<PathBuf> {
    if !named.is_empty() {
        let as_path = PathBuf::from(named);
        if as_path.join("pack.toml").is_file() {
            return Ok(as_path.canonicalize().unwrap_or(as_path));
        }
        let dir = super::packs_dir().join(named);
        if dir.join("pack.toml").is_file() {
            return Ok(dir);
        }
        let system = system::dir().join(named);
        if system.join("pack.toml").is_file() {
            return Ok(system);
        }
        let known: Vec<String> = std::fs::read_dir(super::packs_dir()).into_iter().flatten().flatten().filter_map(|e| e.file_name().into_string().ok()).collect();
        return Err(PackError::new("E0300", format!("no pack {named:?} in {}", super::packs_dir().display())).nearest(named, known.iter().map(String::as_str)).learn("echospot dev list"));
    }
    let mut at = std::env::current_dir().ok();
    while let Some(dir) = at {
        if dir.join("pack.toml").is_file() {
            return Ok(dir);
        }
        at = dir.parent().map(Path::to_path_buf);
    }
    Err(PackError::new("E0100", "which pack? Name one, or run this inside a pack's folder").learn("echospot dev list"))
}

/// The pack named by `--pack`, by a leading `<pack>.` on `name`, or by where the shell is.
fn pack_for(args: &Args, name: &str) -> Result<PathBuf> {
    if let Some(pack) = args.value("--pack") {
        return pack_dir(pack);
    }
    if let Some((pack, _)) = name.split_once('.')
        && let Ok(dir) = pack_dir(pack) {
        return Ok(dir);
    }
    pack_dir("")
}

fn loaded(dir: &Path) -> Result<(Runtime, String)> {
    let mut rt = Runtime::for_cli();
    rt.load_all();
    let id = rt.load(dir, true).ok_or_else(|| PackError::new("E0100", format!("{} isn't a pack", dir.display())))?;
    rt.check_dependencies();
    Ok((rt, id))
}

fn error_lines(errors: &[PackError], mark: &str) -> String {
    errors.iter().map(|e| format!("{mark} {e}\n")).collect()
}

/// A scratch folder for what a command draws, when it wasn't told where to put it.
fn out_dir(args: &Args, pack: &str) -> PathBuf {
    let dir = match args.value("--out") {
        Some(path) if !path.ends_with(".png") => PathBuf::from(path),
        _ => std::env::temp_dir().join("echospot-dev").join(pack),
    };
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// What a headless render is told to draw. The app reads it from the environment when it starts.
pub const SNAP_ENV: &str = "ECHOSPOT_PACK_SNAP";

/// Draws a surface the way the palette would, without a screen: starts this same program
/// headless and has it write a PNG and its laid-out tree. `None` where that isn't possible
/// (no GPU, no display server to speak of).
pub fn snap(spec: &Value, png: &Path) -> Option<Value> {
    let exe = std::env::current_exe().ok()?;
    let tree = png.with_extension("tree.json");
    let _ = std::fs::remove_file(png);
    let mut child = std::process::Command::new(exe)
        .args(["open", "pack", "--snap"]).arg(png).arg("--tree").arg(&tree).args(["--scale", "2"])
        .env(SNAP_ENV, spec.to_string())
        .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().ok()?;
    let deadline = std::time::Instant::now() + Duration::from_secs(40);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if std::time::Instant::now() > deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => return None,
        }
    }
    let read = std::fs::read_to_string(&tree).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
    let _ = std::fs::remove_file(&tree);
    png.exists().then_some(read.unwrap_or(Value::Null))
}

fn scene_json(scene: &super::shared::Scene) -> Value {
    json!({
        "name": scene.name, "load": scene.load, "stores": scene.stores, "errors": scene.errors, "pending": scene.pending.iter().collect::<Vec<_>>(),
        "args": scene.args, "query": scene.query, "dark": scene.dark, "size": scene.size.map(|(w, h)| json!([w, h])),
    })
}

fn dev_new(args: &Args) -> Outcome {
    let (id, from) = (args.word(0), args.value("--from").unwrap_or("blank"));
    if id.is_empty() {
        let list: String = template::names().iter().map(|(n, about)| format!("  {n:<10} {about}\n")).collect();
        return Outcome { ok: false, text: format!("echospot dev new <id> [--from <template>]\n\nTemplates:\n{list}"), json: json!({ "ok": false, "templates": template::names().iter().map(|(n, a)| json!({ "name": n, "about": a })).collect::<Vec<_>>() }) };
    }
    let dir = match template::create(id, from) {
        Ok(dir) => dir,
        Err(e) => return Outcome::fail(&e),
    };
    let mut rt = Runtime::for_cli();
    rt.load_all();
    let report = check::check(&mut rt, &dir);
    let first = rt.packs.get(id).and_then(|p| p.index.abilities.keys().next().cloned()).unwrap_or_default();
    let files = if super::wasm::is_rust(&dir) {
        "  src/lib.rs     the abilities, in Rust\n  init.luau      the UI, in Luau\n  Cargo.toml     the crate; EchoSpot builds it for wasm32-wasip2\n  pack.toml      identity and permissions\n  INTENT.md      what was asked for; keep it current\n  tests/         run with `echospot dev test`\n"
    } else {
        "  init.luau      the pack\n  pack.toml      identity and permissions\n  INTENT.md      what was asked for; keep it current\n  tests/         run with `echospot dev test`\n"
    };
    let docs = if super::wasm::is_rust(&dir) { "rust" } else { "start" };
    let text = format!(
        "Created {}\n\n{files}\nNext:\n  echospot dev check {id}\n  echospot dev render {id}.{first}\n  echospot dev docs {docs}\n{}",
        dir.display(), if report.ok { String::new() } else { format!("\n{}", error_lines(&report.errors, "!")) });
    Outcome { ok: report.ok, text, json: json!({ "ok": report.ok, "pack": id, "dir": dir, "template": from, "check": report }) }
}

fn dev_list() -> Outcome {
    let mut rt = Runtime::for_cli();
    rt.load_all();
    let rows: Vec<Value> = rt.packs.values().map(|p| json!({
        "id": p.id, "name": p.name(), "state": p.state.name(), "system": p.system, "dir": p.dir, "abilities": p.index.abilities.keys().collect::<Vec<_>>(),
        "error": p.error.as_ref().map(|e| e.json()),
    })).collect();
    let mut text = String::new();
    for p in rt.packs.values() {
        text.push_str(&format!("{:<18} {:<12} {}{}\n", p.id, p.state.name(), p.index.abilities.keys().map(|a| format!("{}.{a}", p.id)).collect::<Vec<_>>().join(", "), if p.system { "  (system)" } else { "" }));
        if let Some(e) = &p.error {
            text.push_str(&format!("  ! {e}\n"));
        }
    }
    if text.is_empty() {
        text = format!("No packs yet in {}. Make one: echospot dev new <id>\n", super::packs_dir().display());
    }
    Outcome::ok(text, json!({ "ok": true, "packs": rows, "dir": super::packs_dir() }))
}

fn dev_api(args: &Args) -> Outcome {
    let mut rt = Runtime::for_cli();
    rt.load_all();
    match args.word(0) {
        "search" => {
            let words = args.words[1..].join(" ");
            let found = api::search(&rt, &words);
            let mut text: String = found.iter().take(40).map(|e| format!("{:<10} {:<28} {}\n{:<10} {}\n", e["what"].as_str().unwrap_or(""), e["name"].as_str().unwrap_or(""), e["about"].as_str().unwrap_or(""), "", e["signature"].as_str().unwrap_or(""))).collect();
            if found.is_empty() {
                let near = api::nearest(&rt, &words);
                text = format!("Nothing matches {words:?}.{}\n", if near.is_empty() { String::new() } else { format!(" Did you mean {}?", near.join(", ")) });
            }
            Outcome { ok: !found.is_empty(), text, json: json!({ "ok": !found.is_empty(), "query": words, "results": found }) }
        }
        "show" => {
            let name = args.word(1);
            match api::show(&rt, name) {
                Some(entry) => {
                    let mut text = format!("{}  ({})\n", entry["signature"].as_str().or(entry["name"].as_str()).unwrap_or(name), entry["what"].as_str().unwrap_or(""));
                    for (label, key) in [("", "title"), ("", "about"), ("", "description"), ("Kind: ", "kind_means"), ("Needs: ", "needs"), ("Example: ", "example"), ("Learn: ", "learn")] {
                        if let Some(value) = entry.get(key).and_then(Value::as_str).filter(|v| !v.is_empty()) {
                            text.push_str(&format!("{label}{value}\n"));
                        }
                    }
                    for (label, key) in [("Examples", "examples"), ("Faces it draws itself", "faces"), ("Used by", "used_by"), ("Verbs", "verbs"), ("Modifiers", "modifiers")] {
                        let list: Vec<&str> = entry.get(key).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
                        if !list.is_empty() {
                            text.push_str(&format!("{label}: {}\n", list.join(", ")));
                        }
                    }
                    Outcome::ok(text, json!({ "ok": true, "entry": entry }))
                }
                None => {
                    let near = api::nearest(&rt, name);
                    Outcome::fail(&PackError { code: "E0412", at: String::new(), message: format!("nothing is named {name:?}"), nearest: near, learn: "echospot dev api search <words>".into() })
                }
            }
        }
        _ => Outcome { ok: false, text: "echospot dev api search <words>\nechospot dev api show <name>\n".into(), json: json!({ "ok": false }) },
    }
}

fn findings_text(drawn: &check::Drawn) -> String {
    let mut out = String::new();
    if let Some(e) = &drawn.error {
        out.push_str(&format!("  ! {} in {}: {e}\n", drawn.target, drawn.scene));
    }
    for f in &drawn.findings {
        let mark = match f.severity {
            "error" => "!",
            "warn" => "~",
            _ => "·",
        };
        out.push_str(&format!("  {mark} {} in {} [{}] {}: {}\n", drawn.target, drawn.scene, f.rule, f.at, f.message));
        if !f.fix.is_empty() {
            out.push_str(&format!("      {}\n", f.fix));
        }
    }
    out
}

fn dev_check(args: &Args) -> Outcome {
    let dir = match pack_dir(args.word(0)) {
        Ok(dir) => dir,
        Err(e) => return Outcome::fail(&e),
    };
    let mut rt = Runtime::for_cli();
    rt.load_all();
    let mut report = check::check(&mut rt, &dir);
    let mut text = format!("{}  {}  {} abilit{}, {} surface{}  loaded in {:.1} ms\n", report.pack, report.state, report.abilities, if report.abilities == 1 { "y" } else { "ies" }, report.surfaces, if report.surfaces == 1 { "" } else { "s" }, report.load_ms);
    text.push_str(&error_lines(&report.errors, "!"));
    text.push_str(&error_lines(&report.warnings, "~"));
    for note in &report.notes {
        text.push_str(&format!("· {note}\n"));
    }
    for used in &report.used_by {
        text.push_str(&format!("  used: {used}\n"));
    }
    if args.has("--design") && report.errors.is_empty() {
        let out = out_dir(args, &report.pack);
        let pack = report.pack.clone();
        let no_png = args.has("--no-png");
        let mut budget = 40usize;
        // Scenes are made up again inside the render: by name for the host's, inline for the pack's own.
        let pack_scenes: Vec<_> = scenes::of_pack(&dir).into_iter().flatten().collect();
        let drawn = check::design(&mut rt, &pack, |target, scene| {
            if no_png || budget == 0 {
                return None;
            }
            budget -= 1;
            let png = out.join(format!("{}-{scene}.png", target.replace(':', "-")));
            let inline = pack_scenes.iter().find(|s| s.name == scene).map(scene_json);
            let tree = snap(&json!({ "dir": dir, "target": target, "scene": scene, "scene_data": inline }), &png)?;
            Some((png.display().to_string(), tree))
        });
        let (mut errors, mut warns) = (0, 0);
        for d in &drawn {
            errors += d.findings.iter().filter(|f| f.severity == "error").count() + usize::from(d.error.is_some());
            warns += d.findings.iter().filter(|f| f.severity == "warn").count();
            text.push_str(&findings_text(d));
        }
        let pngs = drawn.iter().filter(|d| !d.png.is_empty()).count();
        text.push_str(&format!("design: {} scene{} drawn, {errors} error{}, {warns} warning{}{}\n", drawn.len(), if drawn.len() == 1 { "" } else { "s" }, if errors == 1 { "" } else { "s" }, if warns == 1 { "" } else { "s" },
            if pngs > 0 { format!("; {pngs} PNG{} in {}", if pngs == 1 { "" } else { "s" }, out.display()) } else { "; no PNGs (no headless renderer here), blocks were checked".into() }));
        report.ok &= errors == 0;
        report.design = drawn;
    }
    if !report.wrote.is_empty() {
        text.push_str(&format!("wrote {}\n", report.wrote.join(", ")));
    }
    text.push_str(if report.ok { "ok\n" } else { "failed\n" });
    Outcome { ok: report.ok, text, json: serde_json::to_value(&report).unwrap_or(Value::Null) }
}

/// The scene a command names: one of the pack's own, or one of the host's.
fn find_scene(rt: &mut Runtime, pack: &str, dir: &Path, target: &str, name: &str) -> Result<super::shared::Scene> {
    let mut own = Vec::new();
    for scene in scenes::of_pack(dir) {
        match scene {
            Ok(scene) => own.push(scene),
            Err(e) if e.at.contains(&format!("{name}.luau")) => return Err(e),
            Err(_) => {}
        }
    }
    if let Some(scene) = own.iter().find(|s| s.name == name) {
        return Ok(scene.clone());
    }
    let stress = scenes::stress(rt, pack, target);
    if let Some(scene) = stress.iter().find(|s| s.name == name || s.name == format!("stress-{name}")) {
        return Ok(scene.clone());
    }
    let names: Vec<String> = own.iter().map(|s| s.name.clone()).chain(stress.iter().map(|s| s.name.clone())).collect();
    Err(PackError::new("E0300", format!("no scene {name:?}")).nearest(name, names.iter().map(String::as_str)).learn("echospot dev scenes"))
}

fn render_one(rt: &mut Runtime, pack: &str, dir: &Path, target: &str, scene: Option<super::shared::Scene>, png: Option<&Path>) -> Value {
    let name = scene.as_ref().map(|s| s.name.clone()).unwrap_or_else(|| "live".into());
    let inline = scene.as_ref().map(scene_json);
    match scenes::draw(rt, pack, target, scene) {
        Ok(blocks) => {
            let look = surface::look_of(rt, pack, target, &blocks, "glass");
            let mut findings = design::check(&blocks, look);
            let mut out = json!({ "ok": true, "target": target, "scene": name, "look": look.name, "tree": super::blocks::tree(&blocks), "blocks": blocks });
            if let Some(png) = png
                && let Some(tree) = snap(&json!({ "dir": dir, "target": target, "scene": name, "scene_data": inline }), png) {
                let measured = design::measure(&tree);
                let _ = design::mark(png, &measured, tree.get("scale").and_then(Value::as_f64).unwrap_or(1.0));
                findings.extend(measured);
                out["png"] = json!(png);
            }
            out["findings"] = serde_json::to_value(&findings).unwrap_or(Value::Null);
            out
        }
        Err(e) => json!({ "ok": false, "target": target, "scene": name, "error": e.json() }),
    }
}

fn render_text(result: &Value) -> String {
    let mut text = format!("{}  scene: {}\n", result["target"].as_str().unwrap_or(""), result["scene"].as_str().unwrap_or(""));
    if let Some(e) = result.get("error") {
        return format!("{text}error: {}\n", PackError::from_json(e));
    }
    text.push_str(result["tree"].as_str().unwrap_or(""));
    if let Some(png) = result.get("png").and_then(Value::as_str) {
        text.push_str(&format!("png: {png}\n"));
    }
    for f in result["findings"].as_array().into_iter().flatten() {
        text.push_str(&format!("  [{}] {}: {}\n", f["rule"].as_str().unwrap_or(""), f["at"].as_str().unwrap_or(""), f["message"].as_str().unwrap_or("")));
    }
    text
}

fn dev_render(args: &Args) -> Outcome {
    let name = args.word(0);
    if name.is_empty() {
        return Outcome { ok: false, text: "echospot dev render <surface> [--scene <name>] [--out <file.png>]\n".into(), json: json!({ "ok": false }) };
    }
    let run = || -> Result<Value> {
        let dir = pack_for(args, name)?;
        let (mut rt, id) = loaded(&dir)?;
        if let Some(error) = rt.packs.get(&id).and_then(|p| p.error.clone()).filter(|_| rt.packs[&id].state == State::Broken && rt.packs[&id].index.abilities.is_empty()) {
            return Err(error);
        }
        let mut target = scenes::target(&rt, &id, name)?;
        if let Some(face) = args.value("--face")
            && let Some(ability) = target.strip_prefix("ability:").and_then(|rest| rest.split(':').next()) {
            target = format!("ability:{ability}:{face}");
        }
        let scene = match (args.value("--scene"), args.value("--inbox")) {
            (Some(scene), _) => Some(find_scene(&mut rt, &id, &dir, &target, scene)?),
            // What a person was looking at when they pointed: the saved blocks stand in for every load.
            // What a person was looking at when they pointed: drawn again in that state, through
            // the code as it is now, beside what they saw then.
            (None, Some(report)) => {
                let found = inbox::find(&dir, report).ok_or_else(|| PackError::new("E0300", format!("no inbox report {report:?}")).learn("echospot dev inbox"))?;
                let png = match args.value("--out") {
                    Some(path) if path.ends_with(".png") => PathBuf::from(path),
                    _ => out_dir(args, &id).join(format!("inbox-{report}.png")),
                };
                let mut result = render_one(&mut rt, &id, &dir, &found.target, Some(inbox::scene(&found, report)), (!args.has("--no-png")).then_some(png.as_path()));
                result["then"] = json!({ "note": found.note, "element": found.element, "tree": found.tree, "screenshot": found.dir.join(&found.screenshot) });
                return Ok(result);
            }
            _ => None,
        };
        let png = match args.value("--out") {
            Some(path) if path.ends_with(".png") => PathBuf::from(path),
            _ => out_dir(args, &id).join(format!("{}-{}.png", target.replace(':', "-"), scene.as_ref().map(|s| s.name.as_str()).unwrap_or("live"))),
        };
        Ok(render_one(&mut rt, &id, &dir, &target, scene, (!args.has("--no-png")).then_some(png.as_path())))
    };
    match run() {
        Ok(result) => Outcome { ok: result["ok"] == json!(true), text: render_text(&result), json: result },
        Err(e) => Outcome::fail(&e),
    }
}

fn dev_scenes(args: &Args) -> Outcome {
    let run = || -> Result<Value> {
        let dir = pack_dir(args.word(0))?;
        let (mut rt, id) = loaded(&dir)?;
        let out = out_dir(args, &id);
        let mut results = Vec::new();
        let own: Vec<_> = scenes::of_pack(&dir).into_iter().flatten().collect();
        for target in scenes::targets(&rt, &id) {
            let mut all = own.clone();
            all.extend(scenes::stress(&mut rt, &id, &target));
            for scene in all {
                let png = out.join(format!("{}-{}.png", target.replace(':', "-"), scene.name));
                results.push(render_one(&mut rt, &id, &dir, &target, Some(scene), (!args.has("--no-png")).then_some(png.as_path())));
            }
        }
        Ok(json!({ "ok": results.iter().all(|r| r["ok"] == json!(true)), "pack": id, "out": out, "scenes": results }))
    };
    match run() {
        Ok(result) => {
            let text: String = result["scenes"].as_array().into_iter().flatten().map(|r| format!("{}\n", render_text(r))).collect();
            Outcome { ok: result["ok"] == json!(true), text: format!("{text}{} drawn, in {}\n", result["scenes"].as_array().map_or(0, Vec::len), result["out"].as_str().unwrap_or("")), json: result }
        }
        Err(e) => Outcome::fail(&e),
    }
}

/// Calls an ability and waits. `dry` stubs every effect and lists them.
fn run_address(address: &str, input: Value, dry: bool) -> (Runtime, Result<Value>) {
    let mut rt = Runtime::for_cli();
    rt.load_all();
    let result = rt.call(address, input, dry);
    rt.settle(Duration::from_secs(2));
    (rt, result)
}

fn dev_run(args: &Args, plain: bool) -> Outcome {
    let address = args.word(0);
    if address.is_empty() {
        return Outcome { ok: false, text: "echospot dev run <address> [--json '{…}'] [--dry]\n".into(), json: json!({ "ok": false }) };
    }
    let input = match args.input() {
        Ok(input) => input,
        Err(e) => return Outcome::fail(&e),
    };
    let dry = args.has("--dry");
    let (rt, result) = run_address(address, input, dry);
    match result {
        Ok(value) => {
            let shown = value.as_str().map(String::from).unwrap_or_else(|| serde_json::to_string_pretty(&value).unwrap_or_default());
            if plain {
                return Outcome::ok(format!("{shown}\n"), value);
            }
            let mut text = format!("{shown}\n");
            if dry {
                text.push_str(&format!("\ndry run: {} effect{} would have happened\n", rt.dry_log.len(), if rt.dry_log.len() == 1 { "" } else { "s" }));
                for effect in &rt.dry_log {
                    text.push_str(&format!("  {} {} {}\n", effect["kind"].as_str().unwrap_or(""), effect["address"].as_str().unwrap_or(""), effect["input"]));
                }
            }
            Outcome::ok(text, json!({ "ok": true, "value": value, "dry": dry, "effects": rt.dry_log }))
        }
        Err(e) => Outcome::fail(&e),
    }
}

fn dev_test(args: &Args) -> Outcome {
    let dir = match pack_dir(args.word(0)) {
        Ok(dir) => dir,
        Err(e) => return Outcome::fail(&e),
    };
    let report = devtest::run(&dir, args.has("--record"), args.value("--only"));
    let mut text = String::new();
    for case in &report.cases {
        text.push_str(&format!("{} {} › {}  ({} ms)\n", if case.ok { "ok  " } else { "FAIL" }, case.file, case.name, case.ms));
        if !case.ok {
            text.push_str(&format!("     {}\n", case.error.replace('\n', "\n     ")));
        }
    }
    if report.cases.is_empty() {
        text.push_str("no tests: write tests/*.luau (echospot dev docs tests)\n");
    }
    text.push_str(&format!("{} passed, {} failed\n", report.passed, report.failed));
    Outcome { ok: report.ok && !report.cases.is_empty(), text, json: serde_json::to_value(&report).unwrap_or(Value::Null) }
}

fn dev_icon(args: &Args) -> Outcome {
    let path = if args.word(0) == "check" { args.word(1) } else { args.word(0) };
    if path.is_empty() {
        return Outcome { ok: false, text: "echospot dev icon check <file.svg> [--sheet <out.png>]\n".into(), json: json!({ "ok": false }) };
    }
    let svg = match std::fs::read_to_string(path) {
        Ok(svg) => svg,
        Err(e) => return Outcome::fail(&PackError::new("E0600", format!("can't read {path}: {e}")).learn("echospot dev docs icons")),
    };
    let sheet = args.value("--sheet").map(PathBuf::from).unwrap_or_else(|| {
        let dir = std::env::temp_dir().join("echospot-dev").join("icons");
        let _ = std::fs::create_dir_all(&dir);
        dir.join(format!("{}-sheet.png", Path::new(path).file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "icon".into())))
    });
    let report = icon::check(&svg, Some(&sheet));
    let mut text = format!("{path}: {}\n", if report.ok { "ok" } else { "doesn't follow the spec" });
    for problem in &report.problems {
        text.push_str(&format!("  ! {problem}\n"));
    }
    text.push_str(&format!("  weight {:.0}% of the grid (EchoSpot's median {:.0}%)\n  nearest built-in: {} ({:.0}% alike)\n", report.weight * 100.0, report.median_weight * 100.0, report.nearest, report.similarity * 100.0));
    if let Some(sheet) = &report.sheet {
        text.push_str(&format!("  sheet: {}  (look at it)\n", sheet.display()));
    }
    Outcome { ok: report.ok, text, json: serde_json::to_value(&report).unwrap_or(Value::Null) }
}

fn dev_eject(args: &Args) -> Outcome {
    let component = args.word(0);
    if component.is_empty() {
        let list = template::EJECTABLE.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ");
        return Outcome { ok: false, text: format!("echospot dev eject <component>\n\nThese can be ejected: {list}\n"), json: json!({ "ok": false, "components": template::EJECTABLE.iter().map(|(n, _)| *n).collect::<Vec<_>>() }) };
    }
    match pack_for(args, "").and_then(|dir| template::eject(&dir, component)) {
        Ok(path) => {
            let name = path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            Outcome::ok(format!("Wrote {}\n\nUse it in place of ui.{name}:\n  local {name} = require(\"@self/ui/{name}\")\n  return {name} {{ … }}\n", path.display()), json!({ "ok": true, "path": path, "component": name }))
        }
        Err(e) => Outcome::fail(&e),
    }
}

/// Log lines and audit entries together, in the order they happened.
fn log_lines(pack: Option<&str>, n: usize) -> Vec<(f64, String, Value)> {
    use chrono::{Local, TimeZone};
    let when = |t: f64| Local.timestamp_opt(t as i64, 0).single().map(|d| d.format("%m-%d %H:%M:%S").to_string()).unwrap_or_default();
    let mut out: Vec<(f64, String, Value)> = std::fs::read_to_string(super::runtime::log_path()).unwrap_or_default().lines().filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|l| pack.is_none_or(|p| l["pack"] == json!(p)))
        .map(|l| (l["t"].as_f64().unwrap_or(0.0), format!("{}  {:<5} {:<12} {}", when(l["t"].as_f64().unwrap_or(0.0)), l["level"].as_str().unwrap_or(""), l["pack"].as_str().unwrap_or(""), l["text"].as_str().unwrap_or("")), json!({ "what": "log", "entry": l })))
        .collect();
    out.extend(audit::read().into_iter().filter(|e| pack.is_none_or(|p| e.pack == p)).map(|e| (e.t, format!("{}  audit", audit::line(&e)).replacen("  ", "  ", 1), json!({ "what": "audit", "entry": e }))));
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    let from = out.len().saturating_sub(n);
    out.split_off(from)
}

fn dev_logs(args: &Args) -> Outcome {
    let pack = Some(args.word(0)).filter(|p| !p.is_empty());
    let n = args.value("-n").and_then(|n| n.parse().ok()).unwrap_or(60);
    let lines = log_lines(pack, n);
    if args.has("--follow") {
        // Prints what there is, then each new line as it lands, until interrupted.
        let mut last = lines.last().map_or(0.0, |l| l.0);
        for (_, text, _) in &lines {
            println!("{text}");
        }
        loop {
            std::thread::sleep(Duration::from_millis(400));
            let since = last;
            for (t, text, _) in log_lines(pack, 400).into_iter().filter(|l| l.0 > since) {
                println!("{text}");
                last = t;
            }
        }
    }
    let text: String = if lines.is_empty() { "Nothing logged yet.\n".into() } else { lines.iter().map(|(_, text, _)| format!("{text}\n")).collect() };
    Outcome::ok(text, json!({ "ok": true, "lines": lines.into_iter().map(|(_, _, v)| v).collect::<Vec<_>>() }))
}

fn dev_inbox(args: &Args) -> Outcome {
    let dirs: Vec<PathBuf> = match pack_dir(args.word(0)) {
        Ok(dir) => vec![dir],
        // No pack named and not inside one: every pack's inbox.
        Err(_) if args.word(0).is_empty() => std::fs::read_dir(super::packs_dir()).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.join("inbox").is_dir()).collect(),
        Err(e) => return Outcome::fail(&e),
    };
    if let Some(name) = args.value("--resolve") {
        let found = dirs.iter().find_map(|dir| inbox::find(dir, name));
        return match found.map(|r| inbox::resolve(&r).map(|()| r)) {
            Some(Ok(report)) => Outcome::ok(format!("Resolved {name}: {}\n", report.note), json!({ "ok": true, "resolved": name })),
            _ => Outcome::fail(&PackError::new("E0300", format!("no inbox report {name:?}")).learn("echospot dev inbox")),
        };
    }
    let reports: Vec<inbox::Report> = dirs.iter().flat_map(|dir| inbox::list(dir)).filter(|r| !r.resolved || args.has("--all")).collect();
    let mut text = String::new();
    for r in &reports {
        let name = r.dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        text.push_str(&format!("{name}  {}  {}\n  \"{}\"\n", r.pack, r.target, r.note));
        if !r.element.is_empty() {
            text.push_str(&format!("  pointing at: {}\n", r.element));
        }
        if !r.error.is_empty() {
            text.push_str(&format!("  error showing: {}\n", r.error));
        }
        text.push_str(&format!("  {}\n  when fixed: echospot dev inbox {} --resolve {name}\n\n", r.dir.join("REPORT.md").display(), r.pack));
    }
    if reports.is_empty() {
        text = "The inbox is empty.\n".into();
    }
    Outcome::ok(text, json!({ "ok": true, "reports": reports.iter().map(|r| json!({ "name": r.dir.file_name().map(|n| n.to_string_lossy().to_string()), "dir": r.dir, "report": r })).collect::<Vec<_>>() }))
}

fn dev_grants(args: &Args) -> Outcome {
    let run = || -> Result<Value> {
        let dir = pack_dir(args.word(0))?;
        let (rt, id) = loaded(&dir)?;
        let pack = &rt.packs[&id];
        let diff = rt.grants.diff(&id, &pack.manifest.permissions);
        let ask = |a: &super::manifest::Ask| json!({ "kind": a.kind, "what": a.what, "sentence": a.sentence() });
        let labels = super::trust::labels(&id, &pack.manifest.permissions, &pack.index, &audit::read());
        Ok(json!({
            "ok": true, "pack": id, "state": pack.state.name(), "system": pack.system,
            "granted": diff.held.iter().map(ask).collect::<Vec<_>>(), "needed": if pack.system { Vec::new() } else { diff.new.iter().map(ask).collect::<Vec<_>>() },
            "no_longer_asked": diff.dropped.iter().filter(|a| a.kind != "picked").map(ask).collect::<Vec<_>>(),
            "picked": diff.dropped.iter().filter(|a| a.kind == "picked").map(|a| a.what.clone()).collect::<Vec<_>>(),
            "facts": labels,
            "how": "A person grants these in EchoSpot's own window: open EchoSpot, find the pack, Review Permissions. No command can.",
        }))
    };
    match run() {
        Ok(result) => {
            let list = |key: &str, mark: &str| result[key].as_array().into_iter().flatten().map(|a| format!("  {mark} {}\n", a["sentence"].as_str().unwrap_or(""))).collect::<String>();
            let mut text = format!("{}  {}\n", result["pack"].as_str().unwrap_or(""), result["state"].as_str().unwrap_or(""));
            if result["system"] == json!(true) {
                text.push_str("A system pack: what it asks for is implied.\n");
            }
            let (granted, needed) = (list("granted", "✓"), list("needed", "·"));
            if !granted.is_empty() {
                text.push_str(&format!("Granted:\n{granted}"));
            }
            if !needed.is_empty() {
                text.push_str(&format!("Not granted yet:\n{needed}\n{}\n", result["how"].as_str().unwrap_or("")));
            } else if granted.is_empty() {
                text.push_str("It asks for nothing.\n");
            }
            for fact in result["facts"].as_array().into_iter().flatten().filter_map(Value::as_str) {
                text.push_str(&format!("  {fact}\n"));
            }
            Outcome::ok(text, result)
        }
        Err(e) => Outcome::fail(&e),
    }
}

fn dev_docs(args: &Args) -> Outcome {
    let rt = Runtime::for_cli();
    let name = args.word(0);
    match docs::topic(&rt, name) {
        Some(page) => Outcome::ok(format!("{page}\n"), json!({ "ok": true, "topic": if name.is_empty() { "index" } else { name }, "markdown": page, "topics": docs::TOPICS.iter().map(|(n, a)| json!({ "name": n, "about": a })).collect::<Vec<_>>() })),
        None => Outcome::fail(&PackError::new("E0412", format!("no topic {name:?}")).nearest(name, docs::TOPICS.iter().map(|(n, _)| *n)).learn("echospot dev docs")),
    }
}

/// EchoSpot's own screens, to compare a pack's with.
const EXEMPLARS: &[&str] = &["clipboard", "emoji", "files", "windows", "snippets", "quicklinks", "notes", "schedule", "store", "themes", "extensions"];

fn dev_exemplar(args: &Args) -> Outcome {
    let name = args.word(0);
    if !EXEMPLARS.contains(&name) {
        return Outcome::fail(&PackError::new("E0412", format!("no exemplar {name:?}; there are {}", EXEMPLARS.join(", "))).nearest(name, EXEMPLARS.iter().copied()).learn("echospot dev exemplar clipboard"));
    }
    let png = match args.value("--out") {
        Some(path) => PathBuf::from(path),
        None => out_dir(args, "exemplars").join(format!("{name}.png")),
    };
    let Ok(exe) = std::env::current_exe() else { return Outcome::fail(&PackError::new("E0900", "can't find this program to start it headless")) };
    let done = std::process::Command::new(exe).args(["open", name, "--snap"]).arg(&png).args(["--scale", "2"]).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
    if done.is_ok_and(|s| s.success()) && png.exists() {
        Outcome::ok(format!("{}\n", png.display()), json!({ "ok": true, "exemplar": name, "png": png }))
    } else {
        Outcome::fail(&PackError::new("E0900", "couldn't render headless here: it needs a GPU the process can reach"))
    }
}

const DEV_HELP: &str = "echospot dev: build and check packs\n";

pub fn dev_help() -> String {
    let mut out = String::from(DEV_HELP);
    for (command, purpose) in docs::COMMANDS.iter().filter(|(c, _)| c.starts_with("echospot dev")) {
        out.push_str(&format!("\n  {command}\n      {purpose}"));
    }
    out.push_str("\n\nEvery command takes --json. Start with: echospot dev docs start\n");
    out
}

fn finish(outcome: Outcome, json: bool) -> i32 {
    if json {
        println!("{}", serde_json::to_string_pretty(&outcome.json).unwrap_or_default());
    } else if outcome.ok {
        print!("{}", outcome.text);
    } else {
        eprint!("{}{}", outcome.text, if outcome.text.ends_with('\n') { "" } else { "\n" });
    }
    i32::from(!outcome.ok)
}

/// `echospot dev <command> …`. Returns the process's exit code.
pub fn dev(raw: &[String]) -> i32 {
    let command = raw.first().map(String::as_str).unwrap_or("");
    let args = Args::parse(raw.get(1..).unwrap_or(&[]));
    // `run` and `render` take `--json '{…}'` as input; a bare `--json` asks for JSON output.
    let json = args.wants_json();
    let outcome = match command {
        "new" => dev_new(&args),
        "list" | "ls" => dev_list(),
        "api" => dev_api(&args),
        "check" => dev_check(&args),
        "render" => dev_render(&args),
        "scenes" => dev_scenes(&args),
        "run" => dev_run(&args, false),
        "test" => dev_test(&args),
        "icon" => dev_icon(&args),
        "eject" => dev_eject(&args),
        "logs" => dev_logs(&args),
        "inbox" => dev_inbox(&args),
        "grants" => dev_grants(&args),
        "docs" => dev_docs(&args),
        "exemplar" => dev_exemplar(&args),
        "defs" => match pack_dir(args.word(0)).and_then(|dir| loaded(&dir)) {
            Ok((rt, id)) => Outcome::ok(defs::definitions(&rt, &id), json!({ "ok": true, "definitions": defs::definitions(&rt, &id) })),
            Err(e) => Outcome::fail(&e),
        },
        "" | "help" | "--help" | "-h" => Outcome::ok(dev_help(), json!({ "ok": true, "commands": docs::COMMANDS.iter().map(|(c, p)| json!({ "command": c, "purpose": p })).collect::<Vec<_>>() })),
        other => {
            let known = ["new", "list", "api", "check", "render", "scenes", "run", "test", "icon", "eject", "logs", "inbox", "grants", "docs", "exemplar"];
            Outcome::fail(&PackError::new("E0412", format!("no command `echospot dev {other}`")).nearest(other, known).learn("echospot dev help"))
        }
    };
    finish(outcome, json)
}

/// `echospot call <address> [--json '{…}'] [--dry]`: an ability's value, and nothing else.
pub fn call(raw: &[String]) -> i32 {
    let args = Args::parse(raw);
    let outcome = dev_run(&args, true);
    if outcome.ok {
        match &outcome.json {
            Value::String(s) => println!("{s}"),
            other => println!("{}", serde_json::to_string_pretty(other).unwrap_or_default()),
        }
        0
    } else {
        finish(outcome, args.wants_json())
    }
}

/// `echospot bar <address>`: one line of JSON per update, in the shape Waybar's custom modules
/// read. The text is the ability's `line` face.
pub fn bar(raw: &[String]) -> i32 {
    let args = Args::parse(raw);
    let address = args.word(0).to_string();
    if address.is_empty() {
        eprintln!("echospot bar <address> [--json '{{…}}'] [--interval 30s]\n\nPrints one JSON line per update: {{\"text\", \"tooltip\", \"class\"}}. In Waybar:\n  \"custom/echospot\": {{ \"exec\": \"echospot bar github.prs\", \"return-type\": \"json\" }}");
        return 1;
    }
    let input = args.input().unwrap_or_else(|_| json!({}));
    let mut rt = Runtime::for_cli();
    rt.load_all();
    let declared = address.split_once('.').and_then(|(pack, name)| rt.packs.get(pack).and_then(|p| p.ability(name)).map(|a| a.refresh.clone())).unwrap_or_default();
    let interval = args.value("--interval").and_then(super::triggers::parse_every).or_else(|| super::triggers::parse_every(&declared)).unwrap_or(30.0).max(1.0);
    let mut last = String::new();
    loop {
        // What changed on disk since the last turn is picked up, as the palette would.
        rt.load_all();
        rt.shared.resources.borrow_mut().clear();
        let line = match rt.call(&address, input.clone(), false) {
            Ok(value) => {
                let face = super::faces::of_value(&mut rt, &address, "line", &input, &json!({}), &value).ok().flatten();
                let text = face.as_ref().and_then(|b| b.get("text")).and_then(Value::as_str).map(String::from).unwrap_or_else(|| super::blocks::show(&value, super::faces::output_type(&rt, &address).as_ref()));
                let class = face.as_ref().and_then(|b| b.get("intent")).and_then(Value::as_str).unwrap_or("");
                json!({ "text": text, "tooltip": super::faces::title_of(&rt, &address), "class": class, "alt": address })
            }
            Err(e) => json!({ "text": "!", "tooltip": e.to_string(), "class": "error", "alt": address }),
        };
        let line = line.to_string();
        if line != last {
            println!("{line}");
            use std::io::Write;
            if std::io::stdout().flush().is_err() {
                return 0;
            }
            last = line;
        }
        std::thread::sleep(Duration::from_secs_f64(interval));
    }
}

/// `echospot fork <id>`: a system pack's source, in the user's packs.
pub fn fork(raw: &[String]) -> i32 {
    let args = Args::parse(raw);
    let id = args.word(0);
    if id.is_empty() {
        println!("echospot fork <id>\n\nSystem packs:");
        for pack in system::PACKS {
            println!("  {}", pack.id);
        }
        return 1;
    }
    let outcome = match system::fork(id) {
        Ok(dir) => Outcome::ok(format!("Forked {id} to {}\n\nIt is yours now: edit init.luau and EchoSpot reloads it. Because it is yours, it asks for its\npermissions like any other pack: open EchoSpot and choose Review Permissions.\nRemove the folder to get EchoSpot's own back.\n", dir.display()), json!({ "ok": true, "pack": id, "dir": dir })),
        Err(e) => Outcome::fail(&e),
    };
    finish(outcome, args.wants_json())
}

/// `echospot agent install [--agents-md <file>]`.
pub fn agent(raw: &[String]) -> i32 {
    let args = Args::parse(raw);
    if args.word(0) != "install" {
        println!("echospot agent install [--agents-md <file>]\n\nWrites the Claude Code skill to {}, and with --agents-md, a section\ninto that AGENTS.md for Codex and other agents.", skill::skill_path().display());
        return 1;
    }
    let targets: Vec<PathBuf> = args.flags.iter().filter(|(n, v)| n == "--agents-md" && v.is_some()).filter_map(|(_, v)| v.as_ref().map(PathBuf::from)).collect();
    let outcome = match skill::install(&targets) {
        Ok(wrote) => Outcome::ok(format!("{}\nAsk your agent to \"make me a command that…\".\n", wrote.iter().map(|p| format!("Wrote {}\n", p.display())).collect::<String>()), json!({ "ok": true, "wrote": wrote })),
        Err(e) => Outcome::fail(&PackError::new("E0200", format!("couldn't write the skill: {e}"))),
    };
    finish(outcome, args.wants_json())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Args {
        Args::parse(&words.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn flags_and_words_are_told_apart() {
        let a = args(&["github.merge", "--json", "{\"n\": 1}", "--dry"]);
        assert_eq!((a.word(0), a.input().unwrap(), a.has("--dry"), a.wants_json()), ("github.merge", json!({ "n": 1 }), true, false));
        // A bare `--json` asks for JSON out; it doesn't swallow the next word.
        let b = args(&["--json", "prs", "--scene", "failing"]);
        assert_eq!((b.word(0), b.value("--scene"), b.wants_json()), ("prs", Some("failing"), true));
        let c = args(&["x.y", "--json={\"a\":2}", "-n", "5"]);
        assert_eq!((c.input().unwrap(), c.value("-n")), (json!({ "a": 2 }), Some("5")));
        assert_eq!(args(&["x.y", "--json", "{nope"]).input().unwrap_err().code, "E0301");
    }

    #[test]
    fn help_lists_every_command() {
        let help = dev_help();
        for command in ["new", "check", "render", "scenes", "run", "test", "icon check", "eject", "logs", "inbox", "grants", "docs", "api search", "api show"] {
            assert!(help.contains(&format!("echospot dev {command}")), "{command} is missing from help");
        }
    }
}
