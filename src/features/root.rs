//! The launcher itself: applications and commands, favorites and suggestions when nothing is
//! typed, ranked results once something is, with the calculator's card leading and the
//! fallbacks trailing.

use crate::app::{App, Built};
use crate::features::{calc, fuzzy, system, windows};
use crate::icons;
use crate::model::*;
use crate::util;

struct Builtin {
    id: &'static str,
    title: &'static str,
    keywords: &'static str,
    icon: &'static rune::Icon,
    route: Route,
}

fn builtins() -> Vec<Builtin> {
    vec![
        Builtin { id: "cmd:clipboard", title: "Clipboard History", keywords: "paste copy history", icon: &icons::CLIPBOARD, route: Route::Clipboard },
        Builtin { id: "cmd:emoji", title: "Search Emoji & Symbols", keywords: "emoji emoticon smiley symbols", icon: &icons::SMILE, route: Route::Emoji },
        Builtin { id: "cmd:files", title: "Search Files", keywords: "find file folder document", icon: &icons::FILE_SEARCH, route: Route::Files },
        Builtin { id: "cmd:windows", title: "Switch Windows", keywords: "window switcher open apps", icon: &icons::WINDOWS, route: Route::Windows },
        Builtin { id: "cmd:snippets", title: "Search Snippets", keywords: "snippet text expand", icon: &icons::TEXT, route: Route::Snippets },
        Builtin { id: "cmd:quicklinks", title: "Search Quicklinks", keywords: "quicklink bookmark url link", icon: &icons::LINK, route: Route::Quicklinks },
        Builtin { id: "cmd:themes", title: "Change Theme", keywords: "omarchy theme appearance colors", icon: &icons::PALETTE, route: Route::Themes },
        Builtin { id: "cmd:store", title: "Store", keywords: "raycast extensions install browse", icon: &icons::STORE, route: Route::Store },
        Builtin { id: "cmd:extensions", title: "Manage Extensions", keywords: "raycast extensions installed uninstall", icon: &icons::PUZZLE, route: Route::Extensions },
    ]
}

/// The usage id a pushed route records under.
pub fn command_id(route: &Route) -> Option<&'static str> {
    builtins().into_iter().find(|b| &b.route == route).map(|b| b.id)
}

fn favorite_action(app: &App, id: &str) -> Action {
    if app.usage.is_favorite(id) {
        Action::new("Remove from Favorites", &icons::STAR, Act::Favorite(id.into())).keys("ctrl+shift+f")
    } else {
        Action::new("Add to Favorites", &icons::STAR, Act::Favorite(id.into())).keys("ctrl+shift+f")
    }
}

struct Entry {
    item: Item,
    keywords: String,
}

fn entries(app: &App) -> Vec<Entry> {
    let mut out = Vec::new();
    for a in &app.apps {
        let id = format!("app:{}", a.id);
        if app.usage.hidden.contains(&id) {
            continue;
        }
        let mut item = Item::new(id.clone(), a.name.clone(), IconSrc::App(a.path.clone())).trailing("Application")
            .action(Action::new("Open Application", &icons::ARROW_UP_RIGHT, Act::Launch(a.path.clone())))
            .action(favorite_action(app, &id))
            .action(Action::new("Copy Name", &icons::COPY, Act::Copy(a.name.clone())).keys("ctrl+shift+c"))
            .action(Action::new("Show Desktop Entry", &icons::FOLDER, Act::Reveal(a.path.clone())))
            .action(Action::new("Hide from Results", &icons::EYE, Act::Hide(id.clone())).section(""));
        item.running = app.running.contains(&a.wm_class.to_lowercase())
            || app.running.contains(&a.id.trim_end_matches(".desktop").to_lowercase());
        out.push(Entry { item, keywords: format!("{} {} {}", a.generic, a.keywords, a.exec.split_whitespace().next().unwrap_or("")) });
    }
    for b in builtins() {
        let item = Item::new(b.id, b.title, IconSrc::Glyph(b.icon)).trailing("Command")
            .action(Action::new("Open Command", &icons::ARROW_UP_RIGHT, Act::Push(b.route.clone())))
            .action(favorite_action(app, b.id));
        out.push(Entry { item, keywords: b.keywords.into() });
    }
    out.push(Entry {
        item: Item::new("cmd:new-snippet", "Create Snippet", IconSrc::Glyph(&icons::PLUS)).trailing("Command")
            .action(Action::new("Open Command", &icons::ARROW_UP_RIGHT, Act::Snippet(SnippetOp::New))),
        keywords: "new snippet add text".into(),
    });
    out.push(Entry {
        item: Item::new("cmd:new-quicklink", "Create Quicklink", IconSrc::Glyph(&icons::PLUS)).trailing("Command")
            .action(Action::new("Open Command", &icons::ARROW_UP_RIGHT, Act::Quicklink(QuicklinkOp::New))),
        keywords: "new quicklink add bookmark url".into(),
    });
    for c in system::COMMANDS.iter().filter(|c| system::available(c.action)) {
        let id = format!("sys:{:?}", c.action);
        let run = Act::System(c.action);
        let act = match c.confirm {
            Some((message, confirm)) => Act::Confirm(Box::new(Confirm {
                title: c.title.into(), message: message.into(), icon: c.icon, confirm: confirm.into(),
                destructive: matches!(c.action, system::SystemAction::EmptyTrash), act: run,
            })),
            None => run,
        };
        let item = Item::new(id.clone(), c.title, IconSrc::Glyph(c.icon)).trailing("Command")
            .action(Action::new("Run Command", &icons::BOLT, act))
            .action(favorite_action(app, &id));
        out.push(Entry { item, keywords: c.keywords.into() });
    }
    if windows::hyprland() {
        for c in windows::COMMANDS {
            let id = format!("win:{:?}", c.op);
            let item = Item::new(id.clone(), c.title, IconSrc::Glyph(c.icon)).trailing("Window")
                .action(Action::new("Run Command", &icons::BOLT, Act::Window(c.op)))
                .action(favorite_action(app, &id));
            out.push(Entry { item, keywords: c.keywords.into() });
        }
    }
    for q in &app.quicklinks.list {
        let id = format!("ql:{}", q.id);
        let item = Item::new(id.clone(), q.name.clone(), IconSrc::Glyph(&icons::LINK)).trailing("Quicklink")
            .action(Action::new("Open Quicklink", &icons::ARROW_UP_RIGHT, Act::Quicklink(QuicklinkOp::Open(q.id.clone(), String::new()))))
            .action(favorite_action(app, &id));
        out.push(Entry { item, keywords: format!("{} {}", q.alias, q.link) });
    }
    for s in &app.snippets.list {
        let id = format!("snip:{}", s.id);
        let item = Item::new(id.clone(), s.name.clone(), IconSrc::Glyph(&icons::TEXT)).trailing("Snippet")
            .action(Action::new(app.paste_target(), &icons::CLIPBOARD, Act::Paste(crate::features::snippets::expand(&s.text))))
            .action(Action::new("Copy to Clipboard", &icons::COPY, Act::Copy(crate::features::snippets::expand(&s.text))).keys("ctrl+shift+c"));
        out.push(Entry { item, keywords: format!("{} {}", s.keyword, util::truncate(&s.text, 60)) });
    }
    out.extend(crate::ext::commands(app).into_iter().map(|(item, keywords)| Entry { item, keywords }));
    out
}

fn fallbacks(app: &App, query: &str) -> Vec<Item> {
    let mut out = vec![
        Item::new("fb:files", "Search Files", IconSrc::Glyph(&icons::FILE_SEARCH)).trailing("Command")
            .action(Action::new("Search Files", &icons::FILE_SEARCH, Act::PushQuery(Route::Files, query.into()))),
    ];
    for q in app.quicklinks.list.iter().filter(|q| q.link.contains("{query}") || q.link.contains("{argument}")) {
        out.push(Item::new(format!("fb:ql:{}", q.id), format!("Search {}", q.name), IconSrc::Glyph(&icons::GLOBE)).trailing("Quicklink")
            .action(Action::new("Open in Browser", &icons::ARROW_UP_RIGHT, Act::Quicklink(QuicklinkOp::Open(q.id.clone(), query.into())))));
    }
    out
}

pub fn build(app: &App, query: &str) -> Built {
    let q = query.trim();
    let all = entries(app);
    let mut sections = Sections::default();
    if q.is_empty() {
        let mut favorites: Vec<Item> = Vec::new();
        for id in &app.usage.favorites {
            if let Some(e) = all.iter().find(|e| &e.item.id == id) {
                favorites.push(e.item.clone());
            }
        }
        let mut ranked: Vec<(&Entry, f32)> = all.iter()
            .filter(|e| !app.usage.is_favorite(&e.item.id))
            .map(|e| (e, app.usage.frecency(&e.item.id)))
            .filter(|(_, f)| *f > 0.0)
            .collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        let suggestions: Vec<Item> = ranked.iter().take(6).map(|(e, _)| e.item.clone()).collect();
        let shown: std::collections::HashSet<&str> = favorites.iter().chain(&suggestions).map(|i| i.id.as_str()).collect();
        let rest = |kind: &str| all.iter().filter(|e| e.item.trailing == kind && !shown.contains(e.item.id.as_str())).map(|e| e.item.clone()).collect::<Vec<_>>();
        let applications = rest("Application");
        let mut commands = rest("Command");
        commands.extend(rest("Window"));
        let extensions: Vec<Item> = all.iter()
            .filter(|e| e.item.id.starts_with("ext:") && !shown.contains(e.item.id.as_str())).map(|e| e.item.clone()).collect();
        sections.push("Favorites", favorites);
        sections.push("Suggestions", suggestions);
        sections.push("Applications", applications);
        sections.push("Extensions", extensions);
        sections.push("Commands", commands.into_iter().filter(|i| !i.id.starts_with("ext:")).collect());
        return Built::list(sections, "Search for apps and commands...");
    }

    if let Some(card) = calc::evaluate(q) {
        let answer = card.right.clone();
        let mut item = Item::new("calc", answer.clone(), IconSrc::Glyph(&icons::CALCULATOR))
            .action(Action::new("Copy Answer", &icons::COPY, Act::Copy(answer.clone())))
            .action(Action::new(app.paste_target(), &icons::CLIPBOARD, Act::Paste(answer.clone())).keys("ctrl+enter"))
            .action(Action::new("Copy Plain Number", &icons::HASH, Act::Copy(answer.replace(',', "").split(' ').next().unwrap_or("").to_string())).keys("ctrl+shift+c"));
        item.card = Some(card);
        sections.push("Calculator", vec![item]);
    }

    let mut scored: Vec<(i32, f32, Item)> = all.into_iter().filter_map(|e| {
        let score = fuzzy::score_item(q, &e.item.title, &e.keywords)?;
        let boost = app.usage.frecency(&e.item.id);
        Some((score + (boost * 12.0) as i32 + if app.usage.is_favorite(&e.item.id) { 20 } else { 0 }, boost, e.item))
    }).collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.total_cmp(&a.1)).then(a.2.title.len().cmp(&b.2.title.len())));
    sections.push("Results", scored.into_iter().take(60).map(|(_, _, item)| item).collect());
    sections.push(&format!("Use “{}” with…", util::truncate(q, 28)), fallbacks(app, q));
    Built::list(sections, "Search for apps and commands...")
}

/// The window switcher: open windows, most recent first.
pub fn windows(app: &App, query: &str) -> Built {
    let q = query.trim();
    let mut scored: Vec<(i32, Item)> = Vec::new();
    for (order, c) in windows::clients().into_iter().enumerate() {
        let entry = app.apps.iter().find(|a| a.wm_class.eq_ignore_ascii_case(&c.class) || a.id.trim_end_matches(".desktop").eq_ignore_ascii_case(&c.class));
        let app_name = entry.map(|a| a.name.clone()).unwrap_or_else(|| c.class.clone());
        let score = if q.is_empty() { 1000 - order as i32 } else {
            match fuzzy::score_item(q, &c.title, &app_name) {
                Some(s) => s,
                None => continue,
            }
        };
        let icon = entry.map(|a| IconSrc::App(a.path.clone())).unwrap_or(IconSrc::Glyph(&icons::WINDOW));
        let workspace = if c.workspace.name.is_empty() { c.workspace.id.to_string() } else { c.workspace.name.clone() };
        let mut item = Item::new(format!("window:{}", c.address), c.title.clone(), icon).subtitle(app_name)
            .trailing(&format!("Workspace {workspace}"))
            .action(Action::new("Switch to Window", &icons::ARROW_UP_RIGHT, Act::FocusWindow(c.address.clone())))
            .action(Action::new("Close Window", &icons::X, Act::CloseWindow(c.address.clone())).keys("ctrl+w").destructive());
        item.running = false;
        scored.push((score, item));
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    let mut sections = Sections::default();
    sections.push("Open Windows", scored.into_iter().map(|(_, i)| i).collect());
    Built::list(sections, "Search open windows...").empty("No Open Windows", "")
}

/// Omarchy's themes, applied with `omarchy theme set`.
pub fn themes(_app: &App, query: &str) -> Built {
    let q = query.trim();
    let current = util::sh_output("omarchy theme current 2>/dev/null").unwrap_or_default().to_lowercase();
    let mut names: Vec<String> = Vec::new();
    for dir in [util::home().join(".config/omarchy/themes"), "/usr/share/omarchy/themes".into()] {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if e.path().is_dir() && !names.contains(&name) {
                    names.push(name);
                }
            }
        }
    }
    names.sort();
    let pretty = |n: &str| n.split('-').map(|w| {
        let mut c = w.chars();
        c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
    }).collect::<Vec<_>>().join(" ");
    let items: Vec<Item> = names.iter().filter(|n| q.is_empty() || fuzzy::score(q, &pretty(n)).is_some()).map(|n| {
        let title = pretty(n);
        let active = current == title.to_lowercase() || current == *n;
        Item::new(format!("theme:{n}"), title.clone(), IconSrc::Glyph(&icons::PALETTE)).trailing(if active { "Current" } else { "Theme" })
            .action(Action::new("Apply Theme", &icons::CHECK, Act::ShellQuiet { cmd: format!("omarchy theme set {}", util::quote(&title)), done: format!("{title} Applied") }))
    }).collect();
    let mut sections = Sections::default();
    sections.push("Themes", items);
    Built::list(sections, "Search themes...").keep()
}
