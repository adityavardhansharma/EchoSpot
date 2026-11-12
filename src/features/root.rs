//! The launcher itself: applications and commands, favorites and suggestions when nothing is
//! typed, ranked results once something is, with the calculator's card leading and the
//! fallbacks trailing.

use crate::app::{App, Built};
use crate::features::{calc, fuzzy, quicklinks, system, windows};
use crate::icons;
use crate::model::*;
use crate::theme::Tint;
use crate::util;

struct Builtin {
    id: &'static str,
    title: &'static str,
    keywords: &'static str,
    icon: &'static rune::Icon,
    tint: Tint,
    route: Route,
}

fn builtins() -> Vec<Builtin> {
    vec![
        Builtin { id: "cmd:clipboard", title: "Clipboard History", keywords: "paste copy history", icon: &icons::CLIPBOARD, tint: Tint::Orange, route: Route::Clipboard },
        Builtin { id: "cmd:emoji", title: "Search Emoji & Symbols", keywords: "emoji emoticon smiley symbols", icon: &icons::SMILE, tint: Tint::Yellow, route: Route::Emoji },
        Builtin { id: "cmd:files", title: "Search Files", keywords: "find file folder document", icon: &icons::FILE_SEARCH, tint: Tint::Blue, route: Route::Files },
        Builtin { id: "cmd:windows", title: "Switch Windows", keywords: "window switcher open apps", icon: &icons::WINDOWS, tint: Tint::Indigo, route: Route::Windows },
        Builtin { id: "cmd:snippets", title: "Search Snippets", keywords: "snippet text expand", icon: &icons::TEXT, tint: Tint::Red, route: Route::Snippets },
        Builtin { id: "cmd:quicklinks", title: "Search Quicklinks", keywords: "quicklink bookmark url link", icon: &icons::LINK, tint: Tint::Teal, route: Route::Quicklinks },
        Builtin { id: "cmd:notes", title: "Search Notes", keywords: "note markdown write memo", icon: &icons::FILE_TEXT, tint: Tint::Yellow, route: Route::Notes },
        Builtin { id: "cmd:schedule", title: "My Schedule", keywords: "calendar events meetings agenda today", icon: &icons::CLOCK, tint: Tint::Red, route: Route::Schedule },
        Builtin { id: "cmd:themes", title: "Change Theme", keywords: "omarchy theme appearance colors", icon: &icons::PALETTE, tint: Tint::Pink, route: Route::Themes },
        Builtin { id: "cmd:store", title: "Store", keywords: "raycast extensions install browse", icon: &icons::STORE, tint: Tint::Cyan, route: Route::Store },
        Builtin { id: "cmd:extensions", title: "Manage Extensions", keywords: "raycast extensions installed uninstall", icon: &icons::PUZZLE, tint: Tint::Green, route: Route::Extensions },
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
        let mut item = Item::new(id.clone(), a.name.clone(), a.icon_src()).trailing("Application")
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
        let item = Item::new(b.id, b.title, IconSrc::Badge(b.icon, b.tint)).trailing("Command")
            .action(Action::new("Open Command", &icons::ARROW_UP_RIGHT, Act::Push(b.route.clone())))
            .action(favorite_action(app, b.id));
        out.push(Entry { item, keywords: b.keywords.into() });
    }
    out.push(Entry {
        item: Item::new("cmd:new-snippet", "Create Snippet", IconSrc::Badge(&icons::PLUS, Tint::Red)).trailing("Command")
            .action(Action::new("Open Command", &icons::ARROW_UP_RIGHT, Act::Snippet(SnippetOp::New))),
        keywords: "new snippet add text".into(),
    });
    out.push(Entry {
        item: Item::new("cmd:new-quicklink", "Create Quicklink", IconSrc::Badge(&icons::PLUS, Tint::Teal)).trailing("Command")
            .action(Action::new("Open Command", &icons::ARROW_UP_RIGHT, Act::Quicklink(QuicklinkOp::New))),
        keywords: "new quicklink add bookmark url".into(),
    });
    out.push(Entry {
        item: Item::new("cmd:new-note", "Create Note", IconSrc::Badge(&icons::PLUS, Tint::Yellow)).trailing("Command")
            .action(Action::new("Open Command", &icons::ARROW_UP_RIGHT, Act::Note(NoteOp::New(String::new())))),
        keywords: "new note write markdown memo".into(),
    });
    for (id, title, op) in [("cmd:backup-export", "Export Backup", BackupOp::Export), ("cmd:backup-import", "Import Backup", BackupOp::Import)] {
        out.push(Entry {
            item: Item::new(id, title, IconSrc::Badge(if matches!(op, BackupOp::Export) { &icons::UPLOAD } else { &icons::DOWNLOAD }, Tint::Graphite)).trailing("Command")
                .action(Action::new("Open Command", &icons::ARROW_UP_RIGHT, Act::Backup(op))),
            keywords: "backup restore export import settings data migrate".into(),
        });
    }
    out.push(Entry {
        item: Item::new("cmd:settings", "EchoSpot Settings", IconSrc::Badge(&icons::GEAR, Tint::Graphite)).trailing("Command")
            .action(Action::new("Open Settings", &icons::GEAR, Act::OpenSettings(String::new()))),
        keywords: "preferences settings configure options".into(),
    });
    out.push(Entry {
        item: Item::new("cmd:ai", "AI Chat", IconSrc::Badge(&icons::AI, Tint::Violet)).trailing("Command")
            .action(Action::new("Open Command", &icons::ARROW_UP_RIGHT, Act::Ai(AiOp::Open(String::new()))))
            .action(favorite_action(app, "cmd:ai")),
        keywords: "ai ask claude gpt assistant quick ai chat".into(),
    });
    for q in crate::features::ai::quick_actions(app) {
        let id = format!("ai:{}", q.id);
        out.push(Entry {
            item: Item::new(id.clone(), q.title.clone(), IconSrc::Badge(q.icon, Tint::Violet)).trailing("Quick Action")
                .action(Action::new("Run on Selected Text", &icons::SPARKLES, Act::Ai(AiOp::Quick(q.id.clone()))))
                .action(favorite_action(app, &id)),
            keywords: q.keywords.clone(),
        });
    }
    for p in &app.ai_library.presets {
        let id = format!("preset:{}", p.id);
        out.push(Entry {
            item: Item::new(id.clone(), format!("Chat with {}", p.name), IconSrc::Badge(&icons::PERSON, Tint::Violet)).trailing("Command")
                .action(Action::new("New Chat with Preset", &icons::CHAT, Act::Ai(AiOp::UsePreset(p.id.clone()))))
                .action(favorite_action(app, &id)),
            keywords: "ai chat preset".into(),
        });
    }
    for c in system::COMMANDS.iter().filter(|c| system::available(c.action)) {
        let id = format!("sys:{:?}", c.action);
        let run = Act::System(c.action);
        let act = match c.confirm {
            Some((message, confirm)) => Act::Confirm(Box::new(Confirm {
                title: c.title.into(), message: message.into(), icon: c.icon, confirm: confirm.into(),
                destructive: matches!(c.action, system::SystemAction::EmptyTrash | system::SystemAction::Shutdown | system::SystemAction::Restart | system::SystemAction::Logout), act: run,
            })),
            None => run,
        };
        let mut item = Item::new(id.clone(), c.title, IconSrc::Badge(c.icon, system::tint(c.action))).trailing("Command")
            .action(Action::new("Run Command", &icons::BOLT, act))
            .action(favorite_action(app, &id));
        // A switch says where it stands before it is flipped.
        if let Some((label, on)) = system::state(c.action, &app.status) {
            item.accessories.push(Accessory { icon: None, text: label, tag: on.then(|| crate::theme::Tint::Green.stops().0) });
        }
        out.push(Entry { item, keywords: c.keywords.into() });
    }
    if windows::hyprland() {
        for c in windows::COMMANDS {
            let id = format!("win:{:?}", c.op);
            let item = Item::new(id.clone(), c.title, IconSrc::Badge(c.icon, Tint::Indigo)).trailing("Window")
                .action(Action::new("Run Command", &icons::BOLT, Act::Window(c.op)))
                .action(favorite_action(app, &id));
            out.push(Entry { item, keywords: c.keywords.into() });
        }
    }
    for q in &app.quicklinks.list {
        let id = format!("ql:{}", q.id);
        // Two links may share a name (a site and a search of it): the host tells them apart.
        let mut item = Item::new(id.clone(), q.name.clone(), quicklinks::icon(&q.link)).trailing("Quicklink")
            .action(Action::new("Open Quicklink", &icons::ARROW_UP_RIGHT, Act::Quicklink(QuicklinkOp::Open(q.id.clone(), String::new()))))
            .action(Action::new("Copy Link", &icons::COPY, Act::Copy(q.link.clone())).keys("ctrl+shift+c"))
            .action(favorite_action(app, &id))
            .action(Action::new("Edit Quicklink", &icons::PENCIL, Act::Quicklink(QuicklinkOp::Edit(q.id.clone()))).keys("ctrl+e").section(""));
        item.subtitle = quicklinks::host(&q.link).map(|h| {
            let h = h.trim_start_matches("www.");
            if quicklinks::takes_query(&q.link) { format!("Search {h}") } else { h.to_string() }
        });
        out.push(Entry { item, keywords: format!("{} {}", q.alias, q.link) });
    }
    for s in &app.snippets.list {
        let id = format!("snip:{}", s.id);
        let item = Item::new(id.clone(), s.name.clone(), IconSrc::Badge(&icons::TEXT, Tint::Red)).trailing("Snippet")
            .action(Action::new(app.paste_target(), &icons::CLIPBOARD, Act::Paste(crate::features::snippets::expand(&s.text))))
            .action(Action::new("Copy to Clipboard", &icons::COPY, Act::Copy(crate::features::snippets::expand(&s.text))).keys("ctrl+shift+c"));
        out.push(Entry { item, keywords: format!("{} {}", s.keyword, util::truncate(&s.text, 60)) });
    }
    out.extend(crate::ext::commands(app).into_iter().map(|(item, keywords)| Entry { item, keywords }));
    out.extend(crate::features::scripts::rows(app).into_iter().map(|(item, keywords)| Entry { item, keywords }));
    // Any row can carry an alias and a global hotkey.
    for e in &mut out {
        let id = e.item.id.clone();
        if let Some(alias) = app.bindings.aliases.get(&id) {
            e.keywords.push_str(&format!(" {alias}"));
            e.item.accessories.push(Accessory { icon: None, text: alias.clone(), tag: None });
        }
        if let Some(chord) = app.bindings.hotkeys.get(&id) {
            e.item.accessories.push(Accessory { icon: Some(&icons::KEYBOARD), text: keycaps(chord).join(" "), tag: None });
        }
        e.item.actions.push(Action::new("Set Alias or Hotkey…", &icons::KEYBOARD, Act::Binding(id, e.item.title.clone())).keys("ctrl+shift+k").section(""));
    }
    out
}

/// What Return does on the launcher row with this id.
pub fn primary(app: &App, id: &str) -> Option<Act> {
    entries(app).into_iter().find(|e| e.item.id == id).and_then(|e| e.item.actions.into_iter().next()).map(|a| a.act)
}

/// Every launcher row as `id<TAB>title`, for `echospot commands`.
pub fn listing(app: &App) -> String {
    entries(app).iter().map(|e| format!("{}\t{}", e.item.id, e.item.title)).collect::<Vec<_>>().join("\n")
}

/// What a query can be handed to when nothing above is what was meant. `shown` holds the ids of
/// the rows already in the results, so a quicklink the query found by name isn't offered twice.
fn fallbacks(app: &App, query: &str, shown: &std::collections::HashSet<String>) -> Vec<Item> {
    let mut out = vec![
        Item::new("fb:ai", "Ask AI", IconSrc::Badge(&icons::AI, Tint::Violet)).trailing("Command")
            .action(Action::new("Ask AI", &icons::SPARKLES, Act::Ai(AiOp::Open(query.into())))),
        Item::new("fb:files", "Search Files", IconSrc::Badge(&icons::FILE_SEARCH, Tint::Blue)).trailing("Command")
            .action(Action::new("Search Files", &icons::FILE_SEARCH, Act::PushQuery(Route::Files, query.into()))),
    ];
    for q in app.quicklinks.list.iter().filter(|q| quicklinks::takes_query(&q.link) && !shown.contains(&format!("ql:{}", q.id))) {
        out.push(Item::new(format!("fb:ql:{}", q.id), format!("Search {}", q.name), quicklinks::icon(&q.link)).trailing("Quicklink")
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
        commands.extend(rest("Script"));
        commands.extend(rest("Quick Action"));
        commands.extend(rest("Window"));
        let extensions: Vec<Item> = all.iter()
            .filter(|e| e.item.id.starts_with("ext:") && !shown.contains(e.item.id.as_str())).map(|e| e.item.clone()).collect();
        sections.push("Up Next", crate::features::calendar::up_next(app));
        sections.push("Favorites", favorites);
        sections.push("Suggestions", suggestions);
        sections.push("Applications", applications);
        sections.push("Extensions", extensions);
        sections.push("Commands", commands.into_iter().filter(|i| !i.id.starts_with("ext:")).collect());
        return Built::list(sections, "Search for apps and commands...");
    }

    if let Some(card) = calc::evaluate(q) {
        let answer = card.right.clone();
        let mut item = Item::new("calc", answer.clone(), IconSrc::Badge(&icons::CALCULATOR, Tint::Orange))
            .action(Action::new("Copy Answer", &icons::COPY, Act::Copy(answer.clone())))
            .action(Action::new(app.paste_target(), &icons::CLIPBOARD, Act::Paste(answer.clone())).keys("ctrl+enter"))
            .action(Action::new("Copy Plain Number", &icons::HASH, Act::Copy(calc::plain(&answer))).keys("ctrl+shift+c"));
        item.card = Some(card);
        sections.push("Calculator", vec![item]);
    }

    // An alias and a space hand the rest of the line to that quicklink: `g rust traits`.
    if let Some((alias, rest)) = q.split_once(' ').map(|(a, r)| (a, r.trim())).filter(|(_, r)| !r.is_empty()) {
        let hits: Vec<Item> = app.quicklinks.list.iter()
            .filter(|l| !l.alias.is_empty() && l.alias.eq_ignore_ascii_case(alias) && quicklinks::takes_query(&l.link))
            .map(|l| Item::new(format!("alias:{}", l.id), format!("Search {} for “{}”", l.name, util::truncate(rest, 40)), quicklinks::icon(&l.link))
                .trailing("Quicklink")
                .action(Action::new("Open in Browser", &icons::ARROW_UP_RIGHT, Act::Quicklink(QuicklinkOp::Open(l.id.clone(), rest.into()))))
                .action(Action::new("Copy Link", &icons::COPY, Act::Copy(quicklinks::resolve(&l.link, rest))).keys("ctrl+shift+c")))
            .collect();
        sections.push("Quicklink", hits);
    }

    let mut scored: Vec<(i32, f32, Item)> = all.into_iter().filter_map(|e| {
        let score = fuzzy::score_item(q, &e.item.title, &e.keywords)?;
        let boost = app.usage.frecency(&e.item.id);
        // An alias typed exactly is that row, whatever else matches.
        let alias = app.bindings.aliases.get(&e.item.id).is_some_and(|a| a.eq_ignore_ascii_case(q));
        Some((score + (boost * 12.0) as i32 + if app.usage.is_favorite(&e.item.id) { 20 } else { 0 } + if alias { 100_000 } else { 0 }, boost, e.item))
    }).collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.total_cmp(&a.1)).then(a.2.title.len().cmp(&b.2.title.len())));
    let results: Vec<Item> = scored.into_iter().take(60).map(|(_, _, item)| item).collect();
    let shown = results.iter().map(|i| i.id.clone()).collect();
    sections.push("Results", results);
    sections.push(&format!("Use “{}” with…", util::truncate(q, 28)), fallbacks(app, q, &shown));
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
        let icon = entry.map(|a| a.icon_src()).unwrap_or(IconSrc::Glyph(&icons::WINDOW));
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
        Item::new(format!("theme:{n}"), title.clone(), IconSrc::Badge(&icons::PALETTE, Tint::Pink)).trailing(if active { "Current" } else { "Theme" })
            .action(Action::new("Apply Theme", &icons::CHECK, Act::ShellQuiet { cmd: format!("omarchy theme set {}", util::quote(&title)), done: format!("{title} Applied") }))
    }).collect();
    let mut sections = Sections::default();
    sections.push("Themes", items);
    Built::list(sections, "Search themes...").keep()
}
