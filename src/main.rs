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
    features::calc::set_waker(waker.clone());
    if !s.settings.currency.is_empty() {
        features::calc::set_home_currency(&s.settings.currency);
    }
    features::calc::warm();
    s.chat.configure(waker);
}

fn poll(s: &mut App, clock: f64, tx: &mut Tx) -> bool {
    s.clock = clock;
    let mut changed = ext::poll(s, tx);
    // Fresh exchange rates turn a waiting currency query into an answer.
    if features::calc::take_dirty() {
        s.refresh_keeping();
        changed = true;
    }
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
        } else if toast.until.is_finite() {
            // A progress toast has no deadline: whatever replaces it wakes the window.
            tx.wake_after(std::time::Duration::from_secs_f64((toast.until - clock).clamp(0.05, 3600.0)));
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
            // A headless run (`--snap`, `--script`) draws its own palette and leaves the resident one alone.
            let headless = args.iter().any(|a| a.starts_with("--snap") || a.starts_with("--tree") || a.starts_with("--script"));
            if !headless && (ipc::send(&args.join(" ")) || matches!(command, "hide" | "quit")) {
                return;
            }
        }
        "run" => {
            // A hotkey's command: only a resident palette can run a row.
            if args.len() < 2 || !ipc::send(&args.join(" ")) {
                eprintln!("echospot run <command-id>: needs a running EchoSpot. `echospot commands` lists the ids.");
            }
            return;
        }
        "commands" => return match ipc::ask("commands") {
            Some(list) => println!("{list}"),
            None => eprintln!("echospot commands: needs a running EchoSpot."),
        },
        "mcp" => return match args.get(1).map(String::as_str) {
            Some("call") => features::mcp::cli_call(args.get(2).map(String::as_str).unwrap_or(""), args.get(3).map(String::as_str).unwrap_or("{}")),
            _ => features::mcp::cli(),
        },
        "backup" | "restore" => return features::backup::cli(command, &args[1..]),
        "ext" => return ext::store::cli(&args[1..]),
        "url" => {
            // The system opened a link with EchoSpot; only a running palette has anyone waiting on it.
            ipc::send(&args.join(" "));
            return;
        }
        "calc" => return features::calc::cli(&args[1..].join(" ")),
        "debug-icons" => {
            for app in features::apps::scan() {
                println!("{:<30} {:<36} {}", app.name, app.icon_name, app.icon.map(|p| p.display().to_string()).unwrap_or_else(|| "MISSING".into()));
            }
            return;
        }
        "--version" | "-V" => return println!("echospot {}", env!("CARGO_PKG_VERSION")),
        "--help" | "-h" | "help" => {
            return println!("echospot — a command palette for Hyprland\n\n  echospot              start the resident palette (hidden)\n  echospot toggle       show or hide it\n  echospot show | hide\n  echospot open <clipboard|emoji|files|windows|snippets|quicklinks|store|ai>\n  echospot settings\n  echospot ext <search|install> <query>\n  echospot calc <expression>\n  echospot backup [file] [--with-keys]\n  echospot restore <file>\n  echospot mcp          list MCP servers and their tools\n  echospot commands     list every launcher row's id\n  echospot run <id>     run a launcher row, as its hotkey does\n  echospot quit");
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
        if !headless {
            state.bindings.apply();
        }
        if show_now {
            state.will_show();
        } else {
            state.read_status();
        }
        if let Some(route) = open_route.as_deref().and_then(ipc::route) {
            if route == model::Route::Notes {
                state.notes = features::notes::load();
            }
            state.screens.push(app::Screen::new(route));
            state.refresh();
        }
        state
    });
    let settings_spec = AppSpec::<settings_ui::SettingsUi> {
        on_key: Some(settings_ui::on_key),
        min_size: Some((0.0, 0.0)),
        ..AppSpec::new(settings_ui::TITLE, settings_ui::SIZE, settings_ui::SettingsUi::new, settings_ui::view)
    };
    let hidden = WindowOptions { transparent: true, borderless: true, resizable: false, visible: false, min_size: Some((0.0, 0.0)), ..Default::default() };
    let (settings_handle, settings_window) = app.window(rune::WindowSpec::new("settings", settings_spec).options(hidden));
    settings_ui::attach(settings_handle, settings_window, handle.clone());
    if !headless {
        let hud = app.overlay(OverlaySpec {
            name: "echospot-hud", size: hud::SIZE, edge: ScreenEdge::Bottom, margin: hud::EDGE_OFFSET,
            align: Align::Center, inset: 0.0, interactive: false, view: hud::view,
        }, hud::Hud::default());
        hud::attach(hud);
        // Twice a minute, extensions' background commands that have come due are run.
        let ticker = handle.clone();
        std::thread::Builder::new().name("echospot-schedule".into()).spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(30));
            ticker.update(|s: &mut App, _| ext::tick(s));
        }).ok();
        ipc::serve(handle);
        features::expander::set_enabled(settings::Settings::load().snippet_expansion);
        if settings::Settings::load().clipboard_history {
            features::clipboard::start_watchers();
        }
    }
    app.run();
}
