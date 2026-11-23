//! `echospot dev check`: everything that can be known about a pack before it runs. It compiles,
//! reads every declaration, checks names and addresses in the source, the icons and the
//! examples, writes the type definitions, and with `--design` draws every scene and checks it.

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use super::builtins::Gate;
use super::design::{self, Finding};
use super::error::PackError;
use super::runtime::{Runtime, State};
use super::scenes;

#[derive(Clone, Debug, Default, Serialize)]
pub struct Drawn {
    pub target: String,
    pub scene: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<PackError>,
    pub findings: Vec<Finding>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub png: String,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct Report {
    pub ok: bool,
    pub pack: String,
    pub state: String,
    pub errors: Vec<PackError>,
    pub warnings: Vec<PackError>,
    /// Suggestions: nothing is wrong, something could be better.
    pub notes: Vec<String>,
    /// Files the check wrote: the type definitions.
    pub wrote: Vec<String>,
    /// What other packs use of this one.
    pub used_by: Vec<String>,
    pub abilities: usize,
    pub surfaces: usize,
    pub load_ms: f64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub design: Vec<Drawn>,
}

/// The names a module was bound to: `local ui = require("@echo/ui")` binds `ui`.
fn bindings(source: &str, module: &str) -> Vec<String> {
    let needle = format!("require(\"{module}\")");
    source.lines().filter(|l| l.contains(&needle)).filter_map(|l| {
        let before = l.split('=').next()?.trim();
        before.strip_prefix("local ").map(|name| name.split(':').next().unwrap_or(name).trim().to_string())
    }).collect()
}

/// Every `<var>.<name>` or `<var>:<name>` in a file, with its line.
fn members(source: &str, var: &str, sep: char) -> Vec<(String, usize)> {
    let mut out = Vec::new();
    let needle = format!("{var}{sep}");
    for (n, line) in source.lines().enumerate() {
        let code = line.split("--").next().unwrap_or(line);
        let mut from = 0;
        while let Some(at) = code[from..].find(&needle) {
            let start = from + at;
            from = start + needle.len();
            // A whole word: `gui.` is not `ui.`, and a string's contents aren't code.
            let boundary = start == 0 || !code[..start].chars().next_back().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.' || c == ':');
            let in_string = code[..start].matches('"').count() % 2 == 1;
            if !boundary || in_string {
                continue;
            }
            let name: String = code[from..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
            if !name.is_empty() {
                out.push((name, n + 1));
            }
        }
    }
    out
}

/// Names and addresses the source uses that don't exist, before anything runs.
pub fn lint(rt: &Runtime, pack: &str) -> (Vec<PackError>, Vec<PackError>) {
    let (mut errors, mut warnings) = (Vec::new(), Vec::new());
    let Some(p) = rt.packs.get(pack) else { return (errors, warnings) };
    let Ok((sources, _)) = super::vm::sources(&p.dir) else { return (errors, warnings) };
    let ui_names = super::vm::ui_names();
    let type_names: Vec<&str> = super::types::CORE.iter().map(|(n, _)| *n).chain(["optional", "is", "plain"]).collect();
    let known: std::collections::HashSet<String> = rt.addresses().into_iter().map(|a| a.0).collect();
    let specs = rt.specs();
    for file in sources.paths().filter(|f| !f.starts_with("__") && f.ends_with(".luau")) {
        let text = sources.get(file).unwrap_or("");
        let mut unknown = |space: &str, names: &[&str], found: Vec<(String, usize)>, learn: &str| {
            for (name, line) in found {
                if !names.contains(&name.as_str()) {
                    let what = if space == "ui" { "component" } else if space == "t" { "type" } else { "function" };
                    let mut e = PackError::new("E0412", format!("{space} has no {what} '{name}'")).at(format!("{file}:{line}")).nearest(&name, names.iter().copied()).learn(learn);
                    if let Some(first) = e.nearest.first().filter(|_| space == "ui") {
                        e.learn = format!("echospot dev api show {first}");
                    }
                    errors.push(e);
                }
            }
        };
        for var in bindings(text, "@echo/ui") {
            unknown("ui", &ui_names.iter().map(String::as_str).chain(["resolve", "pick"]).collect::<Vec<_>>(), members(text, &var, '.'), "echospot dev docs ui");
        }
        for var in bindings(text, "@echo/types") {
            unknown("t", &type_names, members(text, &var, '.'), "echospot dev docs types");
        }
        for var in bindings(text, "@echo") {
            unknown("echo", super::vm::ECHO_NAMES, members(text, &var, '.'), "echospot dev docs echo");
        }
        unknown("cx", super::vm::CX_NAMES, members(text, "cx", ':'), "echospot dev docs echo");

        // Addresses written out in the code: they must exist, and the pack must be allowed them.
        for (address, line) in super::graph::calls_in(text) {
            let at = format!("{file}:{line}");
            let own = address.starts_with(&format!("{pack}."));
            let store = address.contains(".stores.") && rt.shared.stores.borrow().exists(&address.replacen(".stores.", ".", 1));
            if !known.contains(&address) && !store && !address.starts_with("raycast.") {
                errors.push(PackError::new("E0300", format!("no ability at {address:?}")).at(at).nearest(&address, known.iter().map(String::as_str)).learn(format!("echospot dev api search {}", address.replace('.', " "))));
                continue;
            }
            if own || p.system {
                continue;
            }
            let perms = &p.manifest.permissions;
            let declared = perms.calls.iter().any(|c| *c == address);
            let need = match specs.iter().find(|s| s.address == address).map(|s| s.gate) {
                Some(Gate::Free) => None,
                Some(Gate::Calls) | None if !declared => Some(format!("calls = [\"{address}\"]")),
                Some(Gate::Clipboard(level)) if perms.clipboard != "read" && perms.clipboard != level => Some(format!("clipboard = \"{level}\"")),
                Some(Gate::Net) if perms.net.is_empty() => Some("net = [\"<host>\"]".to_string()),
                Some(Gate::Run) if perms.run.is_empty() => Some("run = [{ bin = \"/usr/bin/…\", args = [] }]".to_string()),
                Some(Gate::Ai) if perms.ai.is_none() => Some("ai = { budget_usd_month = 1.00 }".to_string()),
                _ => None,
            };
            if let Some(need) = need {
                errors.push(PackError::new("E0401", format!("{pack} uses {address} but pack.toml doesn't ask for it: add {need} under [permissions]")).at(at).learn("echospot dev docs pack.toml"));
            }
        }
    }
    // Icons: declared ones must exist and follow the spec.
    let mut icons: Vec<(String, String)> = p.index.abilities.iter().map(|(n, a)| (format!("abilities.{n}"), a.icon.clone())).collect();
    icons.extend(p.index.verbs.iter().map(|(n, v)| (format!("verbs.{n}"), v.icon.clone())));
    icons.extend(p.index.boards.iter().map(|(n, b)| (format!("boards.{n}"), b.icon.clone())));
    icons.extend(p.index.surfaces.iter().map(|(n, s)| (format!("surfaces.{n}"), s.icon.clone())));
    let mut seen = Vec::new();
    for (at, icon) in icons {
        if icon.is_empty() || !icon.ends_with(".svg") || seen.contains(&icon) {
            continue;
        }
        seen.push(icon.clone());
        match std::fs::read_to_string(p.dir.join(&icon)) {
            Ok(svg) => {
                for problem in super::icon::check(&svg, None).problems {
                    warnings.push(PackError::new("E0600", problem).at(icon.clone()).learn(format!("echospot dev icon check {icon}")));
                }
            }
            Err(_) => errors.push(PackError::new("E0600", format!("{at} names the icon {icon:?}, which isn't in the pack")).at(at).learn("echospot dev docs icons")),
        }
    }
    for (name, a) in &p.index.abilities {
        for example in super::examples::check(a) {
            warnings.push(PackError::new("E0202", format!("the example {example:?} doesn't fill {pack}.{name}'s input; root search won't learn from it")).at(format!("abilities.{name}.examples")).learn("echospot dev docs abilities"));
        }
        if a.kind == super::index::Kind::Effect && !a.has_undo && a.has_run {
            warnings.push(PackError::new("E0311", format!("{pack}.{name} is an effect with no undo; it can only be taken back if everything it calls records its own")).at(format!("abilities.{name}")).learn("echospot dev docs kinds"));
        }
    }
    (errors, warnings)
}

/// What a type checker says, when one is installed. `None` when there is none to ask.
pub fn analyze(dir: &Path) -> Option<Vec<PackError>> {
    let program = ["luau-analyze", "luau-lsp"].into_iter().find(|p| crate::util::has_program(p))?;
    let files: Vec<String> = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "luau") && !p.to_string_lossy().ends_with(".d.luau")).map(|p| p.display().to_string()).collect();
    let mut command = std::process::Command::new(program);
    if program == "luau-lsp" {
        command.arg("analyze").arg(format!("--definitions={}", dir.join("echospot.d.luau").display()));
    }
    let output = command.args(&files).current_dir(dir).output().ok()?;
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    // `init.luau(31,9): TypeError: …`
    Some(text.lines().filter_map(|line| {
        let (place, rest) = line.split_once("): ")?;
        let (file, position) = place.rsplit_once('(')?;
        let line_no = position.split(',').next()?;
        let file = Path::new(file).file_name()?.to_string_lossy().to_string();
        Some(PackError::new("E0200", rest.trim()).at(format!("{file}:{line_no}")).learn("echospot dev api show <name>"))
    }).collect())
}

/// Draws every target of a pack in every scene and checks the blocks. `render` draws one to a
/// PNG and returns the measured tree, when a headless render is available.
pub fn design(rt: &mut Runtime, pack: &str, mut render: impl FnMut(&str, &str) -> Option<(String, Value)>) -> Vec<Drawn> {
    let mut out = Vec::new();
    let dir = rt.packs.get(pack).map(|p| p.dir.clone()).unwrap_or_default();
    let own: Vec<_> = scenes::of_pack(&dir).into_iter().flatten().collect();
    for target in scenes::targets(rt, pack) {
        let mut all = vec![None];
        all.extend(own.iter().cloned().map(Some));
        all.extend(scenes::stress(rt, pack, &target).into_iter().map(Some));
        for scene in all {
            let name = scene.as_ref().map(|s| s.name.clone()).unwrap_or_else(|| "live".into());
            // The live draw would call the real abilities; a design check stays inside fixtures.
            if scene.is_none() {
                continue;
            }
            let mut drawn = Drawn { target: target.clone(), scene: name.clone(), ..Default::default() };
            match scenes::draw(rt, pack, &target, scene) {
                Ok(blocks) => {
                    let look = super::surface::look_of(rt, pack, &target, &blocks, "glass");
                    drawn.findings = design::check(&blocks, look);
                    if let Some((png, tree)) = render(&target, &name) {
                        let measured = design::measure(&tree);
                        let scale = tree.get("scale").and_then(Value::as_f64).unwrap_or(1.0);
                        let _ = design::mark(Path::new(&png), &measured, scale);
                        drawn.findings.extend(measured);
                        drawn.png = png;
                    }
                }
                Err(e) => drawn.error = Some(e),
            }
            out.push(drawn);
        }
    }
    out
}

/// The whole check. The pack is loaded afresh, so what is reported is what is on disk.
pub fn check(rt: &mut Runtime, dir: &Path) -> Report {
    let id = rt.load(dir, true).unwrap_or_default();
    rt.check_dependencies();
    let mut report = Report { pack: id.clone(), ..Default::default() };
    let Some(pack) = rt.packs.get(&id) else {
        report.errors.push(PackError::new("E0100", format!("{} isn't a pack: it has no pack.toml", dir.display())).learn("echospot dev new <id>"));
        return report;
    };
    report.state = pack.state.name().to_string();
    report.abilities = pack.index.abilities.len();
    report.surfaces = pack.index.surfaces.len() + pack.index.boards.len() + pack.index.abilities.values().filter(|a| a.faces.iter().any(|f| f == "panel")).count();
    report.load_ms = pack.load_ms;
    if let Some(error) = &pack.error {
        report.errors.push(error.clone());
    }
    if pack.state == State::Drafting {
        report.errors.push(PackError::new("E0201", "the pack has no init.luau yet").at("init.luau").learn("echospot dev docs packs"));
    }
    let missing = if pack.system { Vec::new() } else { rt.grants.missing(&id, &pack.manifest.permissions) };
    if !missing.is_empty() {
        report.notes.push(format!("{} permission{} not granted yet: a person grants them in EchoSpot (Review Permissions). `echospot dev grants` lists them.", missing.len(), if missing.len() == 1 { " is" } else { "s are" }));
    }
    let (errors, warnings) = lint(rt, &id);
    for e in errors {
        if !report.errors.contains(&e) {
            report.errors.push(e);
        }
    }
    report.warnings = warnings;
    let pack = &rt.packs[&id];
    for (name, a) in pack.index.abilities.iter().filter(|(_, a)| a.icon.is_empty() && !a.hidden) {
        report.notes.push(format!("{id}.{name} has no icon; it shows a plain tile. Draw one: icon = \"icons/{name}.svg\""));
        let _ = a;
    }
    if !pack.dir.join("INTENT.md").exists() {
        report.notes.push("no INTENT.md: write what the human asked for, in their words, and the decisions made".into());
    }
    if pack.dir.join("tests").read_dir().map(|d| d.count()).unwrap_or(0) == 0 {
        report.notes.push("no tests yet: tests/*.luau, run by `echospot dev test`".into());
    }
    for name in pack.index.abilities.keys() {
        let address = format!("{id}.{name}");
        report.used_by.extend(super::graph::dependents(rt, &address).into_iter().map(|(by, at)| format!("{address} ← {by} ({at})")));
    }
    report.wrote = super::defs::write(rt, &id, &rt.packs[&id].dir.clone());
    match analyze(&rt.packs[&id].dir) {
        Some(found) => report.errors.extend(found),
        None => report.notes.push("types weren't checked: install luau-lsp or luau-analyze for a full type check. Names, addresses and schemas were.".into()),
    }
    report.ok = report.errors.is_empty();
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_is_read_for_names() {
        let source = "local ui = require(\"@echo/ui\")\nlocal types = require(\"@echo/types\")\nlocal x = ui.Table2 { }\n-- ui.Nope\nlocal s = \"see ui.List\"\nlocal gui = {} gui.thing()\nlocal y = types.txt()";
        assert_eq!(bindings(source, "@echo/ui"), vec!["ui".to_string()]);
        assert_eq!(bindings(source, "@echo/types"), vec!["types".to_string()]);
        assert_eq!(members(source, "ui", '.'), vec![("Table2".to_string(), 3)]);
        assert_eq!(members(source, "types", '.'), vec![("txt".to_string(), 7)]);
        assert_eq!(members("cx:call(\"a.b\") cx:foo()", "cx", ':'), vec![("call".to_string(), 1), ("foo".to_string(), 1)]);
    }
}
