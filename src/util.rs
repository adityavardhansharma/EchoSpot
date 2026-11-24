//! Small process and path helpers shared by every feature.

use std::path::PathBuf;
use std::process::{Command, Stdio};

pub fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

/// `~/.config/echospot`
pub fn config_dir() -> PathBuf {
    let base = std::env::var("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|_| home().join(".config"));
    let dir = base.join("echospot");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// `~/.local/share/echospot`
pub fn data_dir() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|_| home().join(".local/share"));
    let dir = base.join("echospot");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn runtime_dir() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|_| std::env::temp_dir())
}

pub fn has_program(name: &str) -> bool {
    std::env::var("PATH").unwrap_or_default().split(':').any(|dir| std::path::Path::new(dir).join(name).is_file())
}

/// Starts a program that outlives the palette: its own session, no inherited stdio.
pub fn spawn_detached(argv: &[String]) -> bool {
    spawn_detached_in(argv, None)
}

/// `spawn_detached`, started in a directory of its own.
pub fn spawn_detached_in(argv: &[String], dir: Option<&std::path::Path>) -> bool {
    let Some((program, args)) = argv.split_first() else { return false };
    let mut cmd = Command::new(program);
    if let Some(dir) = dir {
        cmd.current_dir(dir);
    }
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // Activation tokens belong to the palette's own launch, not to what it starts.
    cmd.env_remove("XDG_ACTIVATION_TOKEN").env_remove("DESKTOP_STARTUP_ID");
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
    match cmd.spawn() {
        Ok(mut child) => {
            // Reap it off-thread so it never lingers as a zombie.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            true
        }
        Err(_) => false,
    }
}

pub fn sh_detached(script: &str) -> bool {
    spawn_detached(&["sh".to_string(), "-c".to_string(), script.to_string()])
}

/// Runs a command to completion and returns its stdout, trimmed.
pub fn output(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn sh_output(script: &str) -> Option<String> {
    output("sh", &["-c", script])
}

/// Whether a `sh -c` script exits cleanly.
pub fn sh_ok(script: &str) -> bool {
    Command::new("sh").args(["-c", script]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
        .status().is_ok_and(|s| s.success())
}

pub fn open_url(url: &str) {
    spawn_detached(&["xdg-open".to_string(), url.to_string()]);
}

/// `~/Documents/x` for a path under home.
pub fn tilde(path: &std::path::Path) -> String {
    let home = home();
    match path.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "kB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} bytes") } else if value >= 100.0 { format!("{value:.0} {}", UNITS[unit]) } else { format!("{value:.1} {}", UNITS[unit]) }
}

pub fn read_json<T: serde::de::DeserializeOwned + Default>(path: &std::path::Path) -> T {
    std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Writes through a temp file, so a crash mid-write never leaves half a store.
pub fn write_json<T: serde::Serialize>(path: &std::path::Path, value: &T) {
    write_json_mode(path, value, 0o644);
}

/// `write_json` for a file that can hold a key or a conversation: readable by this user only,
/// from the moment it exists, not after a chmod that leaves a moment it is anyone's to read.
pub fn write_json_private<T: serde::Serialize>(path: &std::path::Path, value: &T) {
    write_json_mode(path, value, 0o600);
}

fn write_json_mode<T: serde::Serialize>(path: &std::path::Path, value: &T, mode: u32) {
    let Ok(text) = serde_json::to_string_pretty(value) else { return };
    write_file(path, text.as_bytes(), mode);
}

/// Writes a file whole through a temp file beside it, created with `mode`.
pub fn write_file(path: &std::path::Path, bytes: &[u8], mode: u32) -> bool {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    // The temp file's name is the whole file name plus a suffix, so `a.json` and `a.toml` in one
    // folder never share it.
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(format!(".{}.tmp", std::process::id()));
    let tmp = path.with_file_name(name);
    let written = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(mode).open(&tmp)
        .and_then(|mut f| f.write_all(bytes));
    if written.is_err() {
        let _ = std::fs::remove_file(&tmp);
        return false;
    }
    // A temp file left by a crash may have been made with another mode.
    let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(mode));
    std::fs::rename(&tmp, path).is_ok()
}

/// A fresh id for a saved item: the current millisecond in hex, moved on past any `taken` one.
pub fn new_id(taken: impl Fn(&str) -> bool) -> String {
    let mut n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    loop {
        let id = format!("{n:x}");
        if !taken(&id) {
            return id;
        }
        n += 1;
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Shell-quotes one argument.
pub fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The first `max` characters, with an ellipsis when cut.
pub fn truncate(s: &str, max: usize) -> String {
    let mut out: String = s.chars().take(max).collect();
    if s.chars().count() > max {
        out.push('…');
    }
    out
}

/// How long ago a moment was, as a row's quiet label: "Just now", "12m ago", "Yesterday", "3 Nov".
pub fn ago(secs: u64) -> String {
    let d = now_secs().saturating_sub(secs);
    match d {
        0..=59 => "Just now".into(),
        60..=3599 => format!("{}m ago", d / 60),
        3600..=86_399 => format!("{}h ago", d / 3600),
        86_400..=172_799 => "Yesterday".into(),
        172_800..=604_799 => format!("{}d ago", d / 86_400),
        _ => chrono::DateTime::from_timestamp(secs as i64, 0)
            .map(|t| t.with_timezone(&chrono::Local).format("%-d %b").to_string()).unwrap_or_default(),
    }
}
