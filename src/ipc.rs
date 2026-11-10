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

pub fn route(name: &str) -> Option<Route> {
    Some(match name {
        "clipboard" => Route::Clipboard,
        "emoji" => Route::Emoji,
        "files" => Route::Files,
        "windows" => Route::Windows,
        "snippets" => Route::Snippets,
        "quicklinks" => Route::Quicklinks,
        "store" => Route::Store,
        "themes" => Route::Themes,
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
            let mut words = text.split_whitespace();
            match words.next() {
                Some("toggle") => {
                    if handle.is_visible() { hide(&handle) } else { show(&handle, None) }
                }
                Some("show") => show(&handle, None),
                Some("open") => show(&handle, words.next().and_then(route)),
                Some("hide") => hide(&handle),
                Some("quit") => handle.quit(),
                _ => {}
            }
        }
    }).ok();
}
