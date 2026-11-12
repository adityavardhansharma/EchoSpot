//! What a person adds to the AI of their own: quick actions (a prompt run on the selected text)
//! and chat presets (instructions and a model a new chat starts from). Kept in
//! `~/.config/echospot/ai.json`.

use rune::Tx;
use serde::{Deserialize, Serialize};

use crate::app::{App, Tone};
use crate::features::forms::{self, Field, FieldKind, Form, Target};
use crate::util;

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct CustomAction {
    pub id: String,
    pub title: String,
    pub prompt: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub instructions: String,
    /// A model id to switch to with the provider in use; empty keeps the current one.
    pub model: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
#[serde(default)]
pub struct Library {
    pub actions: Vec<CustomAction>,
    pub presets: Vec<Preset>,
}

fn path() -> std::path::PathBuf {
    util::config_dir().join("ai.json")
}

impl Library {
    pub fn load() -> Self {
        util::read_json(&path())
    }
    fn save(&self) {
        util::write_json(&path(), self);
    }
}

fn new_id(n: usize) -> String {
    format!("{:x}", util::now_secs() * 1000 + n as u64)
}

pub fn action_form(app: &App, id: Option<&str>) -> Form {
    let a = id.and_then(|id| app.ai_library.actions.iter().find(|a| a.id == id)).cloned().unwrap_or_default();
    Form::new(if id.is_some() { "Edit AI Action" } else { "Create AI Action" }, "Save Action", vec![
        Field::new("title", "Title", FieldKind::Text).value(&a.title).placeholder("Make It Friendlier").required(),
        Field::new("prompt", "Instruction", FieldKind::Area).value(&a.prompt).placeholder("Rewrite the text below in a warmer tone. Reply with the rewritten text only.").required()
            .info("Runs on the text you have selected (or copied), which is added below this instruction."),
    ], Target::AiAction(id.map(String::from)))
}

pub fn preset_form(app: &App, id: Option<&str>) -> Form {
    let p = id.and_then(|id| app.ai_library.presets.iter().find(|p| p.id == id)).cloned().unwrap_or_default();
    Form::new(if id.is_some() { "Edit Chat Preset" } else { "Create Chat Preset" }, "Save Preset", vec![
        Field::new("name", "Name", FieldKind::Text).value(&p.name).placeholder("Code Reviewer").required(),
        Field::new("instructions", "Instructions", FieldKind::Area).value(&p.instructions).placeholder("You review code. Point out bugs first, then style.").required()
            .info("The system prompt of every chat started from this preset."),
        Field::new("model", "Model", FieldKind::Text).value(&p.model).placeholder("Optional, e.g. claude-opus-5-5")
            .info("A model id of the provider in use. Left empty, the chat uses whichever model is selected."),
    ], Target::AiPreset(id.map(String::from)))
}

pub fn open_action(app: &mut App, id: Option<String>, tx: &mut Tx) {
    let form = action_form(app, id.as_deref());
    forms::open(app, form, tx);
}

pub fn open_preset(app: &mut App, id: Option<String>, tx: &mut Tx) {
    let form = preset_form(app, id.as_deref());
    forms::open(app, form, tx);
}

pub fn save_action(app: &mut App, id: Option<String>, tx: &mut Tx) {
    let Some(form) = &app.form else { return };
    let action = CustomAction { id: id.clone().unwrap_or_else(|| new_id(app.ai_library.actions.len())), title: form.get("title").trim().to_string(), prompt: form.get("prompt").trim().to_string() };
    match app.ai_library.actions.iter_mut().find(|a| Some(&a.id) == id.as_ref()) {
        Some(existing) => *existing = action,
        None => app.ai_library.actions.push(action),
    }
    app.ai_library.save();
    app.pop(tx);
    app.toast("AI Action Saved", Tone::Success);
}

pub fn save_preset(app: &mut App, id: Option<String>, tx: &mut Tx) {
    let Some(form) = &app.form else { return };
    let preset = Preset {
        id: id.clone().unwrap_or_else(|| new_id(app.ai_library.presets.len())), name: form.get("name").trim().to_string(),
        instructions: form.get("instructions").trim().to_string(), model: form.get("model").trim().to_string(),
    };
    match app.ai_library.presets.iter_mut().find(|p| Some(&p.id) == id.as_ref()) {
        Some(existing) => *existing = preset,
        None => app.ai_library.presets.push(preset),
    }
    app.ai_library.save();
    app.pop(tx);
    app.toast("Preset Saved", Tone::Success);
}

pub fn delete_action(app: &mut App, id: &str) {
    app.ai_library.actions.retain(|a| a.id != id);
    app.ai_library.save();
    app.refresh();
    app.toast("AI Action Deleted", Tone::Success);
}

pub fn delete_preset(app: &mut App, id: &str) {
    app.ai_library.presets.retain(|p| p.id != id);
    app.ai_library.save();
    app.refresh();
    app.toast("Preset Deleted", Tone::Success);
}
