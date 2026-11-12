//! Snippet expansion as you type: a snippet's keyword, typed in any app, is replaced by its text.
//!
//! Wayland lets no app watch another's typing, so this reads the keyboards themselves from
//! `/dev/input` (which takes membership of the `input` group) and keeps only the last few
//! characters in memory to compare against the keywords. Nothing is stored or sent anywhere.
//! Keys are read as a US layout.

use std::collections::HashSet;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::util;

/// The most the buffer holds, and how long a pause empties it.
const MAX_BUFFER: usize = 256;
const IDLE: Duration = Duration::from_secs(15);

static ENABLED: AtomicBool = AtomicBool::new(false);
static STARTED: AtomicBool = AtomicBool::new(false);

/// What the watcher compares typing against: `(keyword, text)`.
fn table() -> &'static Mutex<Vec<(String, String)>> {
    static TABLE: OnceLock<Mutex<Vec<(String, String)>>> = OnceLock::new();
    TABLE.get_or_init(Default::default)
}

/// Hands the watcher the snippets that have a keyword. Called whenever the snippets change.
pub fn set_snippets(list: &[crate::features::snippets::Snippet]) {
    if let Ok(mut table) = table().lock() {
        *table = list.iter().filter(|s| s.keyword.trim().chars().count() >= 2 && !s.text.is_empty())
            .map(|s| (s.keyword.trim().to_string(), s.text.clone())).collect();
    }
}

/// Whether the keyboards can be read at all: the reason a switch that is on may do nothing.
pub fn permitted() -> bool {
    keyboards().iter().any(|path| std::fs::File::open(path).is_ok())
}

/// Turns expansion on or off. The watcher starts the first time it is turned on.
pub fn set_enabled(on: bool) {
    ENABLED.store(on, Ordering::Relaxed);
    if on && !STARTED.swap(true, Ordering::Relaxed) {
        std::thread::Builder::new().name("echospot-expander".into()).spawn(supervise).ok();
    }
}

/// The event devices that are keyboards people type on.
fn keyboards() -> Vec<String> {
    let text = std::fs::read_to_string("/proc/bus/input/devices").unwrap_or_default();
    text.split("\n\n").filter_map(|block| {
        let handlers = block.lines().find_map(|l| l.strip_prefix("H: Handlers="))?;
        // A real keyboard handles SysRq and has LEDs; power buttons and lid switches do not.
        let typing = handlers.split_whitespace().any(|h| h == "sysrq") && handlers.split_whitespace().any(|h| h == "kbd");
        let event = handlers.split_whitespace().find(|h| h.starts_with("event"))?;
        typing.then(|| format!("/dev/input/{event}"))
    }).collect()
}

/// Watches for keyboards coming and going: one reader each.
fn supervise() {
    let mut reading: HashSet<String> = HashSet::new();
    let state = std::sync::Arc::new(Mutex::new(Typing::default()));
    let done = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
    loop {
        if let Ok(mut gone) = done.lock() {
            for path in gone.drain(..) {
                reading.remove(&path);
            }
        }
        for path in keyboards() {
            if reading.contains(&path) {
                continue;
            }
            let Ok(file) = std::fs::File::open(&path) else { continue };
            reading.insert(path.clone());
            let (state, done) = (state.clone(), done.clone());
            std::thread::spawn(move || {
                read_device(file, &state);
                if let Ok(mut gone) = done.lock() {
                    gone.push(path);
                }
            });
        }
        std::thread::sleep(Duration::from_secs(8));
    }
}

fn read_device(mut file: std::fs::File, state: &Mutex<Typing>) {
    // struct input_event: a 16-byte timeval, then type, code and value.
    let mut event = [0u8; 24];
    while file.read_exact(&mut event).is_ok() {
        let kind = u16::from_ne_bytes([event[16], event[17]]);
        let code = u16::from_ne_bytes([event[18], event[19]]);
        let value = i32::from_ne_bytes([event[20], event[21], event[22], event[23]]);
        if kind != 1 || !ENABLED.load(Ordering::Relaxed) {
            continue;
        }
        let hit = {
            let Ok(mut typing) = state.lock() else { continue };
            let Ok(table) = table().lock() else { continue };
            typing.key(code, value, &table, Instant::now())
        };
        if let Some((erase, text)) = hit {
            std::thread::spawn(move || replace(erase, &text));
        }
    }
}

/// What has just been typed, as far as the keys tell.
#[derive(Default)]
pub struct Typing {
    buffer: String,
    shift: bool,
    /// Ctrl, Alt or Super held: a shortcut, not text.
    chord: u8,
    last: Option<Instant>,
}

/// The character a key types on a US layout.
fn character(code: u16, shift: bool) -> Option<char> {
    const ROWS: [(u16, &str, &str); 4] = [
        (2, "1234567890-=", "!@#$%^&*()_+"),
        (16, "qwertyuiop[]", "QWERTYUIOP{}"),
        (30, "asdfghjkl;'`", "ASDFGHJKL:\"~"),
        (44, "zxcvbnm,./", "ZXCVBNM<>?"),
    ];
    match code {
        57 => return Some(' '),
        43 => return Some(if shift { '|' } else { '\\' }),
        _ => {}
    }
    ROWS.iter().find_map(|(first, plain, shifted)| {
        let at = code.checked_sub(*first)? as usize;
        (if shift { shifted } else { plain }).chars().nth(at)
    })
}

impl Typing {
    /// Takes one key event. When it completes a keyword: how many characters to erase, and the
    /// snippet's text.
    pub fn key(&mut self, code: u16, value: i32, table: &[(String, String)], now: Instant) -> Option<(usize, String)> {
        match code {
            42 | 54 => {
                self.shift = value != 0;
                return None;
            }
            29 | 97 | 56 | 100 | 125 | 126 => {
                self.chord = if value == 0 { self.chord.saturating_sub(1) } else if value == 1 { self.chord + 1 } else { self.chord };
                self.buffer.clear();
                return None;
            }
            _ => {}
        }
        // Releases type nothing; a held key repeating is still typing.
        if value == 0 {
            return None;
        }
        if self.last.is_some_and(|last| now.duration_since(last) > IDLE) {
            self.buffer.clear();
        }
        self.last = Some(now);
        if self.chord > 0 {
            self.buffer.clear();
            return None;
        }
        match code {
            14 => {
                self.buffer.pop();
                return None;
            }
            // Caps Lock changes nothing typed by itself.
            58 => return None,
            _ => {}
        }
        // Return, Escape, Tab, the arrows: the caret has gone somewhere the buffer cannot follow.
        let Some(c) = character(code, self.shift) else {
            self.buffer.clear();
            return None;
        };
        self.buffer.push(c);
        if self.buffer.chars().count() > MAX_BUFFER {
            let cut = self.buffer.char_indices().nth(self.buffer.chars().count() - MAX_BUFFER).map(|(i, _)| i).unwrap_or(0);
            self.buffer.drain(..cut);
        }
        // The longest keyword wins, so `;sig2` is not taken for `;sig`.
        let hit = table.iter().filter(|(keyword, _)| {
            let Some(before) = self.buffer.strip_suffix(keyword.as_str()) else { return false };
            // A keyword made of letters must start a word: "brb" should not fire inside "herbrb".
            let wordy = keyword.chars().next().is_some_and(char::is_alphanumeric);
            !wordy || !before.chars().next_back().is_some_and(char::is_alphanumeric)
        }).max_by_key(|(keyword, _)| keyword.chars().count())?;
        self.buffer.clear();
        Some((hit.0.chars().count(), hit.1.clone()))
    }
}

/// Erases the keyword and puts the snippet in its place, in whatever has the keyboard.
fn replace(erase: usize, text: &str) {
    if !util::has_program("wtype") {
        return;
    }
    let active = crate::features::windows::active_class();
    // In the palette itself a keyword is something being searched for or edited.
    if active.as_deref() == Some(crate::APP_ID) {
        return;
    }
    let text = crate::features::snippets::expand(text);
    // Let the key that finished the keyword come back up first.
    std::thread::sleep(Duration::from_millis(40));
    let mut erasing: Vec<&str> = Vec::new();
    for _ in 0..erase {
        erasing.extend(["-k", "BackSpace"]);
    }
    util::output("wtype", &erasing);
    if text.chars().count() <= 160 && !text.contains(['\n', '\t']) {
        // Short text is typed: nothing touches the clipboard.
        util::output("wtype", &["-d", "1", "--", &text]);
        return;
    }
    let before = util::output("wl-paste", &["--no-newline", "--type", "text/plain"]);
    crate::features::clipboard::copy_text(&text);
    std::thread::sleep(Duration::from_millis(60));
    let class = active.unwrap_or_default().to_lowercase();
    let terminal = ["alacritty", "kitty", "ghostty", "foot", "wezterm", "konsole", "terminal", "tmux", "xterm"].iter().any(|t| class.contains(t));
    if terminal {
        util::output("wtype", &["-M", "ctrl", "-M", "shift", "v", "-m", "shift", "-m", "ctrl"]);
    } else {
        util::output("wtype", &["-M", "ctrl", "v", "-m", "ctrl"]);
    }
    // What was on the clipboard before goes back once the paste has landed.
    if let Some(before) = before.filter(|b| !b.is_empty()) {
        std::thread::sleep(Duration::from_millis(400));
        crate::features::clipboard::copy_text(&before);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(c: char) -> (u16, bool) {
        for shift in [false, true] {
            if let Some(code) = (1..60u16).find(|k| character(*k, shift) == Some(c)) {
                return (code, shift);
            }
        }
        panic!("no key types {c:?}");
    }

    fn type_text(t: &mut Typing, text: &str, table: &[(String, String)]) -> Option<(usize, String)> {
        let mut hit = None;
        for c in text.chars() {
            let (k, shift) = code(c);
            if shift { t.key(42, 1, table, Instant::now()); }
            hit = t.key(k, 1, table, Instant::now()).or(hit);
            t.key(k, 0, table, Instant::now());
            if shift { t.key(42, 0, table, Instant::now()); }
        }
        hit
    }

    #[test]
    fn keywords() {
        let table = vec![(";sig".to_string(), "Best, A".to_string()), (";sig2".to_string(), "Cheers".to_string()), ("brb".to_string(), "be right back".to_string()), ("!Em".to_string(), "me@example.com".to_string())];
        let mut t = Typing::default();
        assert_eq!(type_text(&mut t, "hello ;sig", &table), Some((4, "Best, A".into())));
        assert_eq!(type_text(&mut t, "say brb", &table), Some((3, "be right back".into())));
        // Inside a word, a keyword of letters stays as typed.
        assert_eq!(type_text(&mut t, " herbrb", &table), None);
        assert_eq!(type_text(&mut t, " x!Em", &table), Some((3, "me@example.com".into())));
        // Backspace is followed: "brx", backspace, "b".
        let mut t = Typing::default();
        type_text(&mut t, "brx", &table);
        t.key(14, 1, &table, Instant::now());
        assert_eq!(type_text(&mut t, "b", &table), Some((3, "be right back".into())));
        // Return, and a shortcut, start over.
        let mut t = Typing::default();
        type_text(&mut t, "br", &table);
        t.key(28, 1, &table, Instant::now());
        assert_eq!(type_text(&mut t, "b", &table), None);
        type_text(&mut t, " br", &table);
        t.key(29, 1, &table, Instant::now());
        t.key(46, 1, &table, Instant::now());
        t.key(29, 0, &table, Instant::now());
        assert_eq!(type_text(&mut t, "b", &table), None);
    }
}
