//! The shortcut that summons EchoSpot. It has to work while EchoSpot is not running (it is what
//! starts it), so it lives in Hyprland's own config as a binding to `echospot toggle`, and this
//! is the one place that reads and rewrites that line. A change is written to the file, so it
//! survives a reload and a restart, and applied to the running compositor at once.

use std::path::{Path, PathBuf};

use crate::features::bindings::hypr_chord;
use crate::util;

/// What Omarchy binds the launcher to, and what a config without the line is given.
pub const DEFAULT: &str = "super+space";
const COMMAND: &str = "echospot toggle";
const DESCRIPTION: &str = "EchoSpot Launcher";
const MARK: &str = "-- EchoSpot: the summon shortcut, set in EchoSpot Settings → General.";
const MARK_CONF: &str = "# EchoSpot: the summon shortcut, set in EchoSpot Settings → General.";

#[derive(Clone, Copy, Debug, PartialEq)]
enum Syntax {
    /// Hyprland's Lua config, as Omarchy writes it: `o.bind("SUPER + SPACE", …)`.
    Lua,
    /// hyprlang: `bind = SUPER, SPACE, exec, echospot toggle`.
    Conf,
}

fn hypr_dir() -> PathBuf {
    std::env::var("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|_| util::home().join(".config")).join("hypr")
}

/// The config file that holds, or should hold, the binding: the one that already binds
/// `echospot toggle`, else the personal bindings file, else the main config.
pub fn config_file() -> Option<PathBuf> {
    let dir = hypr_dir();
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("lua" | "conf")))
        .collect();
    candidates.sort();
    if let Some(found) = candidates.iter().find(|p| std::fs::read_to_string(p).is_ok_and(|t| !find_all(&t, syntax_of(p)).is_empty())) {
        return Some(found.clone());
    }
    ["bindings.lua", "hyprland.lua", "bindings.conf", "hyprland.conf"].iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

fn syntax_of(path: &Path) -> Syntax {
    if path.extension().and_then(|e| e.to_str()) == Some("lua") { Syntax::Lua } else { Syntax::Conf }
}

/// The first string literal on a line, and where it sits: `"SUPER + SPACE"`.
fn first_string(line: &str) -> Option<(usize, usize, &str)> {
    let open = line.find('"')?;
    let close = open + 1 + line[open + 1..].find('"')?;
    Some((open, close + 1, &line[open + 1..close]))
}

/// Every line that binds the summon command, and the chord on each in Hyprland's spelling.
/// Super by itself takes two: one for each Super key.
fn find_all(text: &str, syntax: Syntax) -> Vec<(usize, String)> {
    text.lines().enumerate().filter_map(|(n, line)| {
        let code = line.trim_start();
        if code.starts_with("--") || code.starts_with('#') || !line.contains(COMMAND) {
            return None;
        }
        match syntax {
            Syntax::Lua if code.contains("bind(") && !code.contains("unbind(") => first_string(line).map(|(_, _, chord)| (n, chord.to_string())),
            Syntax::Conf if code.starts_with("bind") => {
                let (_, rest) = code.split_once('=')?;
                let mut parts = rest.split(',').map(str::trim);
                let (mods, key) = (parts.next()?, parts.next()?);
                let mods: Vec<&str> = mods.split(|c: char| c == ' ' || c == '_').filter(|m| !m.is_empty()).collect();
                Some((n, mods.iter().copied().chain([key]).collect::<Vec<_>>().join(" + ")))
            }
            _ => None,
        }
    }).collect()
}

#[cfg(test)]
fn find(text: &str, syntax: Syntax) -> Option<(usize, String)> {
    find_all(text, syntax).into_iter().next()
}

/// Super pressed and let go by itself, as the palette spells it.
pub const SUPER_ALONE: &str = "super";

/// Whether a Hyprland chord is Super by itself: the key and its own modifier.
fn is_super_alone(hypr: &str) -> bool {
    let parts: Vec<String> = hypr.split('+').map(|p| p.trim().to_ascii_uppercase()).collect();
    matches!(parts.iter().map(String::as_str).collect::<Vec<_>>().as_slice(), ["SUPER", "SUPER_L" | "SUPER_R"] | ["SUPER_L" | "SUPER_R"])
}

/// The bindings a chord (the palette's spelling) takes, each with whether it fires on release.
/// Super by itself fires as it is let go, on either Super key, the way Omarchy binds its menu,
/// so Super held for a chord such as Super+F never fires it.
fn binds_for(chord: &str) -> Option<Vec<(String, bool)>> {
    if chord.trim().eq_ignore_ascii_case(SUPER_ALONE) {
        return Some(vec![("SUPER + SUPER_L".into(), true), ("SUPER + SUPER_R".into(), true)]);
    }
    hypr_chord(chord).map(|c| vec![(c, false)])
}

/// Follows a recorder's keys for Super tapped by itself: true as it is let go with nothing else
/// pressed while it was down. `lone` is the recorder's note that it is so far.
pub fn super_tapped(ev: &rune::KeyEvent, lone: &mut bool) -> bool {
    use rune::{Key, Modifier};
    match ev.key {
        Key::Modifier { modifier: Modifier::SuperLeft | Modifier::SuperRight, pressed: true } => {
            let m = ev.mods;
            *lone = !(m.ctrl || m.cmd || m.alt || m.shift);
            false
        }
        Key::Modifier { modifier: Modifier::SuperLeft | Modifier::SuperRight, pressed: false } => std::mem::take(lone),
        _ => {
            *lone = false;
            false
        }
    }
}

/// Whether a chord can summon EchoSpot: one with Super, Ctrl or Alt, or Super by itself.
pub fn usable(chord: &str) -> bool {
    binds_for(chord).is_some()
}

/// `SUPER + SPACE` as the palette spells a chord: `super+space`.
fn palette_chord(hypr: &str) -> String {
    if is_super_alone(hypr) {
        return SUPER_ALONE.to_string();
    }
    hypr.split('+').map(|p| p.trim().to_lowercase()).filter(|p| !p.is_empty()).map(|p| match p.as_str() {
        "control" => "ctrl".to_string(),
        "mod4" | "win" => "super".to_string(),
        _ => p,
    }).collect::<Vec<_>>().join("+")
}

/// The chord that summons EchoSpot now, as the config file has it.
pub fn current() -> String {
    config_file().and_then(|path| {
        let text = std::fs::read_to_string(&path).ok()?;
        find_all(&text, syntax_of(&path)).first().map(|(_, chord)| palette_chord(chord))
    }).unwrap_or_else(|| DEFAULT.to_string())
}

/// `SUPER + ALT + C` as hyprlang writes a bind's first two fields: `SUPER ALT, C`.
fn conf_fields(hypr: &str) -> String {
    let parts: Vec<&str> = hypr.split('+').map(str::trim).collect();
    let (key, mods) = parts.split_last().map(|(k, m)| (*k, m.join(" "))).unwrap_or(("", String::new()));
    format!("{mods}, {key}")
}

/// A Lua bind line made from `template` for another chord: its first string replaced, and its
/// options saying `release = true` or not.
fn lua_line(template: &str, hypr: &str, release: bool) -> String {
    let mut line = template.to_string();
    if let Some((start, end, _)) = first_string(&line) {
        line.replace_range(start..end, &format!("\"{hypr}\""));
    }
    let line = line.replace(", { release = true }", "");
    if release && let Some(close) = line.rfind(')') {
        return format!("{}, {{ release = true }}{}", &line[..close], &line[close..]);
    }
    line
}

/// A hyprlang bind line made from `template` for another chord: `bind` grows or loses its `r`
/// flag, the mods and key are replaced, and the rest (a description, the dispatcher) stays.
fn conf_line(template: &str, hypr: &str, release: bool) -> String {
    let indent: String = template.chars().take_while(|c| c.is_whitespace()).collect();
    let (head, rest) = template.trim_start().split_once('=').unwrap_or(("bind", ""));
    let flags: String = head.trim().trim_start_matches("bind").chars().filter(|c| *c != 'r').collect();
    let head = format!("bind{flags}{}", if release { "r" } else { "" });
    let tail: Vec<&str> = rest.splitn(3, ',').skip(2).collect();
    format!("{indent}{head} = {},{}", conf_fields(hypr), tail.first().copied().unwrap_or(" exec, echospot toggle"))
}

/// The config text with the summon bound as `binds` say: the lines that bound it before (and
/// the unbind written beside each) replaced where the first of them was, or the binding added
/// at the end. Everything else is left as it was.
fn rewrite(text: &str, syntax: Syntax, binds: &[(String, bool)]) -> String {
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let found = find_all(text, syntax);
    let unbind = |chord: &str| match syntax {
        Syntax::Lua => format!("hl.unbind(\"{chord}\")"),
        Syntax::Conf => format!("unbind = {}", conf_fields(chord)),
    };
    if found.is_empty() {
        if lines.last().is_some_and(|l| !l.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(match syntax { Syntax::Lua => MARK, Syntax::Conf => MARK_CONF }.to_string());
        for (chord, release) in binds {
            lines.push(unbind(chord));
            lines.push(match syntax {
                Syntax::Lua => lua_line(&format!("o.bind(\"\", \"{DESCRIPTION}\", \"{COMMAND}\")"), chord, *release),
                Syntax::Conf => conf_line(&format!("bind = SUPER, SPACE, exec, {COMMAND}"), chord, *release),
            });
        }
    } else {
        let template = lines[found[0].0].clone();
        let indent: String = template.chars().take_while(|c| c.is_whitespace()).collect();
        // The binding lines go, with the unbind written right before each of them.
        let mut gone: Vec<usize> = Vec::new();
        for (n, chord) in &found {
            gone.push(*n);
            if *n > 0 && lines[n - 1].trim() == unbind(chord) {
                gone.push(n - 1);
            }
        }
        gone.sort_unstable();
        gone.dedup();
        let at = gone[0];
        for n in gone.iter().rev() {
            lines.remove(*n);
        }
        let mut block = Vec::new();
        for (chord, release) in binds {
            // A chord the file already lets go of elsewhere needs no unbind of its own. Only Lua
            // unbinds in place: hyprlang's binds are unbound by the bind itself replacing them.
            if syntax == Syntax::Lua && !lines.iter().any(|l| l.trim() == unbind(chord)) {
                block.push(format!("{indent}{}", unbind(chord)));
            }
            block.push(match syntax {
                Syntax::Lua => lua_line(&template, chord, *release),
                Syntax::Conf => conf_line(&template, chord, *release),
            });
        }
        lines.splice(at..at, block);
    }
    let mut out = lines.join("\n");
    if text.ends_with('\n') || text.is_empty() {
        out.push('\n');
    }
    out
}

fn lua_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Lets go of a chord in the running compositor, so a recorder hears it instead.
pub fn release(chord: &str) {
    let Some(binds) = binds_for(chord) else { return };
    let Some(path) = config_file() else { return };
    let syntax = syntax_of(&path);
    std::thread::spawn(move || {
        for (hypr, _) in binds {
            match syntax {
                Syntax::Lua => util::output("hyprctl", &["eval", &format!("hl.unbind({})", lua_string(&hypr))]),
                Syntax::Conf => util::output("hyprctl", &["keyword", "unbind", &conf_fields(&hypr)]),
            };
        }
    });
}

/// Binds the summon in the running compositor as `binds` say, letting go of `old` first.
fn bind_live(old: &[String], binds: &[(String, bool)], syntax: Syntax) {
    if !crate::features::windows::hyprland() {
        return;
    }
    let new: Vec<&str> = binds.iter().map(|(c, _)| c.as_str()).collect();
    let calls: Vec<Vec<String>> = match syntax {
        Syntax::Lua => {
            let mut lua = String::new();
            for old in old.iter().filter(|o| !new.contains(&o.as_str())) {
                lua.push_str(&format!("hl.unbind({}); ", lua_string(old)));
            }
            for (hypr, release) in binds {
                lua.push_str(&format!("hl.unbind({c}); hl.bind({c}, hl.dsp.exec_cmd({}), {{ description = {}{} }}); ",
                    lua_string(COMMAND), lua_string(DESCRIPTION), if *release { ", release = true" } else { "" }, c = lua_string(hypr)));
            }
            vec![vec!["eval".into(), lua]]
        }
        Syntax::Conf => {
            let mut calls: Vec<Vec<String>> = old.iter().filter(|o| !new.contains(&o.as_str()))
                .map(|o| vec!["keyword".into(), "unbind".into(), conf_fields(o)]).collect();
            for (hypr, release) in binds {
                calls.push(vec!["keyword".into(), "unbind".into(), conf_fields(hypr)]);
                calls.push(vec!["keyword".into(), if *release { "bindr" } else { "bind" }.into(), format!("{}, exec, {COMMAND}", conf_fields(hypr))]);
            }
            calls
        }
    };
    std::thread::spawn(move || {
        for call in calls {
            util::output("hyprctl", &call.iter().map(String::as_str).collect::<Vec<_>>());
        }
    });
}

/// The Hyprland submap a recorder listens in: it binds nothing anyone can press, so while it is
/// on, every key (Super by itself, which Omarchy's menu takes on release, included) reaches the
/// window that has the keyboard instead of a binding.
const SUBMAP: &str = "echospot-record";

fn token_file() -> PathBuf {
    util::runtime_dir().join("echospot-record")
}

/// Takes every key press for a recorder until `release_keys`. A minute later it is given back
/// whatever happens, by a process of its own, so a recorder that crashed can't keep the keyboard.
pub fn capture_keys() {
    if !crate::features::windows::hyprland() {
        return;
    }
    let lua = config_file().is_none_or(|p| syntax_of(&p) == Syntax::Lua);
    if lua {
        // A submap with nothing in it isn't registered: it holds one chord no keyboard has, a way out.
        let define = format!("hl.define_submap({s}, function() hl.bind(\"SUPER + CTRL + ALT + SHIFT + F24\", hl.dsp.submap(\"reset\")) end)", s = lua_string(SUBMAP));
        util::output("hyprctl", &["eval", &define]);
        util::output("hyprctl", &["dispatch", &format!("hl.dsp.submap({})", lua_string(SUBMAP))]);
    } else {
        util::output("hyprctl", &["--batch", &format!("keyword submap {SUBMAP}; keyword bind SUPER CTRL ALT SHIFT, F24, submap, reset; keyword submap reset")]);
        util::output("hyprctl", &["dispatch", "submap", SUBMAP]);
    }
    let token = format!("{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0));
    let file = token_file();
    let _ = std::fs::write(&file, &token);
    let reset = if lua { "hyprctl dispatch 'hl.dsp.submap(\"reset\")'" } else { "hyprctl dispatch submap reset" };
    let file = util::quote(&file.display().to_string());
    util::sh_detached(&format!("sleep 60; [ \"$(cat {file} 2>/dev/null)\" = {} ] && rm -f {file} && {reset}", util::quote(&token)));
}

/// Gives the key presses back to Hyprland's bindings.
pub fn release_keys() {
    if std::fs::remove_file(token_file()).is_err() || !crate::features::windows::hyprland() {
        return;
    }
    if util::output("hyprctl", &["submap"]).as_deref() != Some(SUBMAP) {
        return;
    }
    if util::output("hyprctl", &["dispatch", "hl.dsp.submap(\"reset\")"]).is_none_or(|o| o.contains("rror")) {
        util::output("hyprctl", &["dispatch", "submap", "reset"]);
    }
}

/// Puts the summon back on the chord the config has, after a recording let go of it.
pub fn restore() {
    let Some(path) = config_file() else { return };
    if let Some(binds) = binds_for(&current()) {
        bind_live(&[], &binds, syntax_of(&path));
    }
}

/// Makes `chord` (the palette's spelling, `super+alt+space`, or `super` for Super by itself) the
/// summon: written to Hyprland's config and bound at once. The error is a sentence to show.
pub fn set(chord: &str) -> Result<(), String> {
    let binds = binds_for(chord).ok_or_else(|| "Add Super, Ctrl or Alt".to_string())?;
    let path = config_file().ok_or_else(|| format!("No Hyprland config in {}", util::tilde(&hypr_dir())))?;
    // A config kept elsewhere and linked in (a dotfiles folder) is written where it really is.
    let real = std::fs::canonicalize(&path).unwrap_or(path.clone());
    let text = std::fs::read_to_string(&real).map_err(|_| format!("Could not read {}", util::tilde(&path)))?;
    let syntax = syntax_of(&path);
    let old: Vec<String> = find_all(&text, syntax).into_iter().map(|(_, chord)| chord).collect();
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(&real).map(|m| m.permissions().mode() & 0o777).unwrap_or(0o644)
    };
    if !util::write_file(&real, rewrite(&text, syntax, &binds).as_bytes(), mode) {
        return Err(format!("Could not write {}", util::tilde(&path)));
    }
    bind_live(&old, &binds, syntax);
    Ok(())
}

// ---- The palette's own recorder ---------------------------------------------------------

/// The Change EchoSpot Shortcut screen: what it shows, and whether the new chord was taken (so
/// leaving doesn't put the old one back over it).
#[derive(Default)]
pub struct Recorder {
    pub current: String,
    pub error: String,
    pub saved: bool,
    /// Super is down with nothing else so far: let go, it is the shortcut by itself.
    pub lone: bool,
}

/// The screen opens: the summon chord is let go of, and the commands' hotkeys too, so whatever
/// is pressed reaches the palette instead of running something.
pub fn begin(app: &mut crate::app::App) {
    let current = current();
    release(&current);
    app.bindings.suspend();
    capture_keys();
    app.recorder = Recorder { current, error: String::new(), saved: false, lone: false };
    // The search field lets go of the keyboard: a focused field takes a modifier pressed by
    // itself for its own, and Super tapped alone would never reach the recorder.
    app.top_mut().query.focused = false;
}

/// The screen is left, however: the old chord comes back unless a new one was taken.
pub fn end(app: &mut crate::app::App) {
    release_keys();
    if !app.recorder.saved {
        restore();
    }
    app.recorder = Recorder::default();
    app.bindings.apply();
}

pub fn build(app: &crate::app::App) -> crate::app::Built {
    let caps = |c: &str| crate::model::keycaps(c).join(" ");
    let title = if app.recorder.error.is_empty() { "Press the new shortcut".to_string() } else { app.recorder.error.clone() };
    let hint = format!("Now {}. Tap Super alone, or hold Super, Ctrl or Alt with a key. Esc keeps it; ⌫ puts back {}.", caps(&app.recorder.current), caps(DEFAULT));
    crate::app::Built::list(crate::model::Sections::default(), "Press the new shortcut…").empty(&title, &hint)
}

/// A key on the screen: Escape leaves, Backspace puts back Omarchy's chord, and a chord with
/// Super, Ctrl or Alt that no command's hotkey has becomes the shortcut.
pub fn key(app: &mut crate::app::App, ev: &rune::KeyEvent, tx: &mut rune::Tx) -> bool {
    use rune::Key;
    let tapped = super_tapped(ev, &mut app.recorder.lone);
    if !tapped && let Key::Modifier { .. } = ev.key {
        return true;
    }
    let m = ev.mods;
    let bare = !(m.logo || m.ctrl || m.cmd || m.alt || m.shift);
    let chord = match ev.key {
        _ if tapped => SUPER_ALONE.to_string(),
        Key::Escape if bare => {
            app.pop(tx);
            return true;
        }
        Key::Backspace | Key::Delete if bare => DEFAULT.to_string(),
        _ => match crate::features::bindings::chord_of(ev, tx.key_base()) {
            Some(chord) => chord,
            None => return false,
        },
    };
    let refused = if !usable(&chord) {
        Some("Add Super, Ctrl or Alt".to_string())
    } else if let Some(other) = app.bindings.holder(&chord, "") {
        let other = other.to_string();
        let title = crate::features::root::catalog(app).into_iter().find(|c| c.id == other).map(|c| c.title).unwrap_or(other);
        Some(format!("Used by {title}"))
    } else {
        set(&chord).err()
    };
    match refused {
        Some(why) => {
            app.recorder.error = why;
            app.top_mut().query.clear();
            app.refresh();
        }
        None => {
            app.recorder.saved = true;
            app.pop(tx);
            app.toast(format!("EchoSpot Opens with {}", crate::model::keycaps(&current()).join(" ")), crate::app::Tone::Success);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const OMARCHY: &str = "-- personal\nhl.unbind(\"SUPER + SPACE\")\no.bind(\"SUPER + S\", \"Snap\", \"snap\")\n\n-- Raycast-style launcher.\no.bind(\"SUPER + SPACE\", \"EchoSpot Launcher\", \"echospot toggle\")\n\nhl.bind(\"CTRL + SHIFT + 2\", x)\n";

    #[test]
    fn reads_the_chord_in_either_syntax() {
        assert_eq!(find(OMARCHY, Syntax::Lua), Some((5, "SUPER + SPACE".into())));
        assert_eq!(palette_chord("SUPER + SPACE"), "super+space");
        let conf = "bind = SUPER, R, exec, wofi\nbindd = SUPER SHIFT, SPACE, Launcher, exec, echospot toggle\n";
        assert_eq!(find(conf, Syntax::Conf), Some((1, "SUPER + SHIFT + SPACE".into())));
        // A commented-out line is not the binding.
        assert_eq!(find("-- o.bind(\"SUPER + X\", nil, \"echospot toggle\")\n", Syntax::Lua), None);
    }

    #[test]
    fn rewrites_only_the_binding() {
        let out = rewrite(OMARCHY, Syntax::Lua, &binds_for("super+alt+space").unwrap());
        assert_eq!(out, "-- personal\nhl.unbind(\"SUPER + SPACE\")\no.bind(\"SUPER + S\", \"Snap\", \"snap\")\n\n-- Raycast-style launcher.\nhl.unbind(\"SUPER + ALT + SPACE\")\no.bind(\"SUPER + ALT + SPACE\", \"EchoSpot Launcher\", \"echospot toggle\")\n\nhl.bind(\"CTRL + SHIFT + 2\", x)\n");
        // Changed again, the unbind it wrote moves with it rather than piling up.
        let again = rewrite(&out, Syntax::Lua, &binds_for("ctrl+space").unwrap());
        assert!(again.contains("hl.unbind(\"CTRL + SPACE\")\no.bind(\"CTRL + SPACE\", \"EchoSpot Launcher\", \"echospot toggle\")"));
        assert!(!again.contains("SUPER + ALT + SPACE"));
        assert_eq!(find(&again, Syntax::Lua).map(|(_, c)| c), Some("CTRL + SPACE".into()));
        // Back to Omarchy's chord, which the file already unbinds near its top: it reads as it began.
        assert_eq!(rewrite(&out, Syntax::Lua, &binds_for("super+space").unwrap()), OMARCHY);
    }

    #[test]
    fn adds_a_missing_binding() {
        let out = rewrite("-- nothing here\n", Syntax::Lua, &binds_for("super+space").unwrap());
        assert!(out.ends_with("o.bind(\"SUPER + SPACE\", \"EchoSpot Launcher\", \"echospot toggle\")\n"));
        assert_eq!(find(&out, Syntax::Lua).map(|(_, c)| c), Some("SUPER + SPACE".into()));
        let conf = rewrite("bind = SUPER, R, exec, wofi\n", Syntax::Conf, &binds_for("super+alt+space").unwrap());
        assert!(conf.ends_with("bind = SUPER ALT, SPACE, exec, echospot toggle\n"));
        assert_eq!(find(&conf, Syntax::Conf).map(|(_, c)| c), Some("SUPER + ALT + SPACE".into()));
    }

    #[test]
    fn keeps_a_conf_lines_dispatcher() {
        let conf = "bindd = SUPER, SPACE, Launcher, exec, echospot toggle\n";
        assert_eq!(rewrite(conf, Syntax::Conf, &binds_for("ctrl+alt+k").unwrap()), "bindd = CTRL ALT, K, Launcher, exec, echospot toggle\n");
    }

    #[test]
    fn super_by_itself_takes_both_super_keys_on_release() {
        let out = rewrite(OMARCHY, Syntax::Lua, &binds_for("super").unwrap());
        assert!(out.contains("hl.unbind(\"SUPER + SUPER_L\")\no.bind(\"SUPER + SUPER_L\", \"EchoSpot Launcher\", \"echospot toggle\", { release = true })\nhl.unbind(\"SUPER + SUPER_R\")\no.bind(\"SUPER + SUPER_R\", \"EchoSpot Launcher\", \"echospot toggle\", { release = true })"), "{out}");
        assert!(!out.contains("o.bind(\"SUPER + SPACE\""));
        let found: Vec<String> = find_all(&out, Syntax::Lua).into_iter().map(|(_, c)| palette_chord(&c)).collect();
        assert_eq!(found, ["super", "super"]);
        // And back: one binding again, the file as it began.
        assert_eq!(rewrite(&out, Syntax::Lua, &binds_for("super+space").unwrap()), OMARCHY);
        let conf = rewrite("bindd = SUPER, SPACE, Launcher, exec, echospot toggle\n", Syntax::Conf, &binds_for("super").unwrap());
        assert_eq!(conf, "binddr = SUPER, SUPER_L, Launcher, exec, echospot toggle\nbinddr = SUPER, SUPER_R, Launcher, exec, echospot toggle\n");
        assert!(usable("super") && !usable("shift") && !usable("k"));
    }

    #[test]
    fn super_tapped_alone_is_told_from_super_held_for_a_chord() {
        use rune::{Key, KeyEvent, Modifier, Mods};
        let sup = |pressed, logo| KeyEvent { key: Key::Modifier { modifier: Modifier::SuperLeft, pressed }, mods: Mods { logo, ..Default::default() } };
        let mut lone = false;
        assert!(!super_tapped(&sup(true, true), &mut lone));
        assert!(super_tapped(&sup(false, false), &mut lone));
        // Super held for Super+K: the K spoils the tap.
        assert!(!super_tapped(&sup(true, true), &mut lone));
        assert!(!super_tapped(&KeyEvent { key: Key::Text("k".into()), mods: Mods { logo: true, ..Default::default() } }, &mut lone));
        assert!(!super_tapped(&sup(false, false), &mut lone));
        // Super pressed while Ctrl is down is no tap of Super alone.
        assert!(!super_tapped(&KeyEvent { key: Key::Modifier { modifier: Modifier::SuperLeft, pressed: true }, mods: Mods { logo: true, ctrl: true, cmd: true, ..Default::default() } }, &mut lone));
        assert!(!super_tapped(&sup(false, false), &mut lone));
    }
}
