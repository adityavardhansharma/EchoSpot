//! EchoSpot: Tinycast's command palette for Linux, drawn with Rune.
//!
//! One process stays resident so a summon is a show, not a launch. `echospot toggle` (bound to a
//! key in Hyprland) talks to it over a socket, and starts it when it isn't running.

mod app;
mod backdrop;
mod ext;
mod features;
mod glass;
mod seethrough;
mod hud;
mod icons;
mod ipc;
mod live;
mod model;
mod packs;
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
    s.packs.rt.set_waker(waker.clone());
    s.chat.configure(waker);
}

fn poll(s: &mut App, clock: f64, tx: &mut Tx) -> bool {
    s.clock = clock;
    let mut changed = ext::poll(s, tx);
    changed |= packs::host::poll(s, tx);
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
        "clip-store" => {
            features::clipboard::store_from_stdin(args.iter().any(|a| a == "--image"));
            // Packs that listen for the clipboard hear of it from the resident palette.
            ipc::send("event clipboard.changed");
            return;
        }
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
                std::process::exit(1);
            }
            return;
        }
        "commands" => return match ipc::ask("commands") {
            Some(list) => println!("{list}"),
            None => {
                eprintln!("echospot commands: needs a running EchoSpot.");
                std::process::exit(1);
            }
        },
        // Packs: what a coding agent builds and checks them with.
        "dev" => std::process::exit(packs::cli::dev(&args[1..])),
        "call" => std::process::exit(packs::cli::call(&args[1..])),
        "bar" => std::process::exit(packs::cli::bar(&args[1..])),
        "fork" => std::process::exit(packs::cli::fork(&args[1..])),
        "agent" => std::process::exit(packs::cli::agent(&args[1..])),
        "mcp" => return match args.get(1).map(String::as_str) {
            Some("serve") => packs::mcp::serve(),
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
        // The shortcut that summons the palette: shown, or changed (`echospot shortcut super+alt+space`).
        "shortcut" => {
            match args.get(1) {
                None => println!("{}", features::summon::current()),
                Some(chord) => match features::summon::set(chord) {
                    Ok(()) => println!("{}", features::summon::current()),
                    Err(why) => {
                        eprintln!("echospot shortcut: {why}");
                        std::process::exit(1);
                    }
                },
            }
            return;
        }
        "debug-icons" => {
            for app in features::apps::scan() {
                println!("{:<30} {:<36} {}", app.name, app.icon_name, app.icon.map(|p| p.display().to_string()).unwrap_or_else(|| "MISSING".into()));
            }
            return;
        }
        "--version" | "-V" => return println!("echospot {}", env!("CARGO_PKG_VERSION")),
        "--help" | "-h" | "help" => {
            return println!("echospot — a command palette for Hyprland\n\n  echospot              start the resident palette (hidden)\n  echospot toggle       show or hide it\n  echospot show | hide\n  echospot open <clipboard|emoji|files|windows|snap|snippets|quicklinks|store|ai>\n  echospot settings\n  echospot shortcut [chord]   show or change the shortcut that summons it\n  echospot ext <search|install> <query>\n  echospot calc <expression>\n  echospot backup [file] [--with-keys]\n  echospot restore <file>\n  echospot mcp          list MCP servers and their tools\n  echospot mcp serve    offer every pack ability as an MCP tool, over stdio\n  echospot dev          build and check packs (echospot dev help)\n  echospot call <address> [--json '{{…}}'] call an ability\n  echospot bar <address>                   a JSON line per update, for Waybar\n  echospot fork <id>    copy a system pack into your packs\n  echospot agent install                   write the skill for coding agents\n  echospot commands     list every launcher row's id\n  echospot run <id>     run a launcher row, as its hotkey does\n  echospot quit");
        }
        _ => {}
    }
    let headless = args.iter().any(|a| a.starts_with("--snap") || a.starts_with("--tree") || a.starts_with("--script") || a.starts_with("--list"));
    // `echospot dev render`: one surface of one pack, in a scene, drawn the way the palette would.
    let pack_snap: Option<serde_json::Value> = (headless && command == "open" && args.get(1).map(String::as_str) == Some("pack"))
        .then(|| std::env::var(packs::cli::SNAP_ENV).ok()).flatten().and_then(|spec| serde_json::from_str(&spec).ok());
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
    let mut app = rune::App::new(spec).window_options(options).on_focus(on_focus).compositor(glass::Glass::default());
    if !headless {
        // `echospot://` and `raycast://` links reach the resident palette from a browser or a notification.
        std::thread::spawn(ext::register_link_handler);
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
        state.packs.headless = headless;
        if let Some(spec) = &pack_snap {
            packs::host::snap(&mut state, spec);
        } else {
            state.packs.reload(true);
        }
        if !headless {
            state.bindings.apply();
        }
        if show_now {
            state.will_show();
        } else {
            state.read_status();
        }
        if let Some(route) = open_route.as_deref().and_then(ipc::route) {
            state.enter(route);
        }
        state
    });
    let settings_spec = AppSpec::<settings_ui::SettingsUi> {
        setup: Some(settings_ui::setup),
        on_key: Some(settings_ui::on_key),
        min_size: Some((0.0, 0.0)),
        ..AppSpec::new(settings_ui::TITLE, settings_ui::SIZE, settings_ui::SettingsUi::new, settings_ui::view)
    };
    let hidden = WindowOptions { transparent: true, borderless: true, resizable: false, visible: false, min_size: Some((0.0, 0.0)), ..Default::default() };
    let (settings_handle, settings_window) = app.window(rune::WindowSpec::from_app("settings", rune::App::new(settings_spec).compositor(glass::Glass::default())).options(hidden).modifier_keys().on_focus(settings_ui::on_focus));
    if !headless {
        // What is behind each window, kept current while it is open.
        let palette = handle.clone();
        live::spawn_palette((theme::size::WINDOW_W, theme::size::WINDOW_H), move |w, h, rgba| {
            palette.update_if(move |s: &mut App, _| s.visible && s.set_backdrop(w, h, rgba));
        });
        let settings = settings_handle.clone();
        live::spawn_settings(settings_ui::SIZE, move |w, h, rgba| {
            settings.update(move |ui: &mut settings_ui::SettingsUi, _| ui.set_backdrop(w, h, rgba));
        });
        // Liquid Glass over the live screen: see-through capture, standard protocols only.
        let palette = handle.clone();
        seethrough::spawn("palette", (theme::size::WINDOW_W, theme::size::WINDOW_H), move |w, h, rgba| {
            palette.update_if(move |s: &mut App, _| s.visible && s.set_backdrop(w, h, rgba));
        });
        let settings = settings_handle.clone();
        seethrough::spawn("settings", settings_ui::SIZE, move |w, h, rgba| {
            settings.update(move |ui: &mut settings_ui::SettingsUi, _| ui.set_backdrop(w, h, rgba));
        });
    }
    settings_ui::attach(settings_handle, settings_window, handle.clone());
    packs::host::attach(handle.clone());
    // A pack's `window` surface: one more window, hidden until a pack shows it.
    let pack_spec = AppSpec::<packs::slots::Slot> {
        setup: Some(packs::slots::setup),
        on_key: Some(packs::slots::window_key),
        min_size: Some((0.0, 0.0)),
        ..AppSpec::new("EchoSpot Pack", packs::slots::WINDOW_SIZE, packs::slots::Slot::default, packs::slots::window_view)
    };
    let hidden = WindowOptions { transparent: true, borderless: true, resizable: false, visible: false, min_size: Some((0.0, 0.0)), ..Default::default() };
    let (pack_handle, pack_window) = app.window(rune::WindowSpec::from_app("pack", rune::App::new(pack_spec)).options(hidden));
    packs::slots::attach_window(pack_handle, pack_window);
    if !headless {
        let hud = app.overlay(OverlaySpec {
            name: "echospot-hud", size: hud::SIZE, edge: ScreenEdge::Bottom, margin: hud::EDGE_OFFSET,
            align: Align::Center, inset: 0.0, interactive: false, view: hud::view,
        }, hud::Hud::default());
        hud::attach(hud);
        // Empty slots a pack's overlay or widget is shown in.
        for n in 0..packs::slots::OVERLAYS + packs::slots::WIDGETS {
            let widget = n >= packs::slots::OVERLAYS;
            packs::slots::attach(app.overlay(packs::slots::spec(widget), packs::slots::Slot::default()), widget);
        }
        // Once a second: a pack an agent just saved, a schedule come due, an overlay's clock.
        let pulse = handle.clone();
        std::thread::Builder::new().name("echospot-packs".into()).spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(1));
            pulse.update_if(|s: &mut App, _| packs::host::tick(s));
        }).ok();
        packs::host::listen(handle.clone());
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
