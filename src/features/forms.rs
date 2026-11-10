//! Forms drawn in the palette: creating a snippet or quicklink, an extension's own form, its
//! preferences. One model, so they all look and move alike.

use rune::{Key, KeyEvent, TextEdit, Tx};

use crate::app::App;
use crate::model::*;

#[derive(Clone, Debug, PartialEq)]
pub enum FieldKind {
    Text,
    Password,
    Area,
    Dropdown(Vec<(String, String)>),
    Checkbox(String),
    /// Prose between fields.
    Description,
    Separator,
}

pub struct Field {
    pub id: String,
    pub label: String,
    pub kind: FieldKind,
    pub edit: TextEdit,
    /// A dropdown's chosen option.
    pub choice: usize,
    pub checked: bool,
    pub placeholder: String,
    pub info: String,
    pub error: String,
    pub required: bool,
}

impl Field {
    pub fn new(id: &str, label: &str, kind: FieldKind) -> Self {
        Self {
            id: id.into(), label: label.into(), kind, edit: TextEdit::new(""), choice: 0, checked: false,
            placeholder: String::new(), info: String::new(), error: String::new(), required: false,
        }
    }
    pub fn value(mut self, text: &str) -> Self {
        self.edit = TextEdit::new(text);
        if let FieldKind::Dropdown(options) = &self.kind {
            self.choice = options.iter().position(|(v, _)| v == text).unwrap_or(0);
        }
        self
    }
    pub fn placeholder(mut self, text: &str) -> Self {
        self.placeholder = text.into();
        self
    }
    pub fn info(mut self, text: &str) -> Self {
        self.info = text.into();
        self
    }
    pub fn required(mut self) -> Self {
        self.required = true;
        self
    }
    pub fn focusable(&self) -> bool {
        !matches!(self.kind, FieldKind::Description | FieldKind::Separator)
    }
    pub fn is_text(&self) -> bool {
        matches!(self.kind, FieldKind::Text | FieldKind::Password | FieldKind::Area)
    }
    /// What the field holds, as the submit handler reads it.
    pub fn text(&self) -> String {
        match &self.kind {
            FieldKind::Dropdown(options) => options.get(self.choice).map(|(v, _)| v.clone()).unwrap_or_default(),
            FieldKind::Checkbox(_) => self.checked.to_string(),
            _ => self.edit.text.clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub enum Target {
    Snippet(Option<String>),
    Quicklink(Option<String>),
    ExtPreferences { extension: String, then: Option<String> },
    ExtForm { session: u64 },
}

pub struct Form {
    pub title: String,
    pub submit: String,
    pub fields: Vec<Field>,
    pub focus: usize,
    pub target: Target,
}

impl Form {
    pub fn new(title: &str, submit: &str, fields: Vec<Field>, target: Target) -> Self {
        let mut form = Self { title: title.into(), submit: submit.into(), fields, focus: 0, target };
        form.focus = form.fields.iter().position(|f| f.focusable()).unwrap_or(0);
        form.refocus();
        form
    }

    /// Hands the keyboard to the focused field.
    pub fn refocus(&mut self) {
        for (i, field) in self.fields.iter_mut().enumerate() {
            field.edit.focused = i == self.focus && field.is_text();
        }
    }

    pub fn get(&self, id: &str) -> String {
        self.fields.iter().find(|f| f.id == id).map(|f| f.text()).unwrap_or_default()
    }

    fn step(&mut self, delta: i32) {
        let order: Vec<usize> = self.fields.iter().enumerate().filter(|(_, f)| f.focusable()).map(|(i, _)| i).collect();
        if order.is_empty() {
            return;
        }
        let at = order.iter().position(|i| *i == self.focus).unwrap_or(0) as i32;
        self.focus = order[(at + delta).rem_euclid(order.len() as i32) as usize];
        self.refocus();
    }

    /// Marks empty required fields; true when the form may submit.
    pub fn validate(&mut self) -> bool {
        let mut first_bad = None;
        for (i, field) in self.fields.iter_mut().enumerate() {
            field.error.clear();
            if field.required && field.is_text() && field.edit.text.trim().is_empty() {
                field.error = "Required".into();
                first_bad.get_or_insert(i);
            }
        }
        if let Some(i) = first_bad {
            self.focus = i;
            self.refocus();
        }
        first_bad.is_none()
    }
}

pub fn open(app: &mut App, form: Form, tx: &mut Tx) {
    app.push(Route::Form, tx);
    app.form = Some(form);
    if app.form.as_ref().is_some_and(|f| !f.fields.get(f.focus).is_some_and(|x| x.is_text())) {
        tx.blur();
    }
}

pub fn submit(app: &mut App, tx: &mut Tx) {
    let Some(form) = &mut app.form else { return };
    if !form.validate() {
        return;
    }
    let target = form.target.clone();
    match target {
        Target::Snippet(id) => crate::features::snippets::save(app, id, tx),
        Target::Quicklink(id) => crate::features::quicklinks::save(app, id, tx),
        Target::ExtPreferences { extension, then } => crate::ext::save_preferences(app, &extension, then, tx),
        Target::ExtForm { session } => crate::ext::submit_form(app, session, tx),
    }
}

/// Keys while a form is up. Text keys fall through to the focused field's editor.
pub fn key(app: &mut App, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let Some(form) = &mut app.form else { return false };
    let focused_text = form.fields.get(form.focus).is_some_and(|f| f.is_text());
    let area = form.fields.get(form.focus).is_some_and(|f| f.kind == FieldKind::Area);
    match &ev.key {
        Key::Escape => {
            app.pop(tx);
        }
        Key::Tab => {
            form.step(if ev.mods.shift { -1 } else { 1 });
            if !form.fields[form.focus].is_text() {
                tx.blur();
            }
            tx.scroll_into_view(format!("field-{}", form.fields[form.focus].id));
        }
        Key::Enter if ev.mods.cmd => submit(app, tx),
        Key::Enter if !area => {
            // Return moves on, and submits from the last field.
            let last = form.fields.iter().rposition(|f| f.focusable()) == Some(form.focus);
            if last {
                submit(app, tx);
            } else {
                form.step(1);
                if !form.fields[form.focus].is_text() {
                    tx.blur();
                }
            }
        }
        Key::Command('k') => {
            if app.top().route == Route::Form
                && let Some(Target::ExtForm { session }) = app.form.as_ref().map(|f| f.target.clone()) {
                crate::ext::open_form_actions(app, session);
            }
        }
        Key::Down | Key::Up | Key::Left | Key::Right | Key::Text(_) if !focused_text => {
            let field = &mut form.fields[form.focus];
            let delta = match &ev.key {
                Key::Down | Key::Right => 1,
                Key::Up | Key::Left => -1,
                Key::Text(t) if t == " " => 1,
                _ => return true,
            };
            match &field.kind {
                FieldKind::Dropdown(options) if !options.is_empty() => {
                    field.choice = (field.choice as i32 + delta).rem_euclid(options.len() as i32) as usize;
                }
                FieldKind::Checkbox(_) => field.checked = !field.checked,
                _ => {}
            }
        }
        _ => return false,
    }
    true
}

pub fn press_field(app: &mut App, index: usize, tx: &mut Tx) {
    let Some(form) = &mut app.form else { return };
    form.focus = index;
    form.refocus();
    let field = &mut form.fields[index];
    match &field.kind {
        FieldKind::Checkbox(_) => {
            field.checked = !field.checked;
            tx.blur();
        }
        FieldKind::Dropdown(options) if !options.is_empty() => {
            field.choice = (field.choice + 1) % options.len();
            tx.blur();
        }
        _ => {}
    }
}
