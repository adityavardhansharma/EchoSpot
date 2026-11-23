//! Blocks to pixels. A pack says what (a List, a Stat, an intent); everything about how is
//! here: spacing, type, colour, states, motion. Nothing a block carries can name a number the
//! design system doesn't own, so whatever an agent returns looks as if it shipped with EchoSpot.
//!
//! The same drawing serves the palette, a board's tiles, an overlay and a window, whatever state
//! type each one's view has: a press reaches the app through `host::dispatch`.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;

use rune::{Color, El, LoopSpec, ScrollSpec, Shape, SurfaceFit, TextEdit, TextStyle, Transition, canvas, col, el, icon, row, spacer, text, text_input};
use serde_json::Value;

use crate::icons;
use crate::model::PackOp;
use crate::store::IconStore;
use crate::theme::{Palette, size, space};

use super::blocks::{self, items, kind};
use super::host;
use super::looks::{self, IntentStyle, Look};
const TILE: f32 = super::surface::TILE as f32;
const TILE_GAP: f32 = super::surface::TILE_GAP as f32;
use super::types::{self, Type};

const FULL: Option<f32> = Some(0.0);
/// The tag an effect slot carries for the compositor; what to draw there rides in its data.
pub const FX_MARK: u32 = 0xEC50;
/// The smallest thing a person is asked to press.
const TARGET: f32 = 28.0;

/// Where a window keeps what has been typed into a pack's `ui.input`s, by key, so a field keeps
/// its text while the surface is drawn again. The palette and the slots outside it each have one.
pub trait PackInputs {
    fn pack_input(&mut self, key: &str, initial: &str) -> &mut TextEdit;
}

pub struct Ctx<'a> {
    pub p: Palette,
    pub look: &'static Look,
    pub scale: f32,
    pub icons: Option<&'a IconStore>,
    pub pack: &'a str,
    pub target: &'a str,
    pub dir: PathBuf,
    /// The room across, in points.
    pub width: f32,
    /// Effect layers this surface may still start.
    fx: Cell<usize>,
    keys: Cell<u32>,
    /// What has been typed into this window's inputs, to draw them with.
    pub inputs: Option<&'a HashMap<String, TextEdit>>,
}

impl<'a> Ctx<'a> {
    pub fn new(p: Palette, look: &'static Look, scale: f32, icons: Option<&'a IconStore>, pack: &'a str, target: &'a str, dir: PathBuf, width: f32) -> Self {
        Self { p: look.palette(p), look, scale, icons, pack, target, dir, width, fx: Cell::new(look.fx_budget), keys: Cell::new(0), inputs: None }
    }
    pub fn with_inputs(mut self, inputs: &'a HashMap<String, TextEdit>) -> Self {
        self.inputs = Some(inputs);
        self
    }
    fn key(&self, what: &str) -> String {
        self.keys.set(self.keys.get() + 1);
        format!("pack-{what}-{}", self.keys.get())
    }
    fn narrow(&self, width: f32) -> Ctx<'a> {
        Ctx { p: self.p, look: self.look, scale: self.scale, icons: self.icons, pack: self.pack, target: self.target, dir: self.dir.clone(), width, fx: Cell::new(self.fx.get()), keys: Cell::new(self.keys.get() + 1000), inputs: self.inputs }
    }
}

fn role(name: &str) -> TextStyle {
    looks::type_role(name).unwrap_or_else(|| looks::type_role("body").expect("body is a role"))
}

fn token(b: &Value, key: &str, of: fn(&str) -> Option<f32>, fallback: f32) -> f32 {
    match b.get(key) {
        Some(Value::String(name)) => of(name).unwrap_or(fallback),
        // A raw number still draws; the checker is what says it should be a token.
        Some(Value::Number(n)) => n.as_f64().unwrap_or(fallback as f64) as f32,
        _ => fallback,
    }
}

fn colour(cx: &Ctx, b: &Value, key: &str, fallback: Color) -> Color {
    b.get(key).and_then(Value::as_str).and_then(|name| looks::colour(&cx.p, name).or_else(|| rune::color::hex(name))).unwrap_or(fallback)
}

fn shown(v: &Value, ty: Option<&Type>) -> String {
    if v.is_null() { String::new() } else { blocks::show(v, ty) }
}

fn type_of(b: &Value) -> Option<Type> {
    b.get("type").and_then(|t| Type::from_json(t).ok()).filter(|t| !t.kind.is_empty() && t.kind != "any")
}

fn intent(cx: &Ctx, b: &Value) -> IntentStyle {
    cx.look.intent(blocks::text(b, "intent"))
}

fn effect_id(name: &str) -> f32 {
    super::fx::EFFECTS.iter().position(|(n, _)| *n == name).map_or(0.0, |i| (i + 1) as f32)
}

/// A running effect over an element, when the surface's budget has room: the compositor paints
/// it at this slot. Past the budget it holds still, as one faint wash.
fn fx_layer(cx: &Ctx, effect: &str, preset: &str, tint: Option<Color>, radius: f32) -> Option<El> {
    if effect.is_empty() {
        return None;
    }
    let wash = tint.unwrap_or(cx.p.primary_action);
    if cx.fx.get() == 0 || !cx.look.effects {
        return Some(el().pin(FULL, FULL, FULL, FULL).radius(radius).fill(wash.alpha(0.07)).inert());
    }
    cx.fx.set(cx.fx.get() - 1);
    let preset_id = super::fx::EFFECTS.iter().find(|(n, _)| *n == effect).and_then(|(_, presets)| presets.iter().position(|p| *p == preset)).unwrap_or(0) as f32;
    Some(el().pin(FULL, FULL, FULL, FULL).radius(radius).inert()
        .child(el().pin(FULL, FULL, FULL, FULL).radius(radius).fill(wash.alpha(0.06)).looping(LoopSpec::opacity(1.8, 0.0, 0.35, 1.0)).inert())
        .child(rune::effect(FX_MARK).effect_data([effect_id(effect), preset_id, radius, 0.0]).effect_bleed(6.0).pin(FULL, FULL, FULL, FULL).inert()))
}

/// What an intent adds to the element that carries it: an edge in its colour, and its effect.
fn with_intent(cx: &Ctx, e: El, style: IntentStyle, radius: f32) -> El {
    let Some(tint) = style.tint else { return if style.quiet { e.opacity(0.62) } else { e } };
    let accent = tint.stops().0;
    let mut e = e;
    if let Some(layer) = fx_layer(cx, style.effect, "default", Some(accent), radius) {
        e = e.child(layer);
    }
    let edge = el().absolute(0.0, 0.0).w(3.0).h_full().radius(1.5).fill(accent).inert();
    e.child(if style.pulse { edge.looping(LoopSpec::opacity(1.4, 0.0, 0.35, 1.0)) } else { edge })
}

/// A key that names the same row from one draw to the next, so a row that moves is seen to
/// move, and one that comes or goes enters or leaves (§13.5). Falls back to the draw's count.
fn row_key(cx: &Ctx, what: &str, r: &Value) -> String {
    match r.get("id").and_then(|v| v.as_str().map(String::from).or_else(|| v.as_i64().map(|n| n.to_string()))) {
        Some(id) => format!("pack-{what}-{}-{id}", cx.target),
        None => cx.key(what),
    }
}

/// How a row, a cell or a value moves under the surface's Look: it slides a little as it comes,
/// fades as it goes, and travels on the Look's curve when its place changes.
fn moving(cx: &Ctx, e: El) -> El {
    e.motion(cx.look.motion).transition(Transition::offset(0.0, 6.0).combined(Transition::opacity()))
}

fn press<S: PackInputs + 'static>(e: El, op: PackOp) -> El {
    e.on_press(move |_: &mut S| host::dispatch(op.clone()))
}

/// What pressing a row does: the pack's own first action, or the first thing its value offers.
fn row_op(cx: &Ctx, r: &Value) -> Option<PackOp> {
    let value = r.get("value").cloned().unwrap_or(Value::Null);
    if let Some(first) = items(r, "actions").first() {
        if let Some(handler) = first.get("run").and_then(|h| h.get("_handler")).or_else(|| first.get("_handler")).and_then(Value::as_u64) {
            return Some(PackOp::Handler { pack: cx.pack.into(), target: cx.target.into(), handler, payload: value });
        }
        if let Some(address) = first.get("call").and_then(Value::as_str) {
            return Some(PackOp::Invoke { address: address.into(), args: first.get("args").cloned().unwrap_or(Value::Null) });
        }
    }
    let link = value.as_object().and_then(|m| ["url", "path", "file"].iter().find_map(|k| m.get(*k))).or(Some(&value)).filter(|v| v.is_string())?;
    let ty = types::infer(link);
    matches!(ty.kind.as_str(), "url" | "file" | "folder" | "image").then(|| PackOp::Verb { id: "open".into(), value: link.clone() })
}

fn picture(cx: &Ctx, source: &str, w: f32, h: f32, cover: bool) -> El {
    let frame = el().size(w, h).radius(looks::radius("thumb").unwrap_or(6.0)).fill(cx.p.icon_placeholder);
    let pixels = cx.icons.and_then(|store| {
        if source.starts_with("http://") || source.starts_with("https://") { store.url(source, w.max(h), cx.scale) } else { store.file(&host::resolve_path(&cx.dir, source), w.max(h), cx.scale) }
    });
    match pixels {
        Some(px) => frame.pixels_fit(&px, if cover { SurfaceFit::Fill } else { SurfaceFit::Fit }),
        None => frame.center().child(icon(&icons::IMAGE, (h * 0.3).clamp(12.0, 28.0)).color(cx.p.text_tertiary)),
    }
}

fn glyph(cx: &Ctx, name: &str, px: f32, color: Color) -> El {
    // An emoji is an icon too.
    if !name.is_ascii() && name.chars().count() <= 2 {
        return text(name.to_string()).font(TextStyle::new(px, 400.0).lh(px * 1.2));
    }
    if name.ends_with(".svg") || name.ends_with(".png") {
        return picture(cx, name, px, px, false);
    }
    icon(icons::by_name(&name.replace('-', "_")).unwrap_or(&icons::CIRCLE), px).color(color)
}

fn hairline(cx: &Ctx) -> El {
    el().w_full().h(size::HAIRLINE).fill(cx.p.separator)
}

/// A line or bars for a series: numbers, or `{ t, v }` points.
fn plot(cx: &Ctx, series: &Value, bars: bool, w: f32, h: f32, color: Color) -> El {
    let points: Vec<f32> = series.as_array().into_iter().flatten().filter_map(|p| p.as_f64().or_else(|| p.get("v").and_then(Value::as_f64))).map(|v| v as f32).collect();
    if points.len() < 2 && !bars || points.is_empty() {
        return el().size(w, h).center().child(text("Not enough points yet").font(role("caption")).color(cx.p.text_tertiary));
    }
    let (lo, hi) = points.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
    let lo = if bars { lo.min(0.0) } else { lo };
    let span = (hi - lo).max(f32::EPSILON);
    let y = |v: f32| h - 2.0 - (v - lo) / span * (h - 4.0);
    let mut shape = Shape::new();
    if bars {
        let slot = w / points.len() as f32;
        let bar = (slot * 0.68).max(1.0);
        for (i, v) in points.iter().enumerate() {
            let x = i as f32 * slot + (slot - bar) / 2.0;
            shape = shape.fill(vec![[x, y(*v)], [x + bar, y(*v)], [x + bar, h], [x, h]], color.alpha(0.85));
        }
    } else {
        let step = w / (points.len() - 1) as f32;
        let line: Vec<[f32; 2]> = points.iter().enumerate().map(|(i, v)| [i as f32 * step, y(*v)]).collect();
        let mut area = line.clone();
        area.push([w, h]);
        area.push([0.0, h]);
        shape = shape.fill(area, color.alpha(0.10)).stroke(line, 1.8, color);
    }
    canvas(shape.view_box(w, h)).size(w, h)
}

fn skeleton(cx: &Ctx, rows: usize, compact: bool) -> El {
    let bar = |w: f32, h: f32| el().w(w).h(h).radius(h / 2.0).fill(cx.p.control_surface);
    let widths = [0.46, 0.62, 0.38, 0.54, 0.42, 0.58];
    let mut c = col().w_full().gap(if compact { space::SM } else { space::LG }).looping(LoopSpec::opacity(1.4, 0.0, 0.45, 1.0));
    for i in 0..rows {
        let w = (cx.width * widths[i % widths.len()]).min(cx.width - 60.0).max(40.0);
        let mut line = row().w_full().gap(space::LG).center_y();
        if !compact {
            line = line.child(el().size(size::RESULT_ROW_ICON - 3.0, size::RESULT_ROW_ICON - 3.0).radius(7.0).fill(cx.p.control_surface));
        }
        c = c.child(line.child(bar(w, if compact { 8.0 } else { 10.0 })));
    }
    c
}

fn empty_state(cx: &Ctx, title: &str, hint: &str, glyph_name: &str, error: bool) -> El {
    let mut c = col().w_full().py(space::XXL).center().gap(space::XS);
    if !glyph_name.is_empty() || error {
        c = c.child(el().pb(space::SM).child(glyph(cx, if error { "x" } else { glyph_name }, 22.0, if error { cx.p.destructive } else { cx.p.text_tertiary })));
    }
    c = c.child(text(if title.is_empty() { "Nothing Here".to_string() } else { title.to_string() }).font(role("headline")).color(cx.p.text_secondary).align(rune::Align::Center).wrap());
    if !hint.is_empty() {
        c = c.child(text(hint.to_string()).font(role("caption")).color(cx.p.text_tertiary).align(rune::Align::Center).wrap().max_w((cx.width - space::XXL * 2.0).max(80.0)));
    }
    c
}

/// A component's own loading and error states: `None` when it has its data.
fn settled<S: PackInputs + 'static>(cx: &Ctx, b: &Value, rows: usize, compact: bool) -> Option<El> {
    match blocks::text(b, "state") {
        "pending" => Some(skeleton(cx, rows, compact)),
        "error" => {
            let (title, hint) = blocks::title_hint(b.get("error"));
            let message = if title.is_empty() { blocks::text(b, "error").to_string() } else if hint.is_empty() { title } else { format!("{title} — {hint}") };
            let retry = press::<S>(row().key(cx.key("retry")).h(TARGET).px(space::XL).center().radius(TARGET / 2.0).fill(cx.p.control_surface).hover(cx.p.control_hover), PackOp::Reload)
                .child(text("Try Again").font(role("caption")).color(cx.p.text));
            Some(empty_state(cx, "Couldn't Load", &message, "", true).child(el().pt(space::MD).child(retry)))
        }
        _ => None,
    }
}

fn list_row<S: PackInputs + 'static>(cx: &Ctx, r: &Value, compact: bool, ty: Option<&Type>) -> El {
    let style = cx.look.intent(blocks::text(r, "intent"));
    let ink = if style.quiet { cx.p.text_secondary } else { cx.p.text };
    let radius = looks::radius("row").unwrap_or(10.0);
    let mut line = row().w_full().min_h(if compact { 24.0 } else { TARGET + 4.0 }).px(space::MD).py(if compact { space::XXS } else { space::SM }).gap(space::LG).center_y().radius(radius);
    let slot = if compact { 16.0 } else { 22.0 };
    if let Some(image) = r.get("image").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        line = line.child(picture(cx, image, slot, slot, true));
    } else if let Some(name) = r.get("icon").and_then(Value::as_str).filter(|s| !s.is_empty()) {
        line = line.child(glyph(cx, name, slot - 4.0, style.tint.map_or(cx.p.text_secondary, |t| t.stops().0)));
    }
    line = line.child(text(shown(r.get("title").unwrap_or(&Value::Null), None)).font(role("body")).color(ink).truncate());
    let subtitle = r.get("subtitle").map(|s| shown(s, None)).unwrap_or_default();
    if !subtitle.is_empty() && !compact {
        line = line.child(text(subtitle).font(role("caption")).color(cx.p.text_secondary).truncate().shrink());
    }
    line = line.child(spacer());
    let trailing = r.get("accessory").or(r.get("badge")).or(r.get("time")).map(|a| match a {
        Value::Object(m) => shown(m.get("text").unwrap_or(&Value::Null), None),
        other => shown(other, None),
    }).unwrap_or_default();
    if !trailing.is_empty() {
        line = line.child(text(trailing).font(role("caption")).color(style.tint.map_or(cx.p.text_secondary, |t| t.stops().0)));
    }
    let _ = ty;
    let line = moving(cx, with_intent(cx, line, style, radius).key(row_key(cx, "row", r)));
    match row_op(cx, r) {
        Some(op) => press::<S>(line.hover(cx.p.row_hover), op),
        None => line,
    }
}

fn list<S: PackInputs + 'static>(cx: &Ctx, b: &Value) -> El {
    let compact = blocks::text(b, "density") == "compact";
    let rows = items(b, "rows");
    let mut c = col().w_full().gap(if compact { 0.0 } else { space::XXS });
    let heading = blocks::text(b, "heading");
    if !heading.is_empty() {
        c = c.child(text(heading.to_string()).font(role("caption")).color(cx.p.text_secondary).truncate().px(space::MD).pb(space::XS));
    }
    if let Some(state) = settled::<S>(cx, b, if compact { 3 } else { 5 }, compact) {
        return c.child(el().w_full().px(space::MD).child(state));
    }
    if rows.is_empty() {
        let (title, hint) = blocks::title_hint(b.get("empty"));
        return c.child(empty_state(cx, if title.is_empty() { "No Results" } else { &title }, &hint, "", false));
    }
    let ty = type_of(b).and_then(|t| t.of.map(|of| *of));
    for r in rows {
        c = c.child(list_row::<S>(cx, r, compact, ty.as_ref()));
    }
    if let Some(more) = b.get("more").and_then(Value::as_u64).filter(|n| *n > 0) {
        c = c.child(text(format!("and {more} more")).font(role("caption")).color(cx.p.text_tertiary).px(space::MD).pt(space::XS));
    }
    c
}

fn grid<S: PackInputs + 'static>(cx: &Ctx, b: &Value, gallery: bool) -> El {
    if let Some(state) = settled::<S>(cx, b, 4, false) {
        return state;
    }
    let rows = items(b, "rows");
    if rows.is_empty() {
        let (title, hint) = blocks::title_hint(b.get("empty"));
        return empty_state(cx, if title.is_empty() { "No Results" } else { &title }, &hint, "", false);
    }
    let columns = if gallery { if cx.width < 360.0 { 1 } else { 3 } } else { b.get("columns").and_then(Value::as_u64).unwrap_or(5).clamp(1, 8) as usize };
    let gap = space::MD;
    let cell = ((cx.width - gap * (columns as f32 - 1.0)) / columns as f32).floor().max(40.0);
    let mut c = col().w_full().gap(gap);
    for chunk in rows.chunks(columns) {
        let mut line = row().w_full().gap(gap);
        for r in chunk {
            let title = shown(r.get("title").unwrap_or(&Value::Null), None);
            let face = match r.get("image").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                Some(image) => picture(cx, image, cell, if gallery { cell * 0.66 } else { cell - 26.0 }, true),
                None => el().size(cell, cell - 26.0).center().radius(looks::radius("card").unwrap_or(10.0)).fill(cx.p.card_fill)
                    .child(glyph(cx, r.get("icon").and_then(Value::as_str).unwrap_or("circle"), (cell * 0.3).min(28.0), cx.p.text_secondary)),
            };
            let tile = col().w(cell).gap(space::XS).radius(looks::radius("card").unwrap_or(10.0)).child(face)
                .child(text(title).font(role("caption")).color(cx.p.text_secondary).truncate().max_w(cell));
            let tile = moving(cx, tile.key(row_key(cx, "cell", r)));
            line = line.child(match row_op(cx, r) {
                Some(op) => press::<S>(tile.hover(cx.p.row_hover), op),
                None => tile,
            });
        }
        c = c.child(line);
    }
    c
}

fn timeline<S: PackInputs + 'static>(cx: &Ctx, b: &Value) -> El {
    if let Some(state) = settled::<S>(cx, b, 4, false) {
        return state;
    }
    let rows = items(b, "rows");
    if rows.is_empty() {
        let (title, hint) = blocks::title_hint(b.get("empty"));
        return empty_state(cx, if title.is_empty() { "Nothing Yet" } else { &title }, &hint, "", false);
    }
    let mut c = col().w_full();
    for (i, r) in rows.iter().enumerate() {
        let style = cx.look.intent(blocks::text(r, "intent"));
        let dot = style.tint.map_or(cx.p.text_tertiary, |t| t.stops().0);
        let rail = col().w(12.0).items_center()
            .child(el().w(size::HAIRLINE).h(space::MD).fill(if i == 0 { Color::TRANSPARENT } else { cx.p.separator }))
            .child(el().size(7.0, 7.0).radius(3.5).fill(dot))
            .child(el().w(size::HAIRLINE).grow(1.0).fill(if i + 1 == rows.len() { Color::TRANSPARENT } else { cx.p.separator }));
        let when = r.get("time").map(|t| match t.as_f64() {
            Some(secs) => crate::util::ago(crate::util::now_secs().saturating_sub(secs as u64)),
            None => shown(t, None),
        }).unwrap_or_default();
        let mut body = col().grow(1.0).py(space::SM).gap(space::XXS)
            .child(text(shown(r.get("title").unwrap_or(&Value::Null), None)).font(role("body")).color(if style.quiet { cx.p.text_secondary } else { cx.p.text }).truncate());
        let subtitle = r.get("subtitle").map(|s| shown(s, None)).unwrap_or_default();
        if !subtitle.is_empty() {
            body = body.child(text(subtitle).font(role("caption")).color(cx.p.text_secondary).truncate());
        }
        let line = row().w_full().gap(space::LG).px(space::MD).radius(looks::radius("row").unwrap_or(10.0)).child(rail).child(body)
            .child(el().py(space::SM).child(text(when).font(role("caption")).color(cx.p.text_tertiary)));
        let line = moving(cx, line.key(row_key(cx, "event", r)));
        c = c.child(match row_op(cx, r) {
            Some(op) => press::<S>(line.hover(cx.p.row_hover), op),
            None => line,
        });
    }
    c
}

fn key_values(cx: &Ctx, list: &[Value], heading: &str) -> El {
    let mut c = col().w_full();
    if !heading.is_empty() {
        c = c.child(text(heading.to_string()).font(role("caption")).color(cx.p.text_secondary).pb(space::XS));
    }
    for (i, item) in list.iter().enumerate() {
        let (label, value) = match item {
            Value::Array(pair) => (shown(pair.first().unwrap_or(&Value::Null), None), shown(pair.get(1).unwrap_or(&Value::Null), None)),
            other => (shown(other.get("label").unwrap_or(&Value::Null), None), shown(other.get("value").unwrap_or(&Value::Null), None)),
        };
        if i > 0 {
            c = c.child(hairline(cx));
        }
        c = c.child(row().w_full().py(space::MD).gap(space::XL).center_y()
            .child(text(label).font(role("caption")).color(cx.p.text_secondary))
            .child(spacer())
            .child(text(value).font(role("tabular")).color(cx.p.text).truncate_middle().shrink()));
    }
    c
}

fn stat<S: PackInputs + 'static>(cx: &Ctx, b: &Value) -> El {
    if let Some(state) = settled::<S>(cx, b, 2, true) {
        return state;
    }
    let style = intent(cx, b);
    let ty = type_of(b);
    let value = shown(b.get("value").unwrap_or(&Value::Null), ty.as_ref());
    let small = cx.width < TILE * 1.4;
    let big = if small || value.chars().count() > 12 { role("title") } else { role("display") };
    let ink = match (style.tint, blocks::text(b, "intent")) {
        (Some(t), "urgent" | "warning" | "success") => t.stops().0,
        _ if style.quiet => cx.p.text_secondary,
        _ => cx.p.text,
    };
    // Keyed by what it says: a new value slides in as the old one leaves (§13.5).
    let figure = text(if value.is_empty() { "—".into() } else { value.clone() }).font(big).color(ink).truncate();
    let figure = moving(cx, el().key(format!("pack-stat-{}-{value}", cx.target)).child(figure));
    let mut line = row().gap(space::SM).items_end().child(figure);
    let unit = blocks::text(b, "unit");
    if !unit.is_empty() {
        line = line.child(text(unit.to_string()).font(role("headline")).color(cx.p.text_secondary).pb(space::XS));
    }
    if let Some(trend) = b.get("trend").filter(|t| !t.is_null()) {
        let (arrow, colour) = match trend {
            Value::Number(n) if n.as_f64().unwrap_or(0.0) > 0.0 => ("↑", cx.p.success),
            Value::Number(n) if n.as_f64().unwrap_or(0.0) < 0.0 => ("↓", cx.p.destructive),
            Value::String(s) if s == "up" => ("↑", cx.p.success),
            Value::String(s) if s == "down" => ("↓", cx.p.destructive),
            _ => ("→", cx.p.text_tertiary),
        };
        let amount = trend.as_f64().map(|n| types::number(n.abs())).unwrap_or_default();
        line = line.child(text(format!("{arrow} {amount}").trim().to_string()).font(role("caption")).color(colour).pb(space::XS));
    }
    let mut c = col().gap(space::XS).child(line);
    let caption = blocks::text(b, "caption");
    if !caption.is_empty() {
        c = c.child(text(caption.to_string()).font(role("caption")).color(cx.p.text_secondary).truncate());
    }
    if let Some(history) = b.get("history").filter(|h| h.as_array().is_some_and(|a| a.len() > 1)) {
        let w = (cx.width - space::XXL * 2.0).clamp(60.0, 360.0);
        c = c.child(el().pt(space::SM).child(plot(cx, history, false, w, if small { 28.0 } else { 54.0 }, style.tint.map_or(cx.p.primary_action, |t| t.stops().0))));
    }
    let live = style.pulse.then(|| el().size(6.0, 6.0).radius(3.0).fill(style.tint.map_or(cx.p.success, |t| t.stops().0)).looping(LoopSpec::opacity(1.4, 0.0, 0.3, 1.0)));
    let mut wrap = row().gap(space::MD).items_start().child(c);
    if let Some(dot) = live {
        wrap = wrap.child(el().pt(space::MD).child(dot));
    }
    match fx_layer(cx, style.effect, "default", style.tint.map(|t| t.stops().0), looks::radius("card").unwrap_or(10.0)).filter(|_| style.tint.is_some()) {
        Some(layer) => el().child(wrap).child(layer),
        None => wrap,
    }
}

fn table<S: PackInputs + 'static>(cx: &Ctx, b: &Value) -> El {
    if let Some(state) = settled::<S>(cx, b, 4, true) {
        return state;
    }
    let columns: Vec<String> = items(b, "columns").iter().map(|c| match c {
        Value::Object(m) => shown(m.get("title").or(m.get("name")).unwrap_or(&Value::Null), None),
        other => shown(other, None),
    }).collect();
    let rows = items(b, "rows");
    if rows.is_empty() {
        return empty_state(cx, "No Rows", "", "", false);
    }
    let line = |cells: Vec<String>, head: bool| {
        row().w_full().py(space::SM).gap(space::XL).children(cells.into_iter().map(|cell| {
            el().grow(1.0).w(0.0).child(text(cell).font(role(if head { "caption" } else { "tabular" })).color(if head { cx.p.text_secondary } else { cx.p.text }).truncate())
        }))
    };
    let mut c = col().w_full();
    if !columns.is_empty() {
        c = c.child(line(columns.clone(), true)).child(hairline(cx));
    }
    for r in rows {
        let cells: Vec<String> = match r {
            Value::Array(cells) => cells.iter().map(|v| shown(v, None)).collect(),
            Value::Object(m) => items(b, "columns").iter().map(|c| shown(m.get(c.get("key").and_then(Value::as_str).or(c.as_str()).unwrap_or("")).unwrap_or(&Value::Null), None)).collect(),
            other => vec![shown(other, None)],
        };
        c = c.child(line(cells, false));
    }
    c
}

/// Tiles on the four-column grid: each takes the first room that fits it, reading order.
fn board<S: PackInputs + 'static>(cx: &Ctx, b: &Value) -> El {
    let tiles = items(b, "tiles");
    if tiles.is_empty() {
        return empty_state(cx, "No Tiles", "Add tiles to the board", "grid", false);
    }
    let columns = if cx.width < TILE * 2.0 { 1 } else if cx.width < TILE * 4.0 { 2 } else { 4 };
    let unit = ((cx.width - TILE_GAP * (columns as f32 - 1.0)) / columns as f32).floor();
    let mut taken: Vec<Vec<bool>> = Vec::new();
    let mut placed = Vec::new();
    for tile in tiles {
        let (w, h) = blocks::text(tile, "size").split_once('x').and_then(|(w, h)| Some((w.trim().parse::<usize>().ok()?, h.trim().parse::<usize>().ok()?))).unwrap_or((1, 1));
        let (w, h) = (w.clamp(1, columns), h.clamp(1, 4));
        let mut at = None;
        'find: for y in 0..64 {
            for x in 0..=(columns - w) {
                let free = (y..y + h).all(|yy| (x..x + w).all(|xx| !taken.get(yy).is_some_and(|r| r[xx])));
                if free {
                    at = Some((x, y));
                    break 'find;
                }
            }
        }
        let Some((x, y)) = at else { continue };
        while taken.len() < y + h {
            taken.push(vec![false; columns]);
        }
        for line in taken.iter_mut().skip(y).take(h) {
            for cell in line.iter_mut().skip(x).take(w) {
                *cell = true;
            }
        }
        placed.push((tile, x, y, w, h));
    }
    let height = taken.len() as f32 * (unit + TILE_GAP) - TILE_GAP;
    let radius = looks::radius("card").unwrap_or(10.0);
    let mut grid = el().w_full().h(height.max(unit));
    for (tile, x, y, w, h) in placed {
        let (tw, th) = (w as f32 * unit + (w as f32 - 1.0) * TILE_GAP, h as f32 * unit + (h as f32 - 1.0) * TILE_GAP);
        let inner = cx.narrow(tw - space::XL * 2.0);
        let content = tile.get("content").unwrap_or(&Value::Null);
        let title = blocks::text(tile, "title");
        // A tile's content names its own heading where it has one.
        let headed = matches!(kind(content), "List" | "KeyValue") && !blocks::text(content, "heading").is_empty() || kind(content) == "Stat" && !blocks::text(content, "caption").is_empty()
            || matches!(kind(content), "Chart" | "Empty") || kind(content) == "text" && !blocks::text(content, "caption").is_empty();
        let mut card = col().absolute(x as f32 * (unit + TILE_GAP), y as f32 * (unit + TILE_GAP)).size(tw, th).padding(space::XL).gap(space::SM)
            .radius(radius).clip_rounded(radius).fill(cx.p.card_fill).border(1.0, cx.p.card_stroke);
        if !title.is_empty() && !headed {
            card = card.child(text(title.to_string()).font(role("caption")).color(cx.p.text_secondary).truncate());
        }
        card = card.child(col().w_full().grow(1.0).min_h(0.0).justify_center().child(block::<S>(&inner, content)));
        if let Some(address) = tile.get("address").and_then(Value::as_str).filter(|a| a.contains('.')) {
            card = press::<S>(card.key(inner.key("tile")).hover(cx.p.row_hover), PackOp::Invoke { address: address.into(), args: tile.get("args").cloned().unwrap_or(Value::Null) });
        }
        grid = grid.child(card);
        cx.fx.set(inner.fx.get());
    }
    grid
}

/// One block and everything inside it.
pub fn block<S: PackInputs + 'static>(cx: &Ctx, b: &Value) -> El {
    let p = &cx.p;
    match kind(b) {
        "" => match b {
            Value::Null => el(),
            other => text(shown(other, None)).font(role("body")).color(p.text).wrap(),
        },
        "text" => {
            let named = blocks::text(b, "role");
            let style = role(if named.is_empty() { "body" } else { named });
            let mut t = text(blocks::text(b, "text").to_string()).font(style).color(colour(cx, b, "color", if intent(cx, b).quiet { p.text_secondary } else { p.text }));
            t = match b.get("clip").and_then(Value::as_u64) {
                Some(lines) => t.wrap().max_lines(lines.max(1) as usize),
                None if b.get("wrap").and_then(Value::as_bool).unwrap_or(false) => t.wrap(),
                None => t.truncate(),
            };
            let caption = blocks::text(b, "caption");
            if caption.is_empty() { t } else { col().w_full().gap(space::XS).child(text(caption.to_string()).font(role("caption")).color(p.text_secondary).truncate()).child(t) }
        }
        "row" | "col" | "box" | "glass" => {
            let name = kind(b);
            let mut e = if name == "row" { row().center_y() } else { col() };
            e = e.gap(token(b, "gap", looks::space, if name == "box" { 0.0 } else { space::MD }));
            let pad = token(b, "pad", looks::space, token(b, "padding", looks::space, if matches!(name, "box" | "glass") { space::XL } else { 0.0 }));
            e = e.padding(pad);
            // One axis of padding, over what `padding` gave both.
            let (px, py) = (token(b, "px", looks::space, pad), token(b, "py", looks::space, pad));
            if b.get("px").is_some() {
                e = e.px(px);
            }
            if b.get("py").is_some() {
                e = e.py(py);
            }
            let pad = pad.max(px);
            // `grow = true` fills the width; `grow = 1` takes a share of what its row leaves.
            match b.get("grow") {
                Some(Value::Number(n)) => e = e.grow(n.as_f64().unwrap_or(0.0).max(0.0) as f32).min_w(0.0),
                other => if other.and_then(Value::as_bool).unwrap_or(name == "col") { e = e.w_full() },
            }
            // Points, or a share of the parent's width as a percentage: `width = "40%"`.
            match b.get("width") {
                Some(Value::Number(n)) => if let Some(w) = n.as_f64().filter(|w| *w > 0.0) { e = e.w(w as f32) },
                Some(Value::String(s)) => if let Some(f) = s.strip_suffix('%').and_then(|n| n.trim().parse::<f32>().ok()) { e = e.w_frac((f / 100.0).clamp(0.0, 1.0)) },
                _ => {}
            }
            if let Some(h) = b.get("height").and_then(Value::as_f64).filter(|h| *h > 0.0) {
                e = e.h(h as f32);
            }
            if name == "row" && b.get("wrap").and_then(Value::as_bool).unwrap_or(false) {
                e = e.flex_wrap();
            }
            match blocks::text(b, "align") {
                "center" => e = e.center(),
                "end" => e = e.items_end(),
                _ => {}
            }
            match blocks::text(b, "justify") {
                "between" => e = e.justify_between(),
                "center" => e = e.justify_center(),
                "end" => e = e.justify_end(),
                _ => {}
            }
            let radius = token(b, "radius", looks::radius, if matches!(name, "box" | "glass") { looks::radius("card").unwrap_or(10.0) } else { 0.0 });
            if name == "glass" {
                e = crate::ui::glass(e, p, radius);
            } else if name == "box" || b.get("fill").is_some() {
                e = e.radius(radius).fill(colour(cx, b, "fill", p.card_fill));
            }
            let inner = cx.narrow((cx.width - pad * 2.0).max(40.0));
            e = e.children(items(b, "children").iter().map(|child| block::<S>(&inner, child)));
            cx.fx.set(inner.fx.get());
            let e = with_intent(cx, e, intent(cx, b), radius);
            // `press = function(payload, cx) … end`: what an ejected component's rows answer with.
            match b.get("press").and_then(|h| h.get("_handler")).and_then(Value::as_u64) {
                Some(handler) => press::<S>(e.key(cx.key("press")).hover(p.row_hover), PackOp::Handler {
                    pack: cx.pack.into(), target: cx.target.into(), handler, payload: b.get("payload").cloned().unwrap_or(Value::Null),
                }),
                None => e,
            }
        }
        "spacer" => spacer(),
        "icon" => glyph(cx, blocks::text(b, "name"), token(b, "size", |n| match n { "sm" => Some(12.0), "md" => Some(16.0), "lg" => Some(22.0), "xl" => Some(32.0), _ => None }, 16.0), colour(cx, b, "color", p.text_secondary)),
        "image" => {
            let cover = b.get("cover").and_then(Value::as_bool).unwrap_or(false);
            let w = cx.width.min(token(b, "width", |_| None, cx.width));
            picture(cx, blocks::text(b, "source"), w, token(b, "height", |_| None, (w * 0.56).min(220.0)), cover)
        }
        "input" => input::<S>(cx, b),
        "markdown" => match settled::<S>(cx, b, 4, true) {
            Some(state) => state,
            None => crate::ext::view::markdown(p, blocks::text(b, "source")),
        },
        "code" => col().w_full().padding(space::XL).radius(looks::radius("card").unwrap_or(10.0)).fill(p.card_fill)
            .child(text(blocks::text(b, "source").to_string()).font(role("mono")).color(p.text.alpha(0.9)).wrap_anywhere().max_lines(40)),
        "chart" | "Chart" => match settled::<S>(cx, b, 3, true) {
            Some(state) => state,
            None => {
                let title = blocks::text(b, "title");
                let w = (cx.width - if cx.width > 400.0 { space::XXL * 2.0 } else { 0.0 }).max(60.0);
                let h = if cx.width < TILE * 1.4 { 64.0 } else if cx.width < 400.0 { 110.0 } else { 190.0 };
                let tint = looks::tint(blocks::text(b, "tint")).map_or(intent(cx, b).tint.map_or(p.primary_action, |t| t.stops().0), |t| t.stops().0);
                let mut c = col().w_full().gap(space::SM);
                if !title.is_empty() {
                    c = c.child(text(title.to_string()).font(role("caption")).color(p.text_secondary).truncate());
                }
                c.child(plot(cx, b.get("series").unwrap_or(&Value::Null), blocks::text(b, "kind") == "bar", w, h, tint))
            }
        },
        "fx" => {
            let child = b.get("child").map(|c| block::<S>(cx, c)).unwrap_or_else(el);
            let radius = looks::radius("card").unwrap_or(10.0);
            let mut wrap = el().child(child);
            if let Ok(layers) = super::fx::parse(b.get("effects").unwrap_or(&Value::Null), "ui.fx") {
                for layer in layers {
                    if let Some(drawn) = fx_layer(cx, &layer.name, &layer.preset, None, radius) {
                        wrap = wrap.child(drawn);
                    }
                }
            }
            wrap
        }
        "List" => list::<S>(cx, b),
        "Grid" => grid::<S>(cx, b, false),
        "Gallery" => grid::<S>(cx, b, true),
        "Timeline" => timeline::<S>(cx, b),
        "Row" => {
            let mut c = col().w_full().gap(space::XXS).child(text(shown(b.get("title").unwrap_or(&Value::Null), None)).font(role(if b.get("big").and_then(Value::as_bool).unwrap_or(false) { "title" } else { "body" })).color(p.text).truncate());
            let subtitle = blocks::text(b, "subtitle");
            if !subtitle.is_empty() {
                c = c.child(text(subtitle.to_string()).font(role("caption")).color(p.text_secondary).truncate());
            }
            if let Some(points) = b.get("sparkline") {
                c = c.child(plot(cx, points, false, cx.width.min(160.0), 24.0, p.primary_action));
            }
            c
        }
        "Detail" => match settled::<S>(cx, b, 5, true) {
            Some(state) => state,
            None => {
                let mut c = col().w_full().gap(space::XL);
                if let Some(image) = b.get("image").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                    c = c.child(picture(cx, image, cx.width, (cx.width * 0.4).min(210.0), false));
                }
                let markdown = blocks::text(b, "markdown");
                if !markdown.is_empty() {
                    c = c.child(crate::ext::view::markdown(p, markdown));
                }
                let fields = items(b, "fields");
                if !fields.is_empty() {
                    c = c.child(key_values(cx, fields, ""));
                }
                c
            }
        },
        "Form" => {
            let fields: Vec<String> = b.get("fields").and_then(Value::as_object).map(|m| m.keys().cloned().collect()).unwrap_or_default();
            let mut c = col().w_full().gap(space::MD);
            for name in fields {
                c = c.child(row().w_full().h(TARGET + 6.0).px(space::XL).center_y().radius(looks::radius("control").unwrap_or(8.0)).fill(p.card_fill).border(1.0, p.card_stroke)
                    .child(text(name).font(role("body")).color(p.text_tertiary)));
            }
            c
        }
        "Stat" => stat::<S>(cx, b),
        "Table" => table::<S>(cx, b),
        "KeyValue" => match settled::<S>(cx, b, 3, true) {
            Some(state) => state,
            None => key_values(cx, items(b, "items"), blocks::text(b, "heading")),
        },
        "Progress" => {
            let value = b.get("value").and_then(Value::as_f64).unwrap_or(0.0).clamp(0.0, 1.0) as f32;
            let style = intent(cx, b);
            let tint = style.tint.map_or(p.primary_action, |t| t.stops().0);
            let w = cx.width.max(40.0);
            let label = blocks::text(b, "label");
            let mut c = col().w_full().gap(space::SM);
            if !label.is_empty() {
                c = c.child(row().w_full().child(text(label.to_string()).font(role("caption")).color(p.text_secondary).truncate()).child(spacer())
                    .child(text(format!("{:.0}%", value * 100.0)).font(role("caption")).color(p.text_tertiary)));
            }
            c.child(el().w_full().h(6.0).radius(3.0).fill(p.control_surface).child(el().absolute(0.0, 0.0).w((w * value).max(6.0).min(w)).h(6.0).radius(3.0).fill(tint)))
        }
        "Banner" => {
            let style = intent(cx, b);
            let tint = style.tint.map_or(p.primary_action, |t| t.stops().0);
            let radius = looks::radius("card").unwrap_or(10.0);
            let mut c = col().w_full().padding(space::XL).pl(space::XL + space::SM).gap(space::XXS).radius(radius).fill(tint.alpha(if style.quiet { 0.06 } else { 0.13 }))
                .child(text(blocks::text(b, "title").to_string()).font(role("headline")).color(p.text).truncate());
            let body = blocks::text(b, "text");
            if !body.is_empty() {
                c = c.child(text(body.to_string()).font(role("caption")).color(p.text_secondary).wrap());
            }
            c.child(el().absolute(0.0, 0.0).w(3.0).h_full().radius(1.5).fill(tint).inert())
        }
        "Empty" => {
            let error = blocks::text(b, "state") == "error";
            let hint = if error { blocks::text(b, "error") } else { blocks::text(b, "hint") };
            let mut e = empty_state(cx, blocks::text(b, "title"), hint, blocks::text(b, "icon"), error);
            let learn = blocks::text(b, "learn");
            if error && !learn.is_empty() {
                e = e.child(text(format!("{} · {learn}", blocks::text(b, "code"))).font(role("mono")).color(p.text_tertiary).pt(space::SM).truncate());
            }
            e
        }
        "Loading" => {
            let face = blocks::text(b, "face");
            let small = face == "tile" || cx.width < TILE * 2.0;
            skeleton(cx, if small { 2 } else { 6 }, small)
        }
        "Tabs" => items(b, "tabs").first().and_then(|t| t.get("content")).map(|c| block::<S>(cx, c)).unwrap_or_else(el),
        "Split" => {
            let half = cx.narrow((cx.width - space::XL * 2.0 - size::HAIRLINE) / 2.0);
            let pane = |key: &str| el().grow(1.0).w(0.0).child(b.get(key).map(|c| block::<S>(&half, c)).unwrap_or_else(el));
            row().w_full().gap(space::XL).items_start().child(pane("left")).child(el().w(size::HAIRLINE).h_full().fill(p.separator)).child(pane("right"))
        }
        "Stream" => {
            let source = blocks::text(b, "text");
            let live = blocks::text(b, "state") == "pending";
            if source.is_empty() && live {
                return skeleton(cx, 3, true);
            }
            rune_kit::markdown::Markdown::new(source.to_string()).streaming(live).selectable(false).render(&crate::ui::markdown_style(p))
        }
        "Board" => board::<S>(cx, b),
        other => text(format!("ui.{other} can't be drawn here")).font(role("caption")).color(p.destructive),
    }
}

/// A field a person types in. What is typed is kept by the window under the input's key; the
/// pack hears it through `change(text, cx)` as it changes and `submit(text, cx)` on Return.
fn input<S: PackInputs + 'static>(cx: &Ctx, b: &Value) -> El {
    let p = &cx.p;
    let name = Some(blocks::text(b, "key")).filter(|k| !k.is_empty()).map(String::from).unwrap_or_else(|| cx.key("input"));
    let key = format!("pack-input-{}-{}-{name}", cx.pack, cx.target);
    let initial = match b.get("value") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => shown(other, None),
    };
    let fresh = TextEdit::new(initial.clone());
    let edit = cx.inputs.and_then(|all| all.get(&key)).unwrap_or(&fresh);
    let handler = |name: &str| b.get(name).and_then(|h| h.get("_handler")).and_then(Value::as_u64);
    let (pack, target) = (cx.pack.to_string(), cx.target.to_string());
    let mut field = text_input(edit, role("body")).key(key.clone()).single_line().grow(1.0).color(p.text)
        .placeholder(blocks::text(b, "placeholder").to_string(), p.text_tertiary).no_focus_ring();
    {
        let (key, initial) = (key.clone(), initial.clone());
        field = field.on_edit(move |s: &mut S| s.pack_input(&key, &initial));
    }
    if let Some(handler) = handler("change") {
        let (key, initial, pack, target) = (key.clone(), initial.clone(), pack.clone(), target.clone());
        field = field.on_input_change(move |s: &mut S, _| {
            let text = s.pack_input(&key, &initial).text.clone();
            host::dispatch(PackOp::Handler { pack: pack.clone(), target: target.clone(), handler, payload: Value::String(text) });
        });
    }
    if let Some(handler) = handler("submit") {
        let (key, initial) = (key.clone(), initial.clone());
        field = field.on_submit(move |s: &mut S, _| {
            let text = s.pack_input(&key, &initial).text.clone();
            host::dispatch(PackOp::Handler { pack: pack.clone(), target: target.clone(), handler, payload: Value::String(text) });
        });
    }
    row().w_full().h(TARGET + 6.0).px(space::XL).center_y().radius(looks::radius("control").unwrap_or(8.0)).fill(p.card_fill).border(1.0, p.card_stroke)
        .child(field)
}

/// A surface filling the palette's body: scrolled under the bars, as every screen is.
pub fn body<S: PackInputs + 'static>(cx: &Ctx, root: &Value) -> El {
    let centred = matches!(kind(root), "Empty" | "Stat") || kind(root) == "Loading" && cx.width < 300.0;
    let inset = if kind(root) == "Board" { space::XL } else { space::XXL + space::XS };
    let inner = cx.narrow(cx.width - inset * 2.0);
    let content = block::<S>(&inner, root);
    if centred {
        return col().pin(FULL, FULL, FULL, FULL).pt(size::HEADER_BAND).pb(size::BOTTOM_BAR_H).center().child(el().px(inset).child(content));
    }
    col().pin(FULL, FULL, FULL, FULL).pt(size::HEADER_BAND).pb(size::BOTTOM_BAR_H)
        .child(col().key("list").w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).scroll_edge_fade(24.0)
            .child(col().w_full().px(inset).pt(space::XS).pb(space::XL).child(content)))
}
