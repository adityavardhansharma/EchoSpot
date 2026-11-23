//! Surfaces outside the palette: overlays over the screen, widgets on the desktop, a window.
//! Rune makes a process's overlays when it starts, so the host makes a few empty slots and
//! lends one to each surface a pack shows. A slot's view draws blocks; the pack's code runs
//! where it always does, and what it returns is handed across.

use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use rune::{Align, AppHandle, Cx, El, OverlayHandle, OverlayInput, OverlayOptions, OverlayPlacement, OverlaySpec, ScreenEdge, ScrollSpec, WaylandLayer, WindowHandle, col, el, row, text};
use serde_json::{Value, json};

use crate::app::{App, Tone};
use crate::store::IconStore;
use crate::theme::{self, radius, size, space};

use super::draw;
use super::looks;
use super::runtime::State;
use super::surface::{self, TILE, TILE_GAP};
use super::triggers::parse_every;

/// How many overlays and how many widgets can show at once.
pub const OVERLAYS: usize = 4;
pub const WIDGETS: usize = 4;
/// The window a pack's `window` surface opens in.
pub const WINDOW_SIZE: (f32, f32) = (560.0 + size::WINDOW_MARGIN * 2.0, 420.0 + size::WINDOW_MARGIN * 2.0);
const EDGE: f32 = 18.0;

/// What one slot is showing.
#[derive(Default)]
pub struct Slot {
    pub blocks: Value,
    pub pack: String,
    pub target: String,
    pub title: String,
    pub look: String,
    pub dir: PathBuf,
    pub size: (f32, f32),
    pub place: String,
    /// The whole display is the slot's, and the surface sits inside it by its place.
    pub full: bool,
    pub icons: IconStore,
    /// What has been typed into the surface's inputs, by key.
    pub inputs: std::collections::HashMap<String, rune::TextEdit>,
}

impl draw::PackInputs for Slot {
    fn pack_input(&mut self, key: &str, initial: &str) -> &mut rune::TextEdit {
        self.inputs.entry(key.to_string()).or_insert_with(|| rune::TextEdit::new(initial))
    }
}

pub fn setup(slot: &mut Slot, waker: rune::Waker) {
    slot.icons.attach(rune::ImageCache::new(waker));
}

struct Lent {
    handle: OverlayHandle<Slot>,
    widget: bool,
    /// The surface it shows, as `<pack>.<name>`.
    key: Option<String>,
    drew: Value,
}

static SLOTS: Mutex<Vec<Lent>> = Mutex::new(Vec::new());
static WINDOW: OnceLock<(AppHandle<Slot>, WindowHandle)> = OnceLock::new();
static WINDOW_KEY: Mutex<Option<String>> = Mutex::new(None);

/// The spec of an empty slot: on the overlay layer, or on the desktop's for a widget.
pub fn spec(widget: bool) -> rune::ConfiguredOverlay<Slot> {
    OverlaySpec { name: if widget { "echospot-widget" } else { "echospot-overlay" }, size: (240.0, 120.0), edge: ScreenEdge::Top, margin: EDGE, align: Align::End, inset: EDGE, interactive: false, view }
        .options(OverlayOptions { wayland_layer: if widget { WaylandLayer::Bottom } else { WaylandLayer::Overlay }, input: OverlayInput::None, ..Default::default() })
        .setup(setup)
}

pub fn attach(handle: OverlayHandle<Slot>, widget: bool) {
    SLOTS.lock().unwrap_or_else(|e| e.into_inner()).push(Lent { handle, widget, key: None, drew: Value::Null });
}

pub fn attach_window(handle: AppHandle<Slot>, window: WindowHandle) {
    let _ = WINDOW.set((handle, window));
}

fn palette(slot: &Slot, dark: bool) -> (theme::Palette, &'static looks::Look) {
    let look = looks::get(if slot.look.is_empty() { "glass" } else { &slot.look });
    (look.palette(theme::palette(dark)), look)
}

/// The card a surface outside the palette stands on.
fn card(slot: &Slot, cx: &mut Cx<Slot>, inset: f32) -> El {
    let env = cx.env();
    let (p, look) = palette(slot, env.system_dark);
    let ctx = draw::Ctx::new(theme::palette(env.system_dark), look, env.scale.max(1.0), Some(&slot.icons), &slot.pack, &slot.target, slot.dir.clone(), (slot.size.0 - inset * 2.0).max(40.0)).with_inputs(&slot.inputs);
    let ground = if look.material == "flat" { p.panel_scrim } else if p.dark { rune::color::hex("#1b1b1d").unwrap_or(rune::Color::BLACK).alpha(0.86) } else { rune::Color::WHITE.alpha(0.90) };
    col().size(slot.size.0, slot.size.1).padding(inset).justify_center().radius(radius::CARD + 6.0).clip_rounded(radius::CARD + 6.0)
        .fill(ground).border_gradient(1.0, p.glass_rim_top, p.glass_rim_bottom)
        .child(draw::block::<Slot>(&ctx, &slot.blocks))
}

pub fn view(slot: &Slot, cx: &mut Cx<Slot>) -> El {
    if slot.blocks.is_null() {
        return el();
    }
    let surface = card(slot, cx, space::XL);
    if !slot.full {
        return el().w_full().h_full().child(surface);
    }
    // The whole display: the surface sits where its place says.
    let line = match slot.place.as_str() {
        "left" => row().w_full().h_full().center_y().pl(EDGE),
        "right" => row().w_full().h_full().center_y().justify_end().pr(EDGE),
        _ => row().w_full().h_full().center(),
    };
    line.child(surface)
}

/// The window a pack's `window` surface opens in: the palette's own panel, smaller.
pub fn window_view(slot: &Slot, cx: &mut Cx<Slot>) -> El {
    let env = cx.env();
    let (p, look) = palette(slot, env.system_dark);
    let (w, h) = (WINDOW_SIZE.0 - size::WINDOW_MARGIN * 2.0, WINDOW_SIZE.1 - size::WINDOW_MARGIN * 2.0);
    let ctx = draw::Ctx::new(theme::palette(env.system_dark), look, env.scale.max(1.0), Some(&slot.icons), &slot.pack, &slot.target, slot.dir.clone(), w - space::XXL * 2.0).with_inputs(&slot.inputs);
    let header = row().w_full().h(size::HEADER_BAND - 12.0).px(space::XXL).center_y()
        .child(text(slot.title.clone()).font(theme::type_::PANEL_TITLE).color(p.text_secondary).truncate());
    let body = col().key("list").w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).scroll_edge_fade(20.0)
        .child(col().w_full().px(space::XXL).pb(space::XXL).child(draw::block::<Slot>(&ctx, &slot.blocks)));
    let panel = crate::ui::panel_surface("pack-window", &p, (w, h), None, "none", false).child(col().w_full().h_full().child(header).child(body)).child(crate::ui::panel_rim(&p));
    el().w_full().h_full().child(panel)
}

/// A surface's size in points: `240x120`, or tiles of the board grid as `2x1`.
fn size_of(text: &str, widget: bool) -> (f32, f32) {
    let tiles = |w: f32, h: f32| (w * TILE as f32 + (w - 1.0) * TILE_GAP as f32, h * TILE as f32 + (h - 1.0) * TILE_GAP as f32);
    match text.split_once('x').and_then(|(w, h)| Some((w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?))) {
        Some((w, h)) if w <= 8.0 && h <= 8.0 => tiles(w.max(1.0), h.max(1.0)),
        Some((w, h)) => (w.clamp(60.0, 1600.0), h.clamp(32.0, 1200.0)),
        None if widget => tiles(2.0, 1.0),
        None => (260.0, 96.0),
    }
}

fn placement(place: &str) -> Option<OverlayPlacement> {
    let edge = |edge, align| Some(OverlayPlacement::Edge { edge, margin: EDGE, align, inset: EDGE });
    match place {
        "top-left" => edge(ScreenEdge::Top, Align::Start),
        "top" => edge(ScreenEdge::Top, Align::Center),
        "" | "top-right" => edge(ScreenEdge::Top, Align::End),
        "bottom-left" => edge(ScreenEdge::Bottom, Align::Start),
        "bottom" => edge(ScreenEdge::Bottom, Align::Center),
        "bottom-right" => edge(ScreenEdge::Bottom, Align::End),
        _ => None,
    }
}

/// What a showing key names: `<pack>.<surface>`, or `<pack>.board:<name>:window` and
/// `<pack>.board:<name>:widget` for a board in its own window or on the desktop (§11). Returns
/// the pack, the target to draw and the surface it shows as.
pub fn showable(app: &App, key: &str) -> Option<(String, String, super::index::Surface)> {
    let (pack, name) = key.split_once('.')?;
    let p = app.packs.rt.packs.get(pack)?;
    if let Some(rest) = name.strip_prefix("board:") {
        let (board, as_) = rest.rsplit_once(':')?;
        if !matches!(as_, "window" | "widget") {
            return None;
        }
        let b = p.index.boards.get(board)?;
        // A widget is as big as its tiles: four columns, as many rows as they fill.
        let (mut rows, mut used, mut line) = (0usize, 0usize, 0usize);
        for tile in &b.tiles {
            let (w, h) = tile.span();
            if used + w > 4 {
                rows += line;
                used = 0;
                line = 0;
            }
            used += w;
            line = line.max(h);
        }
        rows += line;
        let refresh = b.tiles.iter().filter_map(|t| parse_every(&t.refresh).map(|s| (s, t.refresh.clone()))).min_by(|a, b| a.0.total_cmp(&b.0)).map(|(_, r)| r).unwrap_or_else(|| "1m".into());
        let surface = super::index::Surface {
            surface: as_.into(), title: b.title.clone(), icon: b.icon.clone(), place: if as_ == "widget" { "bottom-right".into() } else { String::new() },
            input: "all".into(), size: format!("4x{}", rows.max(1)), look: b.look.clone(), refresh,
        };
        return Some((pack.to_string(), format!("board:{board}"), surface));
    }
    let s = p.index.surfaces.get(name)?.clone();
    Some((pack.to_string(), format!("surface:{name}"), s))
}

/// Whether a showing key is a window, of either kind.
fn is_window(app: &App, key: &str) -> bool {
    showable(app, key).is_some_and(|(_, _, s)| s.surface == "window")
}

/// Shows a board in its own window or on the desktop, or hides it there.
pub fn toggle_board(app: &mut App, pack: &str, board: &str, window: bool) {
    let key = format!("{pack}.board:{board}:{}", if window { "window" } else { "widget" });
    toggle_key(app, key);
}

fn save(app: &App) {
    crate::util::write_json(&super::state_dir().join("surfaces.json"), &app.packs.shown);
}

/// Shows a pack's overlay, widget or window, or hides it when it is showing.
pub fn toggle(app: &mut App, pack: &str, name: &str) {
    toggle_key(app, format!("{pack}.{name}"));
}

fn toggle_key(app: &mut App, key: String) {
    let kind = showable(app, &key).map(|(_, _, s)| s.surface).unwrap_or_default();
    if let Some(at) = app.packs.shown.iter().position(|k| *k == key) {
        app.packs.shown.remove(at);
    } else {
        let widget = kind == "widget";
        let room = match kind.as_str() {
            "window" => true,
            _ => {
                let slots = SLOTS.lock().unwrap_or_else(|e| e.into_inner());
                slots.iter().filter(|s| s.widget == widget).count() > slots.iter().filter(|s| s.widget == widget && s.key.is_some()).count()
            }
        };
        if !room {
            return app.toast(format!("Only {} {}s show at once: hide one first", if widget { WIDGETS } else { OVERLAYS }, kind), Tone::Neutral);
        }
        // One window: a new one takes the last one's place.
        if kind == "window" {
            let shown = std::mem::take(&mut app.packs.shown);
            app.packs.shown = shown.into_iter().filter(|k| !is_window(app, k)).collect();
        }
        app.packs.shown.push(key);
    }
    save(app);
    sync(app);
    app.refresh_keeping();
}

/// Draws every surface that is showing and hands each slot what changed.
pub fn sync(app: &mut App) {
    if app.packs.headless {
        return;
    }
    let dark = app.system_dark.get();
    let shown = app.packs.shown.clone();
    let mut window: Option<String> = None;
    let mut wanted: Vec<(String, bool, Slot, String)> = Vec::new();
    for key in &shown {
        let Some((pack, target, s)) = showable(app, key) else { continue };
        let pack = pack.as_str();
        let Some(p) = app.packs.rt.packs.get(pack) else { continue };
        if !matches!(p.state, State::Ready | State::Broken | State::NeedsGrant) {
            continue;
        }
        let widget = s.surface == "widget";
        let full = s.surface != "window" && placement(&s.place).is_none();
        let size = if s.surface == "window" { (WINDOW_SIZE.0 - size::WINDOW_MARGIN * 2.0, WINDOW_SIZE.1) } else { size_of(&s.size, widget) };
        let (dir, look) = (p.dir.clone(), if s.look.is_empty() { p.index.look.clone() } else { s.look.clone() });
        let state = json!({ "size": [size.0, size.1], "dark": dark });
        let blocks = match surface::render(&mut app.packs.rt, pack, &target, &json!({}), &state) {
            Ok(blocks) => blocks,
            Err(e) => {
                app.packs.errors.insert(pack.to_string(), e.clone());
                surface::failed(&s.title, &e)
            }
        };
        let slot = Slot { blocks, pack: pack.into(), target, title: s.title.clone(), look, dir, size, place: s.place.clone(), full, icons: IconStore::default(), inputs: Default::default() };
        if s.surface == "window" {
            window = Some(key.clone());
            push_window(key, slot);
        } else {
            wanted.push((key.clone(), widget, slot, s.input.clone()));
        }
    }
    let mut slots = SLOTS.lock().unwrap_or_else(|e| e.into_inner());
    // A slot whose surface is no longer showing goes back.
    for lent in slots.iter_mut().filter(|l| l.key.as_ref().is_some_and(|k| !wanted.iter().any(|w| w.0 == *k))) {
        lent.key = None;
        lent.drew = Value::Null;
        lent.handle.hide();
        lent.handle.update(|s, _| s.blocks = Value::Null);
    }
    for (key, widget, slot, input) in wanted {
        let at = slots.iter().position(|l| l.key.as_deref() == Some(key.as_str())).or_else(|| slots.iter().position(|l| l.key.is_none() && l.widget == widget));
        let Some(lent) = at.map(|i| &mut slots[i]) else { continue };
        let fresh = lent.key.is_none();
        let fingerprint = json!([slot.blocks, slot.size.0, slot.size.1, slot.place, slot.look]);
        if !fresh && lent.drew == fingerprint {
            continue;
        }
        lent.key = Some(key);
        lent.drew = fingerprint;
        let (size, place, full) = (slot.size, slot.place.clone(), slot.full);
        lent.handle.update(move |s, tx| {
            // What a showing surface draws next moves there, rather than landing at once (§13.5).
            if !fresh {
                tx.animated();
            }
            // The slot's own picture cache stays: only what it shows changes.
            let (icons, inputs) = (std::mem::take(&mut s.icons), std::mem::take(&mut s.inputs));
            *s = Slot { icons, inputs, ..slot };
        });
        if fresh {
            let interactive = !matches!(input.as_str(), "" | "none");
            match placement(&place) {
                Some(at) => {
                    lent.handle.resize(size);
                    lent.handle.set_placement(Some(at));
                    lent.handle.set_input(if interactive { OverlayInput::All } else { OverlayInput::None });
                }
                None => {
                    lent.handle.set_placement(Some(OverlayPlacement::FullScreen));
                    lent.handle.set_input(if interactive && full { OverlayInput::Regions } else { OverlayInput::None });
                }
            }
            lent.handle.show();
        }
    }
    drop(slots);
    let mut open = WINDOW_KEY.lock().unwrap_or_else(|e| e.into_inner());
    if window.is_none() && open.is_some() && let Some((_, handle)) = WINDOW.get() {
        handle.close();
    }
    *open = window;
}

fn push_window(key: &str, slot: Slot) {
    let Some((handle, window)) = WINDOW.get() else { return };
    let opening = WINDOW_KEY.lock().unwrap_or_else(|e| e.into_inner()).as_deref() != Some(key);
    handle.update(move |s, _| {
        if s.blocks != slot.blocks || s.target != slot.target || s.pack != slot.pack {
            let (icons, inputs) = (std::mem::take(&mut s.icons), std::mem::take(&mut s.inputs));
            *s = Slot { icons, inputs, ..slot };
        }
    });
    if opening {
        window.open();
    }
}

/// The window was closed by its own key: it is no longer showing.
pub fn window_closed(app: &mut App) {
    let shown = std::mem::take(&mut app.packs.shown);
    app.packs.shown = shown.into_iter().filter(|k| !is_window(app, k)).collect();
    *WINDOW_KEY.lock().unwrap_or_else(|e| e.into_inner()) = None;
    save(app);
}

pub fn window_key(_: &mut Slot, ev: &rune::KeyEvent, tx: &mut rune::Tx) -> bool {
    if matches!(ev.key, rune::Key::Escape | rune::Key::Command('w')) {
        tx.close_window();
        super::host::dispatch_closed();
        return true;
    }
    false
}

/// Draws again what asked to be drawn on a clock. True when anything did.
pub fn due(app: &mut App, now: f64) -> bool {
    if app.packs.shown.is_empty() {
        return false;
    }
    let mut any = false;
    for key in app.packs.shown.clone() {
        let every = showable(app, &key).and_then(|(_, _, s)| parse_every(&s.refresh)).unwrap_or(0.0);
        let last = app.packs.drew(&key);
        if last == 0.0 || every > 0.0 && now - last >= every.max(1.0) {
            app.packs.mark_drew(&key, now);
            any = true;
        }
    }
    if any {
        sync(app);
    }
    any
}
