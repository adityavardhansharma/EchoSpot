//! Raycast extensions. Each running command is a Node process (the bundled runtime) that renders
//! its React tree to JSON; this module installs extensions, starts and stops those processes,
//! and turns their trees into the palette's own rows, details and forms.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};

use rune::{Tx, Waker};
use serde_json::{Value, json};

use crate::app::{App, Tone};
use crate::features::forms::{self, Field, FieldKind, Form, Target};
use crate::icons;
use crate::model::*;
use crate::util;

pub mod net;
pub mod store;
pub mod view;

/// The runtime, carried in the binary and unpacked on first use.
static HOST: &str = include_str!("../../runtime/dist/host.cjs");

#[derive(Clone, Debug)]
pub enum ExtAct {
    /// Start a command: extension directory name, command name.
    Run(String, String),
    /// Call a handler in a running command.
    Event { session: u64, handler: String, args: Vec<Value> },
    Install(Box<store::Listing>),
    InstallDemo,
    Uninstall(String),
    Configure(String),
    Trash { session: u64, paths: Vec<String>, handler: Option<String> },
}

#[derive(Clone, Debug, Default)]
pub struct Preference {
    pub name: String,
    pub title: String,
    pub description: String,
    pub kind: String,
    pub required: bool,
    pub default: Option<Value>,
    pub label: String,
    pub placeholder: String,
    pub options: Vec<(String, String)>,
}

#[derive(Clone, Debug, Default)]
pub struct CommandInfo {
    pub name: String,
    pub title: String,
    pub subtitle: String,
    pub description: String,
    pub mode: String,
    pub preferences: Vec<Preference>,
    pub icon: Option<PathBuf>,
}

#[derive(Clone, Debug, Default)]
pub struct Installed {
    /// Directory name under the extensions folder; unique.
    pub id: String,
    pub name: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub dir: PathBuf,
    pub icon: Option<PathBuf>,
    pub commands: Vec<CommandInfo>,
    pub preferences: Vec<Preference>,
}

pub struct Session {
    pub id: u64,
    pub extension: String,
    pub command: String,
    pub title: String,
    pub view: bool,
    child: Child,
    stdin: Option<ChildStdin>,
    /// The newest tree and how deep the command's own navigation is.
    pub tree: Value,
    pub depth: u64,
    pub ready: bool,
    /// The last text sent to `onSearchTextChange`, so a refresh doesn't resend it.
    sent_search: String,
    hud: Option<String>,
    /// Whether the search bar's dropdown has announced its starting value.
    dropdown_ready: bool,
}

impl Session {
    fn send(&mut self, message: Value) {
        if let Some(stdin) = &mut self.stdin {
            let _ = writeln!(stdin, "{message}");
            let _ = stdin.flush();
        }
    }
    /// The root view node: List, Grid, Detail or Form.
    pub fn root(&self) -> Option<&Value> {
        self.tree.get("children")?.as_array()?.first()
    }
}

/// A command waiting for its person to finish signing in, in the browser.
struct SignIn {
    session: u64,
    call: u64,
    state: String,
    provider: String,
    since: u64,
}

/// How long a command is kept alive for a sign-in nobody finishes.
const SIGN_IN_SECS: u64 = 15 * 60;

pub struct Extensions {
    pub installed: Vec<Installed>,
    pub sessions: Vec<Session>,
    sign_ins: Vec<SignIn>,
    tx: Sender<(u64, Value)>,
    rx: Receiver<(u64, Value)>,
    waker: Option<Waker>,
    next_id: u64,
    pub store: store::Store,
}

pub fn extensions_dir() -> PathBuf {
    let dir = util::data_dir().join("extensions");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn prefs_path(name: &str) -> PathBuf {
    let dir = util::config_dir().join("extensions");
    let _ = std::fs::create_dir_all(&dir);
    dir.join(format!("{name}.json"))
}

fn read_prefs(name: &str) -> HashMap<String, Value> {
    util::read_json(&prefs_path(name))
}

fn host_path() -> Option<PathBuf> {
    let path = util::data_dir().join("runtime").join("host.cjs");
    let current = std::fs::metadata(&path).map(|m| m.len()).ok();
    if current != Some(HOST.len() as u64) {
        std::fs::create_dir_all(path.parent()?).ok()?;
        std::fs::write(&path, HOST).ok()?;
    }
    Some(path)
}

fn parse_preferences(value: Option<&Value>) -> Vec<Preference> {
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    value.and_then(Value::as_array).map(|list| list.iter().map(|p| Preference {
        name: s(p, "name"), title: s(p, "title"), description: s(p, "description"), kind: s(p, "type"),
        required: p.get("required").and_then(Value::as_bool).unwrap_or(false),
        default: p.get("default").cloned(), label: s(p, "label"), placeholder: s(p, "placeholder"),
        options: p.get("data").and_then(Value::as_array).map(|d| d.iter().map(|o| (s(o, "value"), s(o, "title"))).collect()).unwrap_or_default(),
    }).collect()).unwrap_or_default()
}

fn asset(dir: &Path, name: &str) -> Option<PathBuf> {
    if name.is_empty() {
        return None;
    }
    [dir.join("assets").join(name), dir.join(name)].into_iter().find(|p| p.is_file())
}

fn read_manifest(dir: &Path) -> Option<Installed> {
    let manifest: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("package.json")).ok()?).ok()?;
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let icon = asset(dir, &s(&manifest, "icon"));
    let commands = manifest.get("commands")?.as_array()?.iter()
        .filter(|c| dir.join(format!("{}.js", s(c, "name"))).is_file())
        .filter(|c| matches!(s(c, "mode").as_str(), "view" | "no-view"))
        .map(|c| CommandInfo {
            name: s(c, "name"), title: s(c, "title"), subtitle: s(c, "subtitle"), description: s(c, "description"), mode: s(c, "mode"),
            preferences: parse_preferences(c.get("preferences")), icon: asset(dir, &s(c, "icon")).or_else(|| icon.clone()),
        }).collect();
    Some(Installed {
        id: dir.file_name()?.to_string_lossy().to_string(),
        name: s(&manifest, "name"),
        title: s(&manifest, "title"),
        description: s(&manifest, "description"),
        author: manifest.get("owner").or(manifest.get("author")).and_then(Value::as_str).unwrap_or("").to_string(),
        dir: dir.to_path_buf(),
        icon,
        commands,
        preferences: parse_preferences(manifest.get("preferences")),
    })
}

impl Extensions {
    pub fn load() -> Self {
        let (tx, rx) = channel();
        let mut ext = Self { installed: Vec::new(), sessions: Vec::new(), sign_ins: Vec::new(), tx, rx, waker: None, next_id: 1, store: store::Store::default() };
        ext.rescan();
        ext
    }

    pub fn configure(&mut self, waker: Waker) {
        net::set_waker(waker.clone());
        self.store.configure(waker.clone());
        self.waker = Some(waker);
    }

    pub fn rescan(&mut self) {
        let mut found: Vec<Installed> = std::fs::read_dir(extensions_dir()).into_iter().flatten().flatten()
            .filter_map(|e| read_manifest(&e.path())).collect();
        found.sort_by_key(|e| e.title.to_lowercase());
        self.installed = found;
    }

    pub fn get(&self, id: &str) -> Option<&Installed> {
        self.installed.iter().find(|e| e.id == id)
    }

    pub fn session(&self, id: u64) -> Option<&Session> {
        self.sessions.iter().find(|s| s.id == id)
    }
    pub fn session_mut(&mut self, id: u64) -> Option<&mut Session> {
        self.sessions.iter_mut().find(|s| s.id == id)
    }

    /// Asks the command to go back one of its own views; false when it is at its first.
    pub fn pop_view(&mut self, id: u64) -> bool {
        match self.session_mut(id) {
            Some(session) if session.depth > 1 => {
                session.send(json!({ "t": "pop" }));
                session.depth -= 1;
                true
            }
            _ => false,
        }
    }

    /// Whether a command is waiting on a browser sign-in, and so should outlive the palette
    /// hiding behind that browser.
    pub fn awaiting_sign_in(&mut self, id: u64) -> bool {
        let now = util::now_secs();
        let stale: Vec<u64> = self.sign_ins.iter().filter(|s| now.saturating_sub(s.since) > SIGN_IN_SECS).map(|s| s.session).collect();
        for session in stale {
            self.stop(session);
        }
        self.sign_ins.iter().any(|s| s.session == id)
    }

    pub fn stop(&mut self, id: u64) {
        self.sign_ins.retain(|s| s.session != id);
        if let Some(at) = self.sessions.iter().position(|s| s.id == id) {
            let mut session = self.sessions.remove(at);
            session.send(json!({ "t": "quit" }));
            session.stdin = None;
            let _ = session.child.kill();
            std::thread::spawn(move || {
                let _ = session.child.wait();
            });
        }
    }

    pub fn event(&mut self, id: u64, handler: &str, args: Vec<Value>) {
        if let Some(session) = self.session_mut(id) {
            session.send(json!({ "t": "event", "handler": handler, "args": args }));
        }
    }

    fn start(&mut self, ext: &Installed, command: &CommandInfo, launch: Value) -> Result<u64, String> {
        let host = host_path().ok_or("Could not unpack the extension runtime.")?;
        if !util::has_program("node") {
            return Err("Node.js is needed to run extensions.".into());
        }
        let mut child = Command::new("node")
            .arg(&host).arg(&ext.dir).arg(&command.name).arg(launch.to_string())
            .current_dir(&ext.dir)
            .env("NODE_NO_WARNINGS", "1")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .spawn().map_err(|e| format!("Could not start Node: {e}"))?;
        let id = self.next_id;
        self.next_id += 1;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let stderr = child.stderr.take().ok_or("no stderr")?;
        let (tx, waker) = (self.tx.clone(), self.waker.clone());
        std::thread::Builder::new().name("echospot-ext".into()).spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                let Some(body) = line.strip_prefix('\u{1e}') else { continue };
                if let Ok(value) = serde_json::from_str::<Value>(body) {
                    let _ = tx.send((id, value));
                    if let Some(waker) = &waker {
                        waker();
                    }
                }
            }
            // The process ended, however it ended.
            let _ = tx.send((id, json!({ "t": "closed" })));
            if let Some(waker) = &waker {
                waker();
            }
        }).map_err(|e| e.to_string())?;
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let _ = tx.send((id, json!({ "t": "log", "level": "error", "text": line })));
            }
        });
        let stdin = child.stdin.take();
        self.sessions.push(Session {
            id, extension: ext.id.clone(), command: command.name.clone(), title: command.title.clone(),
            view: command.mode == "view", child, stdin, tree: Value::Null, depth: 1, ready: false,
            sent_search: String::new(), hud: None, dropdown_ready: false,
        });
        Ok(id)
    }
}

fn log_line(extension: &str, level: &str, text: &str) {
    let dir = util::data_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join(format!("{extension}.log"))) {
        let _ = writeln!(file, "[{}] {level}: {text}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
    }
}

/// The glyph nearest a Raycast icon name such as `star-16` or `arrow-clockwise-16`.
pub fn raycast_glyph(name: &str) -> &'static rune::Icon {
    let n = name.trim_end_matches("-16").trim_end_matches("-filled").to_lowercase();
    const TABLE: &[(&str, &rune::Icon)] = &[
        ("arrow-up-circle", &icons::ARROW_UP), ("arrow-up", &icons::ARROW_UP), ("arrow-down", &icons::DOWNLOAD), ("arrow-left", &icons::ARROW_LEFT),
        ("speech", &icons::CHAT), ("bubble", &icons::CHAT), ("message", &icons::CHAT), ("comment", &icons::CHAT),
        ("magnifying", &icons::SEARCH), ("search", &icons::SEARCH), ("clipboard", &icons::CLIPBOARD), ("copy", &icons::COPY),
        ("trash", &icons::TRASH), ("star", &icons::STAR), ("heart", &icons::HEART), ("gear", &icons::GEAR), ("cog", &icons::GEAR),
        ("globe", &icons::GLOBE), ("link", &icons::LINK), ("eye", &icons::EYE), ("pencil", &icons::PENCIL), ("edit", &icons::PENCIL),
        ("folder", &icons::FOLDER), ("finder", &icons::FOLDER), ("document", &icons::FILE_TEXT), ("doc", &icons::FILE_TEXT), ("text", &icons::TEXT),
        ("terminal", &icons::TERMINAL), ("code", &icons::CODE), ("window", &icons::WINDOW), ("sidebar", &icons::SIDEBAR),
        ("app-window", &icons::WINDOW), ("image", &icons::IMAGE), ("photo", &icons::IMAGE), ("camera", &icons::CAMERA),
        ("clock", &icons::CLOCK), ("calendar", &icons::CLOCK), ("alarm", &icons::BELL), ("bell", &icons::BELL),
        ("person", &icons::PERSON), ("people", &icons::PERSON), ("two-people", &icons::PERSON), ("lock", &icons::LOCK), ("key", &icons::KEY),
        ("download", &icons::DOWNLOAD), ("upload", &icons::UPLOAD), ("arrow-clockwise", &icons::REFRESH), ("repeat", &icons::REFRESH),
        ("rotate", &icons::REFRESH), ("arrow-right", &icons::ARROW_RIGHT), ("arrow-ne", &icons::ARROW_UP_RIGHT), ("arrow-up-right", &icons::ARROW_UP_RIGHT),
        ("chevron-right", &icons::CHEVRON_RIGHT), ("chevron-down", &icons::CHEVRON_DOWN), ("chevron-left", &icons::CHEVRON_LEFT),
        ("check", &icons::CHECK), ("xmark", &icons::X), ("multiply", &icons::X), ("plus", &icons::PLUS), ("add", &icons::PLUS),
        ("info", &icons::INFO), ("question", &icons::INFO), ("warning", &icons::ALERT), ("exclamation", &icons::ALERT), ("bug", &icons::ALERT),
        ("bolt", &icons::BOLT), ("lightning", &icons::BOLT), ("play", &icons::PLAY), ("pause", &icons::PAUSE), ("forward", &icons::SKIP),
        ("music", &icons::MUSIC), ("speaker", &icons::VOLUME), ("microphone", &icons::MIC), ("mic", &icons::MIC), ("moon", &icons::MOON),
        ("sun", &icons::SUN), ("power", &icons::POWER), ("logout", &icons::LOGOUT), ("wifi", &icons::WIFI), ("bluetooth", &icons::BLUETOOTH),
        ("monitor", &icons::DISPLAY), ("desktop", &icons::DISPLAY), ("computer", &icons::DISPLAY), ("keyboard", &icons::KEYBOARD),
        ("tag", &icons::TAG), ("hashtag", &icons::HASH), ("number", &icons::HASH), ("box", &icons::BOX), ("shipping", &icons::BOX),
        ("store", &icons::STORE), ("cart", &icons::STORE), ("house", &icons::HOME), ("home", &icons::HOME), ("list", &icons::LIST),
        ("bullet", &icons::LIST), ("grid", &icons::GRID), ("app", &icons::GRID), ("filter", &icons::FILTER), ("paint", &icons::PALETTE),
        ("brush", &icons::PALETTE), ("swatch", &icons::PALETTE), ("emoji", &icons::SMILE), ("smiley", &icons::SMILE), ("book", &icons::BOOK),
        ("calculator", &icons::CALCULATOR), ("pin", &icons::PIN), ("tack", &icons::PIN), ("share", &icons::SHARE), ("airplane", &icons::SHARE),
        ("stars", &icons::SPARKLES), ("wand", &icons::SPARKLES), ("battery", &icons::BATTERY), ("ellipsis", &icons::ELLIPSIS),
        ("dot", &icons::DOT), ("circle", &icons::CIRCLE), ("undo", &icons::UNDO), ("maximize", &icons::MAXIMIZE), ("message", &icons::TEXT),
        ("bubble", &icons::TEXT), ("envelope", &icons::TEXT), ("save", &icons::DOWNLOAD), ("hammer", &icons::GEAR), ("wrench", &icons::GEAR),
        ("dice", &icons::DICE), ("game", &icons::DICE), ("puzzle", &icons::PUZZLE), ("plug", &icons::PUZZLE),
    ];
    TABLE.iter().find(|(key, _)| n.contains(key)).map(|(_, glyph)| *glyph).unwrap_or(&icons::CIRCLE)
}

pub fn raycast_color(value: &Value, p_dark: bool) -> Option<rune::Color> {
    let name = match value {
        Value::String(s) => s.clone(),
        Value::Object(o) => o.get(if p_dark { "dark" } else { "light" }).and_then(Value::as_str)?.to_string(),
        _ => return None,
    };
    let hex = match name.as_str() {
        "raycast-blue" => "#56c2ff",
        "raycast-green" => "#59d499",
        "raycast-magenta" => "#cf2f98",
        "raycast-orange" => "#f8a300",
        "raycast-purple" => "#a067dc",
        "raycast-red" => "#f84e4e",
        "raycast-yellow" => "#eed535",
        "raycast-primary-text" => "#ffffff",
        "raycast-secondary-text" => "#a0a0a0",
        other => other,
    };
    rune::color::hex(hex)
}

/// An inline SVG that is a coloured box around a few characters (a rank badge, an avatar's
/// initials), as the tile the palette would draw itself. The rasterizer sets no text, so the
/// characters would otherwise be lost.
fn svg_monogram(uri: &str) -> Option<IconSrc> {
    let svg = uri.strip_prefix("data:image/svg+xml")?.split_once(',')?.1;
    let svg = if svg.contains("%3C") { String::from_utf8_lossy(&net::percent_decode(svg)).to_string() } else { svg.to_string() };
    let label = svg.split_once("<text")?.1.split_once('>')?.1.split_once("</text>")?.0.trim().to_string();
    if label.is_empty() || label.chars().count() > 3 || label.contains('<') {
        return None;
    }
    let attr = |tag: &str, name: &str| -> Option<String> {
        let body = svg.split_once(tag)?.1.split_once('>')?.0.to_string();
        let value = body.split_once(&format!("{name}=\""))?.1.split_once('"')?.0.to_string();
        Some(value)
    };
    let fill = attr("<rect", "fill").or_else(|| attr("<circle", "fill")).and_then(|f| rune::color::hex(&f))?;
    Some(IconSrc::Monogram(label, fill))
}

/// A Raycast `Image.ImageLike` as something the palette draws.
pub fn icon_from(value: &Value, dir: &Path) -> IconSrc {
    match value {
        Value::String(s) if s.is_empty() => IconSrc::None,
        Value::String(s) if s.ends_with("-16") && !s.contains('/') && !s.contains('.') => IconSrc::Glyph(raycast_glyph(s)),
        Value::String(s) if s.starts_with("http://") || s.starts_with("https://") => match net::cached(s) {
            Some(path) => IconSrc::File(path),
            None => IconSrc::Glyph(&icons::IMAGE),
        },
        Value::String(s) if s.starts_with("data:") => svg_monogram(s)
            .or_else(|| net::data_uri(s).map(IconSrc::File)).unwrap_or(IconSrc::Glyph(&icons::IMAGE)),
        Value::String(s) => {
            if let Some(path) = asset(dir, s).or_else(|| Path::new(s).is_file().then(|| PathBuf::from(s))) {
                return IconSrc::File(path);
            }
            // A short string that is no file is an emoji.
            if s.chars().count() <= 4 && !s.is_ascii() { IconSrc::Emoji(s.clone()) } else { IconSrc::Glyph(&icons::CIRCLE) }
        }
        Value::Object(o) => {
            if let Some(source) = o.get("source") {
                let source = match source {
                    Value::Object(themed) => themed.get("dark").or(themed.get("light")).cloned().unwrap_or(Value::Null),
                    other => other.clone(),
                };
                let tint = o.get("tintColor").and_then(|c| raycast_color(c, true));
                return match (icon_from(&source, dir), tint) {
                    (IconSrc::Glyph(g), Some(c)) => IconSrc::Tinted(g, c.alpha(0.9)),
                    (icon, _) => icon,
                };
            }
            if let Some(file) = o.get("fileIcon").and_then(Value::as_str) {
                return IconSrc::Glyph(if Path::new(file).is_dir() { &icons::FOLDER } else { &icons::FILE });
            }
            if let Some(value) = o.get("value") {
                return icon_from(value, dir);
            }
            IconSrc::None
        }
        _ => IconSrc::None,
    }
}

fn shortcut_keys(value: Option<&Value>) -> String {
    let Some(value) = value else { return String::new() };
    // A per-platform shortcut: Windows' is written for a Ctrl keyboard.
    let value = value.get("Windows").or(value.get("windows")).or(value.get("macOS")).unwrap_or(value);
    let Some(key) = value.get("key").and_then(Value::as_str) else { return String::new() };
    let mut parts: Vec<String> = Vec::new();
    for m in value.get("modifiers").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str) {
        let part = match m {
            "cmd" | "ctrl" => "ctrl",
            "shift" => "shift",
            "opt" | "alt" => "alt",
            _ => continue,
        };
        if !parts.iter().any(|p| p == part) {
            parts.push(part.to_string());
        }
    }
    parts.push(match key {
        "return" | "enter" => "enter".into(),
        "arrowUp" => "up".into(),
        "arrowDown" => "down".into(),
        "arrowLeft" => "left".into(),
        "arrowRight" => "right".into(),
        "deleteForward" => "delete".into(),
        "delete" | "backspace" => "backspace".into(),
        other => other.to_lowercase(),
    });
    let keys = parts.join("+");
    // The palette's own chords stay the palette's.
    if matches!(keys.as_str(), "ctrl+k" | "ctrl+w" | "ctrl+q" | "ctrl+n" | "ctrl+p" | "enter") { String::new() } else { keys }
}

fn text_of(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Object(o)) => text_of(o.get("value").or(o.get("text"))),
        _ => String::new(),
    }
}

fn handler(value: Option<&Value>) -> Option<String> {
    value?.get("$fn")?.as_str().map(String::from)
}

/// An `ActionPanel` as the palette's actions, sections and submenus flattened.
pub fn actions_from(app: &App, session: u64, panel: Option<&Value>, dir: &Path) -> Vec<Action> {
    let mut out = Vec::new();
    fn walk(app: &App, session: u64, node: &Value, dir: &Path, section: Option<&str>, prefix: &str, out: &mut Vec<Action>) {
        let kind = node.get("type").and_then(Value::as_str).unwrap_or("");
        let props = node.get("props").cloned().unwrap_or(Value::Null);
        let children = node.get("children").and_then(Value::as_array).cloned().unwrap_or_default();
        match kind {
            "ActionPanel" => children.iter().for_each(|c| walk(app, session, c, dir, section, prefix, out)),
            "ActionPanel.Section" => {
                let title = props.get("title").and_then(Value::as_str).unwrap_or("");
                let mut first = true;
                for c in &children {
                    let before = out.len();
                    walk(app, session, c, dir, if first { Some(title) } else { None }, prefix, out);
                    first = first && out.len() == before;
                }
            }
            "ActionPanel.Submenu" => {
                let title = text_of(props.get("title"));
                children.iter().for_each(|c| walk(app, session, c, dir, section, &format!("{prefix}{title} › "), out));
            }
            "Action" => {
                let typed = props.get("kind").and_then(Value::as_str).unwrap_or("");
                let on_action = handler(props.get("onAction"));
                let event = |h: Option<String>| h.map(|handler| Act::Ext(ExtAct::Event { session, handler, args: vec![] }));
                let content = text_of(props.get("content"));
                let (default_title, default_icon, act): (String, &'static rune::Icon, Act) = match typed {
                    "CopyToClipboard" => ("Copy to Clipboard".into(), &icons::COPY, Act::Copy(content)),
                    "Paste" => (app.paste_target(), &icons::CLIPBOARD, Act::Paste(content)),
                    "OpenInBrowser" => ("Open in Browser".into(), &icons::GLOBE, Act::OpenUrl(text_of(props.get("url")))),
                    "Open" => {
                        let target = text_of(props.get("target"));
                        ("Open".into(), &icons::ARROW_UP_RIGHT, if target.contains("://") { Act::OpenUrl(target) } else { Act::OpenPath(target.into()) })
                    }
                    "ShowInFinder" => ("Show in File Manager".into(), &icons::FOLDER, Act::Reveal(text_of(props.get("path")).into())),
                    "SubmitForm" => ("Submit Form".into(), &icons::CHECK, match handler(props.get("onSubmit")) {
                        Some(h) => Act::Ext(ExtAct::Event { session, handler: h, args: vec![json!("$form")] }),
                        None => Act::None,
                    }),
                    "Trash" => {
                        let paths = match props.get("paths") {
                            Some(Value::Array(a)) => a.iter().filter_map(Value::as_str).map(String::from).collect(),
                            Some(Value::String(s)) => vec![s.clone()],
                            _ => vec![],
                        };
                        ("Move to Trash".into(), &icons::TRASH, Act::Ext(ExtAct::Trash { session, paths, handler: handler(props.get("onTrash")) }))
                    }
                    "Push" => ("Open".into(), &icons::ARROW_RIGHT, event(on_action.clone()).unwrap_or(Act::None)),
                    "QuickLook" | "Unsupported" => return,
                    _ => ("Action".into(), &icons::BOLT, event(on_action.clone()).unwrap_or(Act::None)),
                };
                let title = match text_of(props.get("title")) {
                    t if t.is_empty() => default_title,
                    t => t,
                };
                let icon = match props.get("icon").map(|i| icon_from(i, dir)) {
                    Some(IconSrc::Glyph(g)) | Some(IconSrc::Tinted(g, _)) => g,
                    _ => default_icon,
                };
                let mut action = Action::new(format!("{prefix}{title}"), icon, act).keys(&shortcut_keys(props.get("shortcut")));
                action.destructive = props.get("style").and_then(Value::as_str) == Some("destructive");
                if let Some(s) = section {
                    action.section = Some(s.to_string());
                }
                out.push(action);
            }
            _ => {}
        }
    }
    if let Some(panel) = panel {
        walk(app, session, panel, dir, None, "", &mut out);
    }
    if let Some(first) = out.first_mut() {
        first.keys.clear();
    }
    out
}

/// Rows for the launcher: every installed command.
pub fn commands(app: &App) -> Vec<(Item, String)> {
    let mut out = Vec::new();
    for ext in &app.ext.installed {
        for cmd in &ext.commands {
            let id = format!("ext:{}:{}", ext.id, cmd.name);
            let icon = cmd.icon.clone().map(IconSrc::File).unwrap_or(IconSrc::Glyph(&icons::PUZZLE));
            let fav = if app.usage.is_favorite(&id) { "Remove from Favorites" } else { "Add to Favorites" };
            let mut item = Item::new(id.clone(), cmd.title.clone(), icon).trailing(&ext.title)
                .action(Action::new("Open Command", &icons::ARROW_UP_RIGHT, Act::Ext(ExtAct::Run(ext.id.clone(), cmd.name.clone()))))
                .action(Action::new(fav, &icons::STAR, Act::Favorite(id.clone())).keys("ctrl+shift+f"))
                .action(Action::new("Configure Extension", &icons::GEAR, Act::Ext(ExtAct::Configure(ext.id.clone()))).section(""))
                .action(Action::new("Uninstall Extension", &icons::TRASH, Act::Confirm(Box::new(Confirm {
                    title: format!("Uninstall {}?", ext.title), message: "Its commands and saved preferences are removed.".into(),
                    icon: &icons::TRASH, confirm: "Uninstall".into(), destructive: true, act: Act::Ext(ExtAct::Uninstall(ext.id.clone())),
                }))).destructive());
            if !cmd.subtitle.is_empty() {
                item.subtitle = Some(cmd.subtitle.clone());
            }
            out.push((item, format!("{} {} {}", ext.title, cmd.description, ext.name)));
        }
    }
    out
}

fn preference_form(ext: &Installed, command: Option<&CommandInfo>, then: Option<String>) -> Form {
    let saved = read_prefs(&ext.name);
    let mut fields = Vec::new();
    let scoped = ext.preferences.iter().map(|p| (p, String::new()))
        .chain(command.into_iter().flat_map(|c| c.preferences.iter().map(move |p| (p, format!("{}.", c.name)))));
    for (pref, scope) in scoped {
        let key = format!("{scope}{}", pref.name);
        let current = saved.get(&key).or(pref.default.as_ref());
        let as_text = current.map(|v| v.as_str().map(String::from).unwrap_or_else(|| v.to_string())).unwrap_or_default();
        let mut field = match pref.kind.as_str() {
            "password" => Field::new(&key, &pref.title, FieldKind::Password).value(&as_text),
            "checkbox" => {
                let mut f = Field::new(&key, &pref.title, FieldKind::Checkbox(if pref.label.is_empty() { pref.title.clone() } else { pref.label.clone() }));
                f.checked = current.is_some_and(|v| v.as_bool().unwrap_or(v.as_str() == Some("true")));
                f
            }
            "dropdown" => Field::new(&key, &pref.title, FieldKind::Dropdown(pref.options.clone())).value(&as_text),
            _ => Field::new(&key, &pref.title, FieldKind::Text).value(&as_text),
        };
        field.placeholder = pref.placeholder.clone();
        field.info = pref.description.clone();
        field.required = pref.required;
        fields.push(field);
    }
    if fields.is_empty() {
        let mut note = Field::new("note", "", FieldKind::Description);
        note.edit = rune::TextEdit::new("This extension has no preferences.");
        fields.push(note);
    }
    Form::new(&format!("Configure {}", ext.title), if then.is_some() { "Save and Continue" } else { "Save Preferences" }, fields,
        Target::ExtPreferences { extension: ext.id.clone(), then })
}

/// Required preferences that have no value yet.
fn missing_preferences(ext: &Installed, command: &CommandInfo) -> bool {
    let saved = read_prefs(&ext.name);
    let unset = |pref: &Preference, key: String| pref.required && pref.default.is_none()
        && saved.get(&key).is_none_or(|v| v.as_str().is_some_and(|s| s.trim().is_empty()));
    ext.preferences.iter().any(|p| unset(p, p.name.clone()))
        || command.preferences.iter().any(|p| unset(p, format!("{}.{}", command.name, p.name)))
}

pub fn save_preferences(app: &mut App, extension: &str, then: Option<String>, tx: &mut Tx) {
    let (Some(form), Some(ext)) = (&app.form, app.ext.get(extension).cloned()) else { return };
    let mut saved = read_prefs(&ext.name);
    for field in form.fields.iter().filter(|f| f.focusable()) {
        let value = match &field.kind {
            FieldKind::Checkbox(_) => json!(field.checked),
            _ => json!(field.text()),
        };
        saved.insert(field.id.clone(), value);
    }
    let path = prefs_path(&ext.name);
    util::write_json(&path, &saved);
    // Preferences can hold API keys: readable by this user only.
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    app.pop(tx);
    match then {
        Some(command) => perform(app, ExtAct::Run(ext.id, command), tx),
        None => app.toast("Preferences Saved", Tone::Success),
    }
}

/// The values of the form on screen, as `onSubmit` receives them.
fn form_values(form: &Form) -> Value {
    let mut values = serde_json::Map::new();
    for field in form.fields.iter().filter(|f| f.focusable()) {
        values.insert(field.id.clone(), match &field.kind {
            FieldKind::Checkbox(_) => json!(field.checked),
            _ => json!(field.text()),
        });
    }
    Value::Object(values)
}

pub fn submit_form(app: &mut App, session: u64, tx: &mut Tx) {
    let Some(act) = view::form_actions(app, session).into_iter().next().map(|a| a.act) else { return };
    app.perform(act, tx);
}

pub fn open_form_actions(app: &mut App, session: u64) {
    let entries = view::form_actions(app, session);
    if entries.len() > 1 {
        let mut query = rune::TextEdit::new("");
        query.focused = true;
        app.menu = Some(crate::app::Menu { kind: crate::app::MenuKind::Actions, query, sel: 0, entries });
    }
}

pub fn perform(app: &mut App, act: ExtAct, tx: &mut Tx) {
    match act {
        ExtAct::Run(ext_id, command) => {
            let Some(ext) = app.ext.get(&ext_id).cloned() else { return };
            let Some(cmd) = ext.commands.iter().find(|c| c.name == command).cloned() else { return };
            if missing_preferences(&ext, &cmd) {
                return forms::open(app, preference_form(&ext, Some(&cmd), Some(cmd.name.clone())), tx);
            }
            app.usage.record(&format!("ext:{}:{}", ext.id, cmd.name));
            let frontmost = app.prev_window.as_ref().map(|w| json!({ "name": w.class, "bundleId": w.class })).unwrap_or(Value::Null);
            let launch = json!({ "appearance": "dark", "frontmost": frontmost, "arguments": {} });
            match app.ext.start(&ext, &cmd, launch) {
                Ok(id) if cmd.mode == "view" => app.push(Route::Ext(id), tx),
                Ok(_) => app.toast(format!("Running {}…", cmd.title), Tone::Progress),
                Err(error) => app.toast(error, Tone::Danger),
            }
        }
        ExtAct::Event { session, handler, mut args } => {
            // `$form` stands for the values of the form on screen.
            if args.first().and_then(Value::as_str) == Some("$form") {
                let Some(form) = &mut app.form else { return };
                if !form.validate() {
                    return;
                }
                args = vec![form_values(form)];
            }
            app.ext.event(session, &handler, args);
        }
        ExtAct::Trash { session, paths, handler } => {
            for path in &paths {
                util::sh_ok(&format!("gio trash {}", util::quote(path)));
            }
            if let Some(handler) = handler {
                app.ext.event(session, &handler, vec![json!(paths)]);
            }
            app.toast("Moved to Trash", Tone::Success);
        }
        ExtAct::Install(listing) => store::install(app, *listing),
        ExtAct::InstallDemo => store::install_demo(app),
        ExtAct::Uninstall(id) => {
            if let Some(ext) = app.ext.get(&id).cloned() {
                let _ = std::fs::remove_dir_all(&ext.dir);
                let _ = std::fs::remove_file(prefs_path(&ext.name));
                app.ext.rescan();
                app.refresh();
                app.toast(format!("{} Uninstalled", ext.title), Tone::Success);
            }
        }
        ExtAct::Configure(id) => {
            if let Some(ext) = app.ext.get(&id).cloned() {
                forms::open(app, preference_form(&ext, None, None), tx);
            }
        }
    }
}

/// Lands everything the running commands and the store said since the last frame.
pub fn poll(app: &mut App, tx: &mut Tx) -> bool {
    let mut changed = store::poll(app);
    if net::take_dirty() {
        app.icons.forget(&net::dir());
        changed = true;
        app.refresh_keeping();
    }
    while let Ok((id, message)) = app.ext.rx.try_recv() {
        let kind = message.get("t").and_then(Value::as_str).unwrap_or("").to_string();
        let top_is = matches!(app.screens.iter().rev().find(|s| s.route != Route::Form).map(|s| &s.route), Some(Route::Ext(s)) if *s == id);
        let extension = app.ext.session(id).map(|s| s.extension.clone()).unwrap_or_default();
        match kind.as_str() {
            "tree" => {
                let Some(session) = app.ext.session_mut(id) else { continue };
                session.tree = message.get("tree").cloned().unwrap_or(Value::Null);
                session.depth = message.get("depth").and_then(Value::as_u64).unwrap_or(1);
                session.ready = true;
                init_dropdown(app, id);
                if top_is {
                    view::sync_form(app, id, tx);
                    if app.top().route != Route::Form {
                        app.refresh_keeping();
                    }
                    changed = true;
                }
            }
            "call" => {
                let method = message.get("method").and_then(Value::as_str).unwrap_or("");
                let args = message.get("args").and_then(Value::as_array).cloned().unwrap_or_default();
                let call_id = message.get("id").and_then(Value::as_u64).unwrap_or(0);
                match method {
                    "toast" => {
                        let t = args.first().cloned().unwrap_or(Value::Null);
                        let (title, body) = (text_of(t.get("title")), text_of(t.get("message")));
                        let tone = match t.get("style").and_then(Value::as_str) {
                            Some("FAILURE") => Tone::Danger,
                            Some("ANIMATED") => Tone::Progress,
                            _ => Tone::Success,
                        };
                        let text = if body.is_empty() { title } else { format!("{title} · {body}") };
                        // A spinner stays until the command replaces or hides it.
                        if tone == Tone::Progress { app.toast_progress(text) } else { app.toast(text, tone) }
                    }
                    "hideToast" => app.toast = None,
                    "hud" => {
                        let text = text_of(args.first());
                        if let Some(session) = app.ext.session_mut(id) {
                            session.hud = Some(text.clone());
                        }
                        if app.visible {
                            app.hide(tx);
                        }
                        crate::hud::show(&text, Tone::Success);
                    }
                    "closeMainWindow" => {
                        if top_is && app.visible {
                            // The command keeps running; only the window goes.
                            app.visible = false;
                            tx.close_window();
                        }
                    }
                    "popToRoot" => {
                        if top_is {
                            while app.screens.len() > 1 {
                                let before = app.screens.len();
                                app.pop(tx);
                                if app.screens.len() == before {
                                    break;
                                }
                            }
                        }
                    }
                    "clearSearchBar" => {
                        if top_is {
                            app.top_mut().query.clear();
                            app.refresh();
                        }
                    }
                    "paste" => {
                        if app.visible {
                            app.perform(Act::Close, tx);
                        }
                        util::sh_detached("sleep 0.18; wtype -M ctrl v -m ctrl");
                    }
                    "confirm" => {
                        let c = args.first().cloned().unwrap_or(Value::Null);
                        app.dialog_sel = 1;
                        app.dialog = Some(Confirm {
                            title: text_of(c.get("title")), message: text_of(c.get("message")), icon: &icons::INFO,
                            confirm: text_of(c.get("confirm")), destructive: c.get("destructive").and_then(Value::as_bool).unwrap_or(false),
                            act: Act::None,
                        });
                        app.dialog_reply = Some((id, call_id));
                    }
                    "oauth" => {
                        let o = args.first().cloned().unwrap_or(Value::Null);
                        let (url, provider) = (text_of(o.get("url")), text_of(o.get("provider")));
                        if !url.starts_with("https://") {
                            if let Some(s) = app.ext.session_mut(id) {
                                s.send(json!({ "t": "reply", "id": call_id, "ok": false, "value": "The sign-in address is not an https link." }));
                            }
                            continue;
                        }
                        app.ext.sign_ins.retain(|s| s.session != id);
                        app.ext.sign_ins.push(SignIn { session: id, call: call_id, state: text_of(o.get("state")), provider: provider.clone(), since: util::now_secs() });
                        app.toast_progress(format!("Waiting for {provider} in your browser…"));
                        // The browser comes back through a `raycast://` link; make sure it reaches here.
                        std::thread::spawn(move || {
                            register_link_handler();
                            util::open_url(&url);
                        });
                    }
                    "openPreferences" => {
                        if let Some(ext) = app.ext.get(&extension).cloned() {
                            forms::open(app, preference_form(&ext, None, None), tx);
                        }
                    }
                    "launchCommand" => {
                        let name = args.first().and_then(|o| o.get("name")).and_then(Value::as_str).unwrap_or("").to_string();
                        if !name.is_empty() {
                            perform(app, ExtAct::Run(extension.clone(), name), tx);
                        }
                    }
                    _ => {}
                }
                changed = true;
            }
            "log" => log_line(&extension, message.get("level").and_then(Value::as_str).unwrap_or("log"), &text_of(message.get("text"))),
            "error" => {
                let text = text_of(message.get("text"));
                log_line(&extension, "error", &text);
                app.toast(util::truncate(text.lines().next().unwrap_or("The extension failed"), 90), Tone::Danger);
                changed = true;
            }
            "exit" | "closed" => {
                let Some(session) = app.ext.session(id) else { continue };
                let (was_view, said) = (session.view, session.hud.is_some());
                if was_view && kind == "closed" {
                    // A view command's process died under its screen.
                    if let Some(at) = app.screens.iter().position(|s| s.route == Route::Ext(id)) {
                        app.screens.truncate(at.max(1));
                        app.form = None;
                        app.top_mut().query.focused = true;
                        app.refresh();
                    }
                }
                if !was_view || kind == "closed" {
                    app.ext.stop(id);
                    if !was_view && !said && matches!(app.toast.as_ref().map(|t| t.tone), Some(Tone::Progress)) {
                        app.toast = None;
                    }
                }
                changed = true;
            }
            _ => {}
        }
    }
    changed
}

const LINK_HANDLER: &str = "echospot-url.desktop";

/// Makes EchoSpot what opens `raycast://` links, which is how a provider's sign-in page hands
/// its code back.
fn register_link_handler() {
    let Some(apps) = util::data_dir().parent().map(|d| d.join("applications")) else { return };
    let _ = std::fs::create_dir_all(&apps);
    let exe = if util::has_program("echospot") { "echospot".to_string() } else {
        std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "echospot".into())
    };
    let entry = format!("[Desktop Entry]\nType=Application\nName=EchoSpot Links\nExec={exe} url %u\nNoDisplay=true\nTerminal=false\nMimeType=x-scheme-handler/raycast;x-scheme-handler/com.raycast;\n");
    let file = apps.join(LINK_HANDLER);
    if std::fs::read_to_string(&file).ok().as_deref() != Some(entry.as_str()) {
        let _ = std::fs::write(&file, entry);
    }
    for scheme in ["x-scheme-handler/raycast", "x-scheme-handler/com.raycast"] {
        if util::output("xdg-mime", &["query", "default", scheme]).as_deref() != Some(LINK_HANDLER) {
            let _ = util::output("xdg-mime", &["default", LINK_HANDLER, scheme]);
        }
    }
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let (mut out, mut i) = (Vec::with_capacity(bytes.len()), 0);
    while i < bytes.len() {
        let hex = |at: usize| bytes.get(at).and_then(|b| (*b as char).to_digit(16));
        match (bytes[i], hex(i + 1), hex(i + 2)) {
            (b'%', Some(hi), Some(lo)) => {
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            (b'+', ..) => {
                out.push(b' ');
                i += 1;
            }
            (other, ..) => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The parameters of a `raycast://oauth?…` (or `com.raycast:/oauth?…`) link.
fn oauth_link(url: &str) -> Option<HashMap<String, String>> {
    let rest = url.strip_prefix("raycast://").or_else(|| url.strip_prefix("com.raycast:/"))?;
    let (path, query) = rest.split_once('?')?;
    if path.trim_matches('/') != "oauth" {
        return None;
    }
    Some(query.split('&').filter_map(|pair| pair.split_once('=')).map(|(k, v)| (percent_decode(k), percent_decode(v))).collect())
}

/// A link the system handed EchoSpot: the end of a browser sign-in. True when the palette
/// should come back up to show the command that was waiting.
pub fn open_link(app: &mut App, url: &str, tx: &mut Tx) -> bool {
    let Some(params) = oauth_link(url) else { return false };
    let state = params.get("state").cloned().unwrap_or_default();
    // The redirect page drops a state it cannot read; the newest sign-in is then the one meant.
    let Some(at) = app.ext.sign_ins.iter().position(|s| !state.is_empty() && s.state == state).or(app.ext.sign_ins.len().checked_sub(1)) else { return false };
    let waiting = app.ext.sign_ins.remove(at);
    let reply = match (params.get("code"), params.get("error")) {
        (Some(code), _) if !code.is_empty() => json!({ "t": "reply", "id": waiting.call, "ok": true, "value": { "code": code } }),
        (_, error) => {
            let why = params.get("error_description").or(error).cloned().unwrap_or_else(|| "no code came back".into());
            json!({ "t": "reply", "id": waiting.call, "ok": false, "value": format!("{} sign-in failed: {why}", waiting.provider) })
        }
    };
    let failed = reply["ok"] == json!(false);
    let Some(session) = app.ext.session_mut(waiting.session) else { return false };
    session.send(reply);
    let view = session.view;
    if failed {
        app.toast(format!("{} sign-in failed", waiting.provider), Tone::Danger);
    } else {
        app.toast(format!("Signed in to {}", waiting.provider), Tone::Success);
    }
    if !view {
        return false;
    }
    // The palette hid behind the browser and went back to its root; return to the command.
    if !app.visible {
        app.will_show();
    }
    if !app.screens.iter().any(|s| s.route == Route::Ext(waiting.session)) {
        app.push(Route::Ext(waiting.session), tx);
    }
    true
}

/// Answers a `confirmAlert` the extension is waiting on.
pub fn reply_confirm(app: &mut App, confirmed: bool) {
    if let Some((session, call)) = app.dialog_reply.take() {
        if let Some(s) = app.ext.session_mut(session) {
            s.send(json!({ "t": "reply", "id": call, "ok": true, "value": confirmed }));
        }
    }
}

/// The typed query goes to the command when it searches for itself.
pub fn search_changed(app: &mut App, id: u64, text: &str) {
    let Some(session) = app.ext.session_mut(id) else { return };
    let on_search = session.root().and_then(|r| handler(r.get("props").and_then(|p| p.get("onSearchTextChange"))));
    if let Some(handler) = on_search
        && session.sent_search != text {
        session.sent_search = text.to_string();
        session.send(json!({ "t": "event", "handler": handler, "args": [text] }));
    }
}

fn dropdown_store() -> PathBuf {
    util::config_dir().join("extensions").join("dropdowns.json")
}

/// A search bar dropdown starts on a value (the one it was left on when it stores its value,
/// else its default, else its first item) and tells the command so, as Raycast does on mount.
/// Commands that load their list from `onChange` show nothing until it is called.
fn init_dropdown(app: &mut App, id: u64) {
    let Some(session) = app.ext.session(id) else { return };
    if session.dropdown_ready {
        return;
    }
    let Some(dropdown) = session.root().and_then(|r| r.get("props")).and_then(|p| p.get("searchBarAccessory")).cloned() else { return };
    let dp = dropdown.get("props").cloned().unwrap_or(Value::Null);
    let options = view::dropdown_options(&dropdown);
    if options.is_empty() {
        return;
    }
    let key = format!("{}:{}", session.extension, session.command);
    let stored = dp.get("storeValue").and_then(Value::as_bool).unwrap_or(false)
        .then(|| util::read_json::<HashMap<String, String>>(&dropdown_store()).remove(&key)).flatten();
    let wanted = text_of(dp.get("value")).into_some().or(stored).or(text_of(dp.get("defaultValue")).into_some());
    let index = wanted.and_then(|w| options.iter().position(|(value, _)| *value == w)).unwrap_or(0);
    let on_change = handler(dp.get("onChange"));
    if let Some(session) = app.ext.session_mut(id) {
        session.dropdown_ready = true;
        if let Some(handler) = on_change {
            session.send(json!({ "t": "event", "handler": handler, "args": [options[index].0] }));
        }
    }
    if let Some(screen) = app.screens.iter_mut().find(|s| s.route == Route::Ext(id)) {
        screen.filter = index;
    }
}

trait IntoSome: Sized {
    fn into_some(self) -> Option<Self>;
}
impl IntoSome for String {
    /// `None` for the empty string.
    fn into_some(self) -> Option<String> {
        (!self.is_empty()).then_some(self)
    }
}

/// The header's dropdown changed: tell the command which value is chosen.
pub fn filter_changed(app: &mut App, id: u64, index: usize) {
    let Some(session) = app.ext.session_mut(id) else { return };
    let Some(dropdown) = session.root().and_then(|r| r.get("props")).and_then(|p| p.get("searchBarAccessory")).cloned() else { return };
    let values = view::dropdown_options(&dropdown);
    let dp = dropdown.get("props").cloned().unwrap_or(Value::Null);
    let (Some((value, _)), Some(handler)) = (values.get(index), handler(dp.get("onChange"))) else { return };
    session.send(json!({ "t": "event", "handler": handler, "args": [value] }));
    if dp.get("storeValue").and_then(Value::as_bool).unwrap_or(false) {
        let mut stored: HashMap<String, String> = util::read_json(&dropdown_store());
        stored.insert(format!("{}:{}", session.extension, session.command), value.clone());
        util::write_json(&dropdown_store(), &stored);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sign_in_links() {
        let p = oauth_link("raycast://oauth?code=ab%2Bc%20d&package_name=Extension&state=eyJ4IjoxfQ").unwrap();
        assert_eq!(p["code"], "ab+c d");
        assert_eq!(p["state"], "eyJ4IjoxfQ");
        assert_eq!(oauth_link("com.raycast:/oauth?package_name=Extension&error=access_denied").unwrap()["error"], "access_denied");
        assert!(oauth_link("raycast://extensions/raycast/github/my-issues").is_none());
        assert!(oauth_link("https://example.com/oauth?code=1").is_none());
    }
}
