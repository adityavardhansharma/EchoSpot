//! EchoSpot: Tinycast's command palette for Linux, drawn with Rune.
//!
//! One process stays resident so a summon is a show, not a launch. `echospot toggle` (bound to a
//! key in Hyprland) talks to it over a socket, and starts it when it isn't running.

mod app;
mod backdrop;
mod ext;
mod features;
mod hud;
mod icons;
mod ipc;
mod model;
mod settings;
mod settings_ui;
mod store;
mod theme;
mod ui;
mod util;

use rune::{Align, AppSpec, ImageCache, OverlaySpec, ScreenEdge, Tx, Waker, WindowOptions};

use app::App;

/// The Wayland app id: what Hyprland's window rules match.
pub const APP_ID: &str = "echospot";

fn setup(s: &mut App, waker: Waker) {
    s.icons.attach(ImageCache::new(waker.clone()));
    s.files.configure(waker.clone());
    s.ext.configure(waker.clone());
    s.chat.configure(waker);
}

fn poll(s: &mut App, clock: f64, tx: &mut Tx) -> bool {
    s.clock = clock;
    let mut changed = ext::poll(s, tx);
    if s.files.poll() && s.top().route == model::Route::Files {
        s.refresh_keeping();
        changed = true;
    }
    if s.chat.poll() {
        if s.top().route == model::Route::Ai {
            s.refresh();
        }
        changed = true;
    }
    if let Some(toast) = &s.toast {
        if clock >= toast.until {
            s.toast = None;
            changed = true;
        } else {
            tx.wake_after(std::time::Duration::from_secs_f64((toast.until - clock).max(0.05)));
        }
    }
    changed
}

fn on_key(s: &mut App, ev: &rune::KeyEvent, tx: &mut Tx) -> bool {
    s.key(ev, tx)
}

fn on_focus(s: &mut App, focused: bool, tx: &mut Tx) {
    // A new Wayland window hears "unfocused" before its first focus; only a real loss hides.
    if focused {
        s.was_focused = true;
    } else if s.was_focused && s.visible && s.hide_on_blur && s.settings.hide_on_blur {
        s.hide(tx);
    }
    if std::env::var_os("ECHOSPOT_DEBUG").is_some() {
        eprintln!("focus {focused} visible {}", s.visible);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("");
    match command {
        "clip-store" => return features::clipboard::store_from_stdin(args.iter().any(|a| a == "--image")),
        "toggle" | "show" | "hide" | "quit" | "open" | "settings" => {
            // A resident palette answers; without one, this process becomes it.
            if ipc::send(&args.join(" ")) || matches!(command, "hide" | "quit") {
                return;
            }
        }
        "ext" => return ext::store::cli(&args[1..]),
        "debug-icons" => {
            for app in features::apps::scan() {
                println!("{:<30} {:<36} {}", app.name, app.icon_name, app.icon.map(|p| p.display().to_string()).unwrap_or_else(|| "MISSING".into()));
            }
            return;
        }
        "--version" | "-V" => return println!("echospot {}", env!("CARGO_PKG_VERSION")),
        "--help" | "-h" | "help" => {
            return println!("echospot — a command palette for Hyprland\n\n  echospot              start the resident palette (hidden)\n  echospot toggle       show or hide it\n  echospot show | hide\n  echospot open <clipboard|emoji|files|windows|snippets|quicklinks|store|ai>\n  echospot settings\n  echospot ext <search|install> <query>\n  echospot quit");
        }
        _ => {}
    }
    let headless = args.iter().any(|a| a.starts_with("--snap") || a.starts_with("--tree") || a.starts_with("--script") || a.starts_with("--list"));
    let show_now = matches!(command, "toggle" | "show" | "open");
    let open_route = (command == "open").then(|| args.get(1).cloned()).flatten();

    rune::set_app_id(APP_ID);
    let spec = AppSpec::<App> {
        setup: Some(setup),
        poll: Some(poll),
        on_key: Some(on_key),
        min_size: Some((0.0, 0.0)),
        ..AppSpec::new("EchoSpot", (theme::size::WINDOW_W, theme::size::WINDOW_H), App::new, ui::view)
    };
    let options = WindowOptions {
        transparent: true, borderless: true, resizable: false, visible: !headless && show_now,
        min_size: Some((0.0, 0.0)), ..Default::default()
    };
    let mut app = rune::App::new(spec).window_options(options).on_focus(on_focus);
    if !headless {
        app = app.keep_running();
        if !show_now {
            app = app.windowless();
        }
    }
    let handle = app.handle();
    let me = handle.clone();
    app = app.init(move || {
        let mut state = App::new();
        state.me = Some(me);
        if show_now {
            state.will_show();
        }
        if let Some(route) = open_route.as_deref().and_then(ipc::route) {
            state.screens.push(app::Screen::new(route));
            state.refresh();
        }
        state
    });
    let settings_spec = AppSpec::<settings_ui::SettingsUi> {
        on_key: Some(settings_ui::on_key),
        min_size: Some((640.0, 420.0)),
        ..AppSpec::new(settings_ui::TITLE, settings_ui::SIZE, settings_ui::SettingsUi::new, settings_ui::view)
    };
    let hidden = WindowOptions { visible: false, ..Default::default() };
    let (settings_handle, settings_window) = app.window(rune::WindowSpec::new("settings", settings_spec).options(hidden));
    settings_ui::attach(settings_handle, settings_window, handle.clone());
    if !headless {
        let hud = app.overlay(OverlaySpec {
            name: "echospot-hud", size: hud::SIZE, edge: ScreenEdge::Bottom, margin: hud::EDGE_OFFSET,
            align: Align::Center, inset: 0.0, interactive: false, view: hud::view,
        }, hud::Hud::default());
        hud::attach(hud);
        ipc::serve(handle);
        if settings::Settings::load().clipboard_history {
            features::clipboard::start_watchers();
        }
    }
    app.run();
}
