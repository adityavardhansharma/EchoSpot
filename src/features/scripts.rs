//! Script commands: executables in `~/.config/echospot/scripts` that describe themselves in
//! Raycast's header comments (`# @raycast.title Deploy`), and the one-line shell commands of
//! `~/.config/echospot/commands.json`. Each is a launcher row.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use rune::Tx;
use serde::Deserialize;
use serde_json::Value;

use crate::app::{App, Page, Tone};
use crate::features::forms::{self, Field, FieldKind, Form, Target};
use crate::icons;
use crate::model::*;
use crate::theme::Tint;
use crate::util;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    /// Runs out of sight; the last line of output is a HUD.
    Silent,
    /// The palette stays; the last line of output is a toast.
    Compact,
    /// The whole output on a page.
    Full,
    /// In a terminal window, for a command that asks questions or takes its time.
    Terminal,
}

#[derive(Clone, Debug)]
pub struct Argument {
    pub placeholder: String,
    pub optional: bool,
    pub secret: bool,
    pub options: Vec<(String, String)>,
}

#[derive(Clone, Debug)]
pub struct Script {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub icon: String,
    pub mode: Mode,
    /// A script file, or `None` for a `commands.json` shell line.
    pub path: Option<PathBuf>,
    pub command: String,
    pub args: Vec<Argument>,
    pub confirm: bool,
    pub cwd: Option<PathBuf>,
}

pub fn dir() -> PathBuf {
    let dir = util::config_dir().join("scripts");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// The `@raycast.key value` pairs in a script's leading comments.
fn header(text: &str) -> Vec<(String, String)> {
    text.lines().take(60).filter_map(|line| {
        let line = line.trim_start().trim_start_matches(['#', '/', '-', '*', ' ']);
        let rest = line.strip_prefix("@raycast.").or_else(|| line.strip_prefix("@echospot."))?;
        let (key, value) = rest.split_once(char::is_whitespace).unwrap_or((rest, ""));
        Some((key.trim().to_string(), value.trim().to_string()))
    }).collect()
}

fn argument(json: &str) -> Option<Argument> {
    let v: Value = serde_json::from_str(json).ok()?;
    let text = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let options = v.get("data").and_then(Value::as_array).into_iter().flatten()
        .filter_map(|o| Some((o.get("value")?.as_str()?.to_string(), o.get("title").and_then(Value::as_str).unwrap_or(o.get("value")?.as_str()?).to_string()))).collect();
    Some(Argument { placeholder: text("placeholder"), optional: v.get("optional").and_then(Value::as_bool).unwrap_or(false), secret: text("type") == "password" || v.get("secure").and_then(Value::as_bool).unwrap_or(false), options })
}

pub fn parse(path: &std::path::Path, text: &str) -> Option<Script> {
    let pairs = header(text);
    let get = |k: &str| pairs.iter().find(|(key, _)| key == k).map(|(_, v)| v.clone()).unwrap_or_default();
    let title = get("title");
    if title.is_empty() {
        return None;
    }
    let mode = match get("mode").as_str() {
        "silent" => Mode::Silent,
        "fullOutput" => Mode::Full,
        "terminal" => Mode::Terminal,
        // `inline` would live on its row; here it answers in a toast like `compact`.
        _ => Mode::Compact,
    };
    let args = ["argument1", "argument2", "argument3"].iter().filter_map(|k| argument(&get(k))).collect();
    let cwd = Some(get("currentDirectoryPath")).filter(|d| !d.is_empty()).map(|d| if let Some(rest) = d.strip_prefix("~/") { util::home().join(rest) } else { PathBuf::from(d) });
    let name = path.file_name()?.to_string_lossy().to_string();
    let subtitle = [get("packageName"), get("description")].into_iter().find(|s| !s.is_empty()).unwrap_or_default();
    Some(Script {
        id: format!("script:{name}"), title, subtitle, icon: get("icon"), mode, path: Some(path.to_path_buf()), command: String::new(),
        args, confirm: get("needsConfirmation") == "true", cwd,
    })
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Custom {
    id: String,
    name: String,
    command: String,
    description: String,
    icon: String,
    in_terminal: bool,
    confirm: bool,
}

pub fn load() -> Vec<Script> {
    let mut out = Vec::new();
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir()).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
    files.sort();
    for path in files {
        if let Ok(text) = std::fs::read_to_string(&path)
            && let Some(script) = parse(&path, &text) {
            out.push(script);
        }
    }
    let custom: Vec<Custom> = util::read_json(&util::config_dir().join("commands.json"));
    for c in custom.into_iter().filter(|c| !c.name.is_empty() && !c.command.is_empty()) {
        out.push(Script {
            id: format!("custom:{}", if c.id.is_empty() { &c.name } else { &c.id }), title: c.name, subtitle: c.description, icon: c.icon,
            mode: if c.in_terminal { Mode::Terminal } else { Mode::Compact }, path: None, command: c.command, args: Vec::new(), confirm: c.confirm, cwd: None,
        });
    }
    out
}

fn icon_of(script: &Script) -> IconSrc {
    let icon = script.icon.trim();
    if icon.is_empty() {
        IconSrc::Badge(&icons::TERMINAL, Tint::Graphite)
    } else if icon.starts_with('/') || icon.starts_with("~/") || icon.contains('.') {
        let path = if let Some(rest) = icon.strip_prefix("~/") { util::home().join(rest) } else if icon.starts_with('/') { PathBuf::from(icon) } else { dir().join(icon) };
        if path.is_file() { IconSrc::File(path) } else { IconSrc::Badge(&icons::TERMINAL, Tint::Graphite) }
    } else {
        IconSrc::Emoji(icon.to_string())
    }
}

/// The launcher's rows for scripts, with what to search them by.
pub fn rows(app: &App) -> Vec<(Item, String)> {
    let mut out: Vec<(Item, String)> = app.scripts.iter().map(|s| {
        let run = Act::Script(ScriptOp::Run(s.id.clone()));
        let run = if s.confirm {
            Act::Confirm(Box::new(Confirm { title: format!("Run {}?", s.title), message: s.subtitle.clone(), icon: &icons::TERMINAL, confirm: "Run".into(), destructive: false, act: run }))
        } else { run };
        let mut item = Item::new(s.id.clone(), s.title.clone(), icon_of(s)).trailing("Script")
            .action(Action::new("Run Script", &icons::BOLT, run));
        if !s.subtitle.is_empty() {
            item = item.subtitle(util::truncate(&s.subtitle, 60));
        }
        item = match &s.path {
            Some(path) => item
                .action(Action::new("Edit Script", &icons::PENCIL, Act::OpenPath(path.clone())).keys("ctrl+e"))
                .action(Action::new("Show in File Manager", &icons::FOLDER, Act::Reveal(path.clone()))),
            None => item
                .action(Action::new("Copy Command", &icons::COPY, Act::Copy(s.command.clone())).keys("ctrl+shift+c"))
                .action(Action::new("Edit commands.json", &icons::PENCIL, Act::OpenPath(util::config_dir().join("commands.json"))).keys("ctrl+e")),
        };
        (item, format!("script command {} {}", s.subtitle, s.command))
    }).collect();
    out.push((Item::new("cmd:new-script", "Create Script Command", IconSrc::Badge(&icons::PLUS, Tint::Graphite)).trailing("Command")
        .action(Action::new("Open Command", &icons::ARROW_UP_RIGHT, Act::Script(ScriptOp::New))), "new script command shell bash python".into()));
    out.push((Item::new("cmd:scripts-folder", "Open Scripts Folder", IconSrc::Badge(&icons::FOLDER, Tint::Graphite)).trailing("Command")
        .action(Action::new("Open Folder", &icons::FOLDER, Act::OpenPath(dir()))), "script commands directory".into()));
    out
}

fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for d in chars.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else if c != '\r' {
            out.push(c);
        }
    }
    out
}

/// Runs a script to its end: its output, both streams, and whether it exited cleanly.
fn execute(script: &Script, args: &[String]) -> (String, bool) {
    let mut cmd = match &script.path {
        Some(path) => {
            let first = std::fs::read_to_string(path).ok().and_then(|t| t.lines().next().map(String::from)).unwrap_or_default();
            match first.strip_prefix("#!") {
                Some(shebang) => {
                    let mut words = shebang.split_whitespace();
                    let mut c = Command::new(words.next().unwrap_or("sh"));
                    c.args(words).arg(path);
                    c
                }
                None => {
                    let mut c = Command::new("sh");
                    c.arg(path);
                    c
                }
            }
        }
        None => {
            let mut c = Command::new("sh");
            c.args(["-c", &script.command]);
            c
        }
    };
    cmd.args(args).current_dir(script.cwd.clone().filter(|d| d.is_dir()).unwrap_or_else(dir))
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    match cmd.output() {
        Ok(out) => {
            let mut text = String::from_utf8_lossy(&out.stdout).to_string();
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.trim().is_empty() {
                if !text.is_empty() && !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&err);
            }
            (strip_ansi(text.trim_end()), out.status.success())
        }
        Err(e) => (format!("Could not start: {e}"), false),
    }
}

fn last_line(output: &str) -> String {
    util::truncate(output.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim(), 80)
}

fn start(app: &mut App, script: Script, args: Vec<String>, tx: &mut Tx) {
    app.usage.record(&script.id);
    if script.mode == Mode::Terminal {
        let line = match &script.path {
            Some(path) => std::iter::once(util::quote(&path.display().to_string())).chain(args.iter().map(|a| util::quote(a))).collect::<Vec<_>>().join(" "),
            None => script.command.clone(),
        };
        let held = format!("{line}; echo; printf 'Press Return to close…'; read _");
        let terminal = if util::has_program("xdg-terminal-exec") { "xdg-terminal-exec" } else { "x-terminal-emulator" };
        util::spawn_detached(&[terminal.into(), "sh".into(), "-c".into(), held]);
        return app.hide(tx);
    }
    let me = app.me.clone();
    let visible = app.visible;
    match script.mode {
        Mode::Silent => app.hide(tx),
        _ => app.toast_progress(format!("Running {}…", script.title)),
    }
    std::thread::spawn(move || {
        let (output, ok) = execute(&script, &args);
        let tone = if ok { Tone::Success } else { Tone::Danger };
        let line = last_line(&output);
        let summary = if !line.is_empty() { line } else if ok { format!("{} Finished", script.title) } else { format!("{} Failed", script.title) };
        if script.mode == Mode::Silent || !visible {
            return crate::hud::show(&summary, tone);
        }
        let Some(me) = me else { return };
        me.update(move |s: &mut App, tx| {
            // The palette may have been put away while the script ran.
            if !s.visible {
                s.toast = None;
                return crate::hud::show(&summary, tone);
            }
            if script.mode == Mode::Full {
                s.toast = None;
                let body = if output.is_empty() { "*No output.*".to_string() } else { format!("```\n{output}\n```") };
                s.open_page(Page {
                    title: script.title.clone(), markdown: if ok { body } else { format!("**Exited with an error.**\n\n{body}") },
                    actions: vec![
                        Action::new("Copy Output", &icons::COPY, Act::Copy(output.clone())),
                        Action::new("Run Again", &icons::REFRESH, Act::Script(ScriptOp::RunWith(script.id.clone(), args.clone()))).keys("ctrl+r"),
                    ],
                }, tx);
            } else {
                s.toast(summary, tone);
            }
        });
    });
}

fn arguments_form(script: &Script) -> Form {
    let fields = script.args.iter().enumerate().map(|(i, a)| {
        let label = if a.placeholder.is_empty() { format!("Argument {}", i + 1) } else { a.placeholder.clone() };
        let kind = if !a.options.is_empty() { FieldKind::Dropdown(a.options.clone()) } else if a.secret { FieldKind::Password } else { FieldKind::Text };
        let field = Field::new(&format!("arg{i}"), &label, kind);
        if a.optional { field } else { field.required() }
    }).collect();
    Form::new(&script.title, "Run Script", fields, Target::Script(script.id.clone()))
}

fn new_form() -> Form {
    let modes = vec![("compact".to_string(), "Compact: last line in a toast".to_string()), ("fullOutput".into(), "Full Output: a page".into()),
        ("silent".into(), "Silent: nothing but a HUD".into()), ("terminal".into(), "Terminal: runs in a terminal window".into())];
    let languages = vec![("bash".to_string(), "Bash".to_string()), ("python3".into(), "Python".into()), ("node".into(), "Node.js".into())];
    Form::new("Create Script Command", "Create Script", vec![
        Field::new("title", "Title", FieldKind::Text).placeholder("Deploy Site").required(),
        Field::new("mode", "Mode", FieldKind::Dropdown(modes)),
        Field::new("language", "Language", FieldKind::Dropdown(languages)),
        Field::new("argument", "Argument", FieldKind::Text).placeholder("Optional: what to ask for before running"),
    ], Target::ScriptNew)
}

pub fn perform(app: &mut App, op: ScriptOp, tx: &mut Tx) {
    match op {
        ScriptOp::Run(id) => {
            let Some(script) = app.scripts.iter().find(|s| s.id == id).cloned() else { return };
            if script.args.is_empty() { start(app, script, Vec::new(), tx) } else { forms::open(app, arguments_form(&script), tx) }
        }
        ScriptOp::RunWith(id, args) => {
            if let Some(script) = app.scripts.iter().find(|s| s.id == id).cloned() {
                if app.top().route == Route::Page {
                    app.pop(tx);
                }
                start(app, script, args, tx);
            }
        }
        ScriptOp::New => forms::open(app, new_form(), tx),
    }
}

/// The arguments form was submitted.
pub fn submit(app: &mut App, id: String, tx: &mut Tx) {
    let Some(form) = &app.form else { return };
    let Some(script) = app.scripts.iter().find(|s| s.id == id).cloned() else { return };
    let args: Vec<String> = (0..script.args.len()).map(|i| form.get(&format!("arg{i}"))).collect();
    app.pop(tx);
    start(app, script, args, tx);
}

pub fn create(app: &mut App, tx: &mut Tx) {
    let Some(form) = &app.form else { return };
    let (title, mode, language, argument) = (form.get("title").trim().to_string(), form.get("mode"), form.get("language"), form.get("argument").trim().to_string());
    let slug: String = title.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>().split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    let (ext, comment, body) = match language.as_str() {
        "python3" => ("py", "#", if argument.is_empty() { "print(\"Hello from EchoSpot\")\n" } else { "import sys\n\nprint(f\"Hello, {sys.argv[1]}\")\n" }),
        "node" => ("js", "//", if argument.is_empty() { "console.log(\"Hello from EchoSpot\");\n" } else { "console.log(`Hello, ${process.argv[2]}`);\n" }),
        _ => ("sh", "#", if argument.is_empty() { "echo \"Hello from EchoSpot\"\n" } else { "echo \"Hello, $1\"\n" }),
    };
    let mut path = dir().join(format!("{}.{ext}", if slug.is_empty() { "script" } else { &slug }));
    let mut n = 2;
    while path.exists() {
        path = dir().join(format!("{slug}-{n}.{ext}"));
        n += 1;
    }
    let mut text = format!("#!/usr/bin/env {language}\n\n{comment} @raycast.schemaVersion 1\n{comment} @raycast.title {title}\n{comment} @raycast.mode {mode}\n{comment} @raycast.packageName Scripts\n");
    if !argument.is_empty() {
        text.push_str(&format!("{comment} @raycast.argument1 {}\n", serde_json::json!({ "type": "text", "placeholder": argument })));
    }
    text.push_str(&format!("\n{body}"));
    if std::fs::write(&path, text).is_err() {
        return app.toast("Could Not Write the Script", Tone::Danger);
    }
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
    app.scripts = load();
    app.pop(tx);
    app.perform(Act::OpenPath(path), tx);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers() {
        let text = "#!/bin/bash\n\n# Required parameters:\n# @raycast.schemaVersion 1\n# @raycast.title Deploy Site\n# @raycast.mode fullOutput\n# @raycast.icon 🚀\n# @raycast.argument1 { \"type\": \"text\", \"placeholder\": \"Branch\", \"optional\": true }\n# @raycast.needsConfirmation true\necho hi\n";
        let s = parse(std::path::Path::new("/tmp/deploy.sh"), text).unwrap();
        assert_eq!((s.title.as_str(), s.mode, s.icon.as_str(), s.confirm), ("Deploy Site", Mode::Full, "🚀", true));
        assert_eq!((s.args.len(), s.args[0].placeholder.as_str(), s.args[0].optional), (1, "Branch", true));
        assert_eq!(s.id, "script:deploy.sh");
        assert!(parse(std::path::Path::new("/tmp/x.sh"), "#!/bin/sh\necho no header\n").is_none());
        let js = parse(std::path::Path::new("/tmp/a.js"), "#!/usr/bin/env node\n// @raycast.title From Node\n// @raycast.mode silent\n").unwrap();
        assert_eq!((js.title.as_str(), js.mode), ("From Node", Mode::Silent));
    }

    #[test]
    fn output() {
        assert_eq!(strip_ansi("\u{1b}[32mok\u{1b}[0m\r\n"), "ok\n");
        assert_eq!(last_line("one\ntwo\n\n"), "two");
    }
}
