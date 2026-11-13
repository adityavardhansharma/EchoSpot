//! The palette's state and everything that changes it: navigation, selection, keys, and the one
//! place an action runs.

use std::collections::HashSet;

use rune::{AppHandle, Key, KeyEvent, TextEdit, Tx};

use crate::ext::Extensions;
use crate::features::{self, apps::AppEntry, clipboard::Clipboard, system, windows};
use crate::icons;
use crate::model::*;
use crate::store::{IconStore, Usage};
use crate::util;

pub struct Screen {
    pub route: Route,
    pub query: TextEdit,
    pub items: Vec<Item>,
    pub rows: Vec<Row>,
    pub sel: usize,
    pub layout: Layout,
    pub placeholder: String,
    /// The header's pop-up filter: its options and the chosen one.
    pub filters: Vec<String>,
    pub filter: usize,
    pub loading: bool,
    pub empty: (String, String),
}

impl Screen {
    pub fn new(route: Route) -> Self {
        let mut query = TextEdit::new("");
        query.focused = true;
        Self {
            route, query, items: Vec::new(), rows: Vec::new(), sel: 0, layout: Layout::List,
            placeholder: String::new(), filters: Vec::new(), filter: 0, loading: false,
            empty: ("No Results".into(), String::new()),
        }
    }
    pub fn selected(&self) -> Option<&Item> {
        self.items.get(self.sel)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum MenuKind {
    Actions,
    Main,
    Filter,
}

pub struct Menu {
    pub kind: MenuKind,
    pub query: TextEdit,
    pub sel: usize,
    pub entries: Vec<Action>,
}

impl Menu {
    /// The entries the menu's own search leaves, with their original indices.
    pub fn visible(&self) -> Vec<(usize, &Action)> {
        let q = self.query.text.trim();
        self.entries.iter().enumerate()
            .filter(|(_, a)| q.is_empty() || features::fuzzy::score(q, &a.title).is_some())
            .collect()
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Tone {
    Neutral,
    Success,
    Danger,
    Progress,
}

pub struct Toast {
    pub text: String,
    pub tone: Tone,
    pub until: f64,
}

/// A page of Markdown filling the palette: a script's output, a note being read.
#[derive(Clone)]
pub struct Page {
    pub title: String,
    pub markdown: String,
    pub actions: Vec<Action>,
}

pub struct App {
    pub me: Option<AppHandle<App>>,
    pub screens: Vec<Screen>,
    pub menu: Option<Menu>,
    pub dialog: Option<Confirm>,
    /// Which dialog button is lit: 0 cancel, 1 confirm.
    pub dialog_sel: usize,
    pub toast: Option<Toast>,
    pub apps: Vec<AppEntry>,
    pub usage: Usage,
    pub icons: IconStore,
    /// The window that had focus when the palette opened: what Paste and the layouts act on.
    pub prev_window: Option<windows::Client>,
    pub running: HashSet<String>,
    pub clipboard: Clipboard,
    pub emoji: features::emoji::EmojiStore,
    pub files: features::files::FileSearch,
    pub snippets: features::snippets::Snippets,
    pub quicklinks: features::quicklinks::Quicklinks,
    pub bindings: features::bindings::Bindings,
    pub scripts: Vec<features::scripts::Script>,
    pub page: Option<Page>,
    pub notes: Vec<features::notes::Note>,
    pub snap: features::snap::Snap,
    pub schedule: features::calendar::Schedule,
    pub ext: Extensions,
    pub chat: features::ai::Chat,
    pub ai_library: features::ai_library::Library,
    pub form: Option<features::forms::Form>,
    pub clock: f64,
    pub scale: f32,
    pub visible: bool,
    /// Hide when focus leaves; off while a file dialog or a paste owns the moment.
    pub hide_on_blur: bool,
    pub settings: crate::settings::Settings,
    /// The screen behind the panel, captured as it was summoned.
    pub backdrop: rune::Pixels,
    pub has_backdrop: bool,
    /// The same capture, kept whole for the Liquid theme's lens to refract.
    pub glass: crate::glass::Shot,
    /// Whether the system is dark, as the last frame saw it: the glass's tint follows it.
    pub system_dark: std::cell::Cell<bool>,
    pub was_focused: bool,
    /// What the system's switches read when last asked: the state on a toggle's row.
    pub status: system::Status,
    /// When the selection last moved by key, to tell a held key from a press.
    last_step: f64,
    /// The extension call a confirm dialog answers: session and call id.
    pub dialog_reply: Option<(u64, u64)>,
}

impl App {
    pub fn new() -> Self {
        let mut app = Self {
            me: None,
            screens: vec![Screen::new(Route::Root)],
            menu: None,
            dialog: None,
            dialog_sel: 1,
            toast: None,
            apps: features::apps::scan(),
            usage: Usage::load(),
            icons: IconStore::default(),
            prev_window: None,
            running: HashSet::new(),
            clipboard: Clipboard::load(),
            emoji: features::emoji::EmojiStore::load(),
            files: features::files::FileSearch::default(),
            snippets: features::snippets::Snippets::load(),
            quicklinks: features::quicklinks::Quicklinks::load(),
            bindings: features::bindings::Bindings::load(),
            scripts: features::scripts::load(),
            page: None,
            notes: Vec::new(),
            snap: Default::default(),
            schedule: Default::default(),
            ext: Extensions::load(),
            chat: features::ai::Chat::default(),
            ai_library: features::ai_library::Library::load(),
            form: None,
            clock: 0.0,
            scale: 2.0,
            visible: false,
            hide_on_blur: true,
            settings: crate::settings::Settings::load(),
            backdrop: rune::Pixels::new(),
            has_backdrop: false,
            glass: Default::default(),
            system_dark: std::cell::Cell::new(true),
            was_focused: false,
            status: system::Status::default(),
            last_step: f64::NEG_INFINITY,
            dialog_reply: None,
        };
        app.refresh();
        app
    }

    pub fn top(&self) -> &Screen {
        self.screens.last().expect("the root screen never pops")
    }
    pub fn top_mut(&mut self) -> &mut Screen {
        self.screens.last_mut().expect("the root screen never pops")
    }
    pub fn query(&self) -> &str {
        &self.top().query.text
    }

    /// Rebuilds the top screen's rows from its query.
    pub fn refresh(&mut self) {
        let route = self.top().route.clone();
        let query = self.top().query.text.clone();
        let filter = self.top().filter;
        if route == Route::Files {
            let scope = self.settings.files.clone();
            self.files.request(&query, &scope);
        }
        let built = match route {
            Route::Root => features::root::build(self, &query),
            Route::Clipboard => features::clipboard::build(self, &query, filter),
            Route::Emoji => features::emoji::build(self, &query, filter),
            Route::Files => features::files::build(self, &query),
            Route::Windows => features::root::windows(self, &query),
            Route::Snap(only) => features::snap::build(self, only, &query),
            Route::Snippets => features::snippets::build(self, &query),
            Route::Quicklinks => features::quicklinks::build(self, &query),
            Route::Notes => features::notes::build(self, &query),
            Route::Schedule => features::calendar::build(self, &query),
            Route::Themes => features::root::themes(self, &query),
            Route::Form => Built::list(Sections::default(), ""),
            Route::Page => {
                // One unseen row carries the page's actions to the bottom bar.
                let page = self.page.clone();
                let mut built = Built::list(Sections::default(), page.as_ref().map(|p| p.title.as_str()).unwrap_or("")).keep();
                if let Some(page) = page {
                    let mut item = Item::new("page", page.title, IconSrc::None);
                    item.actions = page.actions;
                    built.items = vec![item];
                    built.rows = vec![Row::Item(0)];
                }
                built
            }
            Route::Ai => features::ai::build(self, &query),
            Route::Store => crate::ext::store::build(self, &query),
            Route::Extensions => crate::ext::store::installed(self, &query),
            Route::Ext(id) => crate::ext::view::build(self, id, &query),
        };
        let screen = self.top_mut();
        let keep = screen.selected().map(|i| i.id.clone());
        screen.items = built.items;
        screen.rows = built.rows;
        screen.layout = built.layout;
        screen.placeholder = built.placeholder;
        screen.filters = built.filters;
        screen.loading = built.loading;
        screen.empty = built.empty;
        // A typed query lands on the best match; a refresh in place keeps the row it was on.
        screen.sel = match (built.keep_selection, keep) {
            (true, Some(id)) => screen.items.iter().position(|i| i.id == id).unwrap_or(0),
            _ => 0,
        }.min(screen.items.len().saturating_sub(1));
    }

    pub fn push(&mut self, route: Route, tx: &mut Tx) {
        self.menu = None;
        self.screens.push(Screen::new(route));
        if matches!(self.top().route, Route::Snap(_)) {
            self.snap = features::snap::Snap::load(self.prev_window.as_ref());
        }
        if self.top().route == Route::Clipboard {
            self.clipboard.reload();
        }
        if self.top().route == Route::Notes {
            self.notes = features::notes::load();
        }
        if self.top().route == Route::Schedule {
            features::calendar::refresh(self, false);
        }
        if self.top().route == Route::Ai {
            self.top_mut().filter = features::ai::model_filter(&self.settings.ai).1;
        }
        self.refresh();
        tx.instant();
    }

    /// Reads everything kept on disk again: after a restore, or when another process changed it.
    pub fn reload_stores(&mut self) {
        self.settings = crate::settings::Settings::load();
        self.snippets = features::snippets::Snippets::load();
        self.quicklinks = features::quicklinks::Quicklinks::load();
        self.bindings.reload();
        self.ai_library = features::ai_library::Library::load();
        self.scripts = features::scripts::load();
        self.notes = features::notes::load();
        self.usage = Usage::load();
        self.ext.rescan();
        features::expander::set_enabled(self.settings.snippet_expansion);
        self.schedule.read_at = 0;
        features::calendar::refresh(self, true);
        self.refresh();
    }

    pub fn open_page(&mut self, page: Page, tx: &mut Tx) {
        self.page = Some(page);
        self.push(Route::Page, tx);
    }

    pub fn pop(&mut self, tx: &mut Tx) {
        if self.screens.len() > 1 {
            if let Some(Screen { route: Route::Ext(id), .. }) = self.screens.last() {
                let id = *id;
                if self.ext.pop_view(id) {
                    self.refresh();
                    tx.instant();
                    return;
                }
                self.ext.stop(id);
            }
            let was_form = self.screens.pop().is_some_and(|s| s.route == Route::Form);
            let ext_form = matches!(self.form.take().map(|f| f.target), Some(features::forms::Target::ExtForm { .. }));
            // An extension whose whole view is a form leaves with it.
            if was_form && ext_form && self.screens.len() > 1 {
                return self.pop(tx);
            }
            self.top_mut().query.focused = true;
            self.refresh();
            tx.instant();
        }
    }

    /// Back to the launcher with nothing typed, as a fresh summon finds it.
    pub fn reset(&mut self) {
        let running: Vec<u64> = self.screens.iter().filter_map(|s| if let Route::Ext(id) = s.route { Some(id) } else { None }).collect();
        for id in running {
            // A command waiting on a browser sign-in is still wanted when the palette returns.
            if !self.ext.awaiting_sign_in(id) {
                self.ext.stop(id);
            }
        }
        self.screens.truncate(1);
        self.menu = None;
        self.dialog = None;
        self.form = None;
        self.page = None;
        let root = self.top_mut();
        root.query.clear();
        root.query.focused = true;
        root.sel = 0;
        self.refresh();
    }

    pub fn hide(&mut self, tx: &mut Tx) {
        self.visible = false;
        self.reset();
        tx.settle();
        tx.close_window();
    }

    /// Takes a picture of the screen behind the panel as the panel's surface, for whichever
    /// material the theme draws. False when the theme shows none of it.
    pub fn set_backdrop(&mut self, w: u32, h: u32, rgba: Vec<u8>) -> bool {
        if self.settings.liquid() {
            self.glass.set(w, h, rgba);
        } else if self.settings.backdrop == "capture" {
            self.backdrop.set(w, h, rgba);
            self.has_backdrop = true;
        } else {
            return false;
        }
        true
    }

    /// Called as the palette is summoned: note what it covers, then show fresh rows.
    pub fn will_show(&mut self) {
        self.prev_window = windows::active();
        crate::live::own_shadows_off();
        self.has_backdrop = false;
        self.glass.clear();
        if self.settings.liquid() || self.settings.backdrop == "capture" {
            // A capture for the first frames; the live backdrop follows the screen from there.
            if let Some((w, h, rgba)) = crate::backdrop::capture(self.settings.top_margin) {
                self.set_backdrop(w, h, rgba);
            }
            crate::live::start_palette();
        }
        self.running = windows::clients().into_iter().map(|c| c.class.to_lowercase()).collect();
        self.scripts = features::scripts::load();
        features::calendar::refresh(self, false);
        self.visible = true;
        self.was_focused = false;
        self.hide_on_blur = true;
        self.refresh();
        self.read_status();
        // A Hyprland config reload drops runtime bindings; a summon puts the hotkeys back.
        self.bindings.apply();
    }

    /// Reads the system's switches off the UI thread; the rows that show them update when it lands.
    pub fn read_status(&self) {
        let Some(me) = self.me.clone() else { return };
        std::thread::spawn(move || {
            let status = system::status();
            me.update(move |s, _| {
                s.status = status;
                if s.top().route == Route::Root {
                    s.refresh_keeping();
                }
            });
        });
    }

    pub fn toast(&mut self, text: impl Into<String>, tone: Tone) {
        self.toast = Some(Toast { text: text.into(), tone, until: self.clock + crate::theme::duration::MESSAGE_HUD });
    }

    /// A toast that stays until something replaces it: an install, a command still running.
    pub fn toast_progress(&mut self, text: impl Into<String>) {
        self.toast = Some(Toast { text: text.into(), tone: Tone::Progress, until: f64::INFINITY });
    }

    pub fn select(&mut self, index: usize, tx: &mut Tx) {
        let screen = self.top_mut();
        if screen.items.is_empty() {
            return;
        }
        let index = index.min(screen.items.len() - 1);
        screen.sel = index;
        let key = format!("row-{}", screen.items[index].id);
        if index == 0 {
            tx.scroll_to_y("list", 0.0);
        } else {
            tx.scroll_into_view(key);
        }
    }

    fn step(&mut self, delta: i32, tx: &mut Tx) {
        let screen = self.top();
        let n = screen.items.len() as i32;
        if n == 0 {
            return;
        }
        let next = (screen.sel as i32 + delta).clamp(0, n - 1);
        // A held key repeats faster than a scroll can ease: follow it without animating, so the
        // list keeps up instead of smearing behind the selection.
        let held = self.clock - self.last_step < 0.12;
        self.last_step = self.clock;
        if held {
            tx.instant();
        }
        self.select(next as usize, tx);
    }

    /// Moves through a grid by its sections' rows, so a short last row doesn't trap the selection.
    fn step_grid(&mut self, dx: i32, dy: i32, columns: usize, tx: &mut Tx) {
        if dx != 0 {
            return self.step(dx, tx);
        }
        let screen = self.top();
        // Each section restarts its columns; find the selection's section and column.
        let mut sections: Vec<(usize, usize)> = Vec::new();
        let mut start = None;
        for row in &screen.rows {
            match row {
                Row::Header { .. } => {
                    if let Some((s, e)) = start.take() {
                        sections.push((s, e));
                    }
                }
                Row::Item(i) => start = Some(match start {
                    Some((s, _)) => (s, *i + 1),
                    None => (*i, *i + 1),
                }),
            }
        }
        if let Some(se) = start {
            sections.push(se);
        }
        let Some(at) = sections.iter().position(|(s, e)| (*s..*e).contains(&screen.sel)) else { return };
        let (s, e) = sections[at];
        let col = (screen.sel - s) % columns;
        let target = if dy > 0 {
            if screen.sel + columns < e {
                screen.sel + columns
            } else if (screen.sel - s) / columns < (e - s - 1) / columns {
                e - 1
            } else if let Some((ns, ne)) = sections.get(at + 1) {
                (ns + col).min(ne - 1)
            } else {
                screen.sel
            }
        } else if screen.sel >= s + columns {
            screen.sel - columns
        } else if at > 0 {
            let (ps, pe) = sections[at - 1];
            let last_row = (pe - ps - 1) / columns * columns + ps;
            (last_row + col).min(pe - 1)
        } else {
            screen.sel
        };
        self.select(target, tx);
    }

    pub fn open_actions(&mut self) {
        let Some(item) = self.top().selected() else { return };
        if item.actions.is_empty() {
            return;
        }
        let mut query = TextEdit::new("");
        query.focused = true;
        self.menu = Some(Menu { kind: MenuKind::Actions, query, sel: 0, entries: item.actions.clone() });
    }

    pub fn open_main_menu(&mut self) {
        let mut query = TextEdit::new("");
        query.focused = true;
        let entries = vec![
            Action::new("Settings…", &icons::GEAR, Act::OpenSettings(String::new())).keys("ctrl+,"),
            Action::new("AI Chat", &icons::AI, Act::Ai(AiOp::Open(String::new()))),
            Action::new("Extension Store", &icons::STORE, Act::Push(Route::Store)),
            Action::new("Manage Extensions", &icons::PUZZLE, Act::Push(Route::Extensions)),
            Action::new("Reload Applications", &icons::REFRESH, Act::Toast("reload".into())),
            Action::new("Keyboard Shortcuts", &icons::KEYBOARD, Act::OpenSettings("shortcuts".into())).section(""),
            Action::new("About EchoSpot", &icons::LOGO, Act::OpenSettings("about".into())),
            Action::new("Quit EchoSpot", &icons::POWER, Act::Quit).keys("ctrl+q"),
        ];
        self.menu = Some(Menu { kind: MenuKind::Main, query, sel: 0, entries });
    }

    pub fn open_filter_menu(&mut self) {
        let screen = self.top();
        if screen.filters.is_empty() {
            return;
        }
        let mut query = TextEdit::new("");
        query.focused = true;
        let entries = screen.filters.iter().map(|f| Action::new(f.clone(), &icons::FILTER, Act::None)).collect();
        self.menu = Some(Menu { kind: MenuKind::Filter, query, sel: screen.filter, entries });
    }

    pub fn close_menu(&mut self) {
        if self.menu.take().is_some() {
            self.top_mut().query.focused = true;
            if let Some(form) = &mut self.form {
                form.refocus();
            }
        }
    }

    pub fn run_menu(&mut self, index: usize, tx: &mut Tx) {
        let Some(menu) = self.menu.take() else { return };
        self.top_mut().query.focused = true;
        match menu.kind {
            MenuKind::Filter => {
                self.top_mut().filter = index;
                match self.top().route {
                    Route::Ext(id) => crate::ext::filter_changed(self, id, index),
                    Route::Ai => {
                        // Picking another provider's model reorders the list: its own come first.
                        features::ai::set_model(self, index);
                        self.top_mut().filter = features::ai::model_filter(&self.settings.ai).1;
                    }
                    _ => {}
                }
                self.refresh();
            }
            _ => {
                if let Some(action) = menu.entries.get(index) {
                    let act = action.act.clone();
                    self.perform(act, tx);
                }
            }
        }
    }

    /// Return on the selected row.
    pub fn run_primary(&mut self, tx: &mut Tx) {
        let Some(act) = self.top().selected().and_then(|i| i.actions.first()).map(|a| a.act.clone()) else { return };
        self.perform(act, tx);
    }

    fn paste_keys(&self) -> &'static str {
        let class = self.prev_window.as_ref().map(|w| w.class.to_lowercase()).unwrap_or_default();
        let terminal = ["alacritty", "kitty", "ghostty", "foot", "wezterm", "konsole", "terminal", "tmux", "xterm"]
            .iter().any(|t| class.contains(t));
        if terminal { "wtype -M ctrl -M shift v -m shift -m ctrl" } else { "wtype -M ctrl v -m ctrl" }
    }

    /// The name on a paste action: "Paste to Ghostty".
    pub fn paste_target(&self) -> String {
        let Some(win) = &self.prev_window else { return "Paste".into() };
        let class = win.class.to_lowercase();
        let name = self.apps.iter()
            .find(|a| a.wm_class.to_lowercase() == class || a.id.trim_end_matches(".desktop").to_lowercase() == class)
            .map(|a| a.name.clone())
            .unwrap_or_else(|| {
                let short = win.class.rsplit('.').next().unwrap_or(&win.class);
                let mut c = short.chars();
                c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
            });
        if name.is_empty() { "Paste".into() } else { format!("Paste to {name}") }
    }

    fn copy_then_paste(&mut self, tx: &mut Tx) {
        let keys = self.paste_keys();
        self.hide(tx);
        if util::has_program("wtype") {
            util::sh_detached(&format!("sleep 0.18; {keys}"));
        }
    }

    pub fn perform(&mut self, act: Act, tx: &mut Tx) {
        match act {
            Act::None => {}
            Act::Launch(path) => {
                if let Some(app) = self.apps.iter().find(|a| a.path == path).cloned() {
                    if let Err(why) = features::apps::launch(&app) {
                        // Stay up, so the reason is read where the row was picked.
                        return self.toast(why, Tone::Danger);
                    }
                    self.usage.record(&format!("app:{}", app.id));
                }
                self.hide(tx);
            }
            Act::Spawn(argv) => {
                util::spawn_detached(&argv);
                self.hide(tx);
            }
            Act::Shell(cmd) => {
                util::sh_detached(&cmd);
                self.hide(tx);
            }
            Act::ShellQuiet { cmd, done } => {
                let ok = util::sh_ok(&cmd);
                self.toast(if ok { done } else { "Command Failed".into() }, if ok { Tone::Success } else { Tone::Danger });
                self.refresh();
            }
            Act::OpenUrl(url) => {
                util::open_url(&url);
                self.hide(tx);
            }
            Act::OpenPath(path) => {
                util::spawn_detached(&["xdg-open".into(), path.display().to_string()]);
                self.hide(tx);
            }
            Act::Reveal(path) => {
                let uri = format!("file://{}", path.display());
                let shown = util::has_program("dbus-send") && util::sh_ok(&format!(
                    "dbus-send --session --print-reply --dest=org.freedesktop.FileManager1 /org/freedesktop/FileManager1 org.freedesktop.FileManager1.ShowItems array:string:{} string:''",
                    util::quote(&uri)));
                if !shown {
                    let dir = path.parent().unwrap_or(&path).display().to_string();
                    util::spawn_detached(&["xdg-open".into(), dir]);
                }
                self.hide(tx);
            }
            Act::Copy(text) => {
                features::clipboard::copy_text(&text);
                self.hide(tx);
                crate::hud::show("Copied to Clipboard", Tone::Success);
            }
            Act::Paste(text) => {
                features::clipboard::copy_text(&text);
                self.copy_then_paste(tx);
            }
            Act::CopyFile(path) => {
                features::clipboard::copy_file(&path);
                self.hide(tx);
                crate::hud::show("Copied to Clipboard", Tone::Success);
            }
            Act::PasteFile(path) => {
                features::clipboard::copy_file(&path);
                self.copy_then_paste(tx);
            }
            Act::Push(route) => {
                if let Some(id) = features::root::command_id(&route).or(features::snap::command_id(&route)) {
                    self.usage.record(id);
                }
                self.push(route, tx);
            }
            Act::PushQuery(route, query) => {
                self.push(route, tx);
                self.top_mut().query.replace_text(&query);
                self.refresh();
            }
            Act::Pop => self.pop(tx),
            Act::Close => self.hide(tx),
            Act::System(action) => {
                let cmd = system::command(action);
                self.usage.record(&format!("sys:{:?}", action));
                if system::closes_first(action) {
                    self.hide(tx);
                    system::run(action);
                } else {
                    let out = system::run(action);
                    if let Some(message) = out.message {
                        self.toast(message, if out.ok { Tone::Success } else { Tone::Neutral });
                    }
                    let _ = cmd;
                    self.read_status();
                }
            }
            Act::Window(op) => {
                self.usage.record(&format!("win:{op:?}"));
                let target = self.prev_window.clone();
                self.hide(tx);
                if let Some(win) = target {
                    windows::run(op, &win.address);
                }
            }
            Act::FocusWindow(address) => {
                self.hide(tx);
                windows::focus(&address);
            }
            Act::CloseWindow(address) => {
                windows::close(&address);
                self.running = windows::clients().into_iter().map(|c| c.class.to_lowercase()).collect();
                self.refresh();
            }
            Act::Clip(op, id) => features::clipboard::perform(self, op, id, tx),
            Act::Favorite(id) => {
                let now = self.usage.toggle_favorite(&id);
                self.toast(if now { "Added to Favorites" } else { "Removed from Favorites" }, Tone::Success);
                self.refresh_keeping();
            }
            Act::Hide(id) => {
                self.usage.hidden.push(id);
                self.usage.save();
                self.refresh_keeping();
            }
            Act::Emoji(emoji, op) => features::emoji::perform(self, &emoji, op, tx),
            Act::Snippet(op) => features::snippets::perform(self, op, tx),
            Act::File(op) => features::files::perform(self, op, tx),
            Act::Snap(op) => features::snap::perform(self, op, tx),
            Act::Quicklink(op) => features::quicklinks::perform(self, op, tx),
            Act::Confirm(confirm) => {
                self.menu = None;
                // Return confirms, except where it can't be undone: there it lands on Cancel.
                self.dialog_sel = if confirm.destructive { 0 } else { 1 };
                self.dialog = Some(*confirm);
            }
            Act::Ext(act) => crate::ext::perform(self, act, tx),
            Act::Ai(op) => features::ai::perform(self, op, tx),
            Act::Backup(op) => features::backup::perform(self, op, tx),
            Act::Calendar(op) => features::calendar::perform(self, op, tx),
            Act::Note(op) => features::notes::perform(self, op, tx),
            Act::Script(op) => features::scripts::perform(self, op, tx),
            Act::Binding(id) => {
                self.hide(tx);
                crate::settings_ui::open_command(&id);
            }
            Act::OpenSettings(pane) => {
                self.hide(tx);
                crate::settings_ui::open(&pane);
            }
            Act::Toast(text) => {
                if text == "reload" {
                    self.apps = features::apps::scan();
                    self.refresh();
                    self.toast("Applications Reloaded", Tone::Success);
                } else {
                    self.toast(text, Tone::Neutral);
                }
            }
            Act::Quit => tx.quit(),
        }
    }

    /// Runs a launcher row's primary action by its id, as a hotkey or `echospot run` asks.
    /// Returns whether the palette has something to show for it.
    pub fn run_id(&mut self, id: &str, tx: &mut Tx) -> bool {
        self.reset();
        self.will_show();
        let Some(act) = features::root::primary(self, id) else {
            self.visible = false;
            crate::hud::show("No Such Command", Tone::Danger);
            return false;
        };
        self.perform(act, tx);
        let showing = self.visible && (self.screens.len() > 1 || self.dialog.is_some() || self.form.is_some());
        if !showing {
            if let Some(toast) = self.toast.take() {
                crate::hud::show(&toast.text, toast.tone);
            }
            self.visible = false;
        }
        showing
    }

    /// A refresh that stays on the row it was on.
    pub fn refresh_keeping(&mut self) {
        let keep = self.top().selected().map(|i| i.id.clone());
        self.refresh();
        if let Some(id) = keep {
            let screen = self.top_mut();
            if let Some(at) = screen.items.iter().position(|i| i.id == id) {
                screen.sel = at;
            }
        }
    }

    pub fn query_changed(&mut self, tx: &mut Tx) {
        if let Route::Ext(id) = self.top().route {
            let text = self.top().query.text.clone();
            crate::ext::search_changed(self, id, &text);
        }
        self.refresh();
        tx.scroll_to_instant("list", 0.0, 0.0);
    }

    /// Closes the dialog with its answer: runs its action, or replies to the extension that asked.
    pub fn answer_dialog(&mut self, confirmed: bool, tx: &mut Tx) {
        let Some(dialog) = self.dialog.take() else { return };
        if self.dialog_reply.is_some() {
            crate::ext::reply_confirm(self, confirmed);
        } else if confirmed {
            self.perform(dialog.act, tx);
        }
    }

    fn dialog_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        match ev.key {
            Key::Escape => self.answer_dialog(false, tx),
            Key::Left | Key::Right | Key::Tab => self.dialog_sel = 1 - self.dialog_sel,
            Key::Enter => self.answer_dialog(self.dialog_sel == 1, tx),
            _ => {}
        }
        true
    }

    fn menu_key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        let Some(menu) = &mut self.menu else { return false };
        let visible: Vec<usize> = menu.visible().iter().map(|(i, _)| *i).collect();
        let pos = visible.iter().position(|i| *i == menu.sel).unwrap_or(0);
        match &ev.key {
            Key::Escape => self.close_menu(),
            Key::Command('k') => self.close_menu(),
            Key::Down | Key::Tab if !visible.is_empty() => {
                menu.sel = visible[(pos + 1) % visible.len()];
                tx.scroll_into_view(format!("menu-row-{}", menu.sel));
            }
            Key::Up if !visible.is_empty() => {
                menu.sel = visible[(pos + visible.len() - 1) % visible.len()];
                tx.scroll_into_view(format!("menu-row-{}", menu.sel));
            }
            Key::Enter => {
                if let Some(&index) = visible.get(pos) {
                    self.run_menu(index, tx);
                }
            }
            _ => {
                // A row's own chord still runs while its menu is open.
                if menu.kind == MenuKind::Actions
                    && let Some(index) = menu.entries.iter().position(|a| chord_matches(&a.keys, ev)) {
                    self.run_menu(index, tx);
                    return true;
                }
                return false;
            }
        }
        true
    }

    /// Keys in the search field, before the field's own editor sees them.
    pub fn key(&mut self, ev: &KeyEvent, tx: &mut Tx) -> bool {
        if self.dialog.is_some() {
            return self.dialog_key(ev, tx);
        }
        if self.menu.is_some() {
            return self.menu_key(ev, tx);
        }
        if self.form.is_some() {
            return features::forms::key(self, ev, tx);
        }
        if let Route::Snap(only) = self.top().route
            && features::snap::key(self, only, ev, tx) {
            return true;
        }
        let layout = self.top().layout;
        let empty_query = self.top().query.text.is_empty();
        let m = ev.mods;
        let plain = !m.cmd && !m.alt && !m.shift;
        match &ev.key {
            Key::Down if plain => match layout {
                Layout::Grid { columns } => self.step_grid(0, 1, columns, tx),
                _ => self.step(1, tx),
            },
            Key::Up if plain => match layout {
                Layout::Grid { columns } => self.step_grid(0, -1, columns, tx),
                _ => self.step(-1, tx),
            },
            Key::Command('n') => self.step(1, tx),
            Key::Command('p') => self.step(-1, tx),
            Key::Right if plain && empty_query && matches!(layout, Layout::Grid { .. }) => self.step(1, tx),
            Key::Left if plain && empty_query && matches!(layout, Layout::Grid { .. }) => self.step(-1, tx),
            Key::PageDown => self.step(8, tx),
            Key::PageUp => self.step(-8, tx),
            Key::Escape if self.top().route == Route::Ai && self.chat.streaming => {
                self.chat.cancel();
                self.refresh();
            }
            Key::Escape => {
                if !empty_query {
                    self.top_mut().query.clear();
                    self.query_changed(tx);
                } else if self.screens.len() > 1 {
                    self.pop(tx);
                } else {
                    self.hide(tx);
                }
            }
            Key::Backspace if empty_query && plain && self.screens.len() > 1 => self.pop(tx),
            Key::Command('k') if !m.shift => self.open_actions(),
            Key::Command(',') => self.perform(Act::OpenSettings(String::new()), tx),
            Key::Command('q') => tx.quit(),
            Key::Command('w') => self.hide(tx),
            Key::Tab if plain && !self.top().filters.is_empty() => self.open_filter_menu(),
            Key::Tab if plain && self.top().route == Route::Root => {
                let prompt = self.top().query.text.clone();
                self.perform(Act::Ai(AiOp::Open(prompt)), tx);
            }
            Key::Enter if plain => self.run_primary(tx),
            _ => {
                let act = self.top().selected()
                    .and_then(|item| item.actions.iter().find(|a| chord_matches(&a.keys, ev)))
                    .map(|a| a.act.clone());
                match act {
                    Some(act) => self.perform(act, tx),
                    None => return false,
                }
            }
        }
        true
    }
}

/// What a screen's builder hands back.
pub struct Built {
    pub items: Vec<Item>,
    pub rows: Vec<Row>,
    pub layout: Layout,
    pub placeholder: String,
    pub filters: Vec<String>,
    pub loading: bool,
    pub empty: (String, String),
    pub keep_selection: bool,
}

impl Built {
    pub fn list(sections: Sections, placeholder: &str) -> Self {
        let (items, rows) = sections.flatten(false);
        Self {
            items, rows, layout: Layout::List, placeholder: placeholder.into(), filters: Vec::new(),
            loading: false, empty: ("No Results".into(), String::new()), keep_selection: false,
        }
    }
    pub fn layout(mut self, layout: Layout) -> Self {
        self.layout = layout;
        self
    }
    pub fn filters(mut self, filters: &[&str]) -> Self {
        self.filters = filters.iter().map(|s| s.to_string()).collect();
        self
    }
    pub fn empty(mut self, title: &str, hint: &str) -> Self {
        self.empty = (title.into(), hint.into());
        self
    }
    pub fn loading(mut self, on: bool) -> Self {
        self.loading = on;
        self
    }
    pub fn keep(mut self) -> Self {
        self.keep_selection = true;
        self
    }
}

impl crate::glass::Liquid for App {
    fn pane(&self) -> Option<crate::glass::Pane<'_>> {
        self.settings.liquid().then(|| crate::glass::Pane {
            window: (crate::theme::size::WINDOW_W, crate::theme::size::WINDOW_H),
            dark: self.system_dark.get(), level: self.settings.liquid,
            shot: &self.glass,
        })
    }
}
