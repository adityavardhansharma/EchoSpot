//! What every screen is made of: rows, their icons, and the actions a row offers. Native features
//! and Raycast extensions both produce these, so one view draws them all.

use std::path::PathBuf;

use rune::{Color, Icon, Key, KeyEvent};

use crate::icons;
use crate::theme::Tint;

#[derive(Clone, Debug)]
pub enum IconSrc {
    None,
    /// A glyph on the app-icon-shaped tile commands use.
    Glyph(&'static Icon),
    /// A glyph on a tinted tile (an extension's own colour).
    Tinted(&'static Icon, Color),
    /// A white glyph on one of the palette's own colour tiles: EchoSpot's commands.
    Badge(&'static Icon, Tint),
    /// A site's icon, on the light plate that keeps a dark mark readable.
    Favicon(PathBuf),
    /// An application's icon file, drawn whole.
    App(PathBuf),
    /// An image file: a thumbnail, an extension's asset.
    File(PathBuf),
    Emoji(String),
    Swatch(Color),
    /// A few characters on a coloured tile: a rank, an initial.
    Monogram(String, Color),
}

#[derive(Clone, Debug)]
pub struct Action {
    pub title: String,
    pub icon: &'static Icon,
    /// A chord such as `ctrl+shift+c`; empty for none. The first action always answers Return.
    pub keys: String,
    pub act: Act,
    pub destructive: bool,
    /// Starts a new group in the menu.
    pub section: Option<String>,
}

impl Action {
    pub fn new(title: impl Into<String>, icon: &'static Icon, act: Act) -> Self {
        Self { title: title.into(), icon, keys: String::new(), act, destructive: false, section: None }
    }
    pub fn keys(mut self, keys: &str) -> Self {
        self.keys = keys.into();
        self
    }
    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }
    pub fn section(mut self, name: &str) -> Self {
        self.section = Some(name.into());
        self
    }
}

/// Everything a row can do. `App::perform` is the one place these run.
#[derive(Clone, Debug)]
pub enum Act {
    None,
    /// Launch a desktop entry by its file path.
    Launch(PathBuf),
    /// Run a program detached; the palette closes.
    Spawn(Vec<String>),
    /// Run through `sh -c`, detached.
    Shell(String),
    /// Run through `sh -c` and report the outcome in a toast; the palette stays.
    ShellQuiet { cmd: String, done: String },
    /// Switch Omarchy's theme: its folder's name, and the name people read.
    Theme(String, String),
    OpenUrl(String),
    OpenPath(PathBuf),
    Reveal(PathBuf),
    Copy(String),
    Paste(String),
    CopyFile(PathBuf),
    PasteFile(PathBuf),
    Push(Route),
    /// Push a screen with its search already typed.
    PushQuery(Route, String),
    Pop,
    Close,
    System(crate::features::system::SystemAction),
    Window(crate::features::windows::WindowOp),
    Snap(crate::features::snap::SnapOp),
    FocusWindow(String),
    CloseWindow(String),
    Clip(ClipOp, u64),
    Favorite(String),
    Hide(String),
    /// Bring every row hidden from the results back.
    UnhideAll,
    Emoji(String, EmojiOp),
    Snippet(SnippetOp),
    File(FileOp),
    Quicklink(QuicklinkOp),
    Confirm(Box<Confirm>),
    Ext(crate::ext::ExtAct),
    Ai(AiOp),
    /// Show a launcher row in Settings, where its alias and hotkey are set: its id.
    Binding(String),
    Script(ScriptOp),
    Note(NoteOp),
    Calendar(CalOp),
    Backup(BackupOp),
    /// Open Settings at a pane.
    OpenSettings(String),
    /// Something a pack's row, action or surface asks for: `packs::host::perform` runs it.
    Pack(PackOp),
    Toast(String),
    Quit,
}

/// What the host does for packs. Every one of these starts in EchoSpot's own window, from a key
/// or a click: nothing an agent can type reaches `Grant`.
#[derive(Clone, Debug, PartialEq)]
pub enum PackOp {
    /// Pick an ability: its form when it needs input, its panel when it reads, a run otherwise.
    Invoke { address: String, args: serde_json::Value },
    /// Run an ability now, with this input.
    Run { address: String, input: serde_json::Value },
    /// Show a target of a pack: `ability:<name>:<face>`, `surface:<name>`, `board:<name>`.
    Open { pack: String, target: String, args: serde_json::Value },
    /// A function a drawn surface holds: an action's `run`, a form's `submit`.
    Handler { pack: String, target: String, handler: u64, payload: serde_json::Value },
    /// A verb on a value: `open`, `copy`… or `<pack>.verbs.<name>`.
    Verb { id: String, value: serde_json::Value },
    /// Every ability that takes this value.
    SendTo { value: serde_json::Value, ty: serde_json::Value },
    Send { address: String, slot: String, value: serde_json::Value, ready: bool },
    /// The permission screen, and a person's answers on it.
    Review(String),
    Grant(String),
    Revoke(String),
    CopyError(String),
    Undo(u64),
    /// Every pack, its state and what it may do.
    Manage,
    /// Show or hide an overlay, a widget or a window a pack declares.
    Surface { pack: String, name: String },
    /// Shows a pack's board in its own window (`window`) or on the desktop, or hides it there.
    Board { pack: String, name: String, window: bool },
    /// Point and say: note what is on screen for the next agent session.
    Report,
    /// Keep an AI answer's surface as a pack of its own.
    Keep,
    /// Ask before or stop asking before a final ability runs.
    Confirming { address: String, ask: bool },
    Reload,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AiOp {
    Send,
    Stop,
    New,
    Copy,
    /// A quick action on the selected text, by id.
    Quick(String),
    /// Open the chat, asking this first when it isn't empty.
    Open(String),
    /// Ask the last question again.
    Retry,
    /// Pick a saved conversation back up, by id.
    Resume(u64),
    DeleteChat(u64),
    /// Turn a Quick AI answer into a chat that takes follow-ups.
    Continue,
    NewAction,
    EditAction(String),
    DeleteAction(String),
    NewPreset,
    EditPreset(String),
    DeletePreset(String),
    /// A new chat under a preset's instructions.
    UsePreset(String),
    /// Send a file with the next question.
    Attach(PathBuf),
    AttachClipboard,
    ClearAttachments,
    ToggleWeb,
    ToggleTools,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ClipOp {
    Paste,
    Copy,
    Pin,
    Delete,
    ClearAll,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EmojiOp {
    Paste,
    Copy,
    Pin,
}

#[derive(Clone, Debug)]
pub enum FileOp {
    /// Leave a folder, and everything in it, out of Search Files from now on.
    Exclude(PathBuf),
    /// Move a file or folder to the Trash; it leaves the results with it.
    Trash(PathBuf),
}

#[derive(Clone, Debug)]
pub enum SnippetOp {
    New,
    Edit(String),
    Delete(String),
    /// Fill a snippet's placeholders now and paste it, or copy it.
    Paste(String),
    Copy(String),
}

#[derive(Clone, Copy, Debug)]
pub enum BackupOp {
    Export,
    Import,
}

#[derive(Clone, Debug)]
pub enum CalOp {
    Add,
    Refresh,
    /// Stop reading a calendar, by its address.
    Remove(String),
}

#[derive(Clone, Debug)]
pub enum NoteOp {
    /// A new note, its title already typed.
    New(String),
    Edit(String),
    Delete(String),
}

#[derive(Clone, Debug)]
pub enum ScriptOp {
    /// Run by id, asking for its arguments first when it takes any.
    Run(String),
    RunWith(String, Vec<String>),
    New,
}

#[derive(Clone, Debug)]
pub enum QuicklinkOp {
    New,
    Edit(String),
    Delete(String),
    /// Open with the root query as its argument.
    Open(String, String),
}

/// A destructive or outward step asks first.
#[derive(Clone, Debug)]
pub struct Confirm {
    pub title: String,
    pub message: String,
    pub icon: &'static Icon,
    pub confirm: String,
    pub destructive: bool,
    pub act: Act,
}

/// A screen the palette can show.
#[derive(Clone, Debug, PartialEq)]
pub enum Route {
    Root,
    Clipboard,
    Emoji,
    Files,
    Windows,
    /// The snap layout picker: every layout, or the one a layout's own command names.
    Snap(Option<usize>),
    Snippets,
    Quicklinks,
    Notes,
    Schedule,
    Store,
    Extensions,
    Themes,
    Ai,
    /// A page of Markdown: `App::page`.
    Page,
    /// A form: its fields live in `App::form`.
    Form,
    /// A running extension command; the session owns the tree.
    Ext(u64),
    /// A pack's surface: the host draws the blocks it returns.
    Pack { pack: String, target: String, args: serde_json::Value },
    /// Every pack: its state, its permissions, what it runs unasked.
    Packs,
    /// What a pack asks for, to grant or not.
    PackGrants(String),
    /// Send to…: the abilities that take the value in hand.
    PackSend,
}

#[derive(Clone, Debug, Default)]
pub struct Detail {
    /// A picture above the fields.
    pub image: Option<PathBuf>,
    /// A colour swatch above the fields.
    pub swatch: Option<Color>,
    /// Body text, drawn as Markdown.
    pub markdown: Option<String>,
    /// Monospaced body text (a clipboard entry, a file's head).
    pub text: Option<String>,
    pub heading: Option<String>,
    pub fields: Vec<(String, String)>,
}

/// The calculator's lead card: a source column and a result column.
#[derive(Clone, Debug)]
pub struct Card {
    pub left: String,
    pub left_badge: String,
    pub right: String,
    pub right_badge: String,
}

#[derive(Clone, Debug)]
pub struct Accessory {
    /// A small glyph before the text: an upvote arrow, a comment bubble.
    pub icon: Option<&'static Icon>,
    pub text: String,
    pub tag: Option<Color>,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub id: String,
    pub title: String,
    pub subtitle: Option<String>,
    /// The quiet label at the row's trailing edge ("Application", "Command").
    pub trailing: String,
    pub icon: IconSrc,
    pub actions: Vec<Action>,
    pub detail: Option<Detail>,
    pub card: Option<Card>,
    pub accessories: Vec<Accessory>,
    /// A running app: the dot under its icon.
    pub running: bool,
    /// What the row means (`urgent`, `live`, `muted`…): the Look decides how that shows.
    pub intent: String,
    /// A living icon (API-DESIGN.md §14.4): what a pack's `icon` face says to draw on it.
    pub living: Option<Living>,
}

/// State drawn on an icon: a count, how far along something is, and whether it is live.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Living {
    pub badge: Option<String>,
    pub progress: Option<f32>,
    pub pulse: bool,
}

impl Item {
    pub fn new(id: impl Into<String>, title: impl Into<String>, icon: IconSrc) -> Self {
        Self {
            id: id.into(), title: title.into(), subtitle: None, trailing: String::new(), icon,
            actions: Vec::new(), detail: None, card: None, accessories: Vec::new(), running: false, intent: String::new(), living: None,
        }
    }
    pub fn trailing(mut self, s: &str) -> Self {
        self.trailing = s.into();
        self
    }
    pub fn living(mut self, living: Option<Living>) -> Self {
        self.living = living;
        self
    }
    pub fn subtitle(mut self, s: impl Into<String>) -> Self {
        self.subtitle = Some(s.into());
        self
    }
    pub fn action(mut self, a: Action) -> Self {
        self.actions.push(a);
        self
    }
    pub fn actions(mut self, a: impl IntoIterator<Item = Action>) -> Self {
        self.actions.extend(a);
        self
    }
    pub fn detail(mut self, d: Detail) -> Self {
        self.detail = Some(d);
        self
    }
}

#[derive(Clone, Debug)]
pub enum Row {
    Header { title: String, count: Option<usize>, first: bool },
    /// Index into the screen's items.
    Item(usize),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layout {
    List,
    /// A list beside a preview pane.
    Split,
    Grid { columns: usize },
}

#[derive(Default)]
pub struct Sections(pub Vec<(String, Vec<Item>)>);

impl Sections {
    pub fn push(&mut self, title: &str, items: Vec<Item>) {
        if !items.is_empty() {
            self.0.push((title.to_string(), items));
        }
    }
    pub fn flatten(self, counts: bool) -> (Vec<Item>, Vec<Row>) {
        let (mut items, mut rows) = (Vec::new(), Vec::new());
        for (title, group) in self.0 {
            if !title.is_empty() {
                rows.push(Row::Header { title, count: counts.then_some(group.len()), first: rows.is_empty() });
            }
            for item in group {
                rows.push(Row::Item(items.len()));
                items.push(item);
            }
        }
        (items, rows)
    }
}

/// The keycaps a chord prints: `ctrl+shift+c` is `Ctrl`, `⇧`, `C`.
pub fn keycaps(keys: &str) -> Vec<String> {
    keys.split('+').filter(|p| !p.is_empty()).map(|part| match part.trim().to_ascii_lowercase().as_str() {
        "ctrl" | "mod" | "cmd" => "Ctrl".to_string(),
        "shift" => "⇧".to_string(),
        "alt" | "opt" => "Alt".to_string(),
        "super" => "Super".to_string(),
        "enter" | "return" => "↵".to_string(),
        "backspace" => "⌫".to_string(),
        "delete" => "Del".to_string(),
        "tab" => "⇥".to_string(),
        "escape" | "esc" => "esc".to_string(),
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),
        "left" => "←".to_string(),
        "right" => "→".to_string(),
        "space" => "Space".to_string(),
        "page_up" => "PgUp".to_string(),
        "page_down" => "PgDn".to_string(),
        "home" => "Home".to_string(),
        "end" => "End".to_string(),
        other => other.to_uppercase(),
    }).collect()
}

/// Whether a key event is the chord `keys` names.
pub fn chord_matches(keys: &str, ev: &KeyEvent) -> bool {
    if keys.is_empty() {
        return false;
    }
    let (mut ctrl, mut shift, mut alt, mut name) = (false, false, false, String::new());
    for part in keys.split('+') {
        match part.trim().to_ascii_lowercase().as_str() {
            "ctrl" | "mod" | "cmd" => ctrl = true,
            "shift" => shift = true,
            "alt" | "opt" => alt = true,
            other => name = other.to_string(),
        }
    }
    if ev.mods.cmd != ctrl || ev.mods.shift != shift || ev.mods.alt != alt {
        return false;
    }
    match (&ev.key, name.as_str()) {
        (Key::Enter, "enter" | "return") | (Key::Backspace, "backspace") | (Key::Delete, "delete") => true,
        (Key::Tab, "tab") | (Key::Escape, "escape" | "esc") => true,
        (Key::Up, "up") | (Key::Down, "down") | (Key::Left, "left") | (Key::Right, "right") => true,
        (Key::Command(c), n) => n.chars().count() == 1 && n.chars().next() == Some(c.to_ascii_lowercase()),
        (Key::Text(t), n) => !ctrl && t.to_lowercase() == n,
        _ => false,
    }
}

/// The glyph a Raycast or config icon name maps to.
pub fn glyph(name: &str) -> &'static Icon {
    icons::by_name(name).unwrap_or(&icons::CIRCLE)
}
