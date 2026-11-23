//! The API, described from the declarations the host itself runs on: every ability's signature,
//! every component, type and verb. `echospot dev api search` and `api show` print this, the AI
//! gets its tool definitions from it, and so does MCP. Nothing here is written twice.

use serde_json::{Value, json};

use super::blocks;
use super::index::Kind;
use super::runtime::Runtime;
use super::types::{self, Type};
#[cfg(test)]
use super::vm;

/// One thing an agent can look up.
pub fn entry(what: &str, name: &str, signature: &str, about: &str) -> Value {
    json!({ "what": what, "name": name, "signature": signature, "about": about })
}

/// What `cx` answers to, with how each is called.
pub const CX: &[(&str, &str, &str)] = &[
    ("call", "cx:call(address, input) → value", "Call an ability and wait for its result. Raises its error."),
    ("try", "cx:try(address, input) → ok, value_or_error", "Call an ability without raising."),
    ("load", "cx:load(address, input) → Resource", "In a surface: a Resource that is pending, ok or error. Never waits; the surface is drawn again when it settles."),
    ("store", "cx:store(name) → value", "A store's value now. A surface that reads one is drawn again when it changes."),
    ("storeAt", "cx:storeAt(name, time) → value", "A store's value as it was at a time, in seconds since 1970."),
    ("put", "cx:put(name, value)", "Write one of this pack's own stores."),
    ("push", "cx:push(address, args)", "Open another surface on top of this one."),
    ("replace", "cx:replace(address, args)", "Open another surface in place of this one."),
    ("pop", "cx:pop()", "Go back."),
    ("toast", "cx:toast(text, intent?)", "A short message in the palette."),
    ("hud", "cx:hud(text)", "A short message on the screen, after the palette has gone."),
    ("dry", "cx:dry() → boolean", "True while effects are being stubbed: a dry run, a scene, a test."),
    ("log", "cx:log(...)", "Print to `echospot dev logs`."),
    ("pack", "cx.pack", "This pack's id."),
    ("data", "cx.data", "This pack's own data folder, always readable and writable with files.*."),
];

/// What `echo` answers to.
pub const ECHO: &[(&str, &str, &str)] = &[
    ("pack", "echo.pack { abilities, verbs, stores, surfaces, boards, triggers }", "The pack: what init.luau returns."),
    ("ability", "echo.ability { title, icon, kind, input, output, examples, run, query, undo, faces, every, at, on, refresh, ai, confirm, hidden, keywords }", "A typed function with an address."),
    ("verb", "echo.verb { on, title, icon, kind, run, undo }", "An action offered wherever a value of a type appears."),
    ("store", "echo.store { type, keep, persist, initial }", "Live, typed data with history."),
    ("board", "echo.board { title, look, tiles }", "A dashboard: tiles on the four-column grid."),
    ("trigger", "echo.trigger { every, at, on, run }", "Code that runs on a schedule or an event."),
    ("panel", "echo.panel { title, icon, render }", "A surface in the palette."),
    ("window", "echo.window { title, size, render }", "A surface in a window of its own."),
    ("overlay", "echo.overlay { place, input, size, render }", "A surface on a screen edge or corner, above other windows."),
    ("widget", "echo.widget { place, size, render }", "A surface pinned to the desktop."),
    ("map", "echo.map(list, f) → list", "Each item through f."),
    ("filter", "echo.filter(list, keep) → list", "The items keep says yes to."),
    ("find", "echo.find(list, want) → item?", "The first item want says yes to."),
    ("keys", "echo.keys(map) → list", "A table's keys, sorted."),
    ("sort", "echo.sort(list, by?) → list", "A sorted copy: by a field name or a function of the item."),
    ("json", "echo.json.encode(value), echo.json.decode(text)", "JSON text and back."),
    ("now", "echo.now() → number", "Seconds since 1970."),
    ("date", "echo.date(format?, time?) → text", "A time as local text: echo.date(\"%H:%M\")."),
    ("api", "echo.api", "The API version this host speaks."),
];

fn kind_note(kind: Kind) -> &'static str {
    match kind {
        Kind::Read => "read: changes nothing; may only call reads",
        Kind::Effect => "effect: changes something, and can be undone",
        Kind::Final => "final: can't be undone; asks first",
    }
}

/// An ability as an AI tool or an agent sees it.
pub fn describe(rt: &Runtime, address: &str) -> Option<Value> {
    let (pack, name) = address.split_once('.')?;
    if let Some((p, a)) = rt.packs.get(pack).and_then(|p| p.ability(name).map(|a| (p, a))) {
        return Some(json!({
            "address": address, "title": a.title, "description": a.description, "kind": a.kind.name(), "pack": p.id,
            "signature": format!("{address}({}) → {}", a.input.fields.iter().map(|(k, t)| format!("{k}: {}", t.label())).collect::<Vec<_>>().join(", "), a.output.as_ref().map(|t| t.label()).unwrap_or_else(|| "any".into())),
            "schema": a.input.json_schema(), "input": a.input, "output": a.output, "examples": a.examples, "faces": a.faces,
            "needs": if p.system { "nothing: a system pack" } else { "calls = [\"…\"] in the caller's pack.toml" },
        }));
    }
    let spec = rt.specs().into_iter().find(|s| s.address == address)?;
    Some(json!({
        "address": address, "title": spec.title, "description": "", "kind": spec.kind.name(), "pack": pack,
        "signature": spec.signature(), "schema": spec.input.json_schema(), "input": spec.input, "output": spec.output, "examples": [],
        "faces": [], "needs": spec.gate.label(),
    }))
}

/// Everything there is to look up, in one list.
pub fn catalogue(rt: &Runtime) -> Vec<Value> {
    let mut out = Vec::new();
    for (address, signature, kind, title) in rt.addresses() {
        out.push(json!({ "what": "ability", "name": address, "signature": format!("{address}{signature}"), "about": title, "kind": kind.name() }));
    }
    for (name, about, props) in blocks::COMPONENTS {
        out.push(entry("component", name, &format!("ui.{name} {{ {props} }}"), about));
    }
    for (name, about) in blocks::PRIMITIVES {
        out.push(entry("primitive", name, &format!("ui.{name}"), about));
    }
    for (name, about) in blocks::MODIFIERS {
        out.push(entry("modifier", name, &format!("{name} = …"), about));
    }
    for (name, shape) in types::CORE {
        out.push(entry("type", name, &format!("t.{name}() : {shape}"), "A core type"));
    }
    for (name, signature, about) in CX {
        out.push(entry("cx", name, signature, about));
    }
    for (name, signature, about) in ECHO {
        out.push(entry("echo", name, signature, about));
    }
    for verb in super::verbs::BUILTIN {
        out.push(entry("verb", verb.id, &format!("on {}", verb.on.join(", ")), verb.title));
    }
    for pack in rt.packs.values() {
        for (name, verb) in &pack.index.verbs {
            out.push(entry("verb", &format!("{}.verbs.{name}", pack.id), &format!("on {}", verb.on), &verb.title));
        }
        for name in pack.index.stores.keys() {
            out.push(entry("store", &format!("{}.stores.{name}", pack.id), "cx:store(name)", "A pack's store"));
        }
    }
    for (name, ability) in super::stores::BUILTIN {
        out.push(entry("store", name, &format!("cx:store(\"{name}\")"), &if ability.is_empty() { "A built-in store, free to read".to_string() } else { format!("A built-in store; reading it needs {ability}") }));
    }
    for intent in blocks::INTENTS {
        out.push(entry("intent", intent, &format!("intent = \"{intent}\""), "What something means; the Look decides how it shows"));
    }
    for look in super::looks::LOOKS {
        out.push(entry("look", look.name, &format!("look = \"{}\"", look.name), look.about));
    }
    out
}

/// Entries whose name, signature or description hold every word asked for, best first.
pub fn search(rt: &Runtime, words: &str) -> Vec<Value> {
    let wanted: Vec<String> = words.split_whitespace().map(str::to_lowercase).collect();
    let mut scored: Vec<(i32, Value)> = catalogue(rt).into_iter().filter_map(|e| {
        let (name, rest) = (e["name"].as_str().unwrap_or("").to_lowercase(), format!("{} {}", e["signature"].as_str().unwrap_or(""), e["about"].as_str().unwrap_or("")).to_lowercase());
        let mut score = 0;
        for word in &wanted {
            score += if name == *word { 100 } else if name.split('.').any(|part| part == word) { 60 } else if name.contains(word) { 40 } else if rest.contains(word) { 10 } else { return None };
        }
        Some((score, e))
    }).collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1["name"].as_str().cmp(&b.1["name"].as_str())));
    scored.into_iter().map(|(_, e)| e).collect()
}

/// One name, in full: what `echospot dev api show` prints. `None` when nothing has that name.
pub fn show(rt: &Runtime, name: &str) -> Option<Value> {
    if let Some(mut ability) = describe(rt, name) {
        let kind = Kind::parse(ability["kind"].as_str().unwrap_or("read")).unwrap_or(Kind::Read);
        ability["what"] = json!("ability");
        ability["kind_means"] = json!(kind_note(kind));
        ability["used_by"] = json!(super::graph::dependents(rt, name).into_iter().map(|(pack, at)| format!("{pack} ({at})")).collect::<Vec<_>>());
        ability["example"] = json!(format!("local out = cx:call(\"{name}\", {})", example_input(&serde_json::from_value::<Type>(ability["input"].clone()).unwrap_or_default())));
        ability["learn"] = json!("echospot dev docs abilities");
        return Some(ability);
    }
    let bare = name.trim_start_matches("ui.").trim_start_matches("t.").trim_start_matches("cx:").trim_start_matches("echo.");
    let found = catalogue(rt).into_iter().find(|e| e["what"] != "ability" && e["name"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(bare)))?;
    let mut out = found.clone();
    match found["what"].as_str().unwrap_or("") {
        "component" => {
            out["example"] = json!(component_example(bare));
            out["modifiers"] = json!(blocks::MODIFIERS.iter().map(|(n, _)| *n).collect::<Vec<_>>());
            out["learn"] = json!("echospot dev docs ui");
        }
        "type" => {
            out["example"] = json!(format!("input = {{ value = t.{bare}() }}"));
            out["verbs"] = json!(super::verbs::BUILTIN.iter().filter(|v| v.on.contains(&bare) || v.on.contains(&"any")).map(|v| v.title).collect::<Vec<_>>());
            out["learn"] = json!("echospot dev docs types");
        }
        "cx" | "echo" => out["learn"] = json!("echospot dev docs echo"),
        "look" | "intent" | "modifier" | "primitive" => out["learn"] = json!("echospot dev docs design"),
        _ => out["learn"] = json!("echospot dev docs"),
    }
    Some(out)
}

/// A Luau table literal that fits an input schema: what an example call passes.
pub fn example_input(input: &Type) -> String {
    let fields: Vec<String> = input.fields.iter().filter(|(_, t)| !t.optional && t.default.is_none()).map(|(k, t)| format!("{k} = {}", example_value(t))).collect();
    if fields.is_empty() { "{}".into() } else { format!("{{ {} }}", fields.join(", ")) }
}

fn example_value(t: &Type) -> String {
    match t.kind.as_str() {
        "number" | "percent" | "duration" => "1".into(),
        "bool" => "true".into(),
        "url" => "\"https://example.com\"".into(),
        "currency" => "\"USD\"".into(),
        "money" => "{ amount = 1, currency = \"USD\" }".into(),
        "quantity" => "{ value = 1, unit = \"km\" }".into(),
        "list" | "series" => "{}".into(),
        "record" => example_input(t),
        "enum" => format!("{:?}", t.values.first().cloned().unwrap_or_default()),
        "file" | "folder" | "image" => "\"~/file\"".into(),
        _ => "\"…\"".into(),
    }
}

fn component_example(name: &str) -> String {
    match name {
        "List" => "ui.List { items = cx:load(\"github.prs\", {}), title = \"title\", subtitle = \"repo\", empty = \"No open pull requests\" }",
        "Grid" => "ui.Grid { items = photos, image = \"path\", title = \"name\", columns = 4 }",
        "Detail" => "ui.Detail { markdown = text, fields = { { \"Repo\", repo } }, actions = { { title = \"Open\", run = function(_, cx) cx:call(\"system.open_url\", { url = url }) end } } }",
        "Form" => "ui.Form { fields = { title = t.text() }, submit = function(values, cx) cx:call(\"notes.create\", values) cx:pop() end }",
        "Stat" => "ui.Stat { value = out, caption = \"USD → INR\" }",
        "Chart" => "ui.Chart { series = points, kind = \"line\" }",
        "Table" => "ui.Table { columns = { \"Repo\", \"Stars\" }, rows = { { \"rune\", 12 } } }",
        "Timeline" => "ui.Timeline { items = events, title = \"title\", time = \"at\" }",
        "Gallery" => "ui.Gallery { items = shots, image = \"path\", title = \"name\" }",
        "KeyValue" => "ui.KeyValue { items = { { \"Branch\", \"main\" }, { \"Status\", \"green\" } } }",
        "Progress" => "ui.Progress { value = 0.4, label = \"Uploading\" }",
        "Banner" => "ui.Banner { title = \"Offline\", text = \"Showing saved data\", intent = \"muted\" }",
        "Empty" => "ui.Empty { title = \"No builds\", hint = \"Push a commit to see one\" }",
        "Tabs" => "ui.Tabs { tabs = { { title = \"Open\", content = ui.List { items = open } }, { title = \"Closed\", content = ui.List { items = closed } } } }",
        "Split" => "ui.Split { left = ui.List { items = prs }, right = ui.Detail { markdown = body } }",
        "Stream" => "ui.Stream { source = cx:load(\"ai.stream\", { prompt = question }) }",
        "Board" => "ui.Board { tiles = { { content = ui.Stat { value = n, caption = \"Open\" }, size = \"2x1\" } } }",
        _ => "",
    }.to_string()
}

/// The names an agent might have meant, across the whole catalogue.
pub fn nearest(rt: &Runtime, name: &str) -> Vec<String> {
    let names: Vec<String> = catalogue(rt).into_iter().filter_map(|e| e["name"].as_str().map(String::from)).collect();
    super::error::nearest(name, names.iter().map(String::as_str))
}

/// The names `echo`, `cx` and `ui` answer to must be the ones documented here.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_is_documented_is_what_exists() {
        for name in vm::ECHO_NAMES {
            assert!(ECHO.iter().any(|(n, _, _)| n == name), "echo.{name} has no entry");
        }
        for name in vm::CX_NAMES {
            assert!(CX.iter().any(|(n, _, _)| n == name), "cx.{name} has no entry");
        }
        let ui = vm::ui_names();
        for (name, _, _) in blocks::COMPONENTS {
            assert!(ui.iter().any(|n| n == name), "ui.{name} is documented but @echo/ui has no such component");
            assert!(!component_example(name).is_empty(), "{name} has no example");
        }
        for (name, _) in blocks::PRIMITIVES {
            assert!(ui.iter().any(|n| n == name), "ui.{name} is documented but missing");
        }
    }

    #[test]
    fn search_finds_by_words() {
        let rt = Runtime::ephemeral();
        let hits = search(&rt, "calc convert");
        assert_eq!(hits[0]["name"], json!("calc.convert"));
        assert_eq!(search(&rt, "table")[0]["name"], json!("Table"));
        assert!(search(&rt, "zzzz").is_empty());
        let shown = show(&rt, "ui.List").unwrap();
        assert!(shown["example"].as_str().unwrap().starts_with("ui.List"));
        assert!(show(&rt, "calc.convert").unwrap()["example"].as_str().unwrap().contains("amount = 1"));
        assert!(show(&rt, "Table2").is_none());
        assert_eq!(nearest(&rt, "Table2")[0], "Table");
    }
}
