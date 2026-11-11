//! Quicklinks: named URLs, with `{query}` standing for whatever was typed.

use rune::Tx;
use serde::{Deserialize, Serialize};

use crate::app::{App, Built, Tone};
use crate::features::forms::{self, Field, FieldKind, Form, Target};
use crate::features::fuzzy;
use crate::icons;
use crate::model::*;
use crate::util;

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Quicklink {
    pub id: String,
    pub name: String,
    pub link: String,
    #[serde(default)]
    pub alias: String,
}

#[derive(Default)]
pub struct Quicklinks {
    pub list: Vec<Quicklink>,
}

fn path() -> std::path::PathBuf {
    util::config_dir().join("quicklinks.json")
}

fn defaults() -> Vec<Quicklink> {
    let q = |id: &str, name: &str, link: &str, alias: &str| Quicklink { id: id.into(), name: name.into(), link: link.into(), alias: alias.into() };
    vec![
        q("google", "Google", "https://www.google.com/search?q={query}", "g"),
        q("duckduckgo", "DuckDuckGo", "https://duckduckgo.com/?q={query}", "ddg"),
        q("github", "GitHub", "https://github.com/search?q={query}", "gh"),
        q("youtube", "YouTube", "https://www.youtube.com/results?search_query={query}", "yt"),
        q("wikipedia", "Wikipedia", "https://en.wikipedia.org/w/index.php?search={query}", "wiki"),
        q("archwiki", "Arch Wiki", "https://wiki.archlinux.org/index.php?search={query}", "aw"),
    ]
}

impl Quicklinks {
    pub fn load() -> Self {
        let path = path();
        if !path.exists() {
            let list = defaults();
            util::write_json(&path, &list);
            return Self { list };
        }
        Self { list: util::read_json(&path) }
    }
    fn save(&self) {
        util::write_json(&path(), &self.list);
    }
}

fn encode(s: &str) -> String {
    s.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
        b' ' => "+".to_string(),
        _ => format!("%{b:02X}"),
    }).collect()
}

/// The URL a quicklink opens for a query; without one, its site's front page.
pub fn resolve(link: &str, query: &str) -> String {
    if takes_query(link) && query.trim().is_empty() {
        // Scheme and host only.
        let end = link.find("://").map(|i| i + 3).and_then(|s| link[s..].find('/').map(|e| s + e)).unwrap_or(link.len());
        return link[..end].to_string();
    }
    link.replace("{query}", &encode(query.trim())).replace("{argument}", &encode(query.trim()))
}

/// The host of a link: `github.com` for `https://github.com/search?q={query}`.
pub fn host(link: &str) -> Option<&str> {
    let rest = link.split_once("://").filter(|(scheme, _)| scheme.starts_with("http")).map(|(_, rest)| rest)?;
    let host = rest.split(['/', '?', '#']).next()?.rsplit('@').next()?.split(':').next()?;
    (host.contains('.') && !host.contains(['{', ' '])).then_some(host)
}

/// A quicklink's picture: its site's icon once fetched, a link glyph until then and for links
/// that are not web pages.
pub fn icon(link: &str) -> IconSrc {
    match host(link).and_then(crate::ext::net::favicon) {
        Some(path) => IconSrc::Favicon(path),
        None => IconSrc::Glyph(&icons::LINK),
    }
}

/// Whether a link takes what was typed.
pub fn takes_query(link: &str) -> bool {
    link.contains("{query}") || link.contains("{argument}")
}

pub fn build(app: &App, query: &str) -> Built {
    let q = query.trim();
    let mut scored: Vec<(i32, Item)> = app.quicklinks.list.iter().filter_map(|l| {
        let score = if q.is_empty() { 0 } else { fuzzy::score_item(q, &l.name, &format!("{} {}", l.alias, l.link))? };
        let item = Item::new(format!("quicklink:{}", l.id), l.name.clone(), icon(&l.link))
            .subtitle(util::truncate(&l.link, 64))
            .trailing(if l.alias.is_empty() { "Quicklink" } else { &l.alias })
            .action(Action::new("Open Quicklink", &icons::ARROW_UP_RIGHT, Act::Quicklink(QuicklinkOp::Open(l.id.clone(), String::new()))))
            .action(Action::new("Copy Link", &icons::COPY, Act::Copy(l.link.clone())).keys("ctrl+shift+c"))
            .action(Action::new("Edit Quicklink", &icons::PENCIL, Act::Quicklink(QuicklinkOp::Edit(l.id.clone()))).keys("ctrl+e").section(""))
            .action(Action::new("Create Quicklink", &icons::PLUS, Act::Quicklink(QuicklinkOp::New)).keys("ctrl+shift+n"))
            .action(Action::new("Delete Quicklink", &icons::TRASH, Act::Confirm(Box::new(Confirm {
                title: "Delete Quicklink?".into(), message: l.name.clone(), icon: &icons::TRASH, confirm: "Delete".into(),
                destructive: true, act: Act::Quicklink(QuicklinkOp::Delete(l.id.clone())),
            }))).keys("ctrl+x").destructive());
        Some((score, item))
    }).collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    let mut sections = Sections::default();
    sections.push("Quicklinks", scored.into_iter().map(|(_, i)| i).collect());
    sections.push("", vec![Item::new("quicklink:new", "Create Quicklink", IconSrc::Glyph(&icons::PLUS)).trailing("Command")
        .action(Action::new("Create Quicklink", &icons::PLUS, Act::Quicklink(QuicklinkOp::New)))]);
    Built::list(sections, "Search quicklinks...").keep()
}

fn form(existing: Option<&Quicklink>) -> Form {
    let l = existing.cloned().unwrap_or_default();
    Form::new(
        if existing.is_some() { "Edit Quicklink" } else { "Create Quicklink" },
        "Save Quicklink",
        vec![
            Field::new("name", "Name", FieldKind::Text).value(&l.name).placeholder("Google").required(),
            Field::new("link", "Link", FieldKind::Text).value(&l.link).placeholder("https://www.google.com/search?q={query}").required()
                .info("{query} is replaced with what you type in the launcher."),
            Field::new("alias", "Alias", FieldKind::Text).value(&l.alias).placeholder("Optional, e.g. g"),
        ],
        Target::Quicklink(existing.map(|l| l.id.clone())),
    )
}

pub fn perform(app: &mut App, op: QuicklinkOp, tx: &mut Tx) {
    match op {
        QuicklinkOp::New => forms::open(app, form(None), tx),
        QuicklinkOp::Edit(id) => {
            let existing = app.quicklinks.list.iter().find(|l| l.id == id).cloned();
            forms::open(app, form(existing.as_ref()), tx);
        }
        QuicklinkOp::Delete(id) => {
            app.quicklinks.list.retain(|l| l.id != id);
            app.quicklinks.save();
            app.refresh();
            app.toast("Quicklink Deleted", Tone::Success);
        }
        QuicklinkOp::Open(id, query) => {
            if let Some(link) = app.quicklinks.list.iter().find(|l| l.id == id).cloned() {
                app.usage.record(&format!("ql:{}", link.id));
                app.perform(Act::OpenUrl(resolve(&link.link, &query)), tx);
            }
        }
    }
}

pub fn save(app: &mut App, id: Option<String>, tx: &mut Tx) {
    let Some(form) = &app.form else { return };
    let link = Quicklink {
        id: id.clone().unwrap_or_else(|| format!("{:x}", util::now_secs() * 1000 + app.quicklinks.list.len() as u64)),
        name: form.get("name").trim().to_string(),
        link: form.get("link").trim().to_string(),
        alias: form.get("alias").trim().to_string(),
    };
    match app.quicklinks.list.iter_mut().find(|l| Some(&l.id) == id.as_ref()) {
        Some(existing) => *existing = link,
        None => app.quicklinks.list.push(link),
    }
    app.quicklinks.save();
    app.pop(tx);
    app.toast("Quicklink Saved", Tone::Success);
}
