//! Verbs: actions attached to a type, offered wherever a value of that type appears. A `url` in
//! clipboard history, on a pull request or in a note offers the same things. And Send to…: any
//! value can be piped into every ability whose input accepts its type.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::index::Kind;
use super::runtime::{Runtime, State};
use super::types::Type;

pub struct BuiltinVerb {
    pub id: &'static str,
    pub title: &'static str,
    /// A glyph of EchoSpot's own, by name.
    pub icon: &'static str,
    /// The types it acts on; `any` is every type.
    pub on: &'static [&'static str],
}

pub const BUILTIN: &[BuiltinVerb] = &[
    BuiltinVerb { id: "open", title: "Open", icon: "arrow_up_right", on: &["url", "file", "folder", "image", "app"] },
    BuiltinVerb { id: "copy", title: "Copy to Clipboard", icon: "copy", on: &["any"] },
    BuiltinVerb { id: "paste", title: "Paste", icon: "clipboard", on: &["any"] },
    BuiltinVerb { id: "reveal", title: "Show in File Manager", icon: "folder", on: &["file", "folder", "image"] },
    BuiltinVerb { id: "quick-look", title: "Quick Look", icon: "eye", on: &["markdown", "code", "text", "image", "table", "record", "list", "event"] },
    BuiltinVerb { id: "send", title: "Send to…", icon: "arrow_right", on: &["any"] },
    BuiltinVerb { id: "pin", title: "Pin", icon: "star", on: &["any"] },
];

/// The verb Return runs for a value of a type.
pub fn primary(kind: &str) -> &'static str {
    match kind {
        "url" | "file" | "folder" | "app" => "open",
        "image" | "markdown" | "code" | "table" | "event" => "quick-look",
        _ => "copy",
    }
}

/// One verb on offer for a value.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    /// `open`, `copy`… for a built-in; `<pack>.verbs.<name>` for a pack's.
    pub id: String,
    pub title: String,
    pub icon: String,
    pub kind: Kind,
    /// The pack whose verb it is; empty for a built-in.
    pub pack: String,
    pub name: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Recent {
    /// Verb ids, most recently used first.
    ids: Vec<String>,
}

fn recent_path() -> PathBuf {
    super::state_dir().join("verbs.json")
}

pub fn recent() -> Vec<String> {
    crate::util::read_json::<Recent>(&recent_path()).ids
}

pub fn note_used(id: &str) {
    let mut recent = crate::util::read_json::<Recent>(&recent_path());
    recent.ids.retain(|x| x != id);
    recent.ids.insert(0, id.to_string());
    recent.ids.truncate(40);
    crate::util::write_json(&recent_path(), &recent);
}

/// Whether a verb declared `on = <on>` acts on a value of `ty`: by type, or by a record's name.
fn acts_on(on: &str, ty: &Type) -> bool {
    on == "any" || on == ty.kind || !ty.name.is_empty() && on == ty.name
}

/// Every verb for a value of `ty`: the type's primary first, then what was used lately, then
/// the rest. A pack can't put itself at the top.
pub fn offers(rt: &Runtime, ty: &Type, recent: &[String]) -> Vec<Offer> {
    let mut out: Vec<Offer> = BUILTIN.iter().filter(|v| v.on.iter().any(|on| acts_on(on, ty)))
        .map(|v| Offer { id: v.id.into(), title: v.title.into(), icon: v.icon.into(), kind: if matches!(v.id, "paste" | "pin") { Kind::Effect } else { Kind::Read }, pack: String::new(), name: v.id.into() })
        .collect();
    for pack in rt.packs.values().filter(|p| p.state == State::Ready) {
        for (name, verb) in pack.index.verbs.iter().filter(|(_, v)| acts_on(&v.on, ty)) {
            out.push(Offer { id: format!("{}.verbs.{name}", pack.id), title: verb.title.clone(), icon: verb.icon.clone(), kind: verb.kind, pack: pack.id.clone(), name: name.clone() });
        }
    }
    let first = primary(&ty.kind);
    let rank = |o: &Offer| {
        if o.id == first {
            (0, 0)
        } else if let Some(at) = recent.iter().position(|r| *r == o.id) {
            (1, at)
        } else {
            (2, 0)
        }
    };
    // A stable sort: within a rank, built-ins stay ahead of packs, in the order they were listed.
    out.sort_by_key(rank);
    out
}

/// An ability a value can be sent to, and the input field it would fill.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub address: String,
    pub title: String,
    pub slot: String,
    pub kind: Kind,
    /// The other fields are optional: it can run at once, with no form.
    pub ready: bool,
}

/// Every ability whose input accepts a value of `ty`, pack abilities first.
pub fn send_targets(rt: &Runtime, ty: &Type) -> Vec<Target> {
    let mut out = Vec::new();
    let fits = |input: &Type| -> Option<(String, bool)> {
        if let Some(slot) = input.slot_for(ty) {
            return Some((slot.to_string(), true));
        }
        // Several fields could take it, or others are needed too: the first that fits, and a form.
        input.fields.iter().find(|(_, t)| t.kind != "any" && t.accepts(ty)).map(|(k, _)| (k.clone(), false))
    };
    for pack in rt.packs.values().filter(|p| p.state == State::Ready) {
        for (name, a) in pack.index.abilities.iter().filter(|(_, a)| a.has_run && !a.hidden) {
            if let Some((slot, ready)) = fits(&a.input) {
                out.push(Target { address: format!("{}.{name}", pack.id), title: a.title.clone(), slot, kind: a.kind, ready });
            }
        }
    }
    for spec in rt.specs() {
        if spec.address.starts_with("echo.") || spec.address.starts_with("timeline.") {
            continue;
        }
        if let Some((slot, ready)) = fits(&spec.input) {
            out.push(Target { address: spec.address.clone(), title: spec.title.clone(), slot, kind: spec.kind, ready });
        }
    }
    out
}

/// A value kept at hand: what the Pin verb saves.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Pin {
    pub title: String,
    pub value: Value,
    #[serde(rename = "type")]
    pub ty: Type,
}

fn pins_path() -> PathBuf {
    super::state_dir().join("pins.json")
}

pub fn pins() -> Vec<Pin> {
    crate::util::read_json(&pins_path())
}

/// Pins a value, or takes it down when it is already pinned. Returns whether it is pinned now.
pub fn toggle_pin(title: &str, value: &Value, ty: &Type) -> bool {
    let mut all = pins();
    let before = all.len();
    all.retain(|p| p.value != *value);
    let pinned = all.len() == before;
    if pinned {
        all.insert(0, Pin { title: title.to_string(), value: value.clone(), ty: ty.clone() });
        all.truncate(50);
    }
    crate::util::write_json(&pins_path(), &all);
    pinned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_type_brings_its_verbs() {
        let rt = Runtime::ephemeral();
        let url = offers(&rt, &Type::of("url"), &[]);
        assert_eq!(url[0].id, "open");
        assert!(url.iter().any(|o| o.id == "copy") && url.iter().any(|o| o.id == "send") && !url.iter().any(|o| o.id == "reveal"));
        assert_eq!(offers(&rt, &Type::of("money"), &[])[0].id, "copy");
        // What was used lately comes next, but never ahead of the type's own verb.
        let after = offers(&rt, &Type::of("url"), &["pin".to_string()]);
        assert_eq!((after[0].id.as_str(), after[1].id.as_str()), ("open", "pin"));
    }

    #[test]
    fn a_value_can_be_sent_where_it_fits() {
        let rt = Runtime::ephemeral();
        let targets = send_targets(&rt, &Type::of("url"));
        let open = targets.iter().find(|t| t.address == "system.open_url").expect("a url goes to system.open_url");
        assert_eq!((open.slot.as_str(), open.ready), ("url", true));
        // Text goes to the clipboard at once; a note needs a title as well.
        let text = send_targets(&rt, &Type::of("text"));
        assert!(text.iter().any(|t| t.address == "clipboard.copy" && t.ready));
        assert!(!send_targets(&rt, &Type::of("money")).iter().any(|t| t.address == "system.open_url"));
    }
}
