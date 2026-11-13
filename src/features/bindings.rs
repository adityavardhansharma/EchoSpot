//! Aliases and global hotkeys for launcher rows. An alias typed exactly puts its row first; a
//! hotkey is a Hyprland binding, registered while EchoSpot runs, that calls `echospot run <id>`.

use std::collections::BTreeMap;

use rune::Tx;
use serde::{Deserialize, Serialize};

use crate::app::{App, Tone};
use crate::features::forms::{self, Field, FieldKind, Form, Target};
use crate::util;

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct Bindings {
    /// Row id to alias.
    pub aliases: BTreeMap<String, String>,
    /// Row id to chord, as `super+alt+c`.
    pub hotkeys: BTreeMap<String, String>,
    /// The Hyprland chords this process has bound, so one that is taken away can be unbound.
    #[serde(skip)]
    applied: Vec<String>,
}

fn path() -> std::path::PathBuf {
    util::config_dir().join("bindings.json")
}

/// `super+alt+c` as Hyprland writes it, `SUPER + ALT + C`; `None` when it is not a usable chord.
pub fn hypr_chord(chord: &str) -> Option<String> {
    // Modifiers are held as flags, so the result reads the same however they were ordered.
    let (mut held, mut key) = ([false; 4], None);
    for part in chord.split('+').map(|p| p.trim().to_ascii_lowercase()).filter(|p| !p.is_empty()) {
        match part.as_str() {
            "super" | "win" | "cmd" | "meta" => held[0] = true,
            "ctrl" | "control" => held[1] = true,
            "alt" | "opt" | "option" => held[2] = true,
            "shift" => held[3] = true,
            other if key.is_none() => key = Some(match other {
                "enter" | "return" => "RETURN".to_string(),
                "esc" | "escape" => "ESCAPE".to_string(),
                "," => "COMMA".to_string(),
                "." => "PERIOD".to_string(),
                "/" => "SLASH".to_string(),
                ";" => "SEMICOLON".to_string(),
                "-" => "MINUS".to_string(),
                "=" => "EQUAL".to_string(),
                "'" => "APOSTROPHE".to_string(),
                "[" => "BRACKETLEFT".to_string(),
                "]" => "BRACKETRIGHT".to_string(),
                "\\" => "BACKSLASH".to_string(),
                "`" => "GRAVE".to_string(),
                k if k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => k.to_ascii_uppercase(),
                _ => return None,
            }),
            _ => return None,
        }
    }
    // A bare key, or Shift alone with one, would swallow ordinary typing.
    if !held[..3].contains(&true) {
        return None;
    }
    let mods: Vec<&str> = ["SUPER", "CTRL", "ALT", "SHIFT"].into_iter().zip(held).filter(|(_, on)| *on).map(|(name, _)| name).collect();
    Some(format!("{} + {}", mods.join(" + "), key?))
}

/// The chord in the palette's own spelling: `super+alt+c`.
fn normal(chord: &str) -> String {
    chord.split('+').map(|p| p.trim().to_ascii_lowercase()).filter(|p| !p.is_empty()).collect::<Vec<_>>().join("+")
}

fn lua_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

impl Bindings {
    pub fn load() -> Self {
        util::read_json(&path())
    }
    fn save(&self) {
        util::write_json(&path(), self);
    }

    /// Registers every hotkey with Hyprland, off the UI thread. Safe to repeat: each chord is
    /// unbound before it is bound, so a config reload that dropped them is repaired by the next call.
    pub fn apply(&mut self) {
        if !crate::features::windows::hyprland() {
            return;
        }
        let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "echospot".into());
        let wanted: Vec<(String, String)> = self.hotkeys.iter().filter_map(|(id, chord)| Some((hypr_chord(chord)?, id.clone()))).collect();
        let mut lua = String::new();
        for old in self.applied.iter().filter(|old| !wanted.iter().any(|(c, _)| c == *old)) {
            lua.push_str(&format!("hl.unbind({}); ", lua_string(old)));
        }
        for (chord, id) in &wanted {
            let command = format!("{} run {}", util::quote(&exe), util::quote(id));
            lua.push_str(&format!("hl.unbind({c}); hl.bind({c}, hl.dsp.exec_cmd({}), {{ description = {} }}); ",
                lua_string(&command), lua_string(&format!("EchoSpot: {id}")), c = lua_string(chord)));
        }
        self.applied = wanted.into_iter().map(|(c, _)| c).collect();
        if !lua.is_empty() {
            std::thread::spawn(move || {
                util::output("hyprctl", &["eval", &lua]);
            });
        }
    }
}

fn form(app: &App, id: &str, title: &str) -> Form {
    Form::new(
        &format!("Alias and Hotkey: {title}"),
        "Save",
        vec![
            Field::new("alias", "Alias", FieldKind::Text).value(app.bindings.aliases.get(id).map(String::as_str).unwrap_or("")).placeholder("e.g. cb")
                .info("Typing the alias in the launcher puts this row first."),
            Field::new("hotkey", "Hotkey", FieldKind::Text).value(app.bindings.hotkeys.get(id).map(String::as_str).unwrap_or("")).placeholder("e.g. super+alt+c")
                .info("A global shortcut, written as modifiers and a key joined by +. It needs Super, Ctrl or Alt. Leave empty for none."),
        ],
        Target::Binding(id.to_string()),
    )
}

pub fn edit(app: &mut App, id: &str, title: &str, tx: &mut Tx) {
    let form = form(app, id, title);
    forms::open(app, form, tx);
}

pub fn save(app: &mut App, id: String, tx: &mut Tx) {
    let Some(form) = &mut app.form else { return };
    let (alias, hotkey) = (form.get("alias").trim().to_string(), normal(&form.get("hotkey")));
    if !hotkey.is_empty() {
        let problem = if hypr_chord(&hotkey).is_none() {
            Some("Write it like super+alt+c, with Super, Ctrl or Alt.".to_string())
        } else {
            app.bindings.hotkeys.iter().find(|(other, chord)| **other != id && **chord == hotkey).map(|_| "Another command already uses this hotkey.".to_string())
        };
        if let Some(problem) = problem {
            if let Some(field) = form.fields.iter_mut().find(|f| f.id == "hotkey") {
                field.error = problem;
            }
            return;
        }
    }
    if alias.is_empty() { app.bindings.aliases.remove(&id); } else { app.bindings.aliases.insert(id.clone(), alias); }
    if hotkey.is_empty() { app.bindings.hotkeys.remove(&id); } else { app.bindings.hotkeys.insert(id, hotkey); }
    app.bindings.save();
    app.bindings.apply();
    app.pop(tx);
    app.toast("Saved", Tone::Success);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords() {
        assert_eq!(hypr_chord("super+alt+c").as_deref(), Some("SUPER + ALT + C"));
        assert_eq!(hypr_chord("Ctrl + Shift + Space").as_deref(), Some("CTRL + SHIFT + SPACE"));
        assert_eq!(hypr_chord("super+,").as_deref(), Some("SUPER + COMMA"));
        assert_eq!(hypr_chord("c"), None);
        assert_eq!(hypr_chord("shift+c"), None);
        assert_eq!(hypr_chord("super+a+b"), None);
        assert_eq!(hypr_chord("super"), None);
        assert_eq!(hypr_chord("shift+alt+super+c").as_deref(), Some("SUPER + ALT + SHIFT + C"));
        assert_eq!(hypr_chord("super+super+c").as_deref(), Some("SUPER + C"));
    }
}
