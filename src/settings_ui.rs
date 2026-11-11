//! The Settings window. It is the palette's own material and vocabulary: the same blurred glass
//! panel, the same rows and tiles in its sidebar, the same floating glass pill at its foot.
//! Every control writes `settings.json` at once and tells the palette, so nothing waits for Save.

use std::sync::OnceLock;

use rune::{AppHandle, Color, Cx, El, ScrollFade, ScrollSpec, TextEdit, Tx, WindowHandle, col, el, row, spacer, text, text_input};

use crate::app::App;
use crate::icons;
use crate::settings::Settings;
use crate::theme::{self, Palette, Tint, motion, radius, size, space, type_};
use crate::ui;

pub const TITLE: &str = "EchoSpot Settings";
/// The panel, and the window around it: the palette's margin, where the shadow falls.
pub const PANEL: (f32, f32) = (780.0, 540.0);
pub const SIZE: (f32, f32) = (PANEL.0 + size::WINDOW_MARGIN * 2.0, PANEL.1 + size::WINDOW_MARGIN * 2.0);
const SIDEBAR_W: f32 = 204.0;
const FULL: Option<f32> = Some(0.0);

#[derive(Clone, Copy, PartialEq)]
pub enum Pane {
    General,
    Clipboard,
    Ai,
    Extensions,
    About,
}

const PANES: [(Pane, &str, &rune::Icon, Tint); 5] = [
    (Pane::General, "General", &icons::GEAR, Tint::Graphite),
    (Pane::Clipboard, "Clipboard", &icons::CLIPBOARD, Tint::Orange),
    (Pane::Ai, "AI", &icons::AI, Tint::Violet),
    (Pane::Extensions, "Extensions", &icons::PUZZLE, Tint::Green),
    (Pane::About, "About", &icons::LOGO, Tint::Indigo),
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
    /// The screen behind the panel, captured as the window opens.
    backdrop: rune::Pixels,
    has_backdrop: bool,
}

impl SettingsUi {
    pub fn new() -> Self {
        let mut ui = Self {
            pane: Pane::General, settings: Settings::load(), model: TextEdit::new(""), base_url: TextEdit::new(""),
            api_key: TextEdit::new("").secure(), system_prompt: TextEdit::new(""), extensions: Vec::new(), clipboard_count: 0, note: String::new(),
            backdrop: rune::Pixels::new(), has_backdrop: false,
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

    /// Takes the patch of screen the window is about to cover, as the palette does when summoned.
    fn capture(&mut self) {
        self.has_backdrop = false;
        if self.settings.backdrop != "capture" {
            return;
        }
        let shot = crate::backdrop::centered_origin(SIZE.0, SIZE.1)
            .and_then(|(x, y)| crate::backdrop::capture_rect(x + size::WINDOW_MARGIN, y + size::WINDOW_MARGIN, PANEL.0, PANEL.1));
        if let Some((w, h, rgba)) = shot {
            self.backdrop.set(w, h, rgba);
            self.has_backdrop = true;
        }
    }

    fn editing(&self) -> bool {
        self.model.focused || self.base_url.focused || self.api_key.focused || self.system_prompt.focused
    }

    fn step_pane(&mut self, delta: i32) {
        let at = PANES.iter().position(|(pane, ..)| *pane == self.pane).unwrap_or(0) as i32;
        self.pane = PANES[(at + delta).clamp(0, PANES.len() as i32 - 1) as usize].0;
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
        ui.capture();
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
    let mut control = row().key(key.to_string()).h(size::BAR_BUTTON_H).padding(2.0).radius(size::BAR_BUTTON_H / 2.0)
        .fill(p.control_surface).border(1.0, p.border.alpha(0.10));
    for (value, label) in options {
        let on = *value == current;
        let value = value.to_string();
        control = control.child(row().key(format!("{key}-{value}")).h(size::BAR_BUTTON_H - 4.0).px(space::XL).center().radius(size::BAR_BUTTON_H / 2.0 - 2.0)
            .fill(if on { p.control_pressed } else { Color::TRANSPARENT }).hover(if on { Color::TRANSPARENT } else { p.row_hover }).motion(motion::HOVER)
            .on_press(move |ui: &mut SettingsUi| {
                set(&mut ui.settings, &value);
                ui.commit();
            })
            .child(text(label.to_string()).font(type_::BAR).color(if on { p.text } else { p.text_secondary })));
    }
    control
}

fn button(p: &Palette, key: &str, label: &str, destructive: bool, press: impl Fn(&mut SettingsUi, &mut Tx) + 'static) -> El {
    row().key(key.to_string()).h(size::BAR_BUTTON_H).px(space::XL).center().radius(size::BAR_BUTTON_H / 2.0)
        .fill(p.control_surface).border(1.0, p.border.alpha(0.10))
        .hover(p.control_hover).press(p.control_pressed).motion(motion::HOVER).on_press_tx(press)
        .child(text(label.to_string()).font(type_::BAR).color(if destructive { p.destructive } else { p.text.alpha(0.92) }))
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
    let mut body = col().w_full().radius(radius::FORM_FIELD).fill(p.card_fill).border(1.0, p.card_stroke);
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            body = body.child(el().w_full().px(space::XL + 2.0).child(el().w_full().h(1.0).fill(p.separator)));
        }
        body = body.child(r);
    }
    let mut group = col().w_full().gap(space::SM);
    if !title.is_empty() {
        group = group.child(text(title.to_string()).font(type_::SECTION).color(p.text_secondary).ml(space::MD));
    }
    group.child(body)
}

fn field(p: &Palette, key: &str, edit: &TextEdit, placeholder: &str, access: fn(&mut SettingsUi) -> &mut TextEdit, width: f32) -> El {
    let focused = edit.focused;
    row().key(format!("{key}-box")).w(width).h(30.0).px(space::XL).center_y().radius(radius::ROW).fill(p.card_fill)
        .border(if focused { 2.0 } else { 1.0 }, if focused { p.border.alpha(0.45) } else { p.card_stroke })
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
                .child(col().key("system-box").w_full().min_h(84.0).px(space::LG).py(space::MD).radius(radius::ROW).fill(p.card_fill).border(1.0, p.card_stroke)
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

fn about(_: &SettingsUi, p: &Palette) -> Vec<El> {
    let link = |label: &'static str, url: &'static str| button(p, label, label, false, move |_, _| crate::util::open_url(url));
    vec![
        col().w_full().items_center().gap(space::MD).py(space::XXL)
            .child(ui::badge_tile(&icons::LOGO, 76.0, Tint::Violet))
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

fn sidebar(ui: &SettingsUi, p: &Palette) -> El {
    // The brand row stands where the palette's search field does, and moves the window.
    let brand = row().key("brand").w_full().h(size::HEADER_BAND).px(space::MD).gap(space::LG).center_y().window_drag()
        .child(ui::badge_tile(&icons::LOGO, size::RESULT_ROW_ICON, Tint::Violet))
        .child(text("EchoSpot").font(type_::PANEL_TITLE).color(p.text));
    let mut list = col().w_full().gap(space::XXS)
        .child(row().w_full().px(space::MD).pt(space::XS).pb(space::SECTION_HEADER_BOTTOM)
            .child(text("Settings").font(type_::SECTION).color(p.text_secondary)));
    for (pane, title, glyph, tint) in PANES {
        let on = ui.pane == pane;
        let item = row().key(format!("pane-{title}")).w_full().px(space::MD).py(space::SM).gap(space::LG).center_y().radius(radius::ROW);
        let item = if on { item.fill(p.selection) } else { item.fill(Color::TRANSPARENT).hover(p.row_hover) };
        list = list.child(item.motion(motion::HOVER).on_press(move |ui: &mut SettingsUi| ui.pane = pane)
            .child(ui::badge_tile(glyph, size::RESULT_ROW_ICON, tint))
            .child(text(title).font(type_::ROW_TITLE).color(p.text)));
    }
    col().key("sidebar").w(SIDEBAR_W).h_full().px(space::MD).child(brand).child(list)
}

fn foot(p: &Palette) -> El {
    let done = row().key("done").h(size::BAR_BUTTON_H).px(space::MD).gap(space::SM).center_y().radius(size::BAR_BUTTON_H / 2.0)
        .hover(p.row_hover).press(p.control_surface).motion(motion::HOVER)
        .on_press_tx(|_: &mut SettingsUi, tx| tx.close_window())
        .child(text("Done").font(rune::TextStyle::new(12.0, 600.0).lh(16.0)).color(p.text))
        .child(ui::keycap(p, "esc", false));
    let file = row().key("open-config").h(size::BAR_BUTTON_H).px(space::MD).gap(space::SM).center_y().radius(size::BAR_BUTTON_H / 2.0)
        .hover(p.row_hover).press(p.control_surface).motion(motion::HOVER)
        .on_press(|_: &mut SettingsUi| { crate::util::spawn_detached(&["xdg-open".into(), Settings::path().display().to_string()]); })
        .child(text("Open settings.json").font(type_::BAR).color(p.text.alpha(0.86)));
    let group = row().key("foot-actions").h(size::MENU_BUTTON).px(space::XS).center_y().child(file).child(done);
    row().key("foot").pin(FULL, None, FULL, FULL).h(size::BOTTOM_BAR_H).px(space::MD).center_y()
        .child(spacer()).child(ui::glass(group, p, size::MENU_BUTTON / 2.0))
}

pub fn view(ui: &SettingsUi, cx: &mut Cx<SettingsUi>) -> El {
    let p = theme::palette(ui.settings.dark(cx.system_dark()));
    let (title, cards) = match ui.pane {
        Pane::General => ("General", general(ui, &p)),
        Pane::Clipboard => ("Clipboard", clipboard(ui, &p)),
        Pane::Ai => ("AI", ai(ui, &p)),
        Pane::Extensions => ("Extensions", extensions(ui, &p)),
        Pane::About => ("About", about(ui, &p)),
    };
    let mut content = col().w_full().px(space::XXL).pt(space::XS).pb(size::BOTTOM_BAR_H + space::XL).gap(space::XXL - 2.0);
    for c in cards {
        content = content.child(c);
    }
    if !ui.note.is_empty() {
        content = content.child(text(ui.note.clone()).font(type_::ROW_TRAILING).color(p.text_secondary).ml(space::MD));
    }
    let fade = ScrollFade::strength(size::DISSOLVE_TOP).full_at(4.0).with_end(ScrollFade::strength(size::DISSOLVE_BOTTOM));
    // The pane's name is set as the palette sets what is typed, and the band it sits in moves
    // the window.
    let heading = row().key("heading").w_full().h(size::HEADER_BAND).px(space::XXL + space::MD).center_y().window_drag()
        .child(text(title).font(type_::SEARCH).color(p.text));
    let detail = col().key("detail").grow(1.0).h_full().min_h(0.0).child(heading)
        .child(col().key("settings-scroll").w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).scroll_fade(fade).child(content));
    // The one hairline: between the list and what it selects, as in the palette's split views.
    let rule = col().h_full().pt(size::HEADER_BAND).pb(size::BOTTOM_BAR_H).child(el().w(size::HAIRLINE).grow(1.0).fill(p.separator));
    let panel = ui::panel_surface("settings-panel", &p, PANEL, ui.has_backdrop.then_some(&ui.backdrop), &ui.settings.backdrop)
        .child(row().pin(FULL, FULL, FULL, FULL).child(sidebar(ui, &p)).child(rule).child(detail))
        .child(foot(&p))
        .child(ui::panel_rim(&p));
    el().w_full().h_full().child(panel)
}

pub fn on_key(ui: &mut SettingsUi, ev: &rune::KeyEvent, tx: &mut Tx) -> bool {
    match ev.key {
        rune::Key::Escape | rune::Key::Command('w') => tx.close_window(),
        rune::Key::Down if !ui.editing() => ui.step_pane(1),
        rune::Key::Up if !ui.editing() => ui.step_pane(-1),
        rune::Key::Command(c @ '1'..='5') => ui.pane = PANES[c as usize - '1' as usize].0,
        _ => return false,
    }
    true
}
