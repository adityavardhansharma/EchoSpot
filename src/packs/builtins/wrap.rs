//! What the user already has, as abilities: script commands and `commands.json` lines become
//! `scripts.<name>`, snippets `snippets.<keyword>`, quicklinks `quicklinks.<name>`, and Raycast
//! extension commands `raycast.<extension>.<command>` (opaque: they run, they have no types).

use serde_json::{Value, json};

use super::{Done, Gate, Run, Spec, text};
use crate::features::{quicklinks, scripts, snippets};
use crate::packs::error::PackError;
use crate::packs::index::Kind;
use crate::packs::types::Type;
use crate::util;

/// A name as an address can carry it: lowercase letters, digits and underscores.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') && !out.is_empty() {
            out.push('_');
        }
    }
    out.trim_end_matches('_').to_string()
}

fn script_name(s: &scripts::Script) -> String {
    match &s.path {
        Some(path) => slug(&path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()),
        None => slug(&s.title),
    }
}

/// A script's kind: a read unless its header says `@echospot.kind effect` or `final`. Scripts
/// are the user's own and run with the user's rights, so this is their word, not a sandbox.
fn script_kind(s: &scripts::Script) -> Kind {
    let declared = s.path.as_ref().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| {
        t.lines().take(60).find_map(|l| l.split_once("@echospot.kind").map(|(_, k)| k.trim().to_string()))
    });
    declared.as_deref().and_then(Kind::parse).unwrap_or(Kind::Read)
}

/// The arguments for a script from a call's input: `{ args = { "50" } }`, or named values in
/// the order the script declares its arguments.
fn script_args(s: &scripts::Script, input: &Value) -> Vec<String> {
    let show = |v: &Value| v.as_str().map(String::from).unwrap_or_else(|| v.to_string());
    if let Some(list) = input.get("args").and_then(Value::as_array) {
        return list.iter().map(show).collect();
    }
    let Some(map) = input.as_object() else { return Vec::new() };
    let named: Vec<String> = s.args.iter().filter_map(|a| map.get(&slug(&a.placeholder)).or_else(|| map.get(&a.placeholder))).map(show).collect();
    if !named.is_empty() || map.is_empty() {
        return named;
    }
    map.values().map(show).collect()
}

fn scripts_specs(out: &mut Vec<Spec>) {
    for script in scripts::load() {
        let name = script_name(&script);
        if name.is_empty() {
            continue;
        }
        let kind = script_kind(&script);
        let mut spec = Spec::new(format!("scripts.{name}"), kind, Gate::Calls, script.title.clone(), "", "any", move |_, input| {
            let (script, args) = (script.clone(), script_args(&script, input));
            Run::Later(Box::new(move |_| {
                let (output, ok) = scripts::execute(&script, &args);
                let output = output.trim().to_string();
                if !ok {
                    return Err(PackError::new("E0510", format!("{} failed: {}", script.title, output.lines().last().unwrap_or("no output"))));
                }
                // JSON when it prints JSON, text otherwise.
                Done::value(serde_json::from_str::<Value>(&output).ok().filter(|v| v.is_object() || v.is_array()).unwrap_or(Value::String(output)))
            }))
        });
        spec.input = Type { fields: [("args".to_string(), Type::list(Type::of("text")).optional())].into(), ..Type::of("record") };
        out.push(spec);
    }
}

fn snippet_specs(out: &mut Vec<Spec>) {
    let list: Vec<snippets::Snippet> = util::read_json(&util::config_dir().join("snippets.json"));
    for snippet in list {
        let name = slug(if snippet.keyword.is_empty() { &snippet.name } else { &snippet.keyword });
        if name.is_empty() || name == "expand" {
            continue;
        }
        out.push(Spec::new(format!("snippets.{name}"), Kind::Read, Gate::Free, snippet.name.clone(), "", "text", move |_, _| Run::value(Value::String(snippets::expand(&snippet.text)))));
    }
}

fn quicklink_specs(out: &mut Vec<Spec>) {
    for link in quicklinks::Quicklinks::load().list {
        let name = slug(if link.alias.is_empty() { &link.name } else { &link.alias });
        if name.is_empty() || name == "open" {
            continue;
        }
        out.push(Spec::new(format!("quicklinks.{name}"), Kind::Read, Gate::Free, link.name.clone(), "query:text?", "url", move |_, i| Run::value(Value::String(quicklinks::resolve(&link.link, text(i, "query"))))));
    }
}

pub fn fixed(out: &mut Vec<Spec>) {
    out.push(Spec::new("snippets.expand", Kind::Read, Gate::Free, "A snippet's text by keyword, its placeholders filled", "keyword:text", "text", |_, i| {
        let keyword = text(i, "keyword");
        let list: Vec<snippets::Snippet> = util::read_json(&util::config_dir().join("snippets.json"));
        match list.iter().find(|s| s.keyword == keyword || s.name == keyword) {
            Some(s) => Run::value(Value::String(snippets::expand(&s.text))),
            None => Run::err(PackError::new("E0300", format!("no snippet {keyword:?}")).nearest(keyword, list.iter().map(|s| s.keyword.as_str()))),
        }
    }));
    out.push(Spec::new("quicklinks.open", Kind::Effect, Gate::Free, "Open a quicklink in the browser, with a query", "name:text, query:text?", "url", |_, i| {
        let name = text(i, "name");
        let list = quicklinks::Quicklinks::load().list;
        match list.iter().find(|l| l.alias == name || l.name.eq_ignore_ascii_case(name) || l.id == name) {
            Some(link) => {
                let url = quicklinks::resolve(&link.link, text(i, "query"));
                util::open_url(&url);
                Run::value(json!(url))
            }
            None => Run::err(PackError::new("E0300", format!("no quicklink {name:?}")).nearest(name, list.iter().map(|l| l.name.as_str()))),
        }
    }));
}

/// A Raycast extension's command. The frozen Raycast host runs it; nothing is known of its types.
fn raycast(address: &str) -> Option<Spec> {
    let rest = address.strip_prefix("raycast.")?;
    let (extension, command) = rest.split_once('.')?;
    let dir = crate::ext::extensions_dir().join(extension);
    if !dir.is_dir() {
        return None;
    }
    let id = format!("ext:{extension}:{command}");
    Some(Spec::new(address, Kind::Final, Gate::Calls, format!("Raycast: {extension} {command}"), "", "bool", move |_, _| {
        let id = id.clone();
        // The resident palette owns the Raycast host; this asks it as a hotkey would.
        Run::Later(Box::new(move |_| match crate::ipc::send(&format!("run {id}")) {
            true => Done::value(json!(true)),
            false => Err(PackError::new("E0510", "a Raycast command needs a running EchoSpot")),
        }))
    }))
}

pub fn dynamic() -> Vec<Spec> {
    let mut out = Vec::new();
    scripts_specs(&mut out);
    snippet_specs(&mut out);
    quicklink_specs(&mut out);
    out
}

pub fn find(address: &str) -> Option<Spec> {
    if address.starts_with("raycast.") {
        return raycast(address);
    }
    let ns = address.split('.').next()?;
    let mut out = Vec::new();
    match ns {
        "scripts" => scripts_specs(&mut out),
        "snippets" => snippet_specs(&mut out),
        "quicklinks" => quicklink_specs(&mut out),
        _ => return None,
    }
    out.into_iter().find(|s| s.address == address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_become_addresses() {
        assert_eq!(slug("Deploy to Prod!"), "deploy_to_prod");
        assert_eq!(slug("pi"), "pi");
        let script = scripts::Script {
            id: "script:pi.sh".into(), title: "Pi".into(), subtitle: String::new(), icon: String::new(), mode: scripts::Mode::Compact, path: Some("/x/pi.sh".into()),
            command: String::new(), args: vec![scripts::Argument { placeholder: "Digits".into(), optional: false, secret: false, options: vec![] }], confirm: false, cwd: None,
        };
        assert_eq!(script_name(&script), "pi");
        assert_eq!(script_args(&script, &json!({ "args": ["50"] })), ["50"]);
        assert_eq!(script_args(&script, &json!({ "digits": 8 })), ["8"]);
        assert_eq!(script_args(&script, &json!({ "n": 8 })), ["8"]);
    }
}
