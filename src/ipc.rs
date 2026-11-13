//! The socket a second `echospot` invocation speaks to the resident one over.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use rune::AppHandle;

use crate::app::{App, Screen};
use crate::model::Route;

fn socket() -> PathBuf {
    crate::util::runtime_dir().join("echospot.sock")
}

/// Sends a command to the resident palette; false when none is listening.
pub fn send(command: &str) -> bool {
    let Ok(mut stream) = UnixStream::connect(socket()) else { return false };
    stream.write_all(command.as_bytes()).is_ok()
}

/// Sends a command and reads what the resident palette answers.
pub fn ask(command: &str) -> Option<String> {
    let mut stream = UnixStream::connect(socket()).ok()?;
    stream.write_all(command.as_bytes()).ok()?;
    stream.shutdown(std::net::Shutdown::Write).ok()?;
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(6)));
    let mut text = String::new();
    stream.read_to_string(&mut text).ok()?;
    Some(text)
}

pub fn route(name: &str) -> Option<Route> {
    Some(match name {
        "clipboard" => Route::Clipboard,
        "emoji" => Route::Emoji,
        "files" => Route::Files,
        "windows" => Route::Windows,
        "snap" => Route::Snap(None),
        "snippets" => Route::Snippets,
        "quicklinks" => Route::Quicklinks,
        "notes" => Route::Notes,
        "schedule" => Route::Schedule,
        "store" => Route::Store,
        "themes" => Route::Themes,
        "ai" => Route::Ai,
        "extensions" => Route::Extensions,
        _ => return None,
    })
}

fn show(handle: &AppHandle<App>, route: Option<Route>) {
    handle.update(move |s, _| {
        s.reset();
        s.will_show();
        if let Some(route) = route {
            s.screens.push(Screen::new(route));
            if s.top().route == Route::Clipboard {
                s.clipboard.reload();
            }
            if s.top().route == Route::Notes {
                s.notes = crate::features::notes::load();
            }
            if matches!(s.top().route, Route::Snap(_)) {
                s.snap = crate::features::snap::Snap::load(s.prev_window.as_ref());
            }
            s.refresh();
        }
    });
    handle.open();
}

fn hide(handle: &AppHandle<App>) {
    handle.update(|s, tx| s.hide(tx));
}

pub fn serve(handle: AppHandle<App>) {
    let path = socket();
    let _ = std::fs::remove_file(&path);
    let Ok(listener) = UnixListener::bind(&path) else { return };
    std::thread::Builder::new().name("echospot-ipc".into()).spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut text = String::new();
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(300)));
            if stream.read_to_string(&mut text).is_err() {
                continue;
            }
            // `run <id>`: a hotkey. The id is the rest of the line; some hold spaces.
            if let Some(id) = text.strip_prefix("run ").map(|id| id.trim().to_string()) {
                let (tx, rx) = std::sync::mpsc::channel();
                handle.update(move |s, t| {
                    let _ = tx.send(s.run_id(&id, t));
                });
                if rx.recv_timeout(std::time::Duration::from_secs(5)) == Ok(true) {
                    handle.open();
                }
                continue;
            }
            if text.trim() == "commands" {
                let (tx, rx) = std::sync::mpsc::channel();
                handle.update(move |s, _| {
                    let _ = tx.send(crate::features::root::listing(s));
                });
                if let Ok(list) = rx.recv_timeout(std::time::Duration::from_secs(5)) {
                    let _ = stream.write_all(list.as_bytes());
                }
                continue;
            }
            let mut words = text.split_whitespace();
            match words.next() {
                Some("toggle") => {
                    if handle.is_visible() { hide(&handle) } else { show(&handle, None) }
                }
                Some("show") => show(&handle, None),
                Some("open") => show(&handle, words.next().and_then(route)),
                Some("hide") => hide(&handle),
                // A `raycast://` link from the browser: the end of an extension's sign-in.
                Some("url") => {
                    let url = words.next().unwrap_or("").to_string();
                    let (tx, rx) = std::sync::mpsc::channel();
                    handle.update(move |s, t| {
                        let _ = tx.send(crate::ext::open_link(s, &url, t));
                    });
                    if rx.recv_timeout(std::time::Duration::from_secs(2)) == Ok(true) {
                        handle.open();
                    }
                }
                Some("settings") => crate::settings_ui::open(words.next().unwrap_or("")),
                Some("reload") => handle.update(|s, _| s.reload_stores()),
                Some("quit") => handle.quit(),
                _ => {}
            }
        }
    }).ok();
}
