//! Snippets: saved text, pasted into whatever had focus. `{date}`, `{time}`, `{datetime}`,
//! `{clipboard}` and `{uuid}` fill in as the snippet is used.

use rune::Tx;
use serde::{Deserialize, Serialize};

use crate::app::{App, Built, Tone};
use crate::features::forms::{self, Field, FieldKind, Form, Target};
use crate::features::fuzzy;
use crate::icons;
use crate::model::*;
use crate::util;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Snippet {
    pub id: String,
    pub name: String,
    pub text: String,
    #[serde(default)]
    pub keyword: String,
}

#[derive(Default)]
pub struct Snippets {
    pub list: Vec<Snippet>,
}

fn path() -> std::path::PathBuf {
    util::config_dir().join("snippets.json")
}

impl Snippets {
    pub fn load() -> Self {
        Self { list: util::read_json(&path()) }
    }
    fn save(&self) {
        util::write_json(&path(), &self.list);
    }
}

pub fn expand(text: &str) -> String {
    if !text.contains('{') {
        return text.to_string();
    }
    let now = chrono::Local::now();
    let mut out = text
        .replace("{date}", &now.format("%-d %B %Y").to_string())
        .replace("{time}", &now.format("%H:%M").to_string())
        .replace("{datetime}", &now.format("%-d %B %Y at %H:%M").to_string())
        .replace("{isodate}", &now.format("%Y-%m-%d").to_string());
    if out.contains("{clipboard}") {
        out = out.replace("{clipboard}", &util::output("wl-paste", &["--no-newline"]).unwrap_or_default());
    }
    if out.contains("{uuid}") {
        out = out.replace("{uuid}", &util::output("cat", &["/proc/sys/kernel/random/uuid"]).unwrap_or_default());
    }
    out
}

pub fn build(app: &App, query: &str) -> Built {
    let q = query.trim();
    let mut scored: Vec<(i32, Item)> = app.snippets.list.iter().filter_map(|s| {
        let score = if q.is_empty() { 0 } else { fuzzy::score_item(q, &s.name, &format!("{} {}", s.keyword, util::truncate(&s.text, 200)))? };
        let mut detail = Detail { heading: Some("Information".into()), text: Some(s.text.clone()), ..Default::default() };
        detail.fields.push(("Name".into(), s.name.clone()));
        if !s.keyword.is_empty() {
            detail.fields.push(("Keyword".into(), s.keyword.clone()));
        }
        detail.fields.push(("Characters".into(), s.text.chars().count().to_string()));
        let item = Item::new(format!("snippet:{}", s.id), s.name.clone(), IconSrc::Badge(&icons::TEXT, crate::theme::Tint::Red))
            .action(Action::new(app.paste_target(), &icons::CLIPBOARD, Act::Paste(expand(&s.text))))
            .action(Action::new("Copy to Clipboard", &icons::COPY, Act::Copy(expand(&s.text))).keys("ctrl+enter"))
            .action(Action::new("Edit Snippet", &icons::PENCIL, Act::Snippet(SnippetOp::Edit(s.id.clone()))).keys("ctrl+e").section(""))
            .action(Action::new("Create Snippet", &icons::PLUS, Act::Snippet(SnippetOp::New)).keys("ctrl+shift+n"))
            .action(Action::new("Delete Snippet", &icons::TRASH, Act::Confirm(Box::new(Confirm {
                title: "Delete Snippet?".into(), message: s.name.clone(), icon: &icons::TRASH, confirm: "Delete".into(),
                destructive: true, act: Act::Snippet(SnippetOp::Delete(s.id.clone())),
            }))).keys("ctrl+x").destructive())
            .detail(detail);
        Some((score, item))
    }).collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    let mut sections = Sections::default();
    sections.push("Snippets", scored.into_iter().map(|(_, i)| i).collect());
    if app.snippets.list.is_empty() {
        sections.push("", vec![Item::new("snippet:new", "Create Snippet", IconSrc::Badge(&icons::PLUS, crate::theme::Tint::Red)).trailing("Command")
            .action(Action::new("Create Snippet", &icons::PLUS, Act::Snippet(SnippetOp::New)))]);
    }
    Built::list(sections, "Search snippets...").layout(if app.snippets.list.is_empty() { Layout::List } else { Layout::Split }).keep()
}

fn form(existing: Option<&Snippet>) -> Form {
    let s = existing.cloned().unwrap_or_default();
    Form::new(
        if existing.is_some() { "Edit Snippet" } else { "Create Snippet" },
        "Save Snippet",
        vec![
            Field::new("name", "Name", FieldKind::Text).value(&s.name).placeholder("Email signature").required(),
            Field::new("text", "Snippet", FieldKind::Area).value(&s.text).placeholder("The text to insert").required()
                .info("{date}, {time}, {datetime}, {clipboard} and {uuid} fill in when the snippet is used."),
            Field::new("keyword", "Keyword", FieldKind::Text).value(&s.keyword).placeholder("Optional, for search"),
        ],
        Target::Snippet(existing.map(|s| s.id.clone())),
    )
}

pub fn perform(app: &mut App, op: SnippetOp, tx: &mut Tx) {
    match op {
        SnippetOp::New => forms::open(app, form(None), tx),
        SnippetOp::Edit(id) => {
            let existing = app.snippets.list.iter().find(|s| s.id == id).cloned();
            forms::open(app, form(existing.as_ref()), tx);
        }
        SnippetOp::Delete(id) => {
            app.snippets.list.retain(|s| s.id != id);
            app.snippets.save();
            app.refresh();
            app.toast("Snippet Deleted", Tone::Success);
        }
    }
}

pub fn save(app: &mut App, id: Option<String>, tx: &mut Tx) {
    let Some(form) = &app.form else { return };
    let snippet = Snippet {
        id: id.clone().unwrap_or_else(|| format!("{:x}", util::now_secs() * 1000 + app.snippets.list.len() as u64)),
        name: form.get("name").trim().to_string(),
        text: form.get("text"),
        keyword: form.get("keyword").trim().to_string(),
    };
    match app.snippets.list.iter_mut().find(|s| Some(&s.id) == id.as_ref()) {
        Some(existing) => *existing = snippet,
        None => app.snippets.list.push(snippet),
    }
    app.snippets.save();
    app.pop(tx);
    app.toast("Snippet Saved", Tone::Success);
}
