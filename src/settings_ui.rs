//! The Settings window. It stands on the palette's own glass and speaks its vocabulary (rows,
//! colour tiles, a floating pill at the foot), and it shows what it changes: the theme and the
//! surface are picked from small drawings of the palette, the AI pane leads with the model in
//! use and whether it answers. EchoSpot's violet is the one accent. Every control writes
//! `settings.json` at once and tells the palette, so nothing waits for Save.

use std::sync::OnceLock;

use rune::{AppHandle, Color, Cx, El, ScrollFade, ScrollSpec, TextEdit, TextStyle, Tx, WindowHandle, col, el, icon, row, spacer, text, text_input};

use crate::app::App;
use crate::features::ai;
use crate::icons;
use crate::settings::Settings;
use crate::theme::{self, Palette, Tint, motion, radius, size, space, type_};
use crate::ui;

pub const TITLE: &str = "EchoSpot Settings";
/// The panel, and the window around it: the palette's margin, where the shadow falls.
pub const PANEL: (f32, f32) = (780.0, 540.0);
pub const SIZE: (f32, f32) = (PANEL.0 + size::WINDOW_MARGIN * 2.0, PANEL.1 + size::WINDOW_MARGIN * 2.0);
const SIDEBAR_W: f32 = 200.0;
const FULL: Option<f32> = Some(0.0);
/// The one accent: selection rings, switches, the status hero.
const ACCENT: Tint = Tint::Violet;
const HEAD_H: f32 = 76.0;
const INSET: f32 = space::XL + 2.0;

#[derive(Clone, Copy, PartialEq)]
pub enum Pane {
    General,
    Ai,
    Clipboard,
    Extensions,
    Shortcuts,
    About,
}

struct PaneInfo {
    pane: Pane,
    title: &'static str,
    blurb: &'static str,
    glyph: &'static rune::Icon,
    tint: Tint,
}

const PANES: [PaneInfo; 6] = [
    PaneInfo { pane: Pane::General, title: "General", blurb: "How the palette looks and behaves.", glyph: &icons::GEAR, tint: Tint::Graphite },
    PaneInfo { pane: Pane::Ai, title: "AI", blurb: "The model behind Quick AI and AI Chat.", glyph: &icons::AI, tint: Tint::Violet },
    PaneInfo { pane: Pane::Clipboard, title: "Clipboard", blurb: "What you copy, kept on this machine.", glyph: &icons::CLIPBOARD, tint: Tint::Orange },
    PaneInfo { pane: Pane::Extensions, title: "Extensions", blurb: "Raycast extensions running in EchoSpot.", glyph: &icons::PUZZLE, tint: Tint::Green },
    PaneInfo { pane: Pane::Shortcuts, title: "Shortcuts", blurb: "Everything the keyboard does here.", glyph: &icons::KEYBOARD, tint: Tint::Blue },
    PaneInfo { pane: Pane::About, title: "About", blurb: "EchoSpot, and where it comes from.", glyph: &icons::LOGO, tint: Tint::Indigo },
];

#[derive(Clone, PartialEq)]
enum Probe {
    Idle,
    Running,
    Ok(String),
    Failed(String),
}

struct ExtRow {
    id: String,
    title: String,
    author: String,
    commands: usize,
    icon: Option<std::path::PathBuf>,
}

pub struct SettingsUi {
    pub pane: Pane,
    pub settings: Settings,
    pub model: TextEdit,
    pub base_url: TextEdit,
    pub api_key: TextEdit,
    pub system_prompt: TextEdit,
    extensions: Vec<ExtRow>,
    /// Clipboard entries, how many are images, and bytes on disk.
    clipboard: (usize, usize, u64),
    chats: usize,
    probe: Probe,
    icons: crate::store::IconStore,
    /// The screen behind the panel, captured as the window opens.
    backdrop: rune::Pixels,
    has_backdrop: bool,
}

fn dir_size(dir: &std::path::Path) -> u64 {
    std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| {
        let path = e.path();
        if path.is_dir() { dir_size(&path) } else { e.metadata().map(|m| m.len()).unwrap_or(0) }
    }).sum()
}

impl SettingsUi {
    pub fn new() -> Self {
        let mut ui = Self {
            pane: Pane::General, settings: Settings::load(), model: TextEdit::new(""), base_url: TextEdit::new(""),
            api_key: TextEdit::new("").secure(), system_prompt: TextEdit::new(""), extensions: Vec::new(), clipboard: (0, 0, 0), chats: 0,
            probe: Probe::Idle, icons: Default::default(), backdrop: rune::Pixels::new(), has_backdrop: false,
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
        self.extensions = crate::ext::Extensions::load().installed.iter().map(|e| ExtRow {
            id: e.id.clone(), title: e.title.clone(), author: e.author.clone(), commands: e.commands.len(), icon: e.icon.clone(),
        }).collect();
        let entries = crate::features::clipboard::Clipboard::load().entries;
        let images = entries.iter().filter(|e| e.kind == crate::features::clipboard::Kind::Image).count();
        self.clipboard = (entries.len(), images, dir_size(&crate::util::data_dir().join("clipboard")));
        self.chats = ai::threads().len();
        self.probe = Probe::Idle;
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
        let at = PANES.iter().position(|i| i.pane == self.pane).unwrap_or(0) as i32;
        self.pane = PANES[(at + delta).clamp(0, PANES.len() as i32 - 1) as usize].pane;
    }

    /// Saves and hands the palette the new settings.
    fn commit(&mut self) {
        self.settings.ai.model = self.model.text.trim().to_string();
        self.settings.ai.base_url = self.base_url.text.trim().to_string();
        self.settings.ai.api_key = self.api_key.text.trim().to_string();
        self.settings.ai.system_prompt = self.system_prompt.text.clone();
        self.settings.save();
        self.probe = Probe::Idle;
        if let Some(main) = MAIN.get() {
            let settings = self.settings.clone();
            main.update(move |app: &mut App, _| {
                app.settings = settings;
                app.refresh();
            });
        }
    }

    /// Asks the provider whether the key and endpoint answer, off the UI thread.
    fn test_connection(&mut self) {
        if self.probe == Probe::Running {
            return;
        }
        self.probe = Probe::Running;
        let settings = self.settings.ai.clone();
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let result = ai::probe(&settings);
            let ms = started.elapsed().as_millis();
            if let Some((handle, _)) = WINDOW.get() {
                handle.update(move |ui, _| ui.probe = match result {
                    Ok(0) => Probe::Ok(format!("Connected in {ms} ms")),
                    Ok(n) => Probe::Ok(format!("Connected in {ms} ms · {n} models")),
                    Err(e) => Probe::Failed(e),
                });
            }
        });
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
        "shortcuts" => Some(Pane::Shortcuts),
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

fn tell_main(f: impl FnOnce(&mut App) + Send + 'static) {
    if let Some(main) = MAIN.get() {
        main.update(move |app: &mut App, _| {
            f(app);
            app.refresh();
        });
    }
}

// ---- Controls ---------------------------------------------------------------------------

fn switch(p: &Palette, key: &str, on: bool, toggle: impl Fn(&mut Settings) + 'static) -> El {
    let (top, bottom) = ACCENT.stops();
    let track = el().key(key.to_string()).role("switch").checked(on).size(36.0, 21.0).radius(10.5).motion(motion::LAYOUT);
    let track = if on { track.fill(bottom).gradient(top, bottom) } else { track.fill(p.control_hover) };
    track.border(1.0, Color::WHITE.alpha(if on { 0.16 } else { 0.05 }))
        .on_press(move |ui: &mut SettingsUi| {
            toggle(&mut ui.settings);
            ui.commit();
        })
        .child(el().key(format!("{key}-knob")).size(17.0, 17.0).radius(8.5).fill(Color::WHITE)
            .shadow(1.0, 3.0, Color::BLACK.alpha(0.3)).absolute(if on { 17.0 } else { 2.0 }, 2.0).motion(motion::MENU))
}

fn button(p: &Palette, key: &str, label: &str, destructive: bool, press: impl Fn(&mut SettingsUi, &mut Tx) + 'static) -> El {
    row().key(key.to_string()).h(size::BAR_BUTTON_H).px(space::XL).center().radius(size::BAR_BUTTON_H / 2.0)
        .fill(p.control_surface).border(1.0, p.border.alpha(0.10))
        .hover(p.control_hover).press(p.control_pressed).motion(motion::HOVER).on_press_tx(press)
        .child(text(label.to_string()).font(type_::BAR).color(if destructive { p.destructive } else { p.text.alpha(0.92) }))
}

/// A row of a card: a title with its quieter subtitle, and a control at the trailing edge.
fn setting(p: &Palette, title: &str, subtitle: &str, control: El) -> El {
    let mut labels = col().grow(1.0).min_w(0.0).gap(space::XXS).child(text(title.to_string()).font(type_::ROW_TITLE).color(p.text));
    if !subtitle.is_empty() {
        labels = labels.child(text(subtitle.to_string()).font(type_::SECTION).color(p.text_secondary).wrap());
    }
    row().w_full().min_h(46.0).px(INSET).py(space::LG).gap(space::XL).center_y().child(labels).child(control)
}

fn card(p: &Palette, rows: Vec<El>) -> El {
    let mut body = col().w_full().radius(radius::CODE_BLOCK).fill(p.card_fill).border(1.0, p.card_stroke.alpha(0.07));
    for (i, r) in rows.into_iter().enumerate() {
        if i > 0 {
            body = body.child(el().w_full().pl(INSET).child(el().w_full().h(size::HAIRLINE).fill(p.separator.alpha(0.07))));
        }
        body = body.child(r);
    }
    body
}

/// A titled group: the palette's section header over whatever it holds.
fn group(p: &Palette, title: &str, body: El) -> El {
    col().w_full().gap(space::MD)
        .child(text(title.to_string()).font(type_::SECTION).color(p.text_secondary).ml(space::MD))
        .child(body)
}

fn field(p: &Palette, key: &str, edit: &TextEdit, placeholder: &str, access: fn(&mut SettingsUi) -> &mut TextEdit, width: f32) -> El {
    let focused = edit.focused;
    row().key(format!("{key}-box")).w(width).h(30.0).px(space::XL).center_y().radius(radius::ROW).fill(p.control_surface.alpha(0.06))
        .border(if focused { 1.5 } else { 1.0 }, if focused { ACCENT.stops().0.alpha(0.9) } else { p.card_stroke })
        .child(text_input(edit, type_::ROW_TITLE).key(key.to_string()).single_line().grow(1.0).color(p.text)
            .placeholder(placeholder.to_string(), p.text_tertiary).no_focus_ring()
            .on_edit(access)
            .on_input_change(|ui: &mut SettingsUi, _| ui.commit()))
}

/// One of a few pictured choices: its drawing in a frame that rings when chosen, named beneath.
fn choice(p: &Palette, key: String, on: bool, label: &str, picture: El, pick: impl Fn(&mut Settings) + 'static) -> El {
    let ring = if on { ACCENT.stops().0 } else { Color::TRANSPARENT };
    col().key(key).grow(1.0).gap(space::MD).items_center()
        .on_press(move |ui: &mut SettingsUi| {
            pick(&mut ui.settings);
            ui.commit();
        })
        .child(el().padding(3.0).radius(radius::FORM_FIELD + 1.0).border(2.0, ring).motion(motion::HOVER).child(picture))
        .child(row().gap(space::SM).center_y()
            .child(text(label.to_string()).font(type_::BAR).color(if on { p.text } else { p.text_secondary })))
}

// ---- Drawings ---------------------------------------------------------------------------

const PIC: (f32, f32) = (150.0, 92.0);

/// The palette, very small: a search line over three rows, the first selected.
fn mini_rows(dark: bool) -> El {
    let ink = if dark { Color::WHITE } else { Color::BLACK };
    let line = |tint: Tint, w: f32, selected: bool| {
        row().w_full().h(13.0).px(4.0).gap(5.0).center_y().radius(4.0).fill(if selected { ink.alpha(0.11) } else { Color::TRANSPARENT })
            .child(el().size(7.0, 7.0).radius(2.0).fill(tint.stops().1))
            .child(el().w(w).h(3.0).radius(1.5).fill(ink.alpha(if selected { 0.85 } else { 0.45 })))
    };
    col().w_full().padding(7.0).gap(2.0)
        .child(row().w_full().h(13.0).px(4.0).gap(5.0).center_y()
            .child(el().size(6.0, 6.0).radius(3.0).border(1.2, ink.alpha(0.5)))
            .child(el().w(46.0).h(3.0).radius(1.5).fill(ink.alpha(0.28))))
        .child(line(Tint::Violet, 44.0, true))
        .child(line(Tint::Blue, 58.0, false))
        .child(line(Tint::Orange, 34.0, false))
}

fn mini_palette(dark: bool) -> El {
    let ink = if dark { Color::WHITE } else { Color::BLACK };
    el().size(PIC.0, PIC.1).radius(radius::ROW).clip_rounded(radius::ROW)
        .fill(rune::color::hex(if dark { "#1d1d20" } else { "#f3f3f6" }).unwrap_or(Color::BLACK)).border(1.0, ink.alpha(0.10))
        .child(mini_rows(dark))
}

/// Follow the system: the dark palette on the left half, the light one on the right.
fn mini_system() -> El {
    let half = PIC.0 / 2.0;
    el().size(PIC.0, PIC.1).radius(radius::ROW).clip_rounded(radius::ROW)
        .child(mini_palette(true))
        .child(el().absolute(half, 0.0).size(half, PIC.1).clip_rounded(0.0).child(el().absolute(-half, 0.0).child(mini_palette(false))))
}

/// A desktop to stand behind a surface drawing: a few bright shapes on a dark ground.
fn wallpaper(blur: f32) -> El {
    let shape = |x: f32, y: f32, d: f32, tint: Tint| el().absolute(x, y).size(d, d).radius(d / 2.0).fill(tint.stops().0);
    el().size(PIC.0, PIC.1).fill(rune::color::hex("#15151c").unwrap_or(Color::BLACK))
        .child(el().size(PIC.0, PIC.1).blur(blur)
            .child(shape(-10.0, -14.0, 78.0, Tint::Pink))
            .child(shape(64.0, 30.0, 84.0, Tint::Blue))
            .child(shape(18.0, 52.0, 46.0, Tint::Yellow)))
}

/// A surface choice: the palette over a desktop, blurring it, letting it through, or covering it.
fn mini_surface(mode: &str) -> El {
    let (inset_x, inset_y) = (14.0, 11.0);
    let panel = el().absolute(inset_x, inset_y).size(PIC.0 - inset_x * 2.0, PIC.1 - inset_y * 2.0).radius(7.0).clip_rounded(7.0)
        .border(1.0, Color::WHITE.alpha(0.16));
    let panel = match mode {
        "none" => panel.fill(rune::color::hex("#1d1d20").unwrap_or(Color::BLACK)),
        _ => panel.child(el().absolute(-inset_x, -inset_y).child(wallpaper(if mode == "capture" { 9.0 } else { 0.0 })))
            .child(el().pin(FULL, FULL, FULL, FULL).fill(Color::BLACK.alpha(if mode == "capture" { 0.42 } else { 0.30 }))),
    };
    el().size(PIC.0, PIC.1).radius(radius::ROW).clip_rounded(radius::ROW).child(wallpaper(0.0)).child(panel.child(mini_rows(true).padding(5.0)))
}

// ---- Panes ------------------------------------------------------------------------------

fn general(ui: &SettingsUi, p: &Palette) -> Vec<El> {
    let s = &ui.settings;
    let themes = row().w_full().gap(space::LG)
        .child(choice(p, "theme-dark".into(), s.appearance == "dark", "Dark", mini_palette(true), |s| s.appearance = "dark".into()))
        .child(choice(p, "theme-light".into(), s.appearance == "light", "Light", mini_palette(false), |s| s.appearance = "light".into()))
        .child(choice(p, "theme-system".into(), s.appearance == "system", "System", mini_system(), |s| s.appearance = "system".into()));
    let surfaces = row().w_full().gap(space::LG)
        .child(choice(p, "surface-capture".into(), s.backdrop == "capture", "Blur", mini_surface("capture"), |s| s.backdrop = "capture".into()))
        .child(choice(p, "surface-compositor".into(), s.backdrop == "compositor", "Compositor", mini_surface("compositor"), |s| s.backdrop = "compositor".into()))
        .child(choice(p, "surface-none".into(), s.backdrop == "none", "Solid", mini_surface("none"), |s| s.backdrop = "none".into()));
    let surface_note = match s.backdrop.as_str() {
        "capture" => "EchoSpot blurs the screen behind it itself. Works with Hyprland's blur off.",
        "compositor" => "The panel is left translucent for Hyprland's own blur to fill.",
        _ => "An opaque panel: nothing shows through.",
    };
    vec![
        group(p, "Theme", themes),
        group(p, "Surface", col().w_full().gap(space::LG).child(surfaces)
            .child(text(surface_note).font(type_::SECTION).color(p.text_tertiary).ml(space::MD))),
        group(p, "Behavior", card(p, vec![
            setting(p, "Hide when focus is lost", "Clicking another window dismisses the palette.", switch(p, "hide-on-blur", s.hide_on_blur, |s| s.hide_on_blur = !s.hide_on_blur)),
            setting(p, "Show animations", "Menus, selection and the palette's own motion.", switch(p, "animations", s.animations, |s| s.animations = !s.animations)),
            setting(p, "Summon EchoSpot", "Bound in ~/.config/hypr/bindings.lua to `echospot toggle`.", ui::keycaps_row(p, "super+space", true)),
        ])),
    ]
}

fn status_dot(color: Color) -> El {
    el().size(7.0, 7.0).radius(3.5).fill(color).shadow(0.0, 6.0, color.alpha(0.6))
}

/// The AI pane's lead: the model in use, whether it can be reached, and a way to find out.
fn ai_hero(ui: &SettingsUi, p: &Palette) -> El {
    let a = &ui.settings.ai;
    let (top, bottom) = ACCENT.stops();
    let (dot, line) = match &ui.probe {
        Probe::Ok(m) => (p.success, m.clone()),
        Probe::Failed(m) => (p.destructive, m.clone()),
        Probe::Running => (p.warning, "Asking the provider…".to_string()),
        Probe::Idle if a.ready() => (p.success, format!("Ready · {}", if a.provider == "anthropic" { "Anthropic API".to_string() } else { a.base_url.trim_start_matches("https://").trim_start_matches("http://").to_string() })),
        Probe::Idle => (p.warning, "No API key yet".to_string()),
    };
    row().key("ai-hero").w_full().padding(space::XL + 4.0).gap(space::XL + 2.0).center_y().radius(radius::DIALOG_SYMBOL)
        .fill(bottom.alpha(0.10)).linear_gradient(std::f32::consts::FRAC_PI_2, [(0.0, top.alpha(0.26)), (1.0, bottom.alpha(0.03))])
        .border_gradient(1.0, top.alpha(0.55), bottom.alpha(0.10))
        .child(ui::badge_tile(&icons::AI, 44.0, ACCENT))
        .child(col().grow(1.0).min_w(0.0).gap(space::XS)
            .child(text(ai::model_name(&a.model)).font(type_::TITLE3).color(p.text).truncate())
            .child(row().gap(space::MD).center_y().child(status_dot(dot))
                .child(text(line).font(type_::ROW_TRAILING).color(p.text_secondary).truncate().shrink())))
        .child(button(p, "test-connection", if ui.probe == Probe::Running { "Testing…" } else { "Test Connection" }, false, |ui, _| ui.test_connection()))
}

/// A provider as a card to pick: its mark, its name, what it is.
fn provider_card(p: &Palette, key: &str, on: bool, glyph: &'static rune::Icon, tint: Tint, title: &str, blurb: &str, value: &'static str) -> El {
    row().key(key.to_string()).grow(1.0).min_w(0.0).w(0.0).padding(space::XL).gap(space::LG).center_y().radius(radius::CODE_BLOCK)
        .fill(if on { ACCENT.stops().1.alpha(0.12) } else { p.card_fill }).hover(if on { Color::TRANSPARENT } else { p.row_hover })
        .border(if on { 1.5 } else { 1.0 }, if on { ACCENT.stops().0.alpha(0.85) } else { p.card_stroke.alpha(0.07) }).motion(motion::HOVER)
        .on_press(move |ui: &mut SettingsUi| {
            ui.settings.ai.provider = value.into();
            ui.commit();
        })
        .child(ui::badge_tile(glyph, 30.0, tint))
        .child(col().grow(1.0).min_w(0.0).gap(space::XXS)
            .child(text(title.to_string()).font(type_::ROW_TITLE).color(p.text))
            .child(text(blurb.to_string()).font(type_::SECTION).color(p.text_secondary).truncate()))
}

const CLAUDE: [(&str, &str); 4] = [
    ("claude-fable-5-2", "Most intelligent"),
    ("claude-opus-5-5", "Deep, careful work"),
    ("claude-sonnet-5-5", "Fast and capable"),
    ("claude-haiku-5-5", "Fastest"),
];

fn model_row(p: &Palette, id: &'static str, blurb: &str, on: bool) -> El {
    let mark = el().size(16.0, 16.0).radius(8.0).center();
    let mark = if on { mark.fill(ACCENT.stops().1).child(el().size(6.0, 6.0).radius(3.0).fill(Color::WHITE)) } else { mark.border(1.5, p.border) };
    row().key(format!("model-{id}")).w_full().h(40.0).px(INSET).gap(space::LG).center_y()
        .hover(p.row_hover).motion(motion::HOVER)
        .on_press(move |ui: &mut SettingsUi| {
            ui.model.replace_text(id);
            ui.commit();
        })
        .child(mark)
        .child(text(ai::model_name(id)).font(type_::ROW_TITLE).color(p.text))
        .child(spacer())
        .child(text(blurb.to_string()).font(type_::ROW_TRAILING).color(p.text_secondary))
}

fn ai_pane(ui: &SettingsUi, p: &Palette) -> Vec<El> {
    let a = &ui.settings.ai;
    let anthropic = a.provider == "anthropic";
    let providers = row().w_full().gap(space::LG)
        .child(provider_card(p, "provider-anthropic", anthropic, &icons::AI, Tint::Orange, "Claude", "Anthropic's API", "anthropic"))
        .child(provider_card(p, "provider-openai", !anthropic, &icons::CODE, Tint::Teal, "OpenAI-compatible", "OpenAI, OpenRouter, Ollama…", "openai"));
    let mut model: Vec<El> = Vec::new();
    if anthropic {
        model.extend(CLAUDE.iter().map(|(id, blurb)| model_row(p, id, blurb, a.model == *id)));
        model.push(setting(p, "Another model", "Any model id your key can use.", field(p, "model", &ui.model, "claude-opus-5-5", |ui| &mut ui.model, 230.0)));
    } else {
        model.push(setting(p, "Model", "The name your endpoint expects.", field(p, "model", &ui.model, "gpt-5", |ui| &mut ui.model, 260.0)));
        model.push(setting(p, "Endpoint", "The base URL, up to and including /v1.", field(p, "base-url", &ui.base_url, "https://api.openai.com/v1", |ui| &mut ui.base_url, 260.0)));
    }
    let env = if anthropic { "ANTHROPIC_API_KEY" } else { "OPENAI_API_KEY" };
    vec![
        ai_hero(ui, p),
        group(p, "Provider", providers),
        group(p, "Model", card(p, model)),
        group(p, "Access", card(p, vec![
            setting(p, "API key", &format!("Kept in settings.json, readable only by you. Left empty, {env} is used."),
                field(p, "api-key", &ui.api_key, "Paste a key", |ui| &mut ui.api_key, 260.0)),
        ])),
        group(p, "Instructions", col().key("system-box").w_full().min_h(92.0).px(INSET).py(space::LG).radius(radius::CODE_BLOCK).fill(p.card_fill)
            .border(if ui.system_prompt.focused { 1.5 } else { 1.0 }, if ui.system_prompt.focused { ACCENT.stops().0.alpha(0.9) } else { p.card_stroke.alpha(0.07) })
            .child(text_input(&ui.system_prompt, type_::ROW_TITLE).key("system-prompt").w_full().grow(1.0).color(p.text)
                .placeholder("A system prompt. Leave empty for EchoSpot's own: short, direct answers.", p.text_tertiary).no_focus_ring()
                .on_edit(|ui: &mut SettingsUi| &mut ui.system_prompt)
                .on_input_change(|ui: &mut SettingsUi, _| ui.commit()))),
        group(p, "History", card(p, vec![
            setting(p, "Saved chats", &format!("{} kept in ~/.local/share/echospot/chats. The newest 40 stay.", ui.chats),
                button(p, "clear-chats", "Delete All…", true, |ui, _| {
                    let _ = std::fs::remove_dir_all(crate::util::data_dir().join("chats"));
                    ui.chats = 0;
                    tell_main(|app| app.chat.clear());
                })),
        ])),
    ]
}

/// A number worth a glance, over what it counts.
fn stat(p: &Palette, value: String, label: &str, tint: Tint) -> El {
    col().grow(1.0).min_w(0.0).w(0.0).padding(space::XL + 2.0).gap(space::XS).radius(radius::CODE_BLOCK).fill(p.card_fill).border(1.0, p.card_stroke.alpha(0.07))
        .child(row().gap(space::MD).center_y().child(el().size(6.0, 6.0).radius(3.0).fill(tint.stops().0))
            .child(text(label.to_string()).font(type_::SECTION).color(p.text_secondary)))
        .child(text(value).font(TextStyle::new(24.0, 600.0).lh(30.0)).color(p.text))
}

fn clipboard(ui: &SettingsUi, p: &Palette) -> Vec<El> {
    let s = &ui.settings;
    let (entries, images, bytes) = ui.clipboard;
    vec![
        row().w_full().gap(space::LG)
            .child(stat(p, entries.to_string(), "Entries", Tint::Orange))
            .child(stat(p, images.to_string(), "Images", Tint::Pink))
            .child(stat(p, crate::util::human_size(bytes), "On disk", Tint::Blue)),
        group(p, "History", card(p, vec![
            setting(p, "Keep clipboard history", "Text, links, colors and images you copy. Entries a password manager marks sensitive are never kept. Takes effect the next time EchoSpot starts.",
                switch(p, "clipboard-history", s.clipboard_history, |s| s.clipboard_history = !s.clipboard_history)),
            setting(p, "Clear history", "Removes every entry, pinned ones too, from ~/.local/share/echospot/clipboard.",
                button(p, "clear-clipboard", "Clear…", true, |ui, _| {
                    let _ = std::fs::remove_dir_all(crate::util::data_dir().join("clipboard"));
                    ui.clipboard = (0, 0, 0);
                    tell_main(|app| app.clipboard.reload());
                })),
        ])),
        group(p, "In the palette", card(p, vec![
            setting(p, "Open Clipboard History", "Search “clipboard” in the launcher, or bind `echospot open clipboard` to a key.", ui::keycaps_row(p, "enter", true)),
            setting(p, "Paste the selected entry", "Goes straight into the app you came from.", ui::keycaps_row(p, "enter", true)),
            setting(p, "Pin an entry", "Pinned entries stay at the top and are never pruned.", ui::keycaps_row(p, "ctrl+shift+p", true)),
        ])),
    ]
}

fn extensions(ui: &SettingsUi, p: &Palette, scale: f32) -> Vec<El> {
    let (top, bottom) = Tint::Green.stops();
    let store = row().key("store-card").w_full().padding(space::XL + 4.0).gap(space::XL + 2.0).center_y().radius(radius::DIALOG_SYMBOL)
        .fill(bottom.alpha(0.09)).linear_gradient(std::f32::consts::FRAC_PI_2, [(0.0, top.alpha(0.22)), (1.0, bottom.alpha(0.02))])
        .border_gradient(1.0, top.alpha(0.45), bottom.alpha(0.08))
        .child(ui::badge_tile(&icons::STORE, 44.0, Tint::Green))
        .child(col().grow(1.0).min_w(0.0).gap(space::XS)
            .child(text("Raycast Store").font(type_::TITLE3).color(p.text))
            .child(text("Thousands of extensions, installed from the bundles the store already built. They run in Node.").font(type_::ROW_TRAILING).color(p.text_secondary).wrap()))
        .child(button(p, "open-store", "Open Store", false, |_, tx| {
            tx.close_window();
            std::thread::spawn(|| {
                crate::ipc::send("open store");
            });
        }));
    let mut rows: Vec<El> = ui.extensions.iter().map(|e| {
        let id = e.id.clone();
        let picture = match e.icon.as_ref().and_then(|path| ui.icons.file(path, 28.0, scale)) {
            Some(px) => el().size(28.0, 28.0).radius(7.0).clip_rounded(7.0).pixels_fit(&px, rune::SurfaceFit::Fill),
            None => ui::badge_tile(&icons::PUZZLE, 31.0, Tint::Green),
        };
        let by = if e.author.is_empty() { String::new() } else { format!("{} · ", e.author) };
        row().w_full().min_h(50.0).px(INSET).py(space::MD).gap(space::XL).center_y()
            .child(picture)
            .child(col().grow(1.0).min_w(0.0).gap(space::XXS)
                .child(text(e.title.clone()).font(type_::ROW_TITLE).color(p.text).truncate())
                .child(text(format!("{by}{} command{}", e.commands, if e.commands == 1 { "" } else { "s" })).font(type_::SECTION).color(p.text_secondary).truncate()))
            .child(button(p, &format!("uninstall-{id}"), "Uninstall", true, move |ui, _| {
                let _ = std::fs::remove_dir_all(crate::ext::extensions_dir().join(&id));
                ui.extensions.retain(|e| e.id != id);
                tell_main(|app| app.ext.rescan());
            }))
    }).collect();
    if rows.is_empty() {
        rows.push(setting(p, "Nothing installed yet", "Extensions you install from the store appear here.", el()));
    }
    vec![store, group(p, &format!("Installed · {}", ui.extensions.len()), card(p, rows))]
}

const SHORTCUTS: [(&str, &[(&str, &str)]); 4] = [
    ("Everywhere", &[
        ("Run the selected row", "enter"), ("Show its actions", "ctrl+k"), ("Move through rows", "up"), ("Next row", "ctrl+n"),
        ("Previous row", "ctrl+p"), ("Back, or clear the search", "escape"), ("Back when the search is empty", "backspace"), ("Hide the palette", "ctrl+w"),
    ]),
    ("Launcher", &[
        ("Ask Quick AI what is typed", "tab"), ("Add to or remove from Favorites", "ctrl+shift+f"), ("Copy the name or answer", "ctrl+shift+c"),
        ("Open Settings", "ctrl+,"), ("Quit EchoSpot", "ctrl+q"),
    ]),
    ("AI", &[
        ("Pick a model", "tab"), ("Paste the answer", "ctrl+enter"), ("Ask again", "ctrl+r"), ("Continue Quick AI as a chat", "ctrl+t"), ("New chat", "ctrl+shift+n"),
    ]),
    ("Lists you edit", &[
        ("Edit the selected snippet or quicklink", "ctrl+e"), ("Create a new one", "ctrl+shift+n"), ("Delete it", "ctrl+x"), ("Submit a form", "ctrl+enter"),
    ]),
];

fn shortcuts(p: &Palette) -> Vec<El> {
    SHORTCUTS.iter().map(|(title, list)| {
        group(p, title, card(p, list.iter().map(|(what, keys)| {
            row().w_full().h(38.0).px(INSET).gap(space::XL).center_y()
                .child(text(what.to_string()).font(type_::ROW_TITLE).color(p.text).truncate().grow(1.0))
                .child(ui::keycaps_row(p, keys, true))
        }).collect()))
    }).collect()
}

fn about(p: &Palette) -> Vec<El> {
    let link = |label: &'static str, url: &'static str| button(p, label, label, false, move |_, _| crate::util::open_url(url));
    let (top, _) = ACCENT.stops();
    // The mark in its own light.
    let mark = el().size(190.0, 132.0).center()
        .child(el().absolute(0.0, -14.0).size(190.0, 160.0).radial_gradient([0.5, 0.5], 86.0, [(0.0, top.alpha(0.38)), (1.0, top.alpha(0.0))]).inert())
        .child(ui::badge_tile(&icons::LOGO, 84.0, ACCENT));
    vec![
        col().w_full().items_center().gap(space::XS).pb(space::MD)
            .child(mark)
            .child(text("EchoSpot").font(TextStyle::new(22.0, 650.0).lh(28.0)).color(p.text))
            .child(text(format!("Version {}", env!("CARGO_PKG_VERSION"))).font(type_::ROW_TRAILING).color(p.text_secondary))
            .child(text("A native command palette for Hyprland, drawn with Rune.").font(type_::ROW_TRAILING).color(p.text_tertiary).mt(space::XS)),
        card(p, vec![
            setting(p, "Source", "github.com/adityavardhansharma/EchoSpot · AGPL-3.0", link("GitHub", "https://github.com/adityavardhansharma/EchoSpot")),
            setting(p, "Design", "Tinycast, for macOS, is where this began.", link("Tinycast", "https://tinycast.dev")),
            setting(p, "Drawn with", "Rune draws every pixel, on the GPU.", link("Rune", "https://github.com/anomalyco/rune")),
        ]),
    ]
}

// ---- The window -------------------------------------------------------------------------

fn sidebar(ui: &SettingsUi, p: &Palette) -> El {
    // The brand row stands where the palette's search field does, and moves the window.
    let brand = row().key("brand").w_full().h(HEAD_H).px(space::MD).gap(space::LG).center_y().window_drag()
        .child(ui::badge_tile(&icons::LOGO, 30.0, ACCENT))
        .child(col().gap(0.0)
            .child(text("EchoSpot").font(type_::PANEL_TITLE).color(p.text))
            .child(text("Settings").font(type_::SECTION).color(p.text_secondary)));
    let mut list = col().w_full().gap(space::XXS);
    for info in &PANES {
        let (on, pane) = (ui.pane == info.pane, info.pane);
        let item = row().key(format!("pane-{}", info.title)).w_full().px(space::MD).py(space::SM).gap(space::LG).center_y().radius(radius::ROW);
        let item = if on { item.fill(p.selection) } else { item.fill(Color::TRANSPARENT).hover(p.row_hover) };
        list = list.child(item.motion(motion::HOVER).on_press(move |ui: &mut SettingsUi| ui.pane = pane)
            .child(ui::badge_tile(info.glyph, size::RESULT_ROW_ICON, info.tint))
            .child(text(info.title).font(type_::ROW_TITLE).color(if on { p.text } else { p.text.alpha(0.86) })));
    }
    col().key("sidebar").w(SIDEBAR_W).h_full().px(space::MD).child(brand).child(list)
}

fn foot(p: &Palette) -> El {
    let pill_button = |key: &str| row().key(key.to_string()).h(size::BAR_BUTTON_H).px(space::MD).gap(space::SM).center_y().radius(size::BAR_BUTTON_H / 2.0)
        .hover(p.row_hover).press(p.control_surface).motion(motion::HOVER);
    let done = pill_button("done").on_press_tx(|_: &mut SettingsUi, tx| tx.close_window())
        .child(text("Done").font(TextStyle::new(12.0, 600.0).lh(16.0)).color(p.text))
        .child(ui::keycap(p, "esc", false));
    let file = pill_button("open-config")
        .on_press(|_: &mut SettingsUi| { crate::util::spawn_detached(&["xdg-open".into(), Settings::path().display().to_string()]); })
        .child(icon(&icons::CODE, 13.0).color(p.text_secondary))
        .child(text("settings.json").font(type_::BAR).color(p.text.alpha(0.86)));
    let group = row().key("foot-actions").h(size::MENU_BUTTON).px(space::XS).center_y().child(file).child(done);
    row().key("foot").pin(FULL, None, FULL, FULL).h(size::BOTTOM_BAR_H).px(space::MD).center_y()
        .child(text(format!("v{}", env!("CARGO_PKG_VERSION"))).font(type_::SECTION).color(p.text_tertiary).ml(space::LG))
        .child(spacer()).child(ui::glass(group, p, size::MENU_BUTTON / 2.0))
}

pub fn view(ui: &SettingsUi, cx: &mut Cx<SettingsUi>) -> El {
    let env = cx.env();
    let p = theme::palette(ui.settings.dark(env.system_dark));
    let scale = env.scale.max(1.0);
    let info = PANES.iter().find(|i| i.pane == ui.pane).unwrap_or(&PANES[0]);
    let sections = match ui.pane {
        Pane::General => general(ui, &p),
        Pane::Ai => ai_pane(ui, &p),
        Pane::Clipboard => clipboard(ui, &p),
        Pane::Extensions => extensions(ui, &p, scale),
        Pane::Shortcuts => shortcuts(&p),
        Pane::About => about(&p),
    };
    let content = col().w_full().px(space::XXL).pt(space::XS).pb(size::BOTTOM_BAR_H + space::XL).gap(space::XXL).children(sections);
    let fade = ScrollFade::strength(size::DISSOLVE_TOP).full_at(4.0).with_end(ScrollFade::strength(size::DISSOLVE_BOTTOM));
    // What the pane is, in its own colour; the band moves the window.
    let heading = row().key("heading").w_full().h(HEAD_H).px(space::XXL + space::XS).gap(space::XL).center_y().window_drag()
        .child(ui::badge_tile(info.glyph, 38.0, info.tint))
        .child(col().gap(space::XXS)
            .child(text(info.title).font(type_::TITLE2).color(p.text))
            .child(text(info.blurb).font(type_::ROW_TRAILING).color(p.text_secondary)));
    let detail = col().key("detail").grow(1.0).h_full().min_h(0.0).child(heading)
        .child(col().key(format!("settings-scroll-{}", info.title)).w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).scroll_fade(fade).child(content));
    // The one hairline: between the list and what it selects, as in the palette's split views.
    let rule = col().h_full().py(space::XXL).child(el().w(size::HAIRLINE).grow(1.0).fill(p.separator.alpha(0.07)));
    // EchoSpot's own light, low in the corner behind its mark.
    let glow = el().absolute(-150.0, -170.0).size(460.0, 420.0)
        .radial_gradient([0.5, 0.5], 220.0, [(0.0, ACCENT.stops().0.alpha(if p.dark { 0.20 } else { 0.14 })), (1.0, ACCENT.stops().0.alpha(0.0))]).inert();
    let panel = ui::panel_surface("settings-panel", &p, PANEL, ui.has_backdrop.then_some(&ui.backdrop), &ui.settings.backdrop)
        .child(glow)
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
        rune::Key::Command(c @ '1'..='6') => ui.pane = PANES[c as usize - '1' as usize].pane,
        _ => return false,
    }
    true
}
