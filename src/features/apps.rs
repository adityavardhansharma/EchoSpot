//! Installed applications, read from freedesktop `.desktop` entries.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct AppEntry {
    /// The desktop file id, e.g. `firefox.desktop`.
    pub id: String,
    pub path: PathBuf,
    pub name: String,
    pub generic: String,
    pub keywords: String,
    pub exec: String,
    /// `Path=`: the directory the app expects to start in, or empty.
    pub dir: String,
    pub terminal: bool,
    /// The window class the app's windows report, for the running dot.
    pub wm_class: String,
    /// The `Icon=` value, and the file it resolved to.
    pub icon_name: String,
    pub icon: Option<PathBuf>,
}

fn data_dirs() -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let mut dirs = vec![
        std::env::var("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(&home).join(".local/share")),
    ];
    let system = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    dirs.extend(system.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    dirs.push(PathBuf::from(&home).join(".local/share/flatpak/exports/share"));
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share"));
    dirs
}

fn locale_keys() -> Vec<String> {
    let lang = std::env::var("LC_MESSAGES").or_else(|_| std::env::var("LANG")).unwrap_or_default();
    let lang = lang.split('.').next().unwrap_or("").to_string();
    let short = lang.split('_').next().unwrap_or("").to_string();
    [lang, short].into_iter().filter(|s| !s.is_empty() && s != "C" && s != "POSIX").collect()
}

fn parse(path: &Path, id: String, locales: &[String]) -> Option<AppEntry> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut fields: HashMap<&str, &str> = HashMap::new();
    let mut in_entry = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_entry || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            fields.entry(k.trim()).or_insert(v.trim());
        }
    }
    let flag = |k: &str| fields.get(k).is_some_and(|v| v.eq_ignore_ascii_case("true"));
    if fields.get("Type").is_some_and(|t| *t != "Application") || flag("NoDisplay") || flag("Hidden") {
        return None;
    }
    if let Some(only) = fields.get("OnlyShowIn") {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
        if !only.split(';').any(|d| !d.is_empty() && desktop.split(':').any(|c| c.eq_ignore_ascii_case(d))) {
            return None;
        }
    }
    let localized = |key: &str| {
        locales.iter().find_map(|l| fields.get(format!("{key}[{l}]").as_str()).copied()).or_else(|| fields.get(key).copied())
    };
    let name = localized("Name")?.to_string();
    let exec = fields.get("Exec")?.to_string();
    let wm_class = fields.get("StartupWMClass").map(|s| s.to_string()).unwrap_or_else(|| id.trim_end_matches(".desktop").to_string());
    Some(AppEntry {
        id,
        path: path.to_path_buf(),
        name,
        generic: localized("GenericName").unwrap_or("").to_string(),
        keywords: format!("{} {}", localized("Keywords").unwrap_or("").replace(';', " "), fields.get("Categories").unwrap_or(&"").replace(';', " ")),
        exec,
        dir: fields.get("Path").unwrap_or(&"").to_string(),
        terminal: flag("Terminal"),
        wm_class,
        icon_name: fields.get("Icon").unwrap_or(&"").to_string(),
        icon: None,
    })
}

fn collect(dir: &Path, base: &Path, locales: &[String], seen: &mut HashMap<String, AppEntry>, order: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, base, locales, seen, order);
        } else if path.extension().is_some_and(|e| e == "desktop") {
            // A file id is its path under `applications/` with slashes as dashes.
            let id = path.strip_prefix(base).unwrap_or(&path).to_string_lossy().replace('/', "-");
            if seen.contains_key(&id) {
                continue;
            }
            // A hidden entry in an earlier directory still shadows later ones.
            match parse(&path, id.clone(), locales) {
                Some(app) => {
                    order.push(id.clone());
                    seen.insert(id, app);
                }
                None => {
                    seen.insert(id.clone(), AppEntry {
                        id, path, name: String::new(), generic: String::new(), keywords: String::new(),
                        exec: String::new(), dir: String::new(), terminal: false, wm_class: String::new(), icon_name: String::new(), icon: None,
                    });
                }
            }
        }
    }
}

/// Every launchable application, by name.
pub fn scan() -> Vec<AppEntry> {
    let locales = locale_keys();
    let (mut seen, mut order) = (HashMap::new(), Vec::new());
    for dir in data_dirs() {
        let base = dir.join("applications");
        collect(&base, &base, &locales, &mut seen, &mut order);
    }
    let mut apps: Vec<AppEntry> = order.into_iter().filter_map(|id| seen.remove(&id)).filter(|a| !a.name.is_empty()).collect();
    apps.sort_by_key(|a| a.name.to_lowercase());
    apps.dedup_by(|a, b| a.name == b.name && a.exec == b.exec);
    let index = super::icon_theme::IconIndex::build();
    for app in &mut apps {
        app.icon = index.find(&app.icon_name);
    }
    apps
}

/// The argv an `Exec=` line launches, with its field codes dropped.
pub fn exec_argv(exec: &str) -> Vec<String> {
    let (mut args, mut cur, mut quoted, mut has) = (Vec::new(), String::new(), false, false);
    let mut chars = exec.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                quoted = !quoted;
                has = true;
            }
            '\\' if quoted => {
                if let Some(n) = chars.next() {
                    cur.push(n);
                }
            }
            '%' if !quoted => match chars.next() {
                Some('%') => cur.push('%'),
                _ => {}
            },
            c if c.is_whitespace() && !quoted => {
                if has || !cur.is_empty() {
                    args.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => cur.push(c),
        }
    }
    if has || !cur.is_empty() {
        args.push(cur);
    }
    args
}

impl AppEntry {
    /// The row icon: the app's own, or the generic tile.
    pub fn icon_src(&self) -> crate::model::IconSrc {
        match &self.icon {
            Some(path) => crate::model::IconSrc::App(path.clone()),
            None => crate::model::IconSrc::Glyph(&crate::icons::BOX),
        }
    }
}

/// `~/x` as a path under home; anything else as written.
fn expand_home(path: &str) -> String {
    match path.strip_prefix("~/") {
        Some(rest) => crate::util::home().join(rest).display().to_string(),
        None => path.to_string(),
    }
}

/// Whether `uwsm app` would read this program as a desktop entry rather than run it: it takes
/// any first argument ending in `.desktop` (or `.desktop:action`) for an entry id or file.
fn reads_as_entry(program: &str) -> bool {
    program.ends_with(".desktop") || program.rsplit_once(':').is_some_and(|(head, _)| head.ends_with(".desktop"))
}

/// Starts the app. `Err` says why it could not be started.
pub fn launch(app: &AppEntry) -> Result<(), String> {
    let mut argv = exec_argv(&app.exec);
    if argv.is_empty() {
        return Err(format!("{} has no command to run", app.name));
    }
    // Not in the spec, but hand-written entries use it and a shell would have expanded it.
    argv[0] = expand_home(&argv[0]);
    let found = if argv[0].contains('/') { std::path::Path::new(&argv[0]).is_file() } else { crate::util::has_program(&argv[0]) };
    if !found {
        return Err(format!("{} is missing: {}", app.name, crate::util::tilde(std::path::Path::new(&argv[0]))));
    }
    let dir = expand_home(&app.dir);
    let dir = Some(std::path::Path::new(&dir)).filter(|d| !app.dir.is_empty() && d.is_dir());
    // uwsm keeps launched apps in their own systemd scope where Omarchy provides it.
    let uwsm = crate::util::has_program("uwsm-app");
    if uwsm && reads_as_entry(&argv[0]) {
        // A binary named like an entry (OpenCode's is `ai.opencode.desktop`) has to go by its
        // entry file, which uwsm then reads for the command, terminal and directory itself.
        argv = vec!["uwsm-app".to_string(), "--".to_string(), app.path.display().to_string()];
    } else {
        if app.terminal {
            let term = std::env::var("TERMINAL").unwrap_or_else(|_| "xdg-terminal-exec".into());
            argv.splice(0..0, [term, "-e".to_string()]);
        }
        if uwsm {
            argv.splice(0..0, ["uwsm-app".to_string(), "--".to_string()]);
        }
    }
    if crate::util::spawn_detached_in(&argv, dir) { Ok(()) } else { Err(format!("Could not start {}", app.name)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_named_programs() {
        assert!(reads_as_entry("/home/me/.local/share/opencode-desktop/ai.opencode.desktop"));
        assert!(reads_as_entry("some.app.desktop:new-window"));
        assert!(!reads_as_entry("firefox"));
        assert!(!reads_as_entry("/opt/docker-desktop/bin/docker-desktop"));
    }

    #[test]
    fn field_codes_are_dropped() {
        assert_eq!(exec_argv("/opt/x/ai.opencode.desktop %U"), vec!["/opt/x/ai.opencode.desktop"]);
        assert_eq!(exec_argv(r#"sh -c "echo 100%% %f""#), vec!["sh", "-c", "echo 100%% %f"]);
    }
}
