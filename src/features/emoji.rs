//! Emoji and symbols: the catalogue, pinned and frequently used sets, and the search.

use rune::Tx;
use serde::{Deserialize, Serialize};

use crate::app::{App, Built, Tone};
use crate::icons;
use crate::model::*;
use crate::util;

static RAW: &str = include_str!("../../assets/emoji.txt");

pub struct Emoji {
    pub glyph: &'static str,
    pub name: &'static str,
    pub category: &'static str,
    pub keywords: &'static str,
}

pub const CATEGORIES: [(&str, &str); 14] = [
    ("sp", "Smileys & People"), ("an", "Animals & Nature"), ("fd", "Food & Drink"), ("ac", "Activity"),
    ("tp", "Travel & Places"), ("ob", "Objects"), ("sy", "Symbols"), ("fl", "Flags"), ("xa", "Arrows"),
    ("xc", "Currency"), ("xm", "Math"), ("xs", "Shapes & Punctuation"), ("xj", "CJK Symbols"), ("xk", "Keys & Technical"),
];

#[derive(Serialize, Deserialize, Default)]
struct Saved {
    #[serde(default)]
    pinned: Vec<String>,
    #[serde(default)]
    used: Vec<(String, u32, u64)>,
}

pub struct EmojiStore {
    pub all: Vec<Emoji>,
    saved: Saved,
}

impl EmojiStore {
    pub fn load() -> Self {
        let all = RAW.lines().filter_map(|line| {
            let mut parts = line.splitn(5, '|');
            let glyph = parts.next()?;
            let name = parts.next()?;
            let category = parts.next()?;
            let _tone = parts.next()?;
            let keywords = parts.next().unwrap_or("");
            // The private-use Apple logo has no glyph outside Apple's fonts.
            (!glyph.is_empty() && !glyph.starts_with('\u{f8ff}')).then_some(Emoji { glyph, name, category, keywords })
        }).collect();
        Self { all, saved: util::read_json(&util::data_dir().join("emoji.json")) }
    }

    fn save(&self) {
        util::write_json(&util::data_dir().join("emoji.json"), &self.saved);
    }

    fn used(&mut self, glyph: &str) {
        let now = util::now_secs();
        match self.saved.used.iter_mut().find(|(g, _, _)| g == glyph) {
            Some(entry) => {
                entry.1 += 1;
                entry.2 = now;
            }
            None => self.saved.used.push((glyph.to_string(), 1, now)),
        }
        self.save();
    }

    fn frequent(&self) -> Vec<&str> {
        let mut used: Vec<&(String, u32, u64)> = self.saved.used.iter().collect();
        used.sort_by(|a, b| b.1.cmp(&a.1).then(b.2.cmp(&a.2)));
        used.into_iter().take(16).map(|(g, _, _)| g.as_str()).collect()
    }
}

fn title_case(name: &str) -> String {
    name.split(' ').map(|w| {
        let mut c = w.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    }).collect::<Vec<_>>().join(" ")
}

fn item(app: &App, e: &Emoji, section: &str) -> Item {
    let pinned = app.emoji.saved.pinned.iter().any(|p| p == e.glyph);
    let mut item = Item::new(format!("emoji:{section}:{}", e.glyph), title_case(e.name), IconSrc::Emoji(e.glyph.to_string()))
        .action(Action::new(app.paste_target(), &icons::CLIPBOARD, Act::Emoji(e.glyph.to_string(), EmojiOp::Paste)))
        .action(Action::new("Copy to Clipboard", &icons::COPY, Act::Emoji(e.glyph.to_string(), EmojiOp::Copy)).keys("ctrl+enter"))
        .action(Action::new(if pinned { "Unpin Emoji" } else { "Pin Emoji" }, &icons::PIN, Act::Emoji(e.glyph.to_string(), EmojiOp::Pin)).keys("ctrl+shift+p"))
        .action(Action::new("Copy Name", &icons::TEXT, Act::Copy(title_case(e.name))).keys("ctrl+shift+c"));
    item.subtitle = Some(e.keywords.replace(',', ", "));
    item
}

/// Rank: the name itself, then a word of it, then its keywords.
fn rank(e: &Emoji, q: &str) -> Option<i32> {
    if e.name == q {
        return Some(100);
    }
    if e.name.starts_with(q) {
        return Some(90);
    }
    if e.name.split(' ').any(|w| w.starts_with(q)) {
        return Some(80);
    }
    if e.keywords.split(',').any(|k| k == q) {
        return Some(70);
    }
    if e.keywords.split(',').any(|k| k.starts_with(q)) {
        return Some(60);
    }
    if q.len() >= 3 && (e.name.contains(q) || e.keywords.contains(q)) {
        return Some(40);
    }
    None
}

pub fn filters() -> Vec<&'static str> {
    std::iter::once("All Categories").chain(CATEGORIES.iter().map(|(_, title)| *title)).collect()
}

pub fn build(app: &App, query: &str, filter: usize) -> Built {
    let q = query.trim().to_lowercase();
    let only = filter.checked_sub(1).and_then(|i| CATEGORIES.get(i)).map(|(code, _)| *code);
    let mut sections = Sections::default();
    if !q.is_empty() {
        let terms: Vec<&str> = q.split_whitespace().collect();
        let mut hits: Vec<(i32, &Emoji)> = app.emoji.all.iter()
            .filter(|e| only.is_none_or(|c| e.category == c))
            .filter_map(|e| terms.iter().map(|t| rank(e, t)).sum::<Option<i32>>().map(|s| (s, e)))
            .collect();
        hits.sort_by(|a, b| b.0.cmp(&a.0));
        sections.push("Results", hits.into_iter().take(160).map(|(_, e)| item(app, e, "r")).collect());
    } else {
        if only.is_none() {
            let find = |g: &str| app.emoji.all.iter().find(|e| e.glyph == g);
            sections.push("Pinned", app.emoji.saved.pinned.iter().filter_map(|g| find(g)).map(|e| item(app, e, "p")).collect());
            sections.push("Frequently Used", app.emoji.frequent().into_iter().filter_map(find).map(|e| item(app, e, "f")).collect());
        }
        for (code, title) in CATEGORIES {
            if only.is_none_or(|c| c == code) {
                sections.push(title, app.emoji.all.iter().filter(|e| e.category == code).map(|e| item(app, e, code)).collect());
            }
        }
    }
    let (items, rows) = sections.flatten(true);
    let filters = filters();
    let mut built = Built::list(Sections::default(), "Search emoji and symbols...").layout(Layout::Grid { columns: 8 }).filters(&filters).keep();
    built.items = items;
    built.rows = rows;
    built
}

pub fn perform(app: &mut App, glyph: &str, op: EmojiOp, tx: &mut Tx) {
    match op {
        EmojiOp::Paste => {
            app.emoji.used(glyph);
            app.perform(Act::Paste(glyph.to_string()), tx);
        }
        EmojiOp::Copy => {
            app.emoji.used(glyph);
            app.perform(Act::Copy(glyph.to_string()), tx);
        }
        EmojiOp::Pin => {
            let pinned = &mut app.emoji.saved.pinned;
            let now = match pinned.iter().position(|p| p == glyph) {
                Some(at) => {
                    pinned.remove(at);
                    false
                }
                None => {
                    pinned.push(glyph.to_string());
                    true
                }
            };
            app.emoji.save();
            app.refresh_keeping();
            app.toast(if now { "Emoji Pinned" } else { "Emoji Unpinned" }, Tone::Success);
        }
    }
}
