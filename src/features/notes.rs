//! Notes: Markdown files in `~/.local/share/echospot/notes`, one per note, so any editor and any
//! sync tool can have them too. The first line is the title.

use std::path::PathBuf;

use rune::Tx;

use crate::app::{App, Built, Tone};
use crate::features::forms::{self, Field, FieldKind, Form, Target};
use crate::features::fuzzy;
use crate::icons;
use crate::model::*;
use crate::theme::Tint;
use crate::util;

#[derive(Clone, Debug, Default)]
pub struct Note {
    /// The file's name without `.md`.
    pub id: String,
    pub title: String,
    /// Everything after the title line.
    pub body: String,
    pub modified: u64,
}

pub fn dir() -> PathBuf {
    let dir = util::data_dir().join("notes");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn file(id: &str) -> PathBuf {
    dir().join(format!("{id}.md"))
}

/// A note's title and body from its file: a leading `# Heading` is the title, else the file's name.
pub fn split(id: &str, text: &str) -> (String, String) {
    let text = text.trim_start_matches('\u{feff}');
    match text.lines().next().and_then(|l| l.strip_prefix("# ")).map(str::trim).filter(|t| !t.is_empty()) {
        Some(title) => (title.to_string(), text.split_once('\n').map(|(_, rest)| rest.trim_start_matches('\n').to_string()).unwrap_or_default()),
        None => (id.replace(['-', '_'], " "), text.to_string()),
    }
}

pub fn load() -> Vec<Note> {
    let mut notes: Vec<Note> = std::fs::read_dir(dir()).into_iter().flatten().flatten().filter_map(|e| {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("md") {
            return None;
        }
        let id = path.file_stem()?.to_string_lossy().to_string();
        let text = std::fs::read_to_string(&path).ok()?;
        let (title, body) = split(&id, &text);
        let modified = e.metadata().ok()?.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs();
        Some(Note { id, title, body, modified })
    }).collect();
    notes.sort_by(|a, b| b.modified.cmp(&a.modified));
    notes
}

/// A file name for a title, not already taken.
fn slug(title: &str, taken: &[Note]) -> String {
    let base: String = title.to_lowercase().chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect::<String>()
        .split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-").chars().take(60).collect();
    let base = if base.is_empty() { "note".to_string() } else { base };
    let mut id = base.clone();
    let mut n = 2;
    while taken.iter().any(|t| t.id == id) || file(&id).exists() {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

fn create_action(title: &str) -> Action {
    Action::new("Create Note", &icons::PLUS, Act::Note(NoteOp::New(title.to_string())))
}

pub fn build(app: &App, query: &str) -> Built {
    let q = query.trim();
    let mut scored: Vec<(i32, Item)> = app.notes.iter().filter_map(|n| {
        let score = if q.is_empty() { 0 } else { fuzzy::score_item(q, &n.title, &util::truncate(&n.body, 4000))? };
        let whole = format!("# {}\n\n{}", n.title, n.body);
        let words = n.body.split_whitespace().count();
        let mut detail = Detail { markdown: Some(if n.body.trim().is_empty() { "*Empty note.*".into() } else { util::truncate(&n.body, 6000) }), heading: Some("Information".into()), ..Default::default() };
        detail.fields.push(("Words".into(), words.to_string()));
        detail.fields.push(("Edited".into(), util::ago(n.modified)));
        let item = Item::new(format!("note:{}", n.id), n.title.clone(), IconSrc::Badge(&icons::FILE_TEXT, Tint::Yellow))
            .action(Action::new("Edit Note", &icons::PENCIL, Act::Note(NoteOp::Edit(n.id.clone()))))
            .action(Action::new("Copy Note", &icons::COPY, Act::Copy(n.body.clone())).keys("ctrl+shift+c"))
            .action(Action::new(app.paste_target(), &icons::CLIPBOARD, Act::Paste(n.body.clone())).keys("ctrl+enter"))
            .action(Action::new("Copy as Markdown", &icons::CODE, Act::Copy(whole)))
            .action(Action::new("Open in Editor", &icons::ARROW_UP_RIGHT, Act::OpenPath(file(&n.id))).keys("ctrl+o"))
            .action(create_action("").keys("ctrl+shift+n").section(""))
            .action(Action::new("Delete Note", &icons::TRASH, Act::Confirm(Box::new(Confirm {
                title: "Delete Note?".into(), message: n.title.clone(), icon: &icons::TRASH, confirm: "Delete".into(),
                destructive: true, act: Act::Note(NoteOp::Delete(n.id.clone())),
            }))).keys("ctrl+x").destructive())
            .detail(detail);
        Some((score, item))
    }).collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    let found = !scored.is_empty();
    let mut sections = Sections::default();
    sections.push("Notes", scored.into_iter().map(|(_, i)| i).collect());
    // What was typed and found nothing is most likely a new note's title.
    if !found || app.notes.is_empty() {
        let title = if q.is_empty() { "Create Note".to_string() } else { format!("Create Note “{}”", util::truncate(q, 40)) };
        sections.push("", vec![Item::new("note:new", title, IconSrc::Badge(&icons::PLUS, Tint::Yellow)).trailing("Command").action(create_action(q))]);
    }
    Built::list(sections, "Search notes...").layout(if found { Layout::Split } else { Layout::List }).keep()
}

fn form(existing: Option<&Note>, title: &str) -> Form {
    let n = existing.cloned().unwrap_or_else(|| Note { title: title.to_string(), ..Default::default() });
    Form::new(
        if existing.is_some() { "Edit Note" } else { "Create Note" },
        "Save Note",
        vec![
            Field::new("title", "Title", FieldKind::Text).value(&n.title).placeholder("Meeting notes").required(),
            Field::new("body", "Note", FieldKind::Area).value(&n.body).placeholder("Markdown")
                .info("Ctrl+Return saves. The note is a Markdown file; Open in Editor edits it anywhere."),
        ],
        Target::Note(existing.map(|n| n.id.clone())),
    )
}

pub fn perform(app: &mut App, op: NoteOp, tx: &mut Tx) {
    match op {
        NoteOp::New(title) => forms::open(app, form(None, &title), tx),
        NoteOp::Edit(id) => {
            let existing = app.notes.iter().find(|n| n.id == id).cloned();
            forms::open(app, form(existing.as_ref(), ""), tx);
        }
        NoteOp::Delete(id) => {
            let _ = std::fs::remove_file(file(&id));
            app.notes = load();
            app.refresh();
            app.toast("Note Deleted", Tone::Success);
        }
    }
}

pub fn save(app: &mut App, id: Option<String>, tx: &mut Tx) {
    let Some(form) = &app.form else { return };
    let (title, body) = (form.get("title").trim().to_string(), form.get("body"));
    let id = id.unwrap_or_else(|| slug(&title, &app.notes));
    let text = format!("# {title}\n\n{}\n", body.trim_end());
    if std::fs::write(file(&id), text).is_err() {
        return app.toast("Could Not Save the Note", Tone::Danger);
    }
    app.notes = load();
    app.pop(tx);
    app.toast("Note Saved", Tone::Success);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles() {
        assert_eq!(split("x", "# Groceries\n\nmilk\neggs\n"), ("Groceries".into(), "milk\neggs\n".into()));
        assert_eq!(split("todo-list", "just text\n"), ("todo list".into(), "just text\n".into()));
        assert_eq!(split("x", "# Only a title"), ("Only a title".into(), String::new()));
    }
}
