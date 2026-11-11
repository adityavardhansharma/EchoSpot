//! The Settings window: a sidebar of panes over grouped cards. Every control writes
//! `settings.json` at once and tells the palette, so nothing waits for a Save button.

use std::sync::OnceLock;

use rune::{AppHandle, Color, Cx, El, ScrollSpec, TextEdit, Tx, WindowHandle, col, el, icon, row, spacer, text, text_input};

use crate::app::App;
use crate::icons;
use crate::settings::Settings;
use crate::theme::{self, Palette, motion, radius, space, type_};

pub const TITLE: &str = "EchoSpot Settings";
pub const SIZE: (f32, f32) = (820.0, 560.0);

#[derive(Clone, Copy, PartialEq)]
pub enum Pane {
    General,
    Clipboard,
    Ai,
    Extensions,
    About,
}

const PANES: [(Pane, &str, &rune::Icon, &str); 5] = [
    (Pane::General, "General", &icons::GEAR, "#8e8e93"),
    (Pane::Clipboard, "Clipboard", &icons::CLIPBOARD, "#ff9f0a"),
    (Pane::Ai, "AI", &icons::SPARKLES, "#bf5af2"),
    (Pane::Extensions, "Extensions", &icons::PUZZLE, "#0a84ff"),
    (Pane::About, "About", &icons::INFO, "#30d158"),
];

pub struct SettingsUi {
    pub pane: Pane,
    pub settings: Settings,
    pub model: TextEdit,
    pub base_url: TextEdit,
    pub api_key: TextEdit,
    pub system_prompt: TextEdit,
    pub extensions: Vec<(String, String, usize)>,
    pub clipboard_count: usize,
    pub note: String,
}

impl SettingsUi {
    pub fn new() -> Self {
        let mut ui = Self {
            pane: Pane::General, settings: Settings::load(), model: TextEdit::new(""), base_url: TextEdit::new(""),
            api_key: TextEdit::new("").secure(), system_prompt: TextEdit::new(""), extensions: Vec::new(), clipboard_count: 0, note: String::new(),
        };
        ui.reload();
        ui
    }

    /// Reads the file again, as the window opens.
    pub fn reload(&mut self) {
        self.settings = Settings::load();
        self.model.replace_text(self.settings.ai.model.clone());
        self.base_url.replace_text(self.settings.ai.base_url.clone());
        self.api_key.replace_text(self.settings.ai.api_key.clone());
        self.system_prompt.replace_text(self.settings.ai.system_prompt.clone());
        self.extensions = crate::ext::Extensions::load().installed.iter().map(|e| (e.id.clone(), e.title.clone(), e.commands.len())).collect();
        self.clipboard_count = crate::features::clipboard::Clipboard::load().entries.len();
        self.note.clear();
    }

    /// Saves and hands the palette the new settings.
    fn commit(&mut self) {
        self.settings.ai.model = self.model.text.trim().to_string();
        self.settings.ai.base_url = self.base_url.text.trim().to_string();
        self.settings.ai.api_key = self.api_key.text.trim().to_string();
        self.settings.ai.system_prompt = self.system_prompt.text.clone();
        self.settings.save();
        if let Some(main) = MAIN.get() {
            let settings = self.settings.clone();
            main.update(move |app: &mut App, _| {
                app.settings = settings;
                app.refresh();
            });
        }
    }
}

static WINDOW: OnceLock<(AppHandle<SettingsUi>, WindowHandle)> = OnceLock::new();
static MAIN: OnceLock<AppHandle<App>> = OnceLock::new();

pub fn attach(handle: AppHandle<SettingsUi>, window: WindowHandle, main: AppHandle<App>) {
    let _ = WINDOW.set((handle, window));
    let _ = MAIN.set(main);
}

/// Opens Settings at a pane ("ai", "extensions", …).
pub fn open(pane: &str) {
    let Some((handle, window)) = WINDOW.get() else { return };
    let pane = match pane {
        "ai" => Some(Pane::Ai),
        "clipboard" => Some(Pane::Clipboard),
        "extensions" => Some(Pane::Extensions),
        "about" => Some(Pane::About),
        "general" => Some(Pane::General),
        _ => None,
    };
    handle.update(move |ui, _| {
        ui.reload();
        if let Some(pane) = pane {
            ui.pane = pane;
        }
    });
    window.open();
}

fn switch(p: &Palette, key: &str, on: bool, toggle: impl Fn(&mut Settings) + 'static) -> El {
    el().key(key.to_string()).role("switch").checked(on).size(38.0, 22.0).radius(11.0)
        .fill(if on { p.success } else { p.control_hover }).motion(motion::LAYOUT)
        .on_press(move |ui: &mut SettingsUi| {
            toggle(&mut ui.settings);
            ui.commit();
        })
        .child(el().key(format!("{key}-knob")).size(18.0, 18.0).radius(9.0).fill(Color::WHITE)
            .shadow(1.0, 3.0, Color::BLACK.alpha(0.3)).absolute(if on { 18.0 } else { 2.0 }, 2.0).motion(motion::MENU))
}

fn segmented(p: &Palette, key: &str, options: &[(&str, &str)], current: &str, set: fn(&mut Settings, &str)) -> El {
    let mut control = row().key(key.to_string()).h(28.0).padding(2.0).radius(radius::BAR_CONTROL).fill(p.control_surface);
    for (value, label) in options {
        let on = *value == current;
        let value = value.to_string();
        control = control.child(row().key(format!("{key}-{value}")).h(24.0).px(space::XL).center().radius(radius::BAR_CONTROL - 2.0)
            .fill(if on { p.control_pressed } else { Color::TRANSPARENT }).hover(if on { Color::TRANSPARENT } else { p.row_hover }).motion(motion::HOVER)
            .on_press(move |ui: &mut SettingsUi| {
                set(&mut ui.settings, &value);
                ui.commit();
            })
            .child(text(label.to_string()).font(type_::ROW_TRAILING).color(if on { p.text } else { p.text_secondary })));
    }
    control
}

fn button(p: &Palette, key: &str, label: &str, destructive: bool, press: impl Fn(&mut SettingsUi, &mut Tx) + 'static) -> El {
    row().key(key.to_string()).h(28.0).px(space::XL).center().radius(radius::BAR_CONTROL).fill(p.control_surface)
        .hover(p.row_hover).press(p.control_pressed).on_press_tx(press)
        .child(text(label.to_string()).font(type_::ROW_TRAILING).color(if destructive { p.destructive } else { p.text }))
}

/// A row of a card: a title with its quieter subtitle, and a control at the trailing edge.
fn setting(p: &Palette, title: &str, subtitle: &str, control: El) -> El {
    let mut labels = col().grow(1.0).gap(space::XXS).child(text(title.to_string()).font(type_::ROW_TITLE).color(p.text));
    if !subtitle.is_empty() {
        labels = labels.child(text(subtitle.to_string()).font(type_::SECTION).color(p.text_secondary).wrap());
    }
    row().w_full().min_h(46.0).px(space::XL + 2.0).py(space::MD).gap(space::XL).center_y().child(labels).child(control)
}

fn card(p: &Palette, title: &str, rows: Vec<El>) -> El {
    let mut body = col().w_full().radius(radius::CARD).fill(p.card_fill).border(1.0, p.card_stroke);
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            body = body.child(el().w_full().px(space::XL + 2.0).child(el().w_full().h(1.0).fill(p.separator)));
        }
        body = body.child(r);
    }
    let mut group = col().w_full().gap(space::SM);
    if !title.is_empty() {
        group = group.child(text(title.to_string()).font(type_::SECTION).color(p.text_secondary).ml(space::XS));
    }
    group.child(body)
}

fn field(p: &Palette, key: &str, edit: &TextEdit, placeholder: &str, access: fn(&mut SettingsUi) -> &mut TextEdit, width: f32) -> El {
    let focused = edit.focused;
    row().key(format!("{key}-box")).w(width).h(28.0).px(space::LG).center_y().radius(radius::BAR_CONTROL).fill(p.card_fill)
        .border(if focused { 2.0 } else { 1.0 }, if focused { p.border.alpha(0.5) } else { p.card_stroke })
        .child(text_input(edit, type_::ROW_TITLE).key(key.to_string()).single_line().grow(1.0).color(p.text)
            .placeholder(placeholder.to_string(), p.text_tertiary).no_focus_ring()
            .on_edit(access)
            .on_input_change(|ui: &mut SettingsUi, _| ui.commit()))
}

fn general(ui: &SettingsUi, p: &Palette) -> Vec<El> {
    let s = &ui.settings;
    vec![
        card(p, "Appearance", vec![
            setting(p, "Theme", "The palette's ink: light on dark, or dark on light.", segmented(p, "appearance",
                &[("dark", "Dark"), ("light", "Light"), ("system", "System")], &s.appearance, |s, v| s.appearance = v.into())),
            setting(p, "Surface", "Blur captures the screen behind the panel; Compositor leaves the blur to Hyprland; Solid draws no blur.",
                segmented(p, "backdrop", &[("capture", "Blur"), ("compositor", "Compositor"), ("none", "Solid")], &s.backdrop, |s, v| s.backdrop = v.into())),
        ]),
        card(p, "Behavior", vec![
            setting(p, "Hide when focus is lost", "Clicking another window dismisses the palette.", switch(p, "hide-on-blur", s.hide_on_blur, |s| s.hide_on_blur = !s.hide_on_blur)),
            setting(p, "Show animations", "Menus, selection and the palette's own motion.", switch(p, "animations", s.animations, |s| s.animations = !s.animations)),
        ]),
        card(p, "Shortcut", vec![
            setting(p, "Summon EchoSpot", "Bound in ~/.config/hypr/bindings.lua to `echospot toggle`.",
                row().gap(space::XXS).child(crate::ui::keycap(p, "Super", true)).child(crate::ui::keycap(p, "Space", true))),
        ]),
    ]
}

fn clipboard(ui: &SettingsUi, p: &Palette) -> Vec<El> {
    let s = &ui.settings;
    vec![
        card(p, "History", vec![
            setting(p, "Keep clipboard history", "Text, links, colors and images you copy. Entries marked sensitive by a password manager are never kept. Takes effect the next time EchoSpot starts.",
                switch(p, "clipboard-history", s.clipboard_history, |s| s.clipboard_history = !s.clipboard_history)),
            setting(p, "Stored entries", &format!("{} kept on this machine, in ~/.local/share/echospot/clipboard.", ui.clipboard_count),
                button(p, "clear-clipboard", "Clear History…", true, |ui, _| {
                    let dir = crate::util::data_dir().join("clipboard");
                    let _ = std::fs::remove_dir_all(&dir);
                    ui.clipboard_count = 0;
                    ui.note = "Clipboard history cleared.".into();
                    if let Some(main) = MAIN.get() {
                        main.update(|app: &mut App, _| {
                            app.clipboard.reload();
                            app.refresh();
                        });
                    }
                })),
        ]),
    ]
}

fn ai(ui: &SettingsUi, p: &Palette) -> Vec<El> {
    let a = &ui.settings.ai;
    let anthropic = a.provider == "anthropic";
    let mut provider = vec![
        setting(p, "Provider", "Claude through Anthropic's API, or any endpoint that speaks the OpenAI chat format (OpenAI, OpenRouter, Ollama).",
            segmented(p, "provider", &[("anthropic", "Claude"), ("openai", "OpenAI-compatible")], &a.provider, |s, v| {
                s.ai.provider = v.into();
            })),
        setting(p, "Model", if anthropic { "For example claude-opus-5-5, claude-sonnet-5-5 or claude-haiku-5-5." } else { "The model name your endpoint expects." },
            field(p, "model", &ui.model, "claude-opus-5-5", |ui| &mut ui.model, 240.0)),
    ];
    if !anthropic {
        provider.push(setting(p, "Endpoint", "The base URL, up to and including /v1.", field(p, "base-url", &ui.base_url, "https://api.openai.com/v1", |ui| &mut ui.base_url, 300.0)));
    }
    provider.push(setting(p, "API key", &format!("Stored in settings.json, readable only by you. Left empty, {} is used.", if anthropic { "ANTHROPIC_API_KEY" } else { "OPENAI_API_KEY" }),
        field(p, "api-key", &ui.api_key, "Paste a key", |ui| &mut ui.api_key, 300.0)));
    let status = if a.ready() { ("Ready", p.success) } else { ("No key yet", p.warning) };
    vec![
        card(p, "Model", provider),
        card(p, "Instructions", vec![
            col().w_full().px(space::XL + 2.0).py(space::LG).gap(space::MD)
                .child(text("System prompt").font(type_::ROW_TITLE).color(p.text))
                .child(col().key("system-box").w_full().min_h(84.0).px(space::LG).py(space::MD).radius(radius::BAR_CONTROL).fill(p.card_fill).border(1.0, p.card_stroke)
                    .child(text_input(&ui.system_prompt, type_::ROW_TITLE).key("system-prompt").w_full().grow(1.0).color(p.text)
                        .placeholder("Leave empty for EchoSpot's own: short, direct answers.", p.text_tertiary).no_focus_ring()
                        .on_edit(|ui: &mut SettingsUi| &mut ui.system_prompt)
                        .on_input_change(|ui: &mut SettingsUi, _| ui.commit()))),
        ]),
        card(p, "", vec![setting(p, "Status", "Tab in the launcher asks the model; AI Chat keeps the thread.",
            row().gap(space::SM).center_y().child(el().size(8.0, 8.0).radius(4.0).fill(status.1)).child(text(status.0).font(type_::ROW_TRAILING).color(p.text_secondary)))]),
    ]
}

fn extensions(ui: &SettingsUi, p: &Palette) -> Vec<El> {
    let mut rows: Vec<El> = ui.extensions.iter().map(|(id, title, commands)| {
        let id = id.clone();
        setting(p, title, &format!("{commands} command{}", if *commands == 1 { "" } else { "s" }),
            button(p, &format!("uninstall-{id}"), "Uninstall", true, move |ui, _| {
                let dir = crate::ext::extensions_dir().join(&id);
                let _ = std::fs::remove_dir_all(dir);
                ui.extensions.retain(|(e, _, _)| *e != id);
                if let Some(main) = MAIN.get() {
                    main.update(|app: &mut App, _| {
                        app.ext.rescan();
                        app.refresh();
                    });
                }
            }))
    }).collect();
    if rows.is_empty() {
        rows.push(setting(p, "No extensions installed", "Raycast extensions run here: open the Store from the launcher to add some.", el()));
    }
    vec![
        card(p, "Installed", rows),
        card(p, "", vec![setting(p, "Raycast Store", "Search the store and install the bundle it already built. Extensions run in Node.",
            button(p, "open-store", "Open Store", false, |_, tx| {
                tx.close_window();
                std::thread::spawn(|| {
                    crate::ipc::send("open store");
                });
            }))]),
    ]
}

fn about(_ui: &SettingsUi, p: &Palette) -> Vec<El> {
    let link = |label: &'static str, url: &'static str| button(p, label, label, false, move |_, _| crate::util::open_url(url));
    vec![
        col().w_full().items_center().gap(space::MD).py(space::XXL)
            .child(el().size(72.0, 72.0).radius(18.0).fill(rune::color::hex("#8a5cff").unwrap_or(Color::WHITE)).center()
                .child(icon(&icons::BOLT, 38.0).color(Color::WHITE)))
            .child(text("EchoSpot").font(type_::TITLE2).color(p.text))
            .child(text(format!("Version {}", env!("CARGO_PKG_VERSION"))).font(type_::ROW_TRAILING).color(p.text_secondary))
            .child(text("A native command palette for Hyprland, drawn with Rune.").font(type_::ROW_TRAILING).color(p.text_tertiary)),
        card(p, "", vec![
            setting(p, "Source", "github.com/adityavardhansharma/EchoSpot", link("GitHub", "https://github.com/adityavardhansharma/EchoSpot")),
            setting(p, "Built on", "Tinycast's design, for macOS, is where this began. Rune draws every pixel.", link("Tinycast", "https://tinycast.dev")),
            setting(p, "Settings file", "Everything here is plain JSON you can edit or sync.", button(p, "open-file", "Open File", false, |_, _| {
                crate::util::spawn_detached(&["xdg-open".into(), Settings::path().display().to_string()]);
            })),
        ]),
    ]
}

pub fn view(ui: &SettingsUi, cx: &mut Cx<SettingsUi>) -> El {
    let p = theme::palette(ui.settings.dark(cx.system_dark()));
    let bg = if p.dark { rune::color::hex("#1c1c1e") } else { rune::color::hex("#f2f2f7") }.unwrap_or(Color::BLACK);
    let side_bg = if p.dark { rune::color::hex("#232325") } else { rune::color::hex("#e9e9ee") }.unwrap_or(Color::BLACK);

    let mut sidebar = col().key("sidebar").w(200.0).h_full().padding(space::LG).gap(space::XXS).fill(side_bg)
        .child(text("Settings").font(type_::PANEL_TITLE).color(p.text).ml(space::MD).mt(space::SM).mb(space::LG));
    for (pane, title, glyph, tint) in PANES {
        let on = ui.pane == pane;
        sidebar = sidebar.child(row().key(format!("pane-{title}")).w_full().h(32.0).px(space::MD).gap(space::LG).center_y().radius(radius::BAR_CONTROL)
            .fill(if on { p.selection } else { Color::TRANSPARENT }).hover(if on { Color::TRANSPARENT } else { p.row_hover }).motion(motion::HOVER)
            .on_press(move |ui: &mut SettingsUi| ui.pane = pane)
            .child(el().size(20.0, 20.0).radius(6.0).fill(rune::color::hex(tint).unwrap_or(Color::WHITE)).center().child(icon(glyph, 13.0).color(Color::WHITE)))
            .child(text(title).font(type_::ROW_TITLE).color(p.text)));
    }
    sidebar = sidebar.child(spacer());

    let (title, cards) = match ui.pane {
        Pane::General => ("General", general(ui, &p)),
        Pane::Clipboard => ("Clipboard", clipboard(ui, &p)),
        Pane::Ai => ("AI", ai(ui, &p)),
        Pane::Extensions => ("Extensions", extensions(ui, &p)),
        Pane::About => ("About", about(ui, &p)),
    };
    let mut content = col().w_full().px(space::XXL + space::XS).pt(space::XXL).pb(space::XXL + space::XL).gap(space::XXL)
        .child(text(title).font(type_::TITLE2).color(p.text));
    for c in cards {
        content = content.child(c);
    }
    if !ui.note.is_empty() {
        content = content.child(text(ui.note.clone()).font(type_::ROW_TRAILING).color(p.text_secondary));
    }
    let detail = col().key("detail").grow(1.0).h_full().min_h(0.0)
        .child(col().w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).scroll_edge_fade(20.0).child(content));
    row().w_full().h_full().fill(bg).child(sidebar).child(el().w(1.0).h_full().fill(p.separator)).child(detail)
}

pub fn on_key(_ui: &mut SettingsUi, ev: &rune::KeyEvent, tx: &mut Tx) -> bool {
    match ev.key {
        rune::Key::Escape | rune::Key::Command('w') => tx.close_window(),
        _ => return false,
    }
    true
}
