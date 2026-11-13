//! The Settings window, laid out as macOS lays out its own under Liquid Glass: the sidebar is a
//! pane of glass floating inside the panel, the pane's title and the window's two controls sit in
//! a toolbar the content scrolls beneath, and everything else is grouped rows on quiet cards.
//! Colour is kept for what means something: the sidebar's tiles, a status dot, and one accent
//! (the system blue) for whatever is switched on or chosen. Every control writes `settings.json`
//! at once and tells the palette, so nothing waits for Save.
//!
//! The Commands pane is the one table: every launcher row with its alias and its hotkey. An
//! alias is typed where it shows; a hotkey is recorded by pressing it.

use std::sync::OnceLock;

use rune::{AppHandle, Color, Cx, El, Key, KeyEvent, Mods, ScrollFade, ScrollSpec, TextEdit, TextStyle, Tx, WindowHandle, col, el, icon, row, spacer, text, text_input};

use crate::app::App;
use crate::features::ai;
use crate::features::bindings::{self, Bindings};
use crate::features::root::Command;
use crate::icons;
use crate::settings::Settings;
use crate::theme::{self, Palette, Tint, motion, radius, size, space, type_};
use crate::ui;

pub const TITLE: &str = "EchoSpot Settings";
/// The panel, and the window around it: the palette's margin, where the shadow falls.
pub const PANEL: (f32, f32) = (800.0, 560.0);
pub const SIZE: (f32, f32) = (PANEL.0 + size::WINDOW_MARGIN * 2.0, PANEL.1 + size::WINDOW_MARGIN * 2.0);
const FULL: Option<f32> = Some(0.0);
/// The gap between the panel's edge and the glass sidebar floating in it. The sidebar's corner
/// is the panel's less this gap, so the two curves stay concentric.
const SIDEBAR_GAP: f32 = 8.0;
const SIDEBAR_W: f32 = 204.0;
const SIDEBAR_R: f32 = radius::PANEL - SIDEBAR_GAP;
const TOOLBAR_H: f32 = 56.0;
const TOOL_BUTTON: f32 = 30.0;
/// A card's horizontal inset, and its corner.
const INSET: f32 = 14.0;
const CARD_R: f32 = 14.0;
const CONTROL_H: f32 = 26.0;
/// The Commands table: a row's height, and the widths of its alias and hotkey columns.
const COMMAND_ROW_H: f32 = 44.0;
const ALIAS_W: f32 = 108.0;
const HOTKEY_W: f32 = 168.0;

const PANE_TITLE: TextStyle = TextStyle::new(15.0, 650.0).lh(20.0);
const GROUP_TITLE: TextStyle = TextStyle::new(13.0, 600.0).lh(18.0);
const NOTE: TextStyle = TextStyle::new(11.5, 400.0).lh(15.0);
const CONTROL: TextStyle = TextStyle::new(12.0, 500.0).lh(16.0);

#[derive(Clone, Copy, PartialEq)]
pub enum Pane {
    General,
    Commands,
    Ai,
    Clipboard,
    Files,
    Extensions,
    Shortcuts,
    About,
}

struct PaneInfo {
    pane: Pane,
    title: &'static str,
    glyph: &'static rune::Icon,
    tint: Tint,
}

const PANES: [PaneInfo; 8] = [
    PaneInfo { pane: Pane::General, title: "General", glyph: &icons::GEAR, tint: Tint::Graphite },
    PaneInfo { pane: Pane::Commands, title: "Commands", glyph: &icons::BOLT, tint: Tint::Pink },
    PaneInfo { pane: Pane::Ai, title: "AI", glyph: &icons::AI, tint: Tint::Violet },
    PaneInfo { pane: Pane::Clipboard, title: "Clipboard", glyph: &icons::CLIPBOARD, tint: Tint::Orange },
    PaneInfo { pane: Pane::Files, title: "File Search", glyph: &icons::FILE_SEARCH, tint: Tint::Blue },
    PaneInfo { pane: Pane::Extensions, title: "Extensions", glyph: &icons::PUZZLE, tint: Tint::Green },
    PaneInfo { pane: Pane::Shortcuts, title: "Shortcuts", glyph: &icons::KEYBOARD, tint: Tint::Blue },
    PaneInfo { pane: Pane::About, title: "About", glyph: &icons::LOGO, tint: Tint::Indigo },
];

#[derive(Clone, PartialEq)]
enum Probe {
    Idle,
    Running,
    Ok(String),
    Failed(String),
}

struct ExtRow {
    id: String,
    title: String,
    author: String,
    commands: usize,
    icon: Option<std::path::PathBuf>,
}

pub struct SettingsUi {
    pub pane: Pane,
    pub settings: Settings,
    pub model: TextEdit,
    pub base_url: TextEdit,
    pub api_key: TextEdit,
    pub system_prompt: TextEdit,
    extensions: Vec<ExtRow>,
    /// The models the provider in use said it has, and whose list it is.
    fetched: (String, Vec<String>),
    /// Counts key edits, so only the last of a burst is checked.
    check: u64,
    /// Clipboard entries, how many are images, and bytes on disk.
    clipboard: (usize, usize, u64),
    chats: usize,
    /// The drives mounted as the window opened.
    drives: Vec<std::path::PathBuf>,
    /// The folder being typed for exclusion, and why the last one typed was not taken.
    pub exclude: TextEdit,
    exclude_error: String,
    probe: Probe,
    icons: crate::store::IconStore,
    /// The screen behind the panel, captured as the window opens.
    backdrop: rune::Pixels,
    has_backdrop: bool,
    /// The same capture, whole, for the Liquid theme's lens.
    glass: crate::glass::Shot,
    system_dark: std::cell::Cell<bool>,
    /// Every launcher row as the palette last listed them, and the ones the search leaves.
    commands: Vec<Command>,
    shown: Vec<usize>,
    /// The aliases and hotkeys on file. Settings writes them; the palette is handed each change.
    bindings: Bindings,
    pub filter: TextEdit,
    assigned_only: bool,
    selected: Option<String>,
    /// A row to select and scroll to once the palette's list arrives.
    reveal: Option<String>,
    /// The alias being typed, whose row it is, and what it was before, for Escape.
    pub alias: TextEdit,
    alias_for: Option<String>,
    alias_before: String,
    /// The row whose hotkey is being recorded, the modifiers held so far, and why the last
    /// chord pressed was not taken.
    recording: Option<String>,
    held: Mods,
    record_error: String,
}

fn row_key(id: &str) -> String {
    format!("command-{id}")
}

fn dir_size(dir: &std::path::Path) -> u64 {
    std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| {
        let path = e.path();
        if path.is_dir() { dir_size(&path) } else { e.metadata().map(|m| m.len()).unwrap_or(0) }
    }).sum()
}

impl SettingsUi {
    pub fn new() -> Self {
        let mut ui = Self {
            pane: Pane::General, settings: Settings::load(), model: TextEdit::new(""), base_url: TextEdit::new(""),
            api_key: TextEdit::new("").secure(), system_prompt: TextEdit::new(""), extensions: Vec::new(), fetched: Default::default(), check: 0, clipboard: (0, 0, 0), chats: 0, drives: Vec::new(), exclude: TextEdit::new(""), exclude_error: String::new(),
            probe: Probe::Idle, icons: Default::default(), backdrop: rune::Pixels::new(), has_backdrop: false, glass: Default::default(), system_dark: std::cell::Cell::new(true),
            commands: Vec::new(), shown: Vec::new(), bindings: Bindings::default(), filter: TextEdit::new(""), assigned_only: false,
            selected: None, reveal: None, alias: TextEdit::new(""), alias_for: None, alias_before: String::new(),
            recording: None, held: Mods::default(), record_error: String::new(),
        };
        ui.reload();
        ui
    }

    /// Reads the file again, as the window opens.
    pub fn reload(&mut self) {
        self.settings = Settings::load();
        self.model.replace_text(self.settings.ai.model.clone());
        self.base_url.replace_text(self.settings.ai.base_url.clone());
        self.api_key.replace_text(self.settings.ai.keys.get(&self.settings.ai.provider).cloned().unwrap_or_default());
        self.system_prompt.replace_text(self.settings.ai.system_prompt.clone());
        self.extensions = crate::ext::Extensions::load().installed.iter().map(|e| ExtRow {
            id: e.id.clone(), title: e.title.clone(), author: e.author.clone(), commands: e.commands.len(), icon: e.icon.clone(),
        }).collect();
        let entries = crate::features::clipboard::Clipboard::load().entries;
        let images = entries.iter().filter(|e| e.kind == crate::features::clipboard::Kind::Image).count();
        self.clipboard = (entries.len(), images, dir_size(&crate::util::data_dir().join("clipboard")));
        self.chats = ai::threads().len();
        self.drives = crate::features::files::mounted_drives();
        self.exclude.replace_text("");
        self.exclude_error.clear();
        self.probe = Probe::Idle;
        self.fetched = Default::default();
        self.check += 1;
        self.stop_recording();
        self.end_alias(false);
        self.bindings = Bindings::load();
        self.filter.replace_text("");
        self.assigned_only = false;
        self.selected = None;
        self.refilter();
    }

    /// Takes the patch of screen the window is about to cover, as the palette does when summoned.
    fn capture(&mut self) {
        // Taken whatever the theme is: the live backdrop then feeds whichever is chosen.
        self.has_backdrop = false;
        self.glass.clear();
        let shot = crate::backdrop::centered_origin(SIZE.0, SIZE.1)
            .and_then(|(x, y)| crate::backdrop::capture_rect(x + size::WINDOW_MARGIN, y + size::WINDOW_MARGIN, PANEL.0, PANEL.1));
        if let Some((w, h, rgba)) = shot {
            self.set_backdrop(w, h, rgba);
        }
        crate::live::start_settings();
        crate::live::own_shadows_off();
    }

    /// A picture of the screen behind the panel, for Blur and for Liquid alike.
    pub fn set_backdrop(&mut self, w: u32, h: u32, rgba: Vec<u8>) {
        // Both materials are kept: a still screen sends no second picture after a theme change.
        self.glass.set(w, h, rgba.clone());
        self.backdrop.set(w, h, rgba);
        self.has_backdrop = true;
    }

    fn editing(&self) -> bool {
        self.model.focused || self.base_url.focused || self.api_key.focused || self.system_prompt.focused || self.filter.focused || self.alias.focused || self.exclude.focused
    }

    fn step_pane(&mut self, delta: i32) {
        let at = PANES.iter().position(|i| i.pane == self.pane).unwrap_or(0) as i32;
        self.show(PANES[(at + delta).clamp(0, PANES.len() as i32 - 1) as usize].pane);
    }

    /// Turns to a pane. The AI pane asks its provider whether it is there the first time it shows.
    fn show(&mut self, pane: Pane) {
        self.stop_recording();
        self.end_alias(false);
        self.pane = pane;
        // The search has the keyboard, as it does in the palette, unless a row was asked for.
        self.filter.focused = pane == Pane::Commands && self.reveal.is_none();
        if pane == Pane::Commands {
            self.load_commands();
        }
        if pane == Pane::Ai && self.probe == Probe::Idle && self.settings.ai.ready() {
            self.test_connection();
        }
    }

    /// Makes a provider the one in use: its key into the field, its last model back.
    fn choose_provider(&mut self, id: &str) {
        if self.settings.ai.provider == id {
            return;
        }
        self.settings.ai.model = self.model.text.trim().to_string();
        self.settings.ai.set_provider(id);
        self.model.replace_text(self.settings.ai.model.clone());
        self.api_key.replace_text(self.settings.ai.keys.get(id).cloned().unwrap_or_default());
        self.commit();
        self.recheck(0);
    }

    fn choose_model(&mut self, id: &str) {
        self.settings.ai.use_model(id);
        self.model.replace_text(id);
        self.commit();
    }

    /// The key or the endpoint changed: forget what the old one answered and, once the typing
    /// or the paste has settled, ask again.
    fn recheck(&mut self, delay_ms: u64) {
        self.check += 1;
        self.probe = Probe::Idle;
        self.fetched = Default::default();
        if !self.settings.ai.ready() {
            return;
        }
        if delay_ms == 0 {
            return self.test_connection();
        }
        let stamp = self.check;
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(delay_ms));
            if let Some((handle, _)) = WINDOW.get() {
                handle.update(move |ui, _| if ui.check == stamp { ui.test_connection() });
            }
        });
    }

    /// Saves and hands the palette the new settings.
    fn commit(&mut self) {
        let ai = &mut self.settings.ai;
        ai.model = self.model.text.trim().to_string();
        ai.base_url = self.base_url.text.trim().to_string();
        match self.api_key.text.trim() {
            "" => { ai.keys.remove(&ai.provider); }
            key => { ai.keys.insert(ai.provider.clone(), key.to_string()); }
        }
        ai.system_prompt = self.system_prompt.text.clone();
        self.settings.save();
        if let Some(main) = MAIN.get() {
            let settings = self.settings.clone();
            main.update(move |app: &mut App, _| {
                crate::features::expander::set_enabled(settings.snippet_expansion);
                app.settings = settings;
                app.refresh();
            });
        }
    }

    // ---- File Search ----

    /// Excludes the folder typed in the File Search pane, or says why it cannot.
    fn exclude_typed(&mut self) {
        let home = crate::util::home();
        let Some(path) = crate::features::files::typed_folder(&self.exclude.text, &home) else {
            self.exclude_error = if self.exclude.text.trim().is_empty() { String::new() } else { "Type the whole path, such as ~/Documents/Private or /media/drive/Backups.".into() };
            return;
        };
        let known = |f: &String| crate::features::files::typed_folder(f, &home).as_ref() == Some(&path);
        self.exclude_error = if !path.is_dir() {
            "There is no folder at that path.".into()
        } else if self.settings.files.excluded.iter().any(known) {
            "That folder is already excluded.".into()
        } else {
            self.settings.files.excluded.push(crate::util::tilde(&path));
            self.exclude.replace_text("");
            self.commit();
            String::new()
        };
    }

    // ---- Commands ----

    /// Asks the palette for its rows: only it knows the apps, extensions and scripts there are.
    fn load_commands(&self) {
        let Some(main) = MAIN.get() else { return };
        main.update(|app: &mut App, _| {
            let list = crate::features::root::catalog(app);
            if let Some((handle, _)) = WINDOW.get() {
                handle.update(move |ui, tx| ui.take_commands(list, tx));
            }
        });
    }

    fn take_commands(&mut self, list: Vec<Command>, tx: &mut Tx) {
        self.commands = list;
        if let Some(id) = self.reveal.take().filter(|id| self.commands.iter().any(|c| &c.id == id)) {
            self.filter.replace_text("");
            self.assigned_only = false;
            self.selected = Some(id.clone());
            tx.scroll_into_view(row_key(&id));
        }
        self.refilter();
    }

    /// Works out which rows the search and the scope leave. A row stays listed while its alias
    /// or hotkey is edited, even once it no longer matches: it must not leave under the hand.
    fn refilter(&mut self) {
        let needle = self.filter.text.to_lowercase();
        let terms: Vec<&str> = needle.split_whitespace().collect();
        self.shown = self.commands.iter().enumerate().filter(|(_, c)| {
            let (alias, chord) = (self.bindings.aliases.get(&c.id), self.bindings.hotkeys.get(&c.id));
            if self.assigned_only && alias.is_none() && chord.is_none() {
                return false;
            }
            if terms.is_empty() {
                return true;
            }
            let hay = format!("{} {} {} {}", c.title, c.kind, alias.map(String::as_str).unwrap_or(""), chord.map(String::as_str).unwrap_or("")).to_lowercase();
            terms.iter().all(|t| hay.contains(t))
        }).map(|(i, _)| i).collect();
        if !self.selected.as_ref().is_some_and(|id| self.shown.iter().any(|&i| &self.commands[i].id == id)) {
            self.selected = None;
        }
    }

    fn title_of(&self, id: &str) -> String {
        self.commands.iter().find(|c| c.id == id).map(|c| c.title.clone()).unwrap_or_else(|| id.to_string())
    }

    fn select(&mut self, id: &str) {
        if self.recording.as_deref() != Some(id) {
            self.stop_recording();
        }
        if self.alias_for.as_deref() != Some(id) {
            self.end_alias(false);
        }
        self.selected = Some(id.to_string());
    }

    fn step_row(&mut self, delta: i32, tx: &mut Tx) {
        if self.shown.is_empty() {
            return;
        }
        let at = self.selected.as_ref().and_then(|id| self.shown.iter().position(|&i| &self.commands[i].id == id));
        let next = match at {
            Some(at) => (at as i32 + delta).clamp(0, self.shown.len() as i32 - 1) as usize,
            None => if delta > 0 { 0 } else { self.shown.len() - 1 },
        };
        let id = self.commands[self.shown[next]].id.clone();
        self.select(&id);
        tx.scroll_into_view(row_key(&id));
    }

    /// Hands the palette the aliases and hotkeys as they now stand; `rebind` has it register
    /// the hotkeys with Hyprland again.
    fn push_bindings(&self, rebind: bool) {
        let (aliases, hotkeys) = (self.bindings.aliases.clone(), self.bindings.hotkeys.clone());
        tell_main(move |app| {
            app.bindings.aliases = aliases;
            app.bindings.hotkeys = hotkeys;
            if rebind {
                app.bindings.apply();
            }
        });
    }

    fn alias_editing(&self) -> bool {
        self.alias_for.is_some() && self.alias.focused
    }

    fn begin_alias(&mut self, id: &str) {
        self.stop_recording();
        self.select(id);
        self.alias_before = self.bindings.aliases.get(id).cloned().unwrap_or_default();
        self.alias.replace_text(self.alias_before.clone());
        self.alias_for = Some(id.to_string());
        self.alias.focused = true;
        self.filter.focused = false;
    }

    /// The alias field changed: it is the row's alias from this keystroke on.
    fn alias_changed(&mut self) {
        let Some(id) = self.alias_for.clone() else { return };
        self.bindings.set_alias(&id, &self.alias.text);
        self.push_bindings(false);
    }

    /// Leaves the alias field, with what was typed or, for Escape, with what was there before.
    fn end_alias(&mut self, restore: bool) {
        let Some(id) = self.alias_for.take() else { return };
        self.alias.focused = false;
        if restore && self.bindings.aliases.get(&id).map(String::as_str).unwrap_or("") != self.alias_before {
            self.bindings.set_alias(&id, &self.alias_before.clone());
            self.push_bindings(false);
        }
    }

    /// Starts listening for a row's hotkey. The palette lets go of every hotkey meanwhile, so
    /// pressing one that is taken reaches this window instead of running its command.
    fn begin_record(&mut self, id: &str, tx: &mut Tx) {
        self.select(id);
        self.end_alias(false);
        self.filter.focused = false;
        tx.blur();
        if self.recording.is_none() {
            tell_main(|app| app.bindings.suspend());
        }
        self.recording = Some(id.to_string());
        self.held = Mods::default();
        self.record_error.clear();
    }

    /// Stops listening without a change, and has the palette take its hotkeys back.
    fn stop_recording(&mut self) {
        if self.recording.take().is_some() {
            self.record_error.clear();
            tell_main(|app| app.bindings.apply());
        }
    }

    fn set_hotkey(&mut self, id: &str, chord: &str) {
        self.bindings.set_hotkey(id, chord);
        self.recording = None;
        self.record_error.clear();
        self.push_bindings(true);
    }

    /// A key while a hotkey is being recorded. Escape keeps what was there, Backspace removes
    /// it, and anything else is the hotkey if Hyprland can bind it and no other row has it.
    fn record_key(&mut self, ev: &KeyEvent, base: Option<char>) {
        let Some(id) = self.recording.clone() else { return };
        if let Key::Modifier { .. } = ev.key {
            self.held = ev.mods;
            return;
        }
        let m = ev.mods;
        let bare = !(m.logo || m.ctrl || m.cmd || m.alt || m.shift);
        match ev.key {
            Key::Escape if bare => self.stop_recording(),
            Key::Backspace | Key::Delete if bare => self.set_hotkey(&id, ""),
            _ => {
                let Some(chord) = bindings::chord_of(ev, base) else { return };
                if bindings::hypr_chord(&chord).is_none() {
                    self.record_error = "Add Super, Ctrl or Alt".into();
                } else if let Some(other) = self.bindings.holder(&chord, &id) {
                    self.record_error = format!("Used by {}", self.title_of(other));
                } else {
                    self.set_hotkey(&id, &chord);
                }
            }
        }
    }

    /// Return in the search: record the selected row's hotkey, or the only row's.
    fn record_selected(&mut self, tx: &mut Tx) {
        let id = self.selected.clone().or_else(|| (self.shown.len() == 1).then(|| self.commands[self.shown[0]].id.clone()));
        if let Some(id) = id {
            self.begin_record(&id, tx);
        }
    }

    /// Asks the provider whether the key and endpoint answer, off the UI thread.
    fn test_connection(&mut self) {
        if self.probe == Probe::Running {
            return;
        }
        self.probe = Probe::Running;
        let settings = self.settings.ai.clone();
        let stamp = self.check;
        std::thread::spawn(move || {
            let result = ai::probe(&settings);
            if let Some((handle, _)) = WINDOW.get() {
                handle.update(move |ui, _| {
                    // An answer about a key or provider that has since changed is about nothing.
                    if ui.check != stamp || ui.settings.ai.provider != settings.provider {
                        return;
                    }
                    ui.probe = match result {
                        Ok(found) => {
                            let mut line = "Connected".to_string();
                            if !found.models.is_empty() {
                                line.push_str(&format!(" · {} models", found.models.len()));
                            }
                            if !found.note.is_empty() {
                                line.push_str(&format!(" · {}", found.note));
                            }
                            ui.fetched = (settings.provider.clone(), found.models);
                            Probe::Ok(line)
                        }
                        Err(e) => Probe::Failed(e),
                    };
                });
            }
        });
    }
}

static WINDOW: OnceLock<(AppHandle<SettingsUi>, WindowHandle)> = OnceLock::new();
static MAIN: OnceLock<AppHandle<App>> = OnceLock::new();

pub fn attach(handle: AppHandle<SettingsUi>, window: WindowHandle, main: AppHandle<App>) {
    let _ = WINDOW.set((handle, window));
    let _ = MAIN.set(main);
}

/// Gives Settings what a window is given at start: somewhere to decode icons.
pub fn setup(ui: &mut SettingsUi, waker: rune::Waker) {
    ui.icons.attach(rune::ImageCache::new(waker));
}

/// A recording ends with the window's focus: the keys are no longer its to hear.
pub fn on_focus(ui: &mut SettingsUi, focused: bool, _: &mut Tx) {
    if !focused {
        ui.stop_recording();
    }
}

/// Opens Settings on one launcher row, where its alias and hotkey are set.
pub fn open_command(id: &str) {
    let Some((handle, window)) = WINDOW.get() else { return };
    let id = id.to_string();
    handle.update(move |ui, _| {
        ui.reload();
        ui.capture();
        ui.reveal = Some(id);
        ui.show(Pane::Commands);
    });
    window.open();
}

/// Opens Settings at a pane ("ai", "extensions", …).
pub fn open(pane: &str) {
    let Some((handle, window)) = WINDOW.get() else { return };
    let pane = match pane {
        "commands" => Some(Pane::Commands),
        "ai" => Some(Pane::Ai),
        "clipboard" => Some(Pane::Clipboard),
        "files" => Some(Pane::Files),
        "extensions" => Some(Pane::Extensions),
        "shortcuts" => Some(Pane::Shortcuts),
        "about" => Some(Pane::About),
        "general" => Some(Pane::General),
        _ => None,
    };
    handle.update(move |ui, _| {
        ui.reload();
        ui.capture();
        ui.show(pane.unwrap_or(ui.pane));
    });
    window.open();
}

fn tell_main(f: impl FnOnce(&mut App) + Send + 'static) {
    if let Some(main) = MAIN.get() {
        main.update(move |app: &mut App, _| {
            f(app);
            app.refresh();
        });
    }
}

// ---- Controls ---------------------------------------------------------------------------

/// The accent: what is on, what is chosen, where the keyboard is.
fn accent(p: &Palette) -> Color {
    p.primary_action
}

/// Liquid Glass's switch: a capsule knob, wider than it is tall, riding a capsule track.
fn switch(p: &Palette, key: &str, on: bool, toggle: impl Fn(&mut Settings) + 'static) -> El {
    let (w, h, knob_w) = (40.0, 22.0, 24.0);
    el().key(key.to_string()).role("switch").checked(on).size(w, h).radius(h / 2.0).motion(motion::LAYOUT)
        .fill(if on { accent(p) } else { p.control_hover })
        .on_press(move |ui: &mut SettingsUi| {
            toggle(&mut ui.settings);
            ui.commit();
        })
        .child(el().key(format!("{key}-knob")).size(knob_w, h - 4.0).radius((h - 4.0) / 2.0).fill(Color::WHITE)
            .shadow(1.0, 4.0, Color::BLACK.alpha(0.22)).absolute(if on { w - knob_w - 2.0 } else { 2.0 }, 2.0).motion(motion::MENU))
}

/// A capsule button, as every button is under Liquid Glass; a destructive one writes in red.
fn button(p: &Palette, key: &str, label: &str, destructive: bool, press: impl Fn(&mut SettingsUi, &mut Tx) + 'static) -> El {
    row().key(key.to_string()).h(CONTROL_H).px(space::XL).center().radius(CONTROL_H / 2.0)
        .fill(p.control_surface).hover(p.control_hover).press(p.control_pressed).motion(motion::HOVER).on_press_tx(press)
        .child(text(label.to_string()).font(CONTROL).color(if destructive { p.destructive } else { p.text.alpha(0.92) }))
}

/// A button that leaves for the browser, and says so with its arrow.
fn link_button(p: &Palette, key: &str, label: &str, url: &'static str) -> El {
    row().key(key.to_string()).h(CONTROL_H).pl(space::XL).pr(space::LG).gap(space::XS).center().radius(CONTROL_H / 2.0)
        .fill(p.control_surface).hover(p.control_hover).press(p.control_pressed).motion(motion::HOVER)
        .on_press(move |_: &mut SettingsUi| crate::util::open_url(url))
        .child(text(label.to_string()).font(CONTROL).color(p.text.alpha(0.92)))
        .child(icon(&icons::ARROW_UP_RIGHT, 11.0).color(p.text_secondary))
}

/// A row of a card: a title with its quieter subtitle, and a control at the trailing edge.
fn setting(p: &Palette, title: &str, subtitle: &str, control: El) -> El {
    let mut labels = col().grow(1.0).min_w(0.0).gap(1.0).child(text(title.to_string()).font(type_::ROW_TITLE).color(p.text));
    if !subtitle.is_empty() {
        labels = labels.child(text(subtitle.to_string()).font(NOTE).color(p.text_secondary).wrap());
    }
    row().w_full().min_h(44.0).px(INSET).py(space::LG).gap(space::XL).center_y().child(labels).child(control)
}

fn hairline(p: &Palette) -> El {
    el().w_full().px(INSET).child(el().w_full().h(size::HAIRLINE).fill(p.separator.alpha(if p.dark { 0.08 } else { 0.09 })))
}

fn card_surface(p: &Palette) -> El {
    // Dark, a card is a little light on the glass; light, it is paper laid on it.
    col().w_full().radius(CARD_R).fill(if p.dark { p.card_fill } else { Color::WHITE.alpha(0.66) })
        .border(1.0, if p.dark { Color::WHITE.alpha(0.06) } else { Color::BLACK.alpha(0.06) })
}

fn card(p: &Palette, rows: Vec<El>) -> El {
    let mut body = card_surface(p);
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            body = body.child(hairline(p));
        }
        body = body.child(r);
    }
    body
}

/// A titled group: its name in the content's own ink, what it holds, and a footnote if it has one.
fn group(p: &Palette, title: &str, body: El) -> El {
    col().w_full().gap(space::MD)
        .child(text(title.to_string()).font(GROUP_TITLE).color(p.text).ml(space::XS))
        .child(body)
}

fn footnote(p: &Palette, note: impl Into<String>) -> El {
    text(note.into()).font(NOTE).color(p.text_tertiary).wrap().mx(space::XS)
}

fn noted(p: &Palette, title: &str, body: El, note: impl Into<String>) -> El {
    group(p, title, col().w_full().gap(space::MD).child(body).child(footnote(p, note)))
}

/// A text field's box: quiet until it has the keyboard, then ringed in the accent.
fn field_box(p: &Palette, key: &str, focused: bool) -> El {
    row().key(format!("{key}-box")).h(28.0).px(space::LG).center_y().radius(radius::BAR_CONTROL)
        .fill(if p.dark { Color::BLACK.alpha(0.22) } else { Color::BLACK.alpha(0.035) })
        .border(if focused { 2.0 } else { 1.0 }, if focused { accent(p).alpha(0.85) } else { p.border.alpha(0.12) })
        .motion(motion::HOVER)
}

fn field(p: &Palette, key: &str, edit: &TextEdit, placeholder: &str, access: fn(&mut SettingsUi) -> &mut TextEdit, changed: fn(&mut SettingsUi)) -> El {
    field_box(p, key, edit.focused)
        .child(text_input(edit, type_::ROW_TITLE).key(key.to_string()).single_line().grow(1.0).color(p.text)
            .placeholder(placeholder.to_string(), p.text_tertiary).no_focus_ring()
            .on_edit(access)
            .on_input_change(move |ui: &mut SettingsUi, _| changed(ui)))
}

/// The mark of a chosen row: a filled disc with its tick, or an empty ring.
fn radio(p: &Palette, on: bool) -> El {
    let mark = el().size(18.0, 18.0).radius(9.0).center().motion(motion::HOVER);
    if on { mark.fill(accent(p)).child(icon(&icons::CHECK, 11.0).color(Color::WHITE)) } else { mark.border(1.5, p.border.alpha(0.16)) }
}

fn status_dot(color: Color) -> El {
    el().size(7.0, 7.0).radius(3.5).fill(color)
}

/// One of a few pictured choices: its drawing in a frame that rings when chosen, named beneath.
fn choice(p: &Palette, key: String, on: bool, label: &str, picture: El, pick: impl Fn(&mut Settings) + 'static) -> El {
    col().key(key).gap(space::XS).items_center()
        .on_press(move |ui: &mut SettingsUi| {
            pick(&mut ui.settings);
            ui.commit();
        })
        .child(el().padding(2.5).radius(PIC_R + 4.5).border(2.0, if on { accent(p) } else { Color::TRANSPARENT }).motion(motion::HOVER).child(picture))
        .child(text(label.to_string()).font(if on { TextStyle::new(11.0, 600.0).lh(14.0) } else { type_::SECTION }).color(if on { p.text } else { p.text_secondary }))
}

/// A row that holds pictured choices: what is being chosen on the left, the pictures on the right.
fn pictured(p: &Palette, title: &str, subtitle: &str, choices: Vec<El>) -> El {
    row().w_full().px(INSET).pt(space::XL + 2.0).pb(space::LG).gap(space::XL).items_start()
        .child(col().grow(1.0).min_w(0.0).gap(1.0).pt(space::XXS)
            .child(text(title.to_string()).font(type_::ROW_TITLE).color(p.text))
            .child(text(subtitle.to_string()).font(NOTE).color(p.text_secondary).wrap()))
        .child(row().gap(space::SM).children(choices))
}

// ---- Drawings ---------------------------------------------------------------------------

const PIC: (f32, f32) = (92.0, 58.0);
const PIC_R: f32 = 7.0;

/// The palette, very small: a search line over three rows, the first selected.
fn mini_rows(dark: bool, pad: f32) -> El {
    let ink = if dark { Color::WHITE } else { Color::BLACK };
    let line = |tint: Tint, w: f32, selected: bool| {
        row().w_full().h(9.0).px(3.0).gap(3.5).center_y().radius(3.0).fill(if selected { ink.alpha(0.12) } else { Color::TRANSPARENT })
            .child(el().size(5.0, 5.0).radius(1.5).fill(tint.stops().1))
            .child(el().w(w).h(2.0).radius(1.0).fill(ink.alpha(if selected { 0.85 } else { 0.42 })))
    };
    col().w_full().padding(pad).gap(1.5)
        .child(row().w_full().h(9.0).px(3.0).gap(3.5).center_y()
            .child(el().size(4.5, 4.5).radius(2.25).border(1.0, ink.alpha(0.5)))
            .child(el().w(28.0).h(2.0).radius(1.0).fill(ink.alpha(0.26))))
        .child(line(Tint::Violet, 27.0, true))
        .child(line(Tint::Blue, 36.0, false))
        .child(line(Tint::Orange, 21.0, false))
}

fn mini_palette(dark: bool) -> El {
    let ink = if dark { Color::WHITE } else { Color::BLACK };
    el().size(PIC.0, PIC.1).radius(PIC_R).clip_rounded(PIC_R)
        .fill(rune::color::hex(if dark { "#1d1d20" } else { "#f3f3f6" }).unwrap_or(Color::BLACK)).border(1.0, ink.alpha(0.10))
        .child(mini_rows(dark, 5.0))
}

/// Follow the system: the dark palette on the left half, the light one on the right.
fn mini_system() -> El {
    let half = PIC.0 / 2.0;
    el().size(PIC.0, PIC.1).radius(PIC_R).clip_rounded(PIC_R)
        .child(mini_palette(true))
        .child(el().absolute(half, 0.0).size(half, PIC.1).clip_rounded(0.0).child(el().absolute(-half, 0.0).child(mini_palette(false))))
}

/// Liquid Glass: the desktop showing through a clear pane, lit along its rim.
fn mini_liquid() -> El {
    let (inset_x, inset_y) = (9.0, 7.0);
    let pane = el().absolute(inset_x, inset_y).size(PIC.0 - inset_x * 2.0, PIC.1 - inset_y * 2.0).radius(7.0).clip_rounded(7.0)
        .child(el().absolute(-inset_x - 3.0, -inset_y - 2.0).child(wallpaper(0.0)))
        .child(el().pin(FULL, FULL, FULL, FULL).linear_gradient(std::f32::consts::PI, [(0.0, Color::WHITE.alpha(0.22)), (0.45, Color::WHITE.alpha(0.03)), (1.0, Color::WHITE.alpha(0.10))]))
        .child(mini_rows(true, 3.0))
        .child(el().pin(FULL, FULL, FULL, FULL).radius(7.0).border_gradient(1.0, Color::WHITE.alpha(0.85), Color::WHITE.alpha(0.25)));
    el().size(PIC.0, PIC.1).radius(PIC_R).clip_rounded(PIC_R).child(wallpaper(0.0)).child(pane)
}

/// A desktop to stand behind a surface drawing: a few bright shapes on a dark ground.
fn wallpaper(blur: f32) -> El {
    let shape = |x: f32, y: f32, d: f32, tint: Tint| el().absolute(x, y).size(d, d).radius(d / 2.0).fill(tint.stops().0);
    el().size(PIC.0, PIC.1).fill(rune::color::hex("#15151c").unwrap_or(Color::BLACK))
        .child(el().size(PIC.0, PIC.1).blur(blur)
            .child(shape(-6.0, -9.0, 48.0, Tint::Pink))
            .child(shape(39.0, 18.0, 52.0, Tint::Blue))
            .child(shape(11.0, 32.0, 28.0, Tint::Yellow)))
}

/// A surface choice: the palette over a desktop, blurring it, letting it through, or covering it.
fn mini_surface(mode: &str) -> El {
    let (inset_x, inset_y) = (9.0, 7.0);
    let panel = el().absolute(inset_x, inset_y).size(PIC.0 - inset_x * 2.0, PIC.1 - inset_y * 2.0).radius(5.0).clip_rounded(5.0)
        .border(1.0, Color::WHITE.alpha(0.16));
    let panel = match mode {
        "none" => panel.fill(rune::color::hex("#1d1d20").unwrap_or(Color::BLACK)),
        _ => panel.child(el().absolute(-inset_x, -inset_y).child(wallpaper(if mode == "capture" { 6.0 } else { 0.0 })))
            .child(el().pin(FULL, FULL, FULL, FULL).fill(Color::BLACK.alpha(if mode == "capture" { 0.42 } else { 0.30 }))),
    };
    el().size(PIC.0, PIC.1).radius(PIC_R).clip_rounded(PIC_R).child(wallpaper(0.0)).child(panel.child(mini_rows(true, 3.0)))
}

// ---- Panes ------------------------------------------------------------------------------

fn general(ui: &SettingsUi, p: &Palette) -> Vec<El> {
    let s = &ui.settings;
    let themes = vec![
        choice(p, "theme-system".into(), s.appearance == "system", "Auto", mini_system(), |s| s.appearance = "system".into()),
        choice(p, "theme-light".into(), s.appearance == "light", "Light", mini_palette(false), |s| s.appearance = "light".into()),
        choice(p, "theme-dark".into(), s.appearance == "dark", "Dark", mini_palette(true), |s| s.appearance = "dark".into()),
        choice(p, "theme-liquid".into(), s.liquid(), "Liquid", mini_liquid(), |s| s.appearance = "liquid".into()),
    ];
    let surfaces = vec![
        choice(p, "surface-capture".into(), s.backdrop == "capture", "Blur", mini_surface("capture"), |s| s.backdrop = "capture".into()),
        choice(p, "surface-compositor".into(), s.backdrop == "compositor", "Compositor", mini_surface("compositor"), |s| s.backdrop = "compositor".into()),
        choice(p, "surface-none".into(), s.backdrop == "none", "Solid", mini_surface("none"), |s| s.backdrop = "none".into()),
    ];
    let surface_note = match s.backdrop.as_str() {
        _ if s.liquid() => "Liquid is its own surface: glass that bends the screen behind it.",
        "capture" => "EchoSpot blurs the screen behind it itself. Works with Hyprland's blur off.",
        "compositor" => "Left translucent for Hyprland's own blur to fill.",
        _ => "An opaque panel: nothing shows through.",
    };
    vec![
        group(p, "Appearance", card(p, vec![
            pictured(p, "Theme", "Auto follows the system. Liquid is glass.", themes),
            pictured(p, "Surface", surface_note, surfaces),
        ])),
        group(p, "Behavior", card(p, vec![
            setting(p, "Hide when focus is lost", "Clicking another window dismisses the palette.", switch(p, "hide-on-blur", s.hide_on_blur, |s| s.hide_on_blur = !s.hide_on_blur)),
            setting(p, "Show animations", "Menus, selection and the palette's own motion.", switch(p, "animations", s.animations, |s| s.animations = !s.animations)),
        ])),
        noted(p, "Snippets", card(p, vec![
            setting(p, "Expand snippets as you type", "Typing a snippet's keyword in any app replaces it with the snippet.",
                switch(p, "snippet-expansion", s.snippet_expansion, |s| s.snippet_expansion = !s.snippet_expansion)),
        ]), if !s.snippet_expansion { "Off, EchoSpot never looks at what you type.".to_string() }
            else if crate::features::expander::permitted() { "Reads the keyboard directly, as a US layout, and keeps only the last few characters in memory.".to_string() }
            else { "EchoSpot cannot read the keyboard. Add yourself to the input group (sudo usermod -aG input $USER) and log in again.".to_string() }),
        noted(p, "Shortcut", card(p, vec![
            setting(p, "Summon EchoSpot", "", ui::keycaps_row(p, "super+space", true)),
        ]), "Bound in ~/.config/hypr/bindings.lua to `echospot toggle`."),
    ]
}

/// The two scopes of the Commands table, as a segmented capsule.
fn scope_control(ui: &SettingsUi, p: &Palette) -> El {
    let segment = |key: &str, label: &str, assigned: bool| {
        let on = ui.assigned_only == assigned;
        row().key(key.to_string()).h(22.0).px(space::LG).center().radius(11.0).motion(motion::HOVER)
            .fill(if !on { Color::TRANSPARENT } else if p.dark { Color::WHITE.alpha(0.16) } else { Color::WHITE })
            .on_press(move |ui: &mut SettingsUi| {
                ui.assigned_only = assigned;
                ui.refilter();
            })
            .child(text(label.to_string()).font(CONTROL).color(if on { p.text } else { p.text_secondary }))
    };
    row().h(28.0).px(3.0).center_y().radius(14.0).fill(p.control_surface)
        .child(segment("scope-all", "All", false)).child(segment("scope-assigned", "Assigned", true))
}

/// A cell of the table with nothing in it yet: it lights under the pointer, and on the
/// selected row says what a click there does.
fn empty_cell(p: &Palette, key: String, width: f32, hint: &str, shown: bool) -> El {
    row().key(key).w(width).h(CONTROL_H).px(space::MD).center_y().radius(radius::BAR_CONTROL)
        .fill(Color::TRANSPARENT).hover(p.control_surface).press(p.control_hover).motion(motion::HOVER)
        .child(text(if shown { hint.to_string() } else { String::new() }).font(CONTROL).color(p.text_tertiary))
}

fn alias_cell(ui: &SettingsUi, p: &Palette, c: &Command, selected: bool) -> El {
    let id = c.id.clone();
    if ui.alias_for.as_deref() == Some(&c.id) && ui.alias.focused {
        return field_box(p, "alias", true).w(ALIAS_W).h(CONTROL_H).px(space::MD)
            .child(text_input(&ui.alias, CONTROL).key("alias").single_line().grow(1.0).color(p.text)
                .placeholder("Alias", p.text_tertiary).no_focus_ring()
                .on_edit(|ui: &mut SettingsUi| &mut ui.alias)
                .on_input_change(|ui: &mut SettingsUi, _| ui.alias_changed())
                .on_submit(|ui: &mut SettingsUi, tx| {
                    ui.end_alias(false);
                    tx.blur();
                }));
    }
    let cell = match ui.bindings.aliases.get(&c.id) {
        Some(alias) => row().key(format!("alias-{id}")).w(ALIAS_W).h(CONTROL_H).center_y()
            .child(row().h(22.0).max_w(ALIAS_W).px(space::MD).center_y().radius(radius::KEY_CAP)
                .fill(p.control_surface).hover(p.control_hover).motion(motion::HOVER)
                .child(text(alias.clone()).font(CONTROL).color(p.text.alpha(0.92)).truncate())),
        None => empty_cell(p, format!("alias-{id}"), ALIAS_W, "Add Alias", selected),
    };
    cell.on_press(move |ui: &mut SettingsUi| ui.begin_alias(&id))
}

fn hotkey_cell(ui: &SettingsUi, p: &Palette, c: &Command, selected: bool) -> El {
    let id = c.id.clone();
    if ui.recording.as_deref() == Some(&c.id) {
        let failed = !ui.record_error.is_empty();
        let m = ui.held;
        let held: Vec<&str> = [(m.logo, "super"), (m.ctrl || m.cmd, "ctrl"), (m.alt, "alt"), (m.shift, "shift")].iter().filter(|(on, _)| *on).map(|(_, n)| *n).collect();
        let inside = if failed {
            text(ui.record_error.clone()).font(CONTROL).color(p.destructive).truncate().grow(1.0)
        } else if held.is_empty() {
            text("Press a hotkey…").font(CONTROL).color(p.text_secondary).grow(1.0)
        } else {
            row().grow(1.0).gap(space::XS).center_y().child(ui::keycaps_row(p, &held.join("+"), true))
                .child(text("…").font(CONTROL).color(p.text_tertiary))
        };
        return row().key("recorder").w(HOTKEY_W).h(CONTROL_H).px(space::MD).center_y().radius(radius::BAR_CONTROL)
            .fill(if p.dark { Color::BLACK.alpha(0.22) } else { Color::BLACK.alpha(0.035) })
            .border(2.0, if failed { p.destructive.alpha(0.85) } else { accent(p).alpha(0.85) }).motion(motion::HOVER)
            .child(inside);
    }
    let Some(chord) = ui.bindings.hotkeys.get(&c.id) else {
        return empty_cell(p, format!("hotkey-{id}"), HOTKEY_W, "Record Hotkey", selected)
            .on_press_tx(move |ui: &mut SettingsUi, tx| ui.begin_record(&id, tx));
    };
    let record = id.clone();
    let mut cell = row().key(format!("hotkey-{id}")).w(HOTKEY_W).h(CONTROL_H).pl(space::XS).gap(space::XS).center_y().radius(radius::BAR_CONTROL)
        .fill(Color::TRANSPARENT).hover(p.control_surface).motion(motion::HOVER)
        .on_press_tx(move |ui: &mut SettingsUi, tx| ui.begin_record(&record, tx))
        .child(ui::keycaps_row(p, chord, true)).child(spacer());
    if selected {
        cell = cell.child(el().key(format!("unbind-{id}")).size(CONTROL_H - 4.0, CONTROL_H - 4.0).mr(space::XXS).center().radius(radius::KEY_CAP)
            .hover(p.control_hover).press(p.control_pressed).motion(motion::HOVER)
            .on_press(move |ui: &mut SettingsUi| {
                ui.stop_recording();
                ui.set_hotkey(&id, "");
            })
            .child(icon(&icons::X, 11.0).color(p.text_secondary)));
    }
    cell
}

fn command_row(ui: &SettingsUi, p: &Palette, scale: f32, c: &Command) -> El {
    let id = c.id.clone();
    let selected = ui.selected.as_deref() == Some(&c.id);
    let r = row().key(row_key(&c.id)).w_full().h(COMMAND_ROW_H).px(INSET - space::XS).gap(space::LG).center_y().radius(CARD_R - space::XS);
    let r = if selected { r.fill(p.selection) } else { r.fill(Color::TRANSPARENT).hover(p.row_hover) };
    r.on_press(move |ui: &mut SettingsUi| ui.select(&id))
        .child(ui::icon_slot(&ui.icons, p, scale, &c.icon, 26.0))
        .child(col().grow(1.0).min_w(0.0).gap(1.0)
            .child(text(c.title.clone()).font(type_::ROW_TITLE).color(p.text).truncate())
            .child(text(c.kind.clone()).font(NOTE).color(p.text_secondary).truncate()))
        .child(alias_cell(ui, p, c, selected))
        .child(hotkey_cell(ui, p, c, selected))
}

/// The Commands pane: a search, and under it every launcher row with its alias and hotkey.
/// Unlike the other panes it is one table that fills the window and scrolls inside its card.
fn commands(ui: &SettingsUi, p: &Palette, scale: f32) -> El {
    let search = field_box(p, "command-filter", ui.filter.focused).grow(1.0).min_w(0.0).gap(space::SM)
        .child(icon(&icons::SEARCH, 13.0).color(p.text_tertiary))
        .child(text_input(&ui.filter, type_::ROW_TITLE).key("command-filter").single_line().grow(1.0).color(p.text)
            .placeholder("Search commands, aliases and hotkeys", p.text_tertiary).no_focus_ring()
            .on_edit(|ui: &mut SettingsUi| &mut ui.filter)
            .on_input_change(|ui: &mut SettingsUi, _| ui.refilter())
            .on_submit(|ui: &mut SettingsUi, tx| ui.record_selected(tx)));
    let heading = |label: &str| text(label.to_string()).font(type_::SECTION).color(p.text_tertiary);
    let header = row().w_full().h(30.0).px(INSET).gap(space::LG).center_y()
        .child(heading("Name").grow(1.0))
        .child(heading("Alias").w(ALIAS_W).pl(space::MD))
        .child(heading("Hotkey").w(HOTKEY_W).pl(space::XS));
    let body = if ui.shown.is_empty() {
        let (title, note) = if ui.commands.is_empty() {
            ("Loading commands…".to_string(), String::new())
        } else if !ui.filter.text.trim().is_empty() {
            (format!("Nothing matches “{}”", ui.filter.text.trim()), "Search by name, kind, alias or hotkey.".to_string())
        } else {
            ("Nothing assigned yet".to_string(), "Commands you give an alias or a hotkey are gathered here.".to_string())
        };
        col().w_full().grow(1.0).center().gap(space::XS).px(space::XXL)
            .child(icon(&icons::KEYBOARD, 22.0).color(p.text_tertiary)).child(el().h(space::XS))
            .child(text(title).font(type_::ROW_TITLE).color(p.text_secondary))
            .child(text(note).font(NOTE).color(p.text_tertiary))
    } else {
        col().key("command-scroll").w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).scroll_edge_fade(14.0)
            .child(col().w_full().padding(space::XS).gap(1.0).children(ui.shown.iter().map(|&i| command_row(ui, p, scale, &ui.commands[i]))))
    };
    let table = card_surface(p).grow(1.0).min_h(0.0).clip_rounded(CARD_R)
        .child(header)
        .child(el().w_full().h(size::HAIRLINE).fill(p.separator.alpha(if p.dark { 0.08 } else { 0.09 })))
        .child(body);
    let note = if ui.recording.is_some() {
        "Press the keys to use, with Super, Ctrl or Alt among them. ⌫ removes the hotkey; esc keeps the old one.".to_string()
    } else {
        let assigned = ui.commands.iter().filter(|c| ui.bindings.aliases.contains_key(&c.id) || ui.bindings.hotkeys.contains_key(&c.id)).count();
        format!("{} commands, {assigned} assigned. An alias typed in the launcher puts its command first; a hotkey runs it from anywhere.", ui.commands.len())
    };
    col().key("commands-pane").w_full().grow(1.0).min_h(0.0).px(space::XXL + space::XS).pt(space::SM).pb(space::XL + space::XS).gap(space::XL)
        .child(row().w_full().gap(space::LG).center_y().child(search).child(scope_control(ui, p)))
        .child(table)
        .child(footnote(p, note))
}

/// A provider's tile: its letter on its colour, lit like every other tile here.
fn provider_tile(info: &ai::Provider, slot: f32) -> El {
    if info.mark.is_empty() {
        return ui::badge_tile(&icons::CODE, slot, info.tint);
    }
    let tile = slot - 3.0;
    let (top, bottom) = info.tint.stops();
    el().size(slot, slot).center().child(el().size(tile, tile).radius(tile * 0.28).center().fill(bottom).gradient(top, bottom)
        .border_gradient(1.0, Color::WHITE.alpha(0.42), Color::WHITE.alpha(0.05)).shadow(1.0, 3.0, bottom.alpha(0.38))
        .child(text(info.mark).font(TextStyle::new((tile * 0.54).round(), 700.0).lh(tile)).color(Color::WHITE)))
}

/// A provider as a row to pick: who it is, whether it already has a key, and the mark if it is
/// the one in use.
fn provider_row(ui: &SettingsUi, p: &Palette, info: &'static ai::Provider) -> El {
    let a = &ui.settings.ai;
    let on = a.provider == info.id;
    let have = match a.key_source(info.id) {
        ai::KeySource::Saved => "Key added",
        ai::KeySource::Env => "Key from environment",
        ai::KeySource::None if info.id == "custom" && a.ready_for("custom") => "Local",
        ai::KeySource::None => "",
    };
    let mut r = row().key(format!("provider-{}", info.id)).w_full().h(48.0).px(INSET).gap(space::LG).center_y()
        .hover(if on { Color::TRANSPARENT } else { p.row_hover }).motion(motion::HOVER)
        .on_press(move |ui: &mut SettingsUi| ui.choose_provider(info.id))
        .child(provider_tile(info, 30.0))
        .child(col().grow(1.0).min_w(0.0).gap(1.0)
            .child(text(info.name).font(type_::ROW_TITLE).color(p.text))
            .child(text(info.blurb).font(NOTE).color(p.text_secondary).truncate()));
    if !have.is_empty() {
        r = r.child(row().gap(space::SM).center_y().child(status_dot(p.success))
            .child(text(have).font(NOTE).color(p.text_secondary)));
    }
    r.child(radio(p, on))
}

/// The key of the provider in use: where to get one, the field it goes in, and, the moment it
/// is in, whether the provider knows it.
fn key_card(ui: &SettingsUi, p: &Palette) -> El {
    let a = &ui.settings.ai;
    let info = a.info();
    let source = a.key_source(info.id);
    let (dot, line) = match &ui.probe {
        Probe::Ok(m) => (p.success, m.clone()),
        Probe::Failed(m) => (p.destructive, m.clone()),
        Probe::Running => (p.warning, format!("Asking {}…", info.name)),
        Probe::Idle => match source {
            _ if info.id == "custom" && a.base_url.trim().is_empty() => (p.text_tertiary, "Add an endpoint to connect.".to_string()),
            ai::KeySource::Saved => (p.text_tertiary, "Key added, not checked yet.".to_string()),
            ai::KeySource::Env => (p.text_tertiary, format!("Using {} from your environment.", info.env)),
            ai::KeySource::None if a.ready() => (p.text_tertiary, "A local server needs no key.".to_string()),
            ai::KeySource::None => (p.warning, "No key yet.".to_string()),
        },
    };
    let key = field(p, "api-key", &ui.api_key, if source == ai::KeySource::Env { "Using the key in your environment" } else { info.key_hint },
        |ui| &mut ui.api_key, |ui| { ui.commit(); ui.recheck(650); }).grow(1.0).min_w(0.0);
    let mut entry = row().w_full().gap(space::MD).center_y().child(key);
    if !info.keys_url.is_empty() {
        entry = entry.child(link_button(p, "get-key", "Get a Key", info.keys_url));
    }
    let status = row().w_full().gap(space::MD).center_y()
        .child(if ui.probe == Probe::Running { icon(&icons::REFRESH, 11.0).color(p.text_secondary).looping(rune::LoopSpec::rotation(0.9)) } else { status_dot(dot) })
        .child(text(line).font(NOTE).color(if matches!(ui.probe, Probe::Failed(_)) { p.destructive } else { p.text_secondary }).truncate().grow(1.0));
    // Nothing to check without a key; while one is being checked, nothing to press.
    let status = if a.ready() && ui.probe != Probe::Running {
        status.child(row().key("test-connection").h(20.0).px(space::MD).mr(-space::MD).center().radius(10.0).hover(p.control_surface).press(p.control_hover)
            .on_press(|ui: &mut SettingsUi| ui.test_connection())
            .child(text(if ui.probe == Probe::Idle { "Check" } else { "Check Again" }).font(type_::SECTION).color(p.text_secondary)))
    } else {
        status.min_h(20.0)
    };
    let mut rows = Vec::new();
    if info.id == "custom" {
        rows.push(setting(p, "Endpoint", "The base URL, up to and including /v1.",
            field(p, "base-url", &ui.base_url, "http://localhost:11434/v1", |ui| &mut ui.base_url, |ui| { ui.commit(); ui.recheck(650); }).w(250.0)));
    }
    rows.push(col().w_full().px(INSET).pt(space::XL).pb(space::LG).gap(space::LG)
        .child(text(format!("{} API key", info.name)).font(type_::ROW_TITLE).color(p.text))
        .child(entry).child(status));
    card(p, rows)
}

const CLAUDE: [(&str, &str); 4] = [
    ("claude-fable-5-2", "Most intelligent"),
    ("claude-opus-5-5", "Deep, careful work"),
    ("claude-sonnet-5-5", "Fast and capable"),
    ("claude-haiku-5-5", "Fastest"),
];

fn model_row(p: &Palette, id: String, blurb: &str, on: bool) -> El {
    let pick = id.clone();
    row().key(format!("model-{id}")).w_full().h(38.0).px(INSET).gap(space::LG).center_y()
        .hover(p.row_hover).motion(motion::HOVER)
        .on_press(move |ui: &mut SettingsUi| ui.choose_model(&pick))
        .child(text(ai::model_name(&id)).font(type_::ROW_TITLE).color(p.text).truncate().grow(1.0))
        .child(text(blurb.to_string()).font(type_::ROW_TRAILING).color(p.text_secondary))
        .child(if on { icon(&icons::CHECK, 14.0).color(accent(p)) } else { el().size(14.0, 14.0) })
}

/// How many of a provider's models show under the field at once.
const MODEL_ROWS: usize = 6;

/// The model in use. Claude's are few enough to list; a gateway's are hundreds, so the field is
/// the search: what is typed narrows the provider's own list underneath it.
fn model_card(ui: &SettingsUi, p: &Palette) -> (El, String) {
    let a = &ui.settings.ai;
    let info = a.info();
    let typed = ui.model.text.trim();
    let other = field(p, "model", &ui.model, info.model_hint, |ui| &mut ui.model, |ui| ui.commit()).w(250.0);
    if info.id == "anthropic" {
        let mut rows: Vec<El> = CLAUDE.iter().map(|(id, blurb)| model_row(p, id.to_string(), blurb, a.model == *id)).collect();
        rows.push(setting(p, "Another model", "Any model id your key can use.", other));
        return (card(p, rows), String::new());
    }
    let listed: &[String] = if ui.fetched.0 == info.id { &ui.fetched.1 } else { &[] };
    let mut rows = vec![setting(p, "Model", if listed.is_empty() { "The id your provider expects." } else { "Type to search this provider's models." }, other)];
    let in_list = listed.iter().any(|m| m == typed);
    let exact = typed.is_empty() || listed.is_empty() || in_list;
    let needle = typed.to_lowercase();
    // Nothing being searched for: the ones used before. Otherwise what the typing matches.
    let recent: Vec<&String> = a.picked.get(info.id).into_iter().flatten().filter(|m| m.as_str() != typed).collect();
    let matches: Vec<&String> = if exact { recent } else { listed.iter().filter(|m| m.to_lowercase().contains(&needle)).collect() };
    let more = matches.len().saturating_sub(MODEL_ROWS);
    if in_list {
        rows.push(model_row(p, typed.to_string(), "In use", true));
    }
    rows.extend(matches.iter().take(MODEL_ROWS).map(|m| model_row(p, m.to_string(), if exact { "Used before" } else { "" }, false)));
    let note = if !exact && !listed.is_empty() && matches.is_empty() {
        format!("{} lists no model matching “{typed}”. It is used as typed.", info.name)
    } else if more > 0 {
        format!("{more} more match. Keep typing to narrow them.")
    } else if listed.is_empty() && !a.ready() {
        "Add a key and this provider's models appear here to pick from.".to_string()
    } else {
        String::new()
    };
    (card(p, rows), note)
}

fn ai_pane(ui: &SettingsUi, p: &Palette) -> Vec<El> {
    let a = &ui.settings.ai;
    let info = a.info();
    let (models, model_note) = model_card(ui, p);
    let focused = ui.system_prompt.focused;
    vec![
        noted(p, "Provider", card(p, ai::PROVIDERS.iter().map(|info| provider_row(ui, p, info)).collect()),
            "Each provider keeps its own key. Switching brings back the model you last used with it."),
        noted(p, "Key", key_card(ui, p),
            format!("Kept in settings.json, readable only by you. Left empty, {} is used.", info.env)),
        if model_note.is_empty() { group(p, "Model", models) } else { noted(p, "Model", models, model_note) },
        noted(p, "Instructions", card_surface(p).key("system-box").min_h(88.0).px(INSET).py(space::LG)
            .border(if focused { 2.0 } else { 1.0 }, if focused { accent(p).alpha(0.85) } else { p.card_stroke.alpha(0.06) })
            .child(text_input(&ui.system_prompt, type_::ROW_TITLE).key("system-prompt").w_full().grow(1.0).color(p.text)
                .placeholder("A system prompt for every question.", p.text_tertiary).no_focus_ring()
                .on_edit(|ui: &mut SettingsUi| &mut ui.system_prompt)
                .on_input_change(|ui: &mut SettingsUi, _| ui.commit())),
            "Left empty, EchoSpot asks for short, direct answers."),
        group(p, "History", card(p, vec![
            setting(p, "Saved chats", &format!("{} kept on this machine. The newest 40 stay.", ui.chats),
                button(p, "clear-chats", "Delete All…", true, |ui, _| {
                    let _ = std::fs::remove_dir_all(crate::util::data_dir().join("chats"));
                    ui.chats = 0;
                    tell_main(|app| app.chat.clear());
                })),
        ])),
    ]
}

/// A number worth a glance, over what it counts.
fn stat(p: &Palette, value: String, label: &str) -> El {
    col().grow(1.0).min_w(0.0).w(0.0).px(INSET).py(space::XL).gap(space::XXS)
        .child(text(value).font(TextStyle::new(20.0, 600.0).lh(26.0)).color(p.text))
        .child(text(label.to_string()).font(NOTE).color(p.text_secondary))
}

fn clipboard(ui: &SettingsUi, p: &Palette) -> Vec<El> {
    let s = &ui.settings;
    let (entries, images, bytes) = ui.clipboard;
    let rule = || col().py(space::XL).child(el().w(size::HAIRLINE).grow(1.0).fill(p.separator.alpha(0.08)));
    vec![
        card_surface(p).child(row().w_full()
            .child(stat(p, entries.to_string(), "Entries")).child(rule())
            .child(stat(p, images.to_string(), "Images")).child(rule())
            .child(stat(p, crate::util::human_size(bytes), "On disk"))),
        noted(p, "History", card(p, vec![
            setting(p, "Keep clipboard history", "Text, links, colors and images you copy.",
                switch(p, "clipboard-history", s.clipboard_history, |s| s.clipboard_history = !s.clipboard_history)),
            setting(p, "Clear history", "Removes every entry, pinned ones too.",
                button(p, "clear-clipboard", "Clear…", true, |ui, _| {
                    let _ = std::fs::remove_dir_all(crate::util::data_dir().join("clipboard"));
                    ui.clipboard = (0, 0, 0);
                    tell_main(|app| app.clipboard.reload());
                })),
        ]), "Entries a password manager marks sensitive are never kept. Turning history on or off takes effect the next time EchoSpot starts."),
        noted(p, "In the Palette", card(p, vec![
            setting(p, "Paste the selected entry", "Goes straight into the app you came from.", ui::keycaps_row(p, "enter", true)),
            setting(p, "Pin an entry", "Pinned entries stay at the top and are never pruned.", ui::keycaps_row(p, "ctrl+shift+p", true)),
        ]), "Search “clipboard” in the launcher, or bind `echospot open clipboard` to a key."),
    ]
}

/// Where Search Files looks: the home folder, always, and each mounted drive with its switch.
fn files(ui: &SettingsUi, p: &Palette) -> Vec<El> {
    let (s, home) = (&ui.settings.files, crate::util::home());
    let mut places = vec![
        setting(p, "Home Folder", &home.display().to_string(), text("Always").font(CONTROL).color(p.text_tertiary)),
    ];
    for drive in &ui.drives {
        let at = drive.display().to_string();
        let name = drive.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| at.clone());
        let on = !s.drives_off.contains(&at);
        let toggled = at.clone();
        places.push(setting(p, &name, &at, switch(p, &format!("drive-{at}"), on, move |s| {
            let off = &mut s.files.drives_off;
            match off.iter().position(|d| *d == toggled) {
                Some(i) => { off.remove(i); }
                None => off.push(toggled.clone()),
            }
        })));
    }
    let note = if ui.drives.is_empty() { "No other drives are mounted. One that is mounted later is searched too, and appears here." }
        else { "Every mounted drive is searched unless it is switched off here. One mounted later is searched too." };
    vec![
        noted(p, "Search In", card(p, places), note),
        noted(p, "Excluded Folders", card(p, excluded_rows(ui, p)),
            if ui.exclude_error.is_empty() { "Nothing inside an excluded folder is found, however deep." } else { ui.exclude_error.as_str() }),
        noted(p, "Results", card(p, vec![
            setting(p, "Find hidden files", "Files and folders whose names begin with a dot.", switch(p, "files-hidden", s.hidden, |s| s.files.hidden = !s.files.hidden)),
        ]), "Whatever a .gitignore, .ignore or .fdignore file leaves out is never found: put an .ignore file in a folder to hide parts of it."),
    ]
}

/// The folders left out of the search, each with the button that lets it back in.
fn excluded_rows(ui: &SettingsUi, p: &Palette) -> Vec<El> {
    let home = crate::util::home();
    let mut rows: Vec<El> = ui.settings.files.excluded.iter().map(|folder| {
        let path = crate::features::files::typed_folder(folder, &home).unwrap_or_else(|| folder.into());
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| folder.clone());
        // A folder that is gone, or on a drive not mounted now, keeps its place and says so.
        let place = if path.is_dir() { crate::util::tilde(&path) } else { format!("{} · not there now", crate::util::tilde(&path)) };
        let gone = folder.clone();
        setting(p, &name, &place, button(p, &format!("include-{folder}"), "Remove", false, move |ui, _| {
            ui.settings.files.excluded.retain(|f| *f != gone);
            ui.commit();
        }))
    }).collect();
    if rows.is_empty() {
        rows.push(setting(p, "Nothing excluded", "Folders you exclude from the search appear here.", el()));
    }
    // The last row takes a new one: a path typed or pasted, then Return or Add.
    let typing = field_box(p, "exclude", ui.exclude.focused).grow(1.0).min_w(0.0)
        .child(text_input(&ui.exclude, type_::ROW_TITLE).key("exclude").single_line().grow(1.0).color(p.text)
            .placeholder("~/Documents/Private", p.text_tertiary).no_focus_ring()
            .on_edit(|ui: &mut SettingsUi| &mut ui.exclude)
            .on_input_change(|ui: &mut SettingsUi, _| ui.exclude_error.clear())
            .on_submit(|ui: &mut SettingsUi, _| ui.exclude_typed()));
    rows.push(row().w_full().min_h(44.0).px(INSET).py(space::LG).gap(space::XL).center_y()
        .child(typing)
        .child(button(p, "exclude-add", "Add", false, |ui, _| ui.exclude_typed())));
    rows
}

fn extensions(ui: &SettingsUi, p: &Palette, scale: f32) -> Vec<El> {
    let store = card_surface(p).key("store-card").child(row().w_full().px(INSET).py(space::XL).gap(space::XL).center_y()
        .child(ui::badge_tile(&icons::STORE, 36.0, Tint::Cyan))
        .child(col().grow(1.0).min_w(0.0).gap(1.0)
            .child(text("Raycast Store").font(GROUP_TITLE).color(p.text))
            .child(text("Thousands of extensions, installed from the bundles the store already built.").font(NOTE).color(p.text_secondary).wrap()))
        .child(button(p, "open-store", "Open Store", false, |_, tx| {
            tx.close_window();
            std::thread::spawn(|| {
                crate::ipc::send("open store");
            });
        })));
    let mut rows: Vec<El> = ui.extensions.iter().map(|e| {
        let id = e.id.clone();
        let picture = match e.icon.as_ref().and_then(|path| ui.icons.file(path, 28.0, scale)) {
            Some(px) => el().size(28.0, 28.0).radius(7.0).clip_rounded(7.0).pixels_fit(&px, rune::SurfaceFit::Fill),
            None => ui::badge_tile(&icons::PUZZLE, 31.0, Tint::Green),
        };
        let by = if e.author.is_empty() { String::new() } else { format!("{} · ", e.author) };
        row().w_full().min_h(50.0).px(INSET).py(space::MD).gap(space::XL).center_y()
            .child(picture)
            .child(col().grow(1.0).min_w(0.0).gap(1.0)
                .child(text(e.title.clone()).font(type_::ROW_TITLE).color(p.text).truncate())
                .child(text(format!("{by}{} command{}", e.commands, if e.commands == 1 { "" } else { "s" })).font(NOTE).color(p.text_secondary).truncate()))
            .child(button(p, &format!("uninstall-{id}"), "Uninstall", true, move |ui, _| {
                let _ = std::fs::remove_dir_all(crate::ext::extensions_dir().join(&id));
                ui.extensions.retain(|e| e.id != id);
                tell_main(|app| app.ext.rescan());
            }))
    }).collect();
    if rows.is_empty() {
        rows.push(setting(p, "Nothing installed yet", "Extensions you install from the store appear here.", el()));
    }
    let title = if ui.extensions.is_empty() { "Installed".to_string() } else { format!("Installed · {}", ui.extensions.len()) };
    vec![store, noted(p, &title, card(p, rows), "Extensions run in Node, in a process of their own.")]
}

const SHORTCUTS: [(&str, &[(&str, &str)]); 5] = [

    ("Everywhere", &[
        ("Run the selected row", "enter"), ("Show its actions", "ctrl+k"), ("Move through rows", "up"), ("Next row", "ctrl+n"),
        ("Previous row", "ctrl+p"), ("Back, or clear the search", "escape"), ("Back when the search is empty", "backspace"), ("Hide the palette", "ctrl+w"),
    ]),
    ("Launcher", &[
        ("Ask Quick AI what is typed", "tab"), ("Add to or remove from Favorites", "ctrl+shift+f"), ("Copy the name or answer", "ctrl+shift+c"),
        ("Set the row's alias and hotkey", "ctrl+shift+k"), ("Open Settings", "ctrl+,"), ("Quit EchoSpot", "ctrl+q"),
    ]),
    ("Search Files", &[
        ("Show in the file manager", "ctrl+enter"), ("Copy the path", "ctrl+shift+c"), ("Copy the file", "ctrl+shift+f"),
        ("Exclude its folder from the search", "ctrl+shift+x"), ("Open a terminal there", "ctrl+t"), ("Move to Trash", "ctrl+x"),
    ]),
    ("AI", &[
        ("Pick a model", "tab"), ("Paste the answer", "ctrl+enter"), ("Ask again", "ctrl+r"), ("Continue Quick AI as a chat", "ctrl+t"), ("New chat", "ctrl+shift+n"),
    ]),
    ("Lists You Edit", &[
        ("Edit the selected snippet or quicklink", "ctrl+e"), ("Create a new one", "ctrl+shift+n"), ("Delete it", "ctrl+x"), ("Submit a form", "ctrl+enter"),
    ]),
];

fn shortcuts(p: &Palette) -> Vec<El> {
    SHORTCUTS.iter().map(|(title, list)| {
        group(p, title, card(p, list.iter().map(|(what, keys)| {
            row().w_full().h(36.0).px(INSET).gap(space::XL).center_y()
                .child(text(what.to_string()).font(type_::ROW_TITLE).color(p.text).truncate().grow(1.0))
                .child(ui::keycaps_row(p, keys, true))
        }).collect()))
    }).collect()
}

fn about(p: &Palette) -> Vec<El> {
    let source = |title: &str, subtitle: &str, label: &'static str, url: &'static str| setting(p, title, subtitle, link_button(p, label, label, url));
    vec![
        col().w_full().items_center().gap(space::XXS).pt(space::LG).pb(space::XS)
            .child(el().pb(space::LG).child(ui::badge_tile(&icons::LOGO, 76.0, Tint::Violet)))
            .child(text("EchoSpot").font(TextStyle::new(20.0, 650.0).lh(26.0)).color(p.text))
            .child(text(format!("Version {}", env!("CARGO_PKG_VERSION"))).font(type_::ROW_TRAILING).color(p.text_secondary))
            .child(text("A native command palette for Hyprland, drawn with Rune.").font(type_::ROW_TRAILING).color(p.text_tertiary).mt(space::SM)),
        card(p, vec![
            source("Source", "github.com/adityavardhansharma/EchoSpot · AGPL-3.0", "GitHub", "https://github.com/adityavardhansharma/EchoSpot"),
            source("Design", "Tinycast, for macOS, is where this began.", "Tinycast", "https://tinycast.dev"),
            source("Drawn with", "Rune draws every pixel, on the GPU.", "Rune", "https://github.com/anomalyco/rune"),
        ]),
    ]
}

// ---- The window -------------------------------------------------------------------------

/// The sidebar: a pane of glass floating in the panel, a gap from its edge on three sides.
fn sidebar(ui: &SettingsUi, p: &Palette) -> El {
    // The brand row stands where a Mac window keeps its traffic lights, and moves the window.
    let brand = row().key("brand").w_full().h(TOOLBAR_H - SIDEBAR_GAP).px(space::MD).gap(space::LG).center_y().window_drag()
        .child(ui::badge_tile(&icons::LOGO, 26.0, Tint::Violet))
        .child(text("EchoSpot").font(type_::PANEL_TITLE).color(p.text));
    let mut list = col().w_full().gap(space::XXS);
    for info in &PANES {
        let (on, pane) = (ui.pane == info.pane, info.pane);
        let item = row().key(format!("pane-{}", info.title)).w_full().h(34.0).px(space::MD).gap(space::LG).center_y().radius(SIDEBAR_R - space::MD);
        let item = if on { item.fill(p.selection) } else { item.fill(Color::TRANSPARENT).hover(p.row_hover) };
        list = list.child(item.on_press(move |ui: &mut SettingsUi| ui.show(pane))
            .child(ui::badge_tile(info.glyph, 23.0, info.tint))
            .child(text(info.title).font(if on { TextStyle::new(13.0, 500.0).lh(18.0) } else { type_::ROW_TITLE }).color(p.text)));
    }
    let version = text(format!("Version {}", env!("CARGO_PKG_VERSION"))).font(NOTE).color(p.text_tertiary).ml(space::MD).mb(space::MD);
    col().key("sidebar").absolute(SIDEBAR_GAP, SIDEBAR_GAP).size(SIDEBAR_W, PANEL.1 - SIDEBAR_GAP * 2.0).px(space::MD)
        .radius(SIDEBAR_R)
        .fill(if p.dark { Color::WHITE.alpha(0.055) } else { Color::WHITE.alpha(0.58) })
        .border_gradient(1.0, if p.dark { Color::WHITE.alpha(0.16) } else { Color::WHITE.alpha(0.95) }, if p.dark { Color::WHITE.alpha(0.04) } else { Color::BLACK.alpha(0.08) })
        .shadow(4.0, 18.0, Color::BLACK.alpha(if p.dark { 0.22 } else { 0.10 }))
        .child(brand).child(list).child(spacer()).child(version)
}

/// A toolbar control: a disc of glass, as Liquid Glass floats its buttons over the content.
fn tool_button(p: &Palette, key: &str, glyph: &'static rune::Icon) -> El {
    ui::glass(el().key(key.to_string()).size(TOOL_BUTTON, TOOL_BUTTON).center(), p, TOOL_BUTTON / 2.0)
        .hover(p.row_hover).press(p.control_surface).motion(motion::HOVER)
        .child(icon(glyph, 14.0).color(p.text.alpha(0.82)))
}

pub fn view(ui: &SettingsUi, cx: &mut Cx<SettingsUi>) -> El {
    let env = cx.env();
    let p = theme::palette(ui.settings.dark(env.system_dark));
    let scale = env.scale.max(1.0);
    let info = PANES.iter().find(|i| i.pane == ui.pane).unwrap_or(&PANES[0]);
    let sections = match ui.pane {
        Pane::General => general(ui, &p),
        Pane::Commands => Vec::new(),
        Pane::Ai => ai_pane(ui, &p),
        Pane::Clipboard => clipboard(ui, &p),
        Pane::Files => files(ui, &p),
        Pane::Extensions => extensions(ui, &p, scale),
        Pane::Shortcuts => shortcuts(&p),
        Pane::About => about(&p),
    };
    let content = col().w_full().px(space::XXL + space::XS).pt(space::SM).pb(space::XXL + space::XS).gap(space::XXL + space::XXS).children(sections);
    let fade = ScrollFade::strength(size::DISSOLVE_TOP).full_at(4.0).with_end(ScrollFade::strength(28.0));
    // The toolbar: the pane's name, which moves the window, and the window's two controls.
    let toolbar = row().key("toolbar").w_full().h(TOOLBAR_H).pl(space::XXL + space::XS + space::XS).pr(space::XL + 1.0).gap(space::MD).center_y()
        .child(row().key("heading").grow(1.0).h_full().center_y().window_drag()
            .child(text(info.title).font(PANE_TITLE).color(p.text)))
        .child(tool_button(&p, "open-config", &icons::CODE)
            .on_press(|_: &mut SettingsUi| { crate::util::spawn_detached(&["xdg-open".into(), Settings::path().display().to_string()]); }))
        .child(tool_button(&p, "done", &icons::X).on_press_tx(close));
    let left = SIDEBAR_GAP + SIDEBAR_W;
    let detail = col().key("detail").pin(Some(left), FULL, FULL, FULL).child(toolbar);
    let detail = if ui.pane == Pane::Commands { detail.child(commands(ui, &p, scale)) } else { detail
        .child(col().key(format!("settings-scroll-{}", info.title)).w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).scroll_fade(fade).child(content)) };
    ui.system_dark.set(env.system_dark);
    let liquid = ui.settings.liquid() && ui.glass.ready();
    let panel = ui::panel_surface("settings-panel", &p, PANEL, (ui.has_backdrop && ui.settings.backdrop == "capture").then_some(&ui.backdrop), &ui.settings.backdrop, liquid)
        .child(detail)
        .child(sidebar(ui, &p));
    el().w_full().h_full().child(if liquid { panel } else { panel.child(ui::panel_rim(&p)) })
}

fn close(ui: &mut SettingsUi, tx: &mut Tx) {
    ui.stop_recording();
    ui.end_alias(false);
    tx.close_window();
}

pub fn on_key(ui: &mut SettingsUi, ev: &KeyEvent, tx: &mut Tx) -> bool {
    // A hotkey being recorded takes every key, the window's own shortcuts included.
    if ui.recording.is_some() {
        ui.record_key(ev, tx.key_base());
        return true;
    }
    let table = ui.pane == Pane::Commands && !ui.alias_editing();
    match ev.key {
        Key::Modifier { .. } => return false,
        Key::Escape if ui.alias_editing() => {
            ui.end_alias(true);
            tx.blur();
        }
        Key::Escape if table && !ui.filter.text.is_empty() => {
            ui.filter.replace_text("");
            ui.refilter();
        }
        Key::Escape | Key::Command('w') => close(ui, tx),
        Key::Down if table => ui.step_row(1, tx),
        Key::Up if table => ui.step_row(-1, tx),
        Key::Enter if table && !ui.filter.focused => ui.record_selected(tx),
        Key::Command('f') if table => ui.filter.focused = true,
        Key::Down if !ui.editing() => ui.step_pane(1),
        Key::Up if !ui.editing() => ui.step_pane(-1),
        Key::Command(c @ '1'..='8') => ui.show(PANES[c as usize - '1' as usize].pane),
        _ => return false,
    }
    true
}

impl crate::glass::Liquid for SettingsUi {
    fn pane(&self) -> Option<crate::glass::Pane<'_>> {
        self.settings.liquid().then(|| crate::glass::Pane { window: SIZE, dark: self.system_dark.get(), level: self.settings.liquid, shot: &self.glass })
    }
}
