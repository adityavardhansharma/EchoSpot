//! `notes.*`: the user's notes, Markdown files. A pack names each address it uses in `calls`.

use serde_json::{Value, json};

use super::{Done, Gate, Run, Spec, missing, text};
use crate::features::notes;
use crate::packs::error::PackError;
use crate::packs::index::Kind;

fn note(n: &notes::Note) -> Value {
    json!({ "id": n.id, "title": n.title, "body": n.body, "modified": n.modified })
}

/// A note's id can only name a file in the notes folder.
fn safe(id: &str) -> Result<(), PackError> {
    if id.is_empty() || id.contains(['/', '\\']) || id.starts_with('.') {
        return Err(PackError::new("E0301", format!("{id:?} isn't a note's id")).learn("echospot dev api show notes.list"));
    }
    Ok(())
}

fn read(id: &str) -> Result<String, PackError> {
    safe(id)?;
    std::fs::read_to_string(notes::file(id)).map_err(|_| PackError::new("E0300", format!("no note {id:?}")).learn("echospot dev api show notes.list"))
}

fn write(id: &str, content: &str) -> Result<(), PackError> {
    safe(id)?;
    std::fs::write(notes::file(id), content).map_err(|e| PackError::new("E0403", format!("can't write the note: {e}")))
}

pub fn register(out: &mut Vec<Spec>) {
    out.push(Spec::new("notes.list", Kind::Read, Gate::Calls, "Every note, most recently changed first", "query:text?", "list<record>", |_, i| {
        let query = text(i, "query").to_lowercase();
        Run::value(Value::Array(notes::load().iter().filter(|n| query.is_empty() || n.title.to_lowercase().contains(&query) || n.body.to_lowercase().contains(&query)).map(note).collect()))
    }));
    out.push(Spec::new("notes.read", Kind::Read, Gate::Calls, "A note's Markdown", "id:text", "markdown", |_, i| Run::Now(read(text(i, "id")).and_then(|t| Done::value(Value::String(t))))));
    out.push(Spec::new("notes.create", Kind::Effect, Gate::Calls, "Create a note", "title:text, body:text?", "text", |_, i| {
        let title = text(i, "title").trim();
        if title.is_empty() {
            return Run::err(missing("title"));
        }
        let id = notes::slug(title, &notes::load());
        Run::Now(write(&id, &format!("# {title}\n\n{}\n", text(i, "body").trim_end())).map(|()| Done { value: json!(id), undo: Some(("notes.delete".into(), json!({ "id": id }))), ..Default::default() }))
    }));
    out.push(Spec::new("notes.append", Kind::Effect, Gate::Calls, "Add text to the end of a note", "id:text, text:text", "bool", |_, i| {
        let id = text(i, "id");
        Run::Now(read(id).and_then(|before| {
            write(id, &format!("{}\n{}\n", before.trim_end(), text(i, "text").trim_end()))?;
            Ok(Done { value: json!(true), undo: Some(("notes.write".into(), json!({ "id": id, "text": before }))), ..Default::default() })
        }))
    }));
    out.push(Spec::new("notes.write", Kind::Effect, Gate::Calls, "Replace a note's whole text", "id:text, text:text", "bool", |_, i| {
        let id = text(i, "id");
        let before = read(id);
        Run::Now(write(id, text(i, "text")).map(|()| Done {
            value: json!(true),
            undo: Some(match before {
                Ok(before) => ("notes.write".into(), json!({ "id": id, "text": before })),
                Err(_) => ("notes.delete".into(), json!({ "id": id })),
            }),
            ..Default::default()
        }))
    }));
    out.push(Spec::new("notes.delete", Kind::Effect, Gate::Calls, "Delete a note", "id:text", "bool", |_, i| {
        let id = text(i, "id");
        Run::Now(read(id).and_then(|before| {
            std::fs::remove_file(notes::file(id)).map_err(|e| PackError::new("E0403", format!("can't delete the note: {e}")))?;
            Ok(Done { value: json!(true), undo: Some(("notes.write".into(), json!({ "id": id, "text": before }))), ..Default::default() })
        }))
    }));
}
