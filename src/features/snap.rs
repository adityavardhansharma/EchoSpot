//! Snap Layouts: a layout's zones drawn in the palette, where picking a zone places a window in
//! it. The windows are the ones on the workspace the palette was summoned over; each placement
//! moves on to the next window, and "All" arranges every one of them at once.

use std::collections::HashMap;

use rune::{Color, El, Icon, Key, KeyEvent, Tx, col, el, icon, row, spacer, text};

use crate::app::{App, Built};
use crate::features::{fuzzy, windows};
use crate::icons;
use crate::model::*;
use crate::theme::{Palette, Tint, radius, size, space, type_};
use crate::ui;
use crate::util;

pub struct Zone {
    pub name: &'static str,
    /// `[x, y, w, h]` as fractions of the work area.
    pub at: [f32; 4],
}

pub struct SnapLayout {
    /// The launcher row's id, and what usage is recorded under.
    pub id: &'static str,
    pub name: &'static str,
    pub describes: &'static str,
    pub icon: &'static Icon,
    pub zones: &'static [Zone],
}

const fn zone(name: &'static str, x: f32, y: f32, w: f32, h: f32) -> Zone {
    Zone { name, at: [x, y, w, h] }
}

const THIRD: f32 = 1.0 / 3.0;

pub static LAYOUTS: &[SnapLayout] = &[
    SnapLayout { id: "snap:halves", name: "Halves", describes: "Two windows side by side", icon: &icons::SNAP_HALVES, zones: &[
        zone("Left Half", 0.0, 0.0, 0.5, 1.0), zone("Right Half", 0.5, 0.0, 0.5, 1.0)] },
    SnapLayout { id: "snap:two-thirds", name: "Two Thirds", describes: "A wide window beside a narrow one", icon: &icons::SNAP_TWO_THIRDS, zones: &[
        zone("Left Two Thirds", 0.0, 0.0, 2.0 * THIRD, 1.0), zone("Right Third", 2.0 * THIRD, 0.0, THIRD, 1.0)] },
    SnapLayout { id: "snap:thirds", name: "Thirds", describes: "Three columns", icon: &icons::SNAP_THIRDS, zones: &[
        zone("Left Third", 0.0, 0.0, THIRD, 1.0), zone("Center Third", THIRD, 0.0, THIRD, 1.0), zone("Right Third", 2.0 * THIRD, 0.0, THIRD, 1.0)] },
    SnapLayout { id: "snap:quarters", name: "Quarters", describes: "A window in each corner", icon: &icons::SNAP_QUARTERS, zones: &[
        zone("Top Left Quarter", 0.0, 0.0, 0.5, 0.5), zone("Top Right Quarter", 0.5, 0.0, 0.5, 0.5),
        zone("Bottom Left Quarter", 0.0, 0.5, 0.5, 0.5), zone("Bottom Right Quarter", 0.5, 0.5, 0.5, 0.5)] },
    SnapLayout { id: "snap:side-stack", name: "Side Stack", describes: "One tall window, two stacked beside it", icon: &icons::SNAP_SIDE_STACK, zones: &[
        zone("Left Half", 0.0, 0.0, 0.5, 1.0), zone("Top Right Quarter", 0.5, 0.0, 0.5, 0.5), zone("Bottom Right Quarter", 0.5, 0.5, 0.5, 0.5)] },
    SnapLayout { id: "snap:center", name: "Center", describes: "One window in the middle", icon: &icons::ALMOST_MAXIMIZE, zones: &[
        zone("Center", 0.16, 0.06, 0.68, 0.88)] },
];

#[derive(Clone, Debug)]
pub enum SnapOp {
    /// Put the window being placed in a zone: layout, zone.
    Place(usize, usize),
    /// Arrange every window with a layout.
    All(usize),
    /// Place this window next, by its index.
    Target(usize),
    /// Bring a layout to the stage.
    Layout(usize),
    NextTarget,
    /// A window command on the window being placed.
    Window(windows::WindowOp),
}

/// The picker's state: which windows there are to place, and which is next.
#[derive(Default)]
pub struct Snap {
    pub windows: Vec<windows::Client>,
    pub target: usize,
    /// Where each placed window went: its address to layout and zone.
    pub placed: HashMap<String, (usize, usize)>,
}

impl Snap {
    /// Reads the windows on the workspace the palette covers, the one that had focus first.
    pub fn load(focused: Option<&windows::Client>) -> Self {
        Self { windows: windows::on_workspace(focused), target: 0, placed: HashMap::new() }
    }
    fn current(&self) -> Option<&windows::Client> {
        self.windows.get(self.target)
    }
}

/// The usage id a layout's own command records under.
pub fn command_id(route: &Route) -> Option<&'static str> {
    match route {
        Route::Snap(Some(i)) => LAYOUTS.get(*i).map(|l| l.id),
        _ => None,
    }
}

/// The launcher's rows: the picker with every layout, and one command per layout.
pub fn rows() -> Vec<(Item, String)> {
    let open = |route| Action::new("Open Command", &icons::ARROW_UP_RIGHT, Act::Push(route));
    let mut out = vec![(
        Item::new("cmd:snap", "Snap Layouts", IconSrc::Badge(&icons::GRID, crate::theme::Tint::Indigo)).trailing("Command")
            .subtitle("Pick a zone for each window").action(open(Route::Snap(None))),
        "window snap layout zones tile arrange halves thirds quarters".to_string(),
    )];
    for (i, l) in LAYOUTS.iter().enumerate() {
        out.push((
            Item::new(l.id, format!("Snap Layout: {}", l.name), IconSrc::Badge(l.icon, crate::theme::Tint::Indigo)).trailing("Window")
                .subtitle(l.describes).action(open(Route::Snap(Some(i)))),
            format!("window snap layout zones tile arrange {}", l.zones.iter().map(|z| z.name).collect::<Vec<_>>().join(" ")),
        ));
    }
    out
}

/// The layouts a screen shows: the one its command names, or those the query leaves.
fn shown(only: Option<usize>, query: &str) -> Vec<usize> {
    let q = query.trim();
    (0..LAYOUTS.len())
        .filter(|i| only.is_none_or(|o| o == *i))
        .filter(|i| q.is_empty() || fuzzy::score_item(q, LAYOUTS[*i].name, LAYOUTS[*i].describes).is_some())
        .collect()
}

/// One item per zone, in the order they are drawn: the selection is a zone, and the bottom bar
/// and the action menu are that zone's.
pub fn build(app: &App, only: Option<usize>, query: &str) -> Built {
    let snap = &app.snap;
    let Some(target) = snap.current() else {
        return Built::list(Sections::default(), "Snap Layouts").empty("No Windows on This Workspace", "Open a window, then pick a zone for it.");
    };
    let mut items = Vec::new();
    for li in shown(only, query) {
        let layout = &LAYOUTS[li];
        for (zi, z) in layout.zones.iter().enumerate() {
            let mut item = Item::new(format!("zone:{li}:{zi}"), z.name, IconSrc::None)
                .action(Action::new(format!("Snap to {}", z.name), layout.icon, Act::Snap(SnapOp::Place(li, zi))))
                .action(Action::new(format!("Arrange All as {}", layout.name), &icons::BOLT, Act::Snap(SnapOp::All(li))).keys("ctrl+enter"));
            if snap.windows.len() > 1 {
                item = item.action(Action::new("Place Next Window Instead", &icons::WINDOWS, Act::Snap(SnapOp::NextTarget)).keys("tab"));
            }
            items.push(item.actions([
                Action::new("Tile Window", &icons::GRID, Act::Snap(SnapOp::Window(windows::WindowOp::Tile))).section(""),
                Action::new("Maximize Window", &icons::MAXIMIZE, Act::Snap(SnapOp::Window(windows::WindowOp::Maximize))),
                Action::new("Toggle Fullscreen", &icons::FULLSCREEN, Act::Snap(SnapOp::Window(windows::WindowOp::Fullscreen))),
                Action::new("Close Window", &icons::X, Act::Snap(SnapOp::Window(windows::WindowOp::Close))).destructive(),
            ]));
        }
    }
    let mut sections = Sections::default();
    sections.push("", items);
    Built::list(sections, &format!("Pick a zone for {}", util::truncate(&target.title, 44))).keep()
}

pub fn perform(app: &mut App, op: SnapOp, tx: &mut Tx) {
    let Some(target) = app.snap.current().cloned() else { return app.hide(tx) };
    match op {
        SnapOp::Place(li, zi) => {
            let Some(z) = LAYOUTS.get(li).and_then(|l| l.zones.get(zi)) else { return };
            app.usage.record(LAYOUTS[li].id);
            app.snap.placed.insert(target.address.clone(), (li, zi));
            let next = (0..app.snap.windows.len()).find(|i| !app.snap.placed.contains_key(&app.snap.windows[*i].address));
            match next {
                // Windows are left: this one goes to its zone under the palette, which moves on.
                Some(next) => {
                    windows::snap(&target.address, z.at);
                    windows::raise_palette();
                    app.snap.target = next;
                    app.refresh_keeping();
                    // The next window most likely wants the next zone still empty.
                    let count = LAYOUTS[li].zones.len();
                    let free = (1..count).map(|n| (zi + n) % count).find(|z| !app.snap.placed.values().any(|at| *at == (li, *z)));
                    if let Some(at) = free.and_then(|z| app.top().items.iter().position(|i| i.id == format!("zone:{li}:{z}"))) {
                        app.top_mut().sel = at;
                    }
                }
                None => {
                    app.hide(tx);
                    windows::snap(&target.address, z.at);
                    windows::focus(&target.address);
                }
            }
        }
        SnapOp::All(li) => {
            let Some(layout) = LAYOUTS.get(li) else { return };
            app.usage.record(layout.id);
            let addresses: Vec<String> = app.snap.windows.iter().map(|w| w.address.clone()).collect();
            let zones: Vec<[f32; 4]> = layout.zones.iter().map(|z| z.at).collect();
            app.hide(tx);
            windows::arrange(&addresses, &zones);
        }
        SnapOp::Target(i) => {
            app.snap.target = i.min(app.snap.windows.len().saturating_sub(1));
            app.refresh_keeping();
        }
        SnapOp::Layout(li) => {
            if let Some(at) = app.top().items.iter().position(|i| i.id == format!("zone:{li}:0")) {
                app.top_mut().sel = at;
            }
        }
        SnapOp::NextTarget => {
            app.snap.target = (app.snap.target + 1) % app.snap.windows.len().max(1);
            app.refresh_keeping();
        }
        SnapOp::Window(op) => {
            app.hide(tx);
            windows::run(op, &target.address);
        }
    }
}

/// The layout and zone a selection is: its place among the shown layouts, the layout, the zone.
fn locate(layouts: &[usize], sel: usize) -> Option<(usize, usize, usize)> {
    let mut first = 0;
    for (n, li) in layouts.iter().enumerate() {
        let count = LAYOUTS[*li].zones.len();
        if sel < first + count {
            return Some((n, *li, sel - first));
        }
        first += count;
    }
    None
}

/// The selection for a zone of the `n`th shown layout.
fn index_of(layouts: &[usize], n: usize, zone: usize) -> usize {
    layouts[..n].iter().map(|li| LAYOUTS[*li].zones.len()).sum::<usize>() + zone
}

/// The zone an arrow key reaches inside a layout: the nearest one that way, if there is one.
fn toward(zones: &[Zone], from: usize, (dx, dy): (f32, f32)) -> Option<usize> {
    let centre = |z: &Zone| (z.at[0] + z.at[2] / 2.0, z.at[1] + z.at[3] / 2.0);
    let (x, y) = centre(zones.get(from)?);
    let mut best = (None, f32::INFINITY);
    for (i, z) in zones.iter().enumerate() {
        let (cx, cy) = centre(z);
        let (along, across) = ((cx - x) * dx + (cy - y) * dy, ((cx - x) * dy).abs() + ((cy - y) * dx).abs());
        let cost = along + across * 3.0;
        if along > 0.01 && cost < best.1 {
            best = (Some(i), cost);
        }
    }
    best.0
}

/// Where an arrow key takes the selection: across the staged layout's zones, and off its left
/// or right edge into the layout beside it.
fn step(layouts: &[usize], sel: usize, dir: (f32, f32)) -> usize {
    let Some((n, li, zi)) = locate(layouts, sel) else { return 0 };
    if let Some(zone) = toward(LAYOUTS[li].zones, zi, dir) {
        return index_of(layouts, n, zone);
    }
    if dir.0 > 0.0 && n + 1 < layouts.len() {
        index_of(layouts, n + 1, 0)
    } else if dir.0 < 0.0 && n > 0 {
        index_of(layouts, n - 1, LAYOUTS[layouts[n - 1]].zones.len() - 1)
    } else {
        sel
    }
}

/// The picker's own keys: the arrows move between zones the way they are drawn, and a digit
/// places the window in the zone that carries it.
pub fn key(app: &mut App, only: Option<usize>, ev: &KeyEvent, tx: &mut Tx) -> bool {
    let m = ev.mods;
    if m.cmd || m.alt || m.shift {
        return false;
    }
    let query = app.top().query.text.clone();
    let layouts = shown(only, &query);
    let dir = match &ev.key {
        Key::Left if query.is_empty() => (-1.0, 0.0),
        Key::Right if query.is_empty() => (1.0, 0.0),
        Key::Up => (0.0, -1.0),
        Key::Down => (0.0, 1.0),
        Key::Text(t) if query.is_empty() => {
            let (Some(n), Some((_, li, _))) = (t.parse::<usize>().ok(), locate(&layouts, app.top().sel)) else { return false };
            if n == 0 || n > LAYOUTS[li].zones.len() {
                return false;
            }
            app.perform(Act::Snap(SnapOp::Place(li, n - 1)), tx);
            return true;
        }
        _ => return false,
    };
    let next = step(&layouts, app.top().sel, dir);
    if next < app.top().items.len() {
        app.top_mut().sel = next;
    }
    tx.instant();
    true
}

const FULL: Option<f32> = Some(0.0);
/// The stage: the screen in small, 16 to 9.
const STAGE: (f32, f32) = (464.0, 261.0);

/// A window's own icon, as the launcher shows its application.
fn icon_of(app: &App, w: &windows::Client) -> IconSrc {
    app.apps.iter()
        .find(|a| a.wm_class.eq_ignore_ascii_case(&w.class) || a.id.trim_end_matches(".desktop").eq_ignore_ascii_case(&w.class))
        .map(|a| a.icon_src()).unwrap_or(IconSrc::Glyph(&icons::WINDOW))
}

/// The staged layout as the screen it will become: each zone a tile, the selected one holding
/// the window about to land there, the filled ones the windows already placed.
fn stage(app: &App, p: &Palette, scale: f32, li: usize, selected: usize) -> El {
    let (w, h) = STAGE;
    let (pad, gap) = (6.0, 3.0);
    let (iw, ih) = (w - pad * 2.0, h - pad * 2.0);
    let (accent_top, accent) = Tint::Blue.stops();
    let target = app.snap.windows.get(app.snap.target);
    let mut frame = el().size(w, h).radius(18.0)
        .fill(if p.dark { Color::BLACK.alpha(0.26) } else { Color::BLACK.alpha(0.05) })
        .border_gradient(1.0, p.glass_rim_bottom, p.glass_rim_top.alpha(0.16));
    for (zi, z) in LAYOUTS[li].zones.iter().enumerate() {
        let [zx, zy, zw, zh] = z.at;
        let (tw, th) = (zw * iw - gap * 2.0, zh * ih - gap * 2.0);
        let lit = zi == selected;
        let holds = app.snap.windows.iter().find(|c| app.snap.placed.get(&c.address) == Some(&(li, zi)));
        let label = |s: &str, ink: Color, style| text(s.to_string()).font(style).color(ink).truncate().max_w(tw - space::XXL);
        let mut face = col().center().gap(space::XS);
        let mut tile = el().key(format!("zone-{li}-{zi}")).absolute(pad + zx * iw + gap, pad + zy * ih + gap).size(tw, th).radius(12.0).center()
            .on_press_tx(move |s: &mut App, tx| s.perform(Act::Snap(SnapOp::Place(li, zi)), tx));
        if lit {
            // The window about to land here, on the accent.
            if let Some(win) = target {
                face = face.child(ui::icon_slot(&app.icons, p, scale, &icon_of(app, win), 38.0))
                    .child(el().h(space::XXS))
                    .child(label(&win.title, Color::WHITE, type_::PANEL_TITLE));
            }
            face = face.child(label(z.name, Color::WHITE.alpha(0.72), type_::SECTION));
            tile = tile.fill(accent).gradient(accent_top, accent)
                .border_gradient(1.0, Color::WHITE.alpha(0.55), Color::WHITE.alpha(0.08))
                .shadow(6.0, 22.0, accent.alpha(0.45));
        } else if let Some(win) = holds {
            face = face.child(ui::icon_slot(&app.icons, p, scale, &icon_of(app, win), 30.0))
                .child(label(&win.title, p.text.alpha(0.86), type_::ROW_TRAILING));
            tile = tile.fill(accent.alpha(0.16)).border(1.0, accent.alpha(0.50)).hover(accent.alpha(0.28));
        } else {
            face = face.child(label(z.name, p.text_tertiary, type_::ROW_TRAILING));
            tile = tile.fill(p.card_fill).border_gradient(1.0, p.glass_rim_top.alpha(0.20), p.glass_rim_bottom)
                .hover(p.control_hover).press(p.control_pressed);
        }
        // The digit that places a window here.
        let cap = row().absolute(space::MD, space::MD).min_w(size::KEY_CAP).h(size::KEY_CAP).px(space::XS).center().radius(radius::KEY_CAP)
            .fill(if lit { Color::WHITE.alpha(0.20) } else { p.control_surface })
            .child(text((zi + 1).to_string()).font(type_::KEY_CAP).color(if lit { Color::WHITE } else { p.text_secondary })).inert();
        frame = frame.child(tile.child(face).child(cap));
    }
    frame
}

/// One layout in the rail under the stage: its zones in miniature over its name.
fn thumb(app: &App, p: &Palette, li: usize, staged: bool) -> El {
    let (w, h) = (70.0, 40.0);
    let accent = Tint::Blue.stops().1;
    let mut mini = el().size(w, h).radius(9.0).fill(if staged { accent.alpha(0.14) } else { p.card_fill })
        .border(1.0, if staged { accent.alpha(0.85) } else { p.card_stroke });
    for (zi, z) in LAYOUTS[li].zones.iter().enumerate() {
        let [zx, zy, zw, zh] = z.at;
        let (iw, ih) = (w - 6.0, h - 6.0);
        let filled = app.snap.placed.values().any(|at| *at == (li, zi));
        mini = mini.child(el().absolute(3.0 + zx * iw + 1.5, 3.0 + zy * ih + 1.5).size(zw * iw - 3.0, zh * ih - 3.0).radius(4.5)
            .fill(if filled { accent } else if staged { accent.alpha(0.45) } else { p.control_hover }).inert());
    }
    col().key(format!("layout-{li}")).w(w).gap(space::XS + 1.0).items_center().radius(9.0)
        .on_press_tx(move |s: &mut App, tx| s.perform(Act::Snap(SnapOp::Layout(li)), tx))
        .child(if staged { mini } else { mini.hover(p.control_surface) })
        .child(text(LAYOUTS[li].name).font(type_::SECTION).color(if staged { p.text } else { p.text_secondary }).truncate().max_w(w))
}

/// The windows there are to place, as a queue: the lit one is next, a tick marks those done.
fn queue(app: &App, p: &Palette, scale: f32, li: usize) -> El {
    const MOST: usize = 7;
    let snap = &app.snap;
    let accent = Tint::Blue.stops().0;
    let done = snap.windows.iter().filter(|w| snap.placed.contains_key(&w.address)).count();
    let mut list = col().w_full().gap(space::XXS)
        .child(row().w_full().px(space::MD).pb(space::SM).center_y()
            .child(text("Windows").font(type_::SECTION).color(p.text_secondary))
            .child(spacer())
            .child(text(format!("{done} of {} placed", snap.windows.len())).font(type_::SECTION).color(p.text_tertiary)));
    let start = snap.target.saturating_sub(MOST - 1).min(snap.windows.len().saturating_sub(MOST));
    for (i, w) in snap.windows.iter().enumerate().skip(start).take(MOST) {
        let lit = i == snap.target;
        let at = snap.placed.get(&w.address).and_then(|(l, z)| LAYOUTS.get(*l)?.zones.get(*z));
        let line = row().key(format!("window-{}", w.address)).w_full().h(32.0).px(space::MD).gap(space::MD).center_y().radius(radius::ROW);
        let line = if lit { line.fill(p.selection) } else { line.hover(p.row_hover) };
        let mut line = line.on_press_tx(move |s: &mut App, tx| s.perform(Act::Snap(SnapOp::Target(i)), tx))
            .child(ui::icon_slot(&app.icons, p, scale, &icon_of(app, w), 20.0))
            .child(text(w.title.clone()).font(type_::ROW_TITLE).color(if lit { p.text } else { p.text.alpha(0.78) }).truncate().shrink().grow(1.0));
        if lit {
            line = line.child(text("Placing").font(type_::SECTION).color(accent));
        } else if at.is_some() {
            line = line.child(icon(&icons::CHECK, 13.0).color(p.success));
        }
        list = list.child(line);
    }
    let hidden = snap.windows.len().saturating_sub(MOST);
    if hidden > 0 {
        list = list.child(text(format!("and {hidden} more")).font(type_::SECTION).color(p.text_tertiary).ml(space::MD).mt(space::XS));
    }
    let mut side = col().grow(1.0).h_full().min_w(0.0).child(list).child(spacer());
    if snap.windows.len() > 1 {
        side = side.child(row().key("arrange-all").w_full().h(34.0).px(space::LG).gap(space::MD).center_y().radius(radius::FORM_FIELD)
            .fill(p.card_fill).border_gradient(1.0, p.glass_rim_top.alpha(0.22), p.glass_rim_bottom)
            .hover(p.control_hover).press(p.control_pressed)
            .on_press_tx(move |s: &mut App, tx| s.perform(Act::Snap(SnapOp::All(li)), tx))
            .child(icon(&icons::BOLT, 13.0).color(accent))
            .child(text("Arrange All").font(type_::BAR).color(p.text))
            .child(spacer())
            .child(ui::keycaps_row(p, "ctrl+enter", false)));
    }
    side
}

pub fn body(app: &App, p: &Palette, scale: f32, only: Option<usize>) -> El {
    let screen = app.top();
    let layouts = shown(only, &screen.query.text);
    let (_, li, zi) = locate(&layouts, screen.sel).unwrap_or((0, layouts[0], 0));
    let mut left = col().w(STAGE.0).gap(space::XL).child(stage(app, p, scale, li, zi));
    left = if layouts.len() > 1 {
        left.child(row().w_full().gap(space::MD + 0.8).children(layouts.iter().map(|l| thumb(app, p, *l, *l == li))))
    } else {
        // A layout's own command has no rail: say what the layout is instead.
        left.child(row().w_full().px(space::XS).gap(space::MD).center_y()
            .child(text(LAYOUTS[li].name).font(type_::PANEL_TITLE).color(p.text))
            .child(text(LAYOUTS[li].describes).font(type_::ROW_TRAILING).color(p.text_secondary).truncate().shrink()))
    };
    row().pin(FULL, FULL, FULL, FULL).pt(size::HEADER_BAND + space::XS).pb(size::BOTTOM_BAR_H + space::MD).px(space::XXL).gap(space::XXL)
        .child(left)
        .child(queue(app, p, scale, li))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zones_stay_on_screen() {
        for l in LAYOUTS {
            for z in l.zones {
                let [x, y, w, h] = z.at;
                assert!(x >= 0.0 && y >= 0.0 && x + w <= 1.001 && y + h <= 1.001, "{} {}", l.name, z.name);
            }
        }
    }

    #[test]
    fn arrows_follow_the_drawing() {
        let all: Vec<usize> = (0..LAYOUTS.len()).collect();
        // Halves: 0, 1. Two Thirds: 2, 3. Thirds: 4..7. Quarters: 7..11. Side Stack: 11..14.
        assert_eq!(step(&all, 0, (1.0, 0.0)), 1);
        // Off a layout's edge is the layout beside it.
        assert_eq!(step(&all, 1, (1.0, 0.0)), 2);
        assert_eq!(step(&all, 2, (-1.0, 0.0)), 1);
        assert_eq!(step(&all, 0, (-1.0, 0.0)), 0);
        assert_eq!(step(&all, 0, (0.0, 1.0)), 0);
        assert_eq!(step(&all, 7, (0.0, 1.0)), 9);
        assert_eq!(step(&all, 9, (0.0, -1.0)), 7);
        assert_eq!(step(&all, 11, (1.0, 0.0)), 12);
        assert_eq!(step(&all, 12, (0.0, 1.0)), 13);
        assert_eq!(locate(&all, 13), Some((4, 4, 2)));
    }

    #[test]
    fn a_layout_command_shows_only_its_layout() {
        assert_eq!(shown(Some(3), ""), vec![3]);
        assert_eq!(shown(None, "quart"), vec![3]);
        assert_eq!(shown(None, "").len(), LAYOUTS.len());
    }
}
