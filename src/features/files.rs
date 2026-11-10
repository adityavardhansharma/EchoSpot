//! File search. EchoFiles' index answers when it is installed, `fd` otherwise; either way the
//! search runs off the UI thread and only the newest answer lands.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rune::Latest;

use crate::app::{App, Built};
use crate::icons;
use crate::model::*;
use crate::util;

#[derive(Default)]
pub struct FileSearch {
    pub results: Latest<(String, Vec<PathBuf>)>,
    pub hits: Vec<PathBuf>,
    pub answered: String,
    pub requested: String,
}

impl FileSearch {
    pub fn configure(&mut self, waker: rune::Waker) {
        self.results = Latest::default().debounce(Duration::from_millis(70));
        self.results.set_waker(waker);
    }

    pub fn request(&mut self, query: &str) {
        let q = query.trim().to_string();
        if q == self.requested {
            return;
        }
        self.requested = q.clone();
        if q.is_empty() {
            self.results.cancel();
            self.hits.clear();
            self.answered.clear();
            return;
        }
        self.results.request(q, |q| {
            let hits = search(&q);
            (q, hits)
        });
    }

    /// Lands an answer; true when the list changed.
    pub fn poll(&mut self) -> bool {
        let Some((query, hits)) = self.results.take() else { return false };
        self.answered = query;
        self.hits = hits;
        true
    }

    pub fn pending(&self) -> bool {
        self.requested != self.answered
    }
}

fn run_lines(program: &str, args: &[&str]) -> Vec<PathBuf> {
    util::output(program, args).map(|out| out.lines().filter(|l| !l.is_empty()).map(PathBuf::from).collect()).unwrap_or_default()
}

fn search(query: &str) -> Vec<PathBuf> {
    let home = util::home().display().to_string();
    let mut hits = if util::has_program("fd") {
        // Smart case, every term somewhere in the path's name, hidden and ignored files left out.
        let pattern = query.split_whitespace().map(regex_escape).collect::<Vec<_>>().join(".*");
        run_lines("fd", &["--max-results", "120", "--absolute-path", "--exclude", "node_modules", "--exclude", "target", "--exclude", ".cache", &pattern, &home])
    } else {
        let script = format!("find {} -maxdepth 6 -not -path '*/.*' -iname {} 2>/dev/null | head -n 120", util::quote(&home), util::quote(&format!("*{query}*")));
        util::sh_output(&script).map(|o| o.lines().map(PathBuf::from).collect()).unwrap_or_default()
    };
    // Shallow paths and exact names first: the file someone means is rarely the deepest one.
    let q = query.to_lowercase();
    hits.sort_by_key(|p| {
        let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        let tier = if name == q { 0 } else if name.starts_with(&q) { 1 } else { 2 };
        (tier, p.components().count(), name.len())
    });
    hits
}

fn regex_escape(s: &str) -> String {
    s.chars().flat_map(|c| if "\\.+*?()|[]{}^$".contains(c) { vec!['\\', c] } else { vec![c] }).collect()
}

fn kind_of(path: &Path) -> (&'static str, &'static rune::Icon) {
    if path.is_dir() {
        return ("Folder", &icons::FOLDER);
    }
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()).as_deref() {
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "avif" | "heic") => ("Image", &icons::IMAGE),
        Some("pdf") => ("PDF Document", &icons::FILE_TEXT),
        Some("md" | "txt" | "rtf" | "log") => ("Text", &icons::FILE_TEXT),
        Some("rs" | "js" | "ts" | "tsx" | "jsx" | "py" | "go" | "c" | "h" | "cpp" | "java" | "rb" | "sh" | "lua" | "swift" | "kt" | "zig" | "css" | "html") => ("Source Code", &icons::CODE),
        Some("json" | "toml" | "yaml" | "yml" | "xml" | "ini" | "conf") => ("Configuration", &icons::GEAR),
        Some("mp3" | "flac" | "wav" | "ogg" | "m4a" | "opus") => ("Audio", &icons::MUSIC),
        Some("mp4" | "mkv" | "mov" | "webm" | "avi") => ("Video", &icons::PLAY),
        Some("zip" | "tar" | "gz" | "xz" | "zst" | "7z" | "rar") => ("Archive", &icons::BOX),
        _ => ("Document", &icons::FILE),
    }
}

fn is_text(path: &Path) -> bool {
    matches!(kind_of(path).0, "Text" | "Source Code" | "Configuration")
}

fn item(path: &Path) -> Item {
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path.display().to_string());
    let (kind, glyph) = kind_of(path);
    let meta = std::fs::metadata(path).ok();
    let mut detail = Detail { heading: Some("Information".into()), ..Default::default() };
    let icon = if kind == "Image" {
        detail.image = Some(path.to_path_buf());
        IconSrc::File(path.to_path_buf())
    } else {
        IconSrc::Glyph(glyph)
    };
    if is_text(path) && meta.as_ref().is_some_and(|m| m.len() < 2_000_000) {
        detail.text = std::fs::read(path).ok().map(|bytes| {
            let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4000)]).to_string();
            head.lines().take(60).collect::<Vec<_>>().join("\n")
        });
    }
    detail.fields.push(("Name".into(), name.clone()));
    detail.fields.push(("Kind".into(), kind.into()));
    detail.fields.push(("Where".into(), util::tilde(path.parent().unwrap_or(path))));
    if let Some(meta) = &meta {
        if meta.is_file() {
            detail.fields.push(("Size".into(), util::human_size(meta.len())));
        }
        if let Ok(modified) = meta.modified() {
            let when: chrono::DateTime<chrono::Local> = modified.into();
            detail.fields.push(("Modified".into(), when.format("%-d %b %Y at %H:%M").to_string()));
        }
    }
    let dir = path.parent().unwrap_or(path).to_path_buf();
    Item::new(format!("file:{}", path.display()), name, icon)
        .subtitle(util::tilde(&dir))
        .action(Action::new("Open", &icons::ARROW_UP_RIGHT, Act::OpenPath(path.to_path_buf())))
        .action(Action::new("Show in File Manager", &icons::FOLDER, Act::Reveal(path.to_path_buf())).keys("ctrl+enter"))
        .action(Action::new("Copy Path", &icons::COPY, Act::Copy(path.display().to_string())).keys("ctrl+shift+c"))
        .action(Action::new("Copy File", &icons::CLIPBOARD, Act::CopyFile(path.to_path_buf())).keys("ctrl+shift+f"))
        .action(Action::new("Open Terminal Here", &icons::TERMINAL, Act::Spawn(vec![
            "sh".into(), "-c".into(), format!("cd {} && exec xdg-terminal-exec", util::quote(&dir.display().to_string())),
        ])).keys("ctrl+t").section(""))
        .action(Action::new("Move to Trash", &icons::TRASH, Act::Confirm(Box::new(Confirm {
            title: "Move to Trash?".into(), message: util::tilde(path), icon: &icons::TRASH, confirm: "Move to Trash".into(), destructive: true,
            act: Act::ShellQuiet { cmd: format!("gio trash {}", util::quote(&path.display().to_string())), done: "Moved to Trash".into() },
        }))).keys("ctrl+x").destructive())
        .detail(detail)
}

fn recent() -> Vec<PathBuf> {
    // Recently used files, newest first, from the desktop's own list.
    let path = util::data_dir().parent().map(|d| d.join("recently-used.xbel")).unwrap_or_default();
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new() };
    let mut files: Vec<PathBuf> = text.split("href=\"file://").skip(1)
        .filter_map(|rest| rest.split('"').next())
        .map(|p| PathBuf::from(percent_decode(p)))
        .filter(|p| p.exists())
        .collect();
    files.reverse();
    files.dedup();
    files.truncate(30);
    files
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let (mut out, mut i) = (Vec::with_capacity(bytes.len()), 0);
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len()
            && let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

pub fn build(app: &App, query: &str) -> Built {
    let q = query.trim();
    let mut sections = Sections::default();
    if q.is_empty() {
        sections.push("Recent", recent().iter().map(|p| item(p)).collect());
        return Built::list(sections, "Search files by name...").layout(Layout::Split)
            .empty("Search Files", "Type a name to search your home folder.");
    }
    sections.push("Files", app.files.hits.iter().take(80).map(|p| item(p)).collect());
    Built::list(sections, "Search files by name...").layout(Layout::Split).loading(app.files.pending())
        .empty(if app.files.pending() { "Searching…" } else { "No Results" }, "")
}
