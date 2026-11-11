//! Clipboard history. `wl-paste --watch` hands every copy to `echospot clip-store`, which files it
//! under `~/.local/share/echospot/clipboard`; the palette reads that store when its screen opens.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use chrono::{Local, TimeZone};
use rune::Tx;
use serde::{Deserialize, Serialize};

use crate::app::{App, Built, Tone};
use crate::features::fuzzy;
use crate::icons;
use crate::model::*;
use crate::theme::Tint;
use crate::util;

const MAX_ENTRIES: usize = 500;
const MAX_TEXT: usize = 1_000_000;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug, Default)]
pub enum Kind {
    #[default]
    Text,
    Link,
    Color,
    Image,
    File,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct Entry {
    pub id: u64,
    pub kind: Kind,
    /// The text itself; for an image, its file name in the store.
    pub text: String,
    pub time: u64,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub dims: Option<(u32, u32)>,
}

#[derive(Default)]
pub struct Clipboard {
    pub entries: Vec<Entry>,
}

fn dir() -> PathBuf {
    let dir = util::data_dir().join("clipboard");
    let _ = std::fs::create_dir_all(dir.join("images"));
    dir
}

fn index_path() -> PathBuf {
    dir().join("index.json")
}

pub fn image_path(name: &str) -> PathBuf {
    dir().join("images").join(name)
}

/// Holds the store's lock for a read-modify-write: two watchers fire for one copy.
fn with_store<R>(f: impl FnOnce(&mut Vec<Entry>) -> R) -> Option<R> {
    let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir().join("lock")).ok()?;
    lock.lock().ok()?;
    let mut entries: Vec<Entry> = util::read_json(&index_path());
    let out = f(&mut entries);
    util::write_json(&index_path(), &entries);
    let _ = lock.unlock();
    Some(out)
}

fn classify(text: &str) -> Kind {
    let t = text.trim();
    let hex = t.strip_prefix('#').unwrap_or("");
    if matches!(hex.len(), 3 | 6 | 8) && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Kind::Color;
    }
    if !t.contains(char::is_whitespace) && (t.starts_with("http://") || t.starts_with("https://")) {
        return Kind::Link;
    }
    if !t.is_empty() && t.lines().all(|l| l.starts_with("file://") || (l.starts_with('/') && Path::new(l).exists())) {
        return Kind::File;
    }
    Kind::Text
}

fn png_dims(bytes: &[u8]) -> Option<(u32, u32)> {
    (bytes.len() > 24 && &bytes[1..4] == b"PNG").then(|| {
        (u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]), u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]))
    })
}

fn source_app() -> String {
    crate::features::windows::active().map(|w| w.class).unwrap_or_default()
}

/// `echospot clip-store [--image]`: reads one copy from stdin and files it.
pub fn store_from_stdin(image: bool) {
    // Password managers mark what they copy; it never reaches the history.
    if matches!(std::env::var("CLIPBOARD_STATE").as_deref(), Ok("sensitive") | Ok("clear") | Ok("nil")) {
        return;
    }
    let mut bytes = Vec::new();
    if std::io::stdin().take(64 * 1024 * 1024).read_to_end(&mut bytes).is_err() || bytes.is_empty() {
        return;
    }
    let now = util::now_secs();
    let id = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(now);
    let entry = if image {
        let name = format!("{id}.png");
        if std::fs::write(image_path(&name), &bytes).is_err() {
            return;
        }
        Entry { id, kind: Kind::Image, text: name, time: now, pinned: false, source: source_app(), size: bytes.len() as u64, dims: png_dims(&bytes) }
    } else {
        // A copied picture also offers markup; the image watcher has the real thing.
        let types = util::output("wl-paste", &["--list-types"]).unwrap_or_default();
        if types.lines().any(|t| t.starts_with("image/")) && !types.lines().any(|t| t == "text/plain" || t.starts_with("text/plain;")) {
            return;
        }
        let Ok(text) = String::from_utf8(bytes) else { return };
        if text.trim().is_empty() || text.len() > MAX_TEXT {
            return;
        }
        Entry { id, kind: classify(&text), size: text.len() as u64, text, time: now, pinned: false, source: source_app(), dims: None }
    };
    with_store(|entries| {
        // The same text copied again moves up instead of repeating.
        let mut entry = entry;
        if entry.kind != Kind::Image
            && let Some(at) = entries.iter().position(|e| e.kind != Kind::Image && e.text == entry.text) {
            entry.pinned = entries.remove(at).pinned;
        }
        entries.insert(0, entry);
        let mut kept = 0;
        entries.retain(|e| {
            kept += 1;
            let keep = e.pinned || kept <= MAX_ENTRIES;
            if !keep && e.kind == Kind::Image {
                let _ = std::fs::remove_file(image_path(&e.text));
            }
            keep
        });
    });
}

/// Starts the two watchers, once per session; they live as long as the palette's process.
pub fn start_watchers() {
    if !util::has_program("wl-paste") {
        return;
    }
    let Ok(exe) = std::env::current_exe() else { return };
    let exe = exe.display().to_string();
    for args in [vec!["--type", "text", "--watch", exe.as_str(), "clip-store"], vec!["--type", "image/png", "--watch", exe.as_str(), "clip-store", "--image"]] {
        let mut cmd = Command::new("wl-paste");
        cmd.args(&args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        // The watchers end with the palette instead of outliving it.
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                unsafe extern "C" {
                    fn prctl(option: i32, arg2: u64, arg3: u64, arg4: u64, arg5: u64) -> i32;
                }
                const PR_SET_PDEATHSIG: i32 = 1;
                const SIGTERM: u64 = 15;
                prctl(PR_SET_PDEATHSIG, SIGTERM, 0, 0, 0);
                Ok(())
            });
        }
        let _ = cmd.spawn();
    }
}

pub fn copy_text(text: &str) {
    let Ok(mut child) = Command::new("wl-copy").stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn() else { return };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    let _ = child.wait();
}

/// Puts a file on the clipboard: a picture as its pixels, anything else as a file reference.
pub fn copy_file(path: &Path) {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let mime = match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        _ => None,
    };
    match (mime, std::fs::File::open(path)) {
        (Some(mime), Ok(file)) => {
            let _ = Command::new("wl-copy").args(["--type", mime]).stdin(file).stdout(Stdio::null()).stderr(Stdio::null()).status();
        }
        _ => {
            let uri = format!("file://{}\n", path.display());
            if let Ok(mut child) = Command::new("wl-copy").args(["--type", "text/uri-list"]).stdin(Stdio::piped()).spawn() {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(uri.as_bytes());
                }
                let _ = child.wait();
            }
        }
    }
}

impl Clipboard {
    pub fn load() -> Self {
        Self { entries: util::read_json(&index_path()) }
    }
    pub fn reload(&mut self) {
        self.entries = util::read_json(&index_path());
    }
}

fn bucket(time: u64) -> &'static str {
    let now = Local::now();
    let Some(then) = Local.timestamp_opt(time as i64, 0).single() else { return "Older" };
    let days = (now.date_naive() - then.date_naive()).num_days();
    match days {
        0 => "Today",
        1 => "Yesterday",
        2..=6 => "This Week",
        7..=30 => "This Month",
        _ => "Older",
    }
}

fn when(time: u64) -> String {
    let Some(then) = Local.timestamp_opt(time as i64, 0).single() else { return String::new() };
    match bucket(time) {
        "Today" => then.format("Today at %H:%M:%S").to_string(),
        "Yesterday" => then.format("Yesterday at %H:%M:%S").to_string(),
        _ => then.format("%-d %B %Y at %H:%M").to_string(),
    }
}

fn parse_color(text: &str) -> Option<rune::Color> {
    let hex = text.trim().strip_prefix('#')?;
    let full = if hex.len() == 3 { hex.chars().flat_map(|c| [c, c]).collect::<String>() } else { hex.chars().take(6).collect() };
    rune::color::hex(&format!("#{full}"))
}

fn item(app: &App, e: &Entry) -> Item {
    let first_line = e.text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    let source = if e.source.is_empty() { None } else {
        Some(app.apps.iter().find(|a| a.wm_class.eq_ignore_ascii_case(&e.source) || a.id.trim_end_matches(".desktop").eq_ignore_ascii_case(&e.source))
            .map(|a| a.name.clone()).unwrap_or_else(|| e.source.clone()))
    };
    let mut detail = Detail { heading: Some("Information".into()), ..Default::default() };
    if let Some(source) = source {
        detail.fields.push(("Source".into(), source));
    }
    let (title, icon, kind_name) = match e.kind {
        Kind::Image => {
            let path = image_path(&e.text);
            detail.image = Some(path.clone());
            if let Some((w, h)) = e.dims {
                detail.fields.push(("Dimensions".into(), format!("{w} × {h}")));
            }
            let title = e.dims.map(|(w, h)| format!("Image ({w}×{h})")).unwrap_or_else(|| "Image".into());
            (title, IconSrc::File(path), "PNG image")
        }
        Kind::Color => {
            let color = parse_color(&e.text);
            detail.swatch = color;
            (e.text.trim().to_uppercase(), color.map(IconSrc::Swatch).unwrap_or(IconSrc::Badge(&icons::PALETTE, Tint::Pink)), "Color")
        }
        Kind::Link => {
            detail.text = Some(e.text.clone());
            (first_line.to_string(), IconSrc::Badge(&icons::LINK, Tint::Blue), "Link")
        }
        Kind::File => {
            let path = PathBuf::from(first_line.trim_start_matches("file://"));
            detail.fields.push(("Path".into(), util::tilde(&path)));
            let is_image = matches!(path.extension().and_then(|x| x.to_str()).map(|x| x.to_lowercase()).as_deref(), Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg"));
            if is_image {
                detail.image = Some(path.clone());
            } else {
                detail.text = Some(e.text.clone());
            }
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| first_line.to_string());
            (name, if is_image { IconSrc::File(path) } else { IconSrc::Badge(&icons::FILE, Tint::Teal) }, "File")
        }
        Kind::Text => {
            detail.text = Some(util::truncate(&e.text, 6000));
            detail.fields.push(("Characters".into(), e.text.chars().count().to_string()));
            detail.fields.push(("Words".into(), e.text.split_whitespace().count().to_string()));
            (util::truncate(first_line, 80), IconSrc::Badge(&icons::TEXT, Tint::Graphite), "Text")
        }
    };
    detail.fields.push(("Type".into(), kind_name.into()));
    if e.kind != Kind::Text {
        detail.fields.push(("Size".into(), util::human_size(e.size)));
    }
    detail.fields.push(("Copied".into(), when(e.time)));
    let mut actions = vec![
        Action::new(app.paste_target(), &icons::CLIPBOARD, Act::Clip(ClipOp::Paste, e.id)),
        Action::new("Copy to Clipboard", &icons::COPY, Act::Clip(ClipOp::Copy, e.id)).keys("ctrl+enter"),
    ];
    if e.kind == Kind::Link {
        actions.push(Action::new("Open in Browser", &icons::GLOBE, Act::OpenUrl(e.text.trim().to_string())).keys("ctrl+o"));
    }
    actions.push(Action::new(if e.pinned { "Unpin Entry" } else { "Pin Entry" }, &icons::PIN, Act::Clip(ClipOp::Pin, e.id)).keys("ctrl+shift+p").section(""));
    actions.push(Action::new("Delete Entry", &icons::TRASH, Act::Clip(ClipOp::Delete, e.id)).keys("ctrl+x").destructive());
    actions.push(Action::new("Delete All Entries", &icons::TRASH, Act::Confirm(Box::new(Confirm {
        title: "Delete All Entries?".into(),
        message: "Pinned entries are kept. This can't be undone.".into(),
        icon: &icons::TRASH, confirm: "Delete All".into(), destructive: true, act: Act::Clip(ClipOp::ClearAll, 0),
    }))).keys("ctrl+shift+x").destructive());
    Item::new(format!("clip:{}", e.id), title, icon).actions(actions).detail(detail)
}

pub const FILTERS: [&str; 6] = ["All Types", "Text", "Images", "Links", "Colors", "Files"];

pub fn build(app: &App, query: &str, filter: usize) -> Built {
    let q = query.trim();
    let wanted = [None, Some(Kind::Text), Some(Kind::Image), Some(Kind::Link), Some(Kind::Color), Some(Kind::File)][filter.min(5)];
    let mut sections = Sections::default();
    let matches = |e: &&Entry| wanted.is_none_or(|k| e.kind == k)
        && (q.is_empty() || (e.kind != Kind::Image && (e.text.to_lowercase().contains(&q.to_lowercase()) || fuzzy::score(q, &util::truncate(&e.text, 200)).is_some())));
    let pinned: Vec<Item> = app.clipboard.entries.iter().filter(|e| e.pinned).filter(matches).map(|e| item(app, e)).collect();
    sections.push("Pinned", pinned);
    for name in ["Today", "Yesterday", "This Week", "This Month", "Older"] {
        let items: Vec<Item> = app.clipboard.entries.iter().filter(|e| !e.pinned && bucket(e.time) == name).filter(matches).map(|e| item(app, e)).collect();
        sections.push(name, items);
    }
    Built::list(sections, "Type to filter entries...").layout(Layout::Split).filters(&FILTERS)
        .empty("No Entries", "Text, links, colors and images you copy appear here.").keep()
}

pub fn perform(app: &mut App, op: ClipOp, id: u64, tx: &mut Tx) {
    let entry = app.clipboard.entries.iter().find(|e| e.id == id).cloned();
    match (op, entry) {
        (ClipOp::Paste, Some(e)) => app.perform(if e.kind == Kind::Image { Act::PasteFile(image_path(&e.text)) } else { Act::Paste(e.text) }, tx),
        (ClipOp::Copy, Some(e)) => app.perform(if e.kind == Kind::Image { Act::CopyFile(image_path(&e.text)) } else { Act::Copy(e.text) }, tx),
        (ClipOp::Pin, Some(e)) => {
            with_store(|entries| entries.iter_mut().filter(|x| x.id == id).for_each(|x| x.pinned = !x.pinned));
            app.clipboard.reload();
            app.refresh_keeping();
            app.toast(if e.pinned { "Entry Unpinned" } else { "Entry Pinned" }, Tone::Success);
        }
        (ClipOp::Delete, Some(e)) => {
            with_store(|entries| entries.retain(|x| x.id != id));
            if e.kind == Kind::Image {
                let _ = std::fs::remove_file(image_path(&e.text));
            }
            let at = app.top().sel;
            app.clipboard.reload();
            app.refresh();
            app.select(at, tx);
        }
        (ClipOp::ClearAll, _) => {
            with_store(|entries| entries.retain(|x| {
                if !x.pinned && x.kind == Kind::Image {
                    let _ = std::fs::remove_file(image_path(&x.text));
                }
                x.pinned
            }));
            app.clipboard.reload();
            app.refresh();
            app.toast("History Cleared", Tone::Success);
        }
        _ => {}
    }
}
