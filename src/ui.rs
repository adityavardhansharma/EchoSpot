//! The palette. One borderless panel whose surface is the desktop's blur under a scrim; the
//! header and bottom bar float over the list as transparent overlays, rows dissolve beneath
//! them, and only the floating controls are glass.

use rune::{Color, Cx, El, ScrollFade, ScrollSpec, SurfaceFit, TextStyle, Transition, col, el, icon, row, spacer, text, text_input};

use crate::app::{App, Menu, MenuKind, Screen};
use crate::features::forms::{self, FieldKind, Form};
use crate::icons;
use crate::model::*;
use crate::theme::{self, Palette, motion, radius, size, space, type_};

const FULL: Option<f32> = Some(0.0);

/// A floating control's glass: the list blurred behind a tint, lit along its top rim.
pub fn glass(e: El, p: &Palette, r: f32) -> El {
    e.radius(r).backdrop_blur(16.0).fill(p.glass_tint)
        .border_gradient(1.0, p.glass_rim_top, p.glass_rim_bottom)
        .shadow(3.0, 14.0, p.glass_shadow)
}

pub fn keycap(p: &Palette, label: &str, outline: bool) -> El {
    let cap = row().min_w(size::KEY_CAP).h(size::KEY_CAP).px(space::XS).center().radius(radius::KEY_CAP);
    let cap = if outline { cap.border(1.0, p.border) } else { cap.fill(p.control_surface).border(1.0, p.border.alpha(0.12)) };
    cap.child(match label {
        "↵" => icon(&icons::RETURN, 11.0).color(p.text_secondary),
        "⇥" => icon(&icons::TAB, 11.0).color(p.text_secondary),
        _ => text(label.to_string()).font(type_::KEY_CAP).color(p.text_secondary),
    })
}

fn keycaps_row(p: &Palette, keys: &str, outline: bool) -> El {
    row().gap(space::XXS).center_y().children(keycaps(keys).iter().map(|k| keycap(p, k, outline)))
}

/// The tile a command's glyph sits on, shaped and sized like an app icon.
fn glyph_tile(p: &Palette, glyph: &'static rune::Icon, slot: f32, tint: Option<Color>) -> El {
    let tile = slot - 3.0;
    let face = el().size(tile, tile).radius(tile * 0.26).center()
        .fill(tint.unwrap_or(p.control_surface))
        .border(1.0, if tint.is_some() { Color::WHITE.alpha(0.12) } else { p.border.alpha(0.10) })
        .child(icon(glyph, (tile * 0.62).round()).color(if tint.is_some() { Color::WHITE } else { p.text.alpha(0.86) }));
    el().size(slot, slot).center().child(face)
}

/// A command's own tile: its glyph in white on the family colour, lit from above.
pub fn badge_tile(glyph: &'static rune::Icon, slot: f32, tint: theme::Tint) -> El {
    let tile = slot - 3.0;
    let (top, bottom) = tint.stops();
    let face = el().size(tile, tile).radius(tile * 0.28).center().fill(bottom).gradient(top, bottom)
        .border_gradient(1.0, Color::WHITE.alpha(0.42), Color::WHITE.alpha(0.05))
        .shadow(1.0, 3.0, bottom.alpha(0.38))
        .child(icon(glyph, (tile * 0.62).round()).color(Color::WHITE));
    el().size(slot, slot).center().child(face)
}

/// A site's icon on a light plate: most favicons are drawn for a white tab strip.
fn favicon_tile(px: &rune::Pixels, slot: f32) -> El {
    let tile = slot - 3.0;
    let inset = (tile * 0.16).round();
    let face = el().size(tile, tile).radius(tile * 0.28).center().fill(Color::WHITE.alpha(0.96))
        .shadow(1.0, 3.0, Color::BLACK.alpha(0.22))
        .child(el().size(tile - inset * 2.0, tile - inset * 2.0).radius(2.0).pixels_fit(px, SurfaceFit::Fit));
    el().size(slot, slot).center().child(face)
}

/// Markdown set in the palette's own type: an answer, an extension's page, a store listing.
pub fn markdown_style(p: &Palette) -> rune_kit::markdown::MarkdownStyle {
    rune_kit::markdown::MarkdownStyle::neutral(p.dark)
        .text(p.text.alpha(0.92)).muted(p.text_secondary).link(p.primary_action)
        .inline_code_bg(p.control_surface).code_block(p.card_fill, p.card_stroke).code_block_padding(space::XL, space::LG)
        .body_font(TextStyle::new(13.0, 400.0).lh(20.0))
        .heading_font(1, TextStyle::new(17.0, 650.0).lh(24.0))
        .heading_font(2, TextStyle::new(15.0, 620.0).lh(22.0))
        .heading_font(3, TextStyle::new(13.0, 650.0).lh(20.0))
        .code_block_font(TextStyle::new(12.0, 400.0).lh(18.0).mono())
        .rule(p.separator).quote_rule(p.border).list(18.0, 4.0)
}

pub fn icon_slot(s: &App, p: &Palette, scale: f32, src: &IconSrc, slot: f32) -> El {
    match src {
        IconSrc::None => el().size(slot, slot),
        IconSrc::Glyph(g) => glyph_tile(p, g, slot, None),
        IconSrc::Tinted(g, c) => glyph_tile(p, g, slot, Some(*c)),
        IconSrc::Badge(g, tint) => badge_tile(g, slot, *tint),
        IconSrc::Favicon(path) => match s.icons.file(path, slot, scale) {
            Some(px) => favicon_tile(&px, slot),
            None => glyph_tile(p, &icons::LINK, slot, None),
        },
        IconSrc::App(path) => match s.icons.file(path, slot, scale) {
            Some(px) => el().size(slot, slot).pixels_fit(&px, SurfaceFit::Fit),
            None => glyph_tile(p, &icons::BOX, slot, None),
        },
        IconSrc::File(path) => match s.icons.file(path, slot, scale) {
            Some(px) => el().size(slot, slot).center()
                .child(el().size(slot - 2.0, slot - 2.0).radius(radius::THUMBNAIL).fill(p.icon_placeholder).pixels_fit(&px, SurfaceFit::Fill)),
            None => glyph_tile(p, &icons::IMAGE, slot, None),
        },
        IconSrc::Emoji(e) => el().size(slot, slot).center().child(text(e.clone()).font(TextStyle::new(slot * 0.72, 400.0).lh(slot))),
        IconSrc::Swatch(c) => el().size(slot, slot).center()
            .child(el().size(slot - 4.0, slot - 4.0).radius(radius::THUMBNAIL).fill(*c).border(1.0, Color::WHITE.alpha(0.14))),
    }
}

fn section_header(p: &Palette, title: &str, count: Option<usize>, first: bool) -> El {
    let mut header = row().w_full().px(space::MD).gap(space::SM)
        .pt(if first { space::XS } else { space::SECTION_SPACING }).pb(space::SECTION_HEADER_BOTTOM)
        .child(text(title.to_string()).font(type_::SECTION).color(p.text_secondary).truncate());
    if let Some(n) = count {
        header = header.child(text(n.to_string()).font(type_::SECTION).color(p.text_tertiary));
    }
    header
}

/// Selection always beats hover: a selected row paints one fill and no hover over it.
fn row_fill(e: El, p: &Palette, selected: bool) -> El {
    if selected { e.fill(p.selection) } else { e.fill(Color::TRANSPARENT).hover(p.row_hover) }.motion(motion::HOVER)
}

fn result_row(s: &App, p: &Palette, scale: f32, item: &Item, index: usize, selected: bool, compact: bool) -> El {
    let mut r = row().key(format!("row-{}", item.id)).w_full().px(space::MD).py(space::SM).gap(space::LG).center_y().radius(radius::ROW);
    r = row_fill(r, p, selected)
        .on_press_tx(move |s: &mut App, tx| s.select(index, tx))
        .on_double_press_tx(|s: &mut App, tx| s.run_primary(tx));
    let mut slot = icon_slot(s, p, scale, &item.icon, size::RESULT_ROW_ICON);
    if item.running {
        slot = slot.child(el().absolute(size::RESULT_ROW_ICON / 2.0 - 1.5, size::RESULT_ROW_ICON + 1.0).size(3.0, 3.0).radius(1.5).fill(p.text_secondary));
    }
    r = r.child(slot).child(text(item.title.clone()).font(type_::ROW_TITLE).color(p.text).truncate());
    if !compact {
        if let Some(sub) = &item.subtitle {
            r = r.child(text(sub.clone()).font(type_::ROW_TRAILING).color(p.text_secondary).truncate().shrink());
        }
    }
    r = r.child(spacer());
    if !compact {
        for a in &item.accessories {
            r = r.child(match a.tag {
                Some(c) => row().px(space::SM).py(space::XXS).radius(radius::MENU).fill(c.alpha(0.18))
                    .child(text(a.text.clone()).font(type_::ROW_TRAILING).color(c)),
                None => text(a.text.clone()).font(type_::ROW_TRAILING).color(p.text_secondary),
            });
        }
        if !item.trailing.is_empty() {
            r = r.child(text(item.trailing.clone()).font(type_::ROW_TRAILING).color(p.text_secondary));
        }
    }
    r
}

/// The calculator's lead card: source and result columns around an arrow.
fn lead_card(p: &Palette, item: &Item, card: &Card, index: usize, selected: bool) -> El {
    let column = |value: &str, badge: &str| {
        col().grow(1.0).center().gap(space::MD)
            .child(text(value.to_string()).font(type_::CALC_RESULT).color(p.text.alpha(0.96)).truncate())
            .child(row().px(space::SM).py(space::XXS).radius(radius::KEY_CAP).fill(p.control_surface)
                .child(text(badge.to_string()).font(type_::KEY_CAP).color(p.text_secondary)))
    };
    row().key(format!("row-{}", item.id)).w_full().h(106.0).px(space::XXL).center_y().radius(radius::CARD)
        .fill(if selected { p.selection } else { p.card_fill }).motion(motion::HOVER)
        .on_press_tx(move |s: &mut App, tx| s.select(index, tx))
        .on_double_press_tx(|s: &mut App, tx| s.run_primary(tx))
        .child(column(&card.left, &card.left_badge))
        .child(icon(&icons::ARROW_RIGHT, 16.0).color(p.text_tertiary))
        .child(column(&card.right, &card.right_badge))
}

fn empty_state(p: &Palette, screen: &Screen) -> El {
    let mut c = col().w_full().h_full().center().gap(space::XS)
        .child(text(screen.empty.0.clone()).font(type_::PANEL_TITLE).color(p.text_secondary));
    if !screen.empty.1.is_empty() {
        c = c.child(text(screen.empty.1.clone()).font(type_::ROW_TRAILING).color(p.text_tertiary));
    }
    c
}

/// A palette list's scroll viewport: it fills the panel and dissolves under the bars.
/// A palette list's scroll viewport. It runs under the bottom bar, where rows dissolve over a
/// long band, but stops short of the header: a row leaving the top fades out within a few points
/// of the search field instead of sliding through its text.
fn list_viewport(content: El) -> El {
    let fade = ScrollFade::strength(size::DISSOLVE_TOP).full_at(4.0).with_end(ScrollFade::strength(size::DISSOLVE_BOTTOM));
    col().w_full().h_full().pt(size::LIST_CLIP_TOP).child(
        col().key("list").w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false })
            .scroll_fade(fade).scroll_margin(size::BOTTOM_BAR_H + space::SM)
            .child(content))
}

fn list_rows(s: &App, p: &Palette, scale: f32, screen: &Screen, compact: bool) -> El {
    let mut content = col().w_full().px(space::MD).pt(size::LIST_TOP_PAD).pb(size::BOTTOM_BAR_H + space::SM);
    for r in &screen.rows {
        content = content.child(match r {
            Row::Header { title, count, first } => section_header(p, title, *count, *first),
            Row::Item(i) => {
                let item = &screen.items[*i];
                match &item.card {
                    Some(card) => lead_card(p, item, card, *i, *i == screen.sel),
                    None => result_row(s, p, scale, item, *i, *i == screen.sel, compact),
                }
            }
        });
    }
    content
}

fn list_body(s: &App, p: &Palette, scale: f32, screen: &Screen) -> El {
    if screen.items.is_empty() {
        return el().pin(FULL, FULL, FULL, FULL).child(empty_state(p, screen));
    }
    el().pin(FULL, FULL, FULL, FULL).child(list_viewport(list_rows(s, p, scale, screen, false)))
}

fn detail_pane(s: &App, p: &Palette, scale: f32, detail: &Detail) -> El {
    let mut c = col().w_full().px(space::XL + 2.0).pt(space::XS).pb(space::MD).gap(space::MD);
    if let Some(path) = &detail.image {
        let frame = el().w_full().h(210.0).radius(radius::CARD).fill(p.icon_placeholder);
        c = c.child(match s.icons.file(path, 460.0, scale) {
            Some(px) => frame.pixels_fit(&px, SurfaceFit::Fit),
            None => frame.center().child(icon(&icons::IMAGE, 28.0).color(p.text_tertiary)),
        });
    }
    if let Some(color) = detail.swatch {
        c = c.child(el().w_full().h(150.0).radius(radius::CARD).fill(color).border(1.0, Color::WHITE.alpha(0.10)));
    }
    if let Some(body) = &detail.text {
        c = c.child(col().w_full().padding(space::XL).radius(radius::CARD).fill(p.card_fill)
            .child(text(body.clone()).font(TextStyle::new(12.0, 400.0).lh(17.0).mono()).color(p.text.alpha(0.88)).wrap_anywhere().max_lines(16)));
    }
    if let Some(md) = &detail.markdown {
        c = c.child(crate::ext::view::markdown(p, md));
    }
    if !detail.fields.is_empty() {
        if let Some(heading) = &detail.heading {
            c = c.child(text(heading.clone()).font(type_::SECTION).color(p.text_secondary));
        }
        let mut fields = col().w_full();
        for (i, (label, value)) in detail.fields.iter().enumerate() {
            if i > 0 {
                fields = fields.child(el().w_full().h(size::HAIRLINE).fill(p.separator));
            }
            fields = fields.child(row().w_full().py(space::MD).gap(space::XL).center_y()
                .child(text(label.clone()).font(type_::ROW_TRAILING).color(p.text_secondary))
                .child(spacer())
                .child(text(value.clone()).font(type_::ROW_TRAILING).color(p.text).truncate_middle().shrink()));
        }
        c = c.child(fields);
    }
    col().key("detail").grow(1.0).h_full().min_h(0.0).pt(size::HEADER_BAND).pb(size::BOTTOM_BAR_H)
        .child(col().w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).scroll_edge_fade(24.0).child(c))
}

fn split_body(s: &App, p: &Palette, scale: f32, screen: &Screen) -> El {
    if screen.items.is_empty() {
        return el().pin(FULL, FULL, FULL, FULL).child(empty_state(p, screen));
    }
    let left = el().w(size::CLIPBOARD_LIST_W + space::MD).h_full().child(list_viewport(list_rows(s, p, scale, screen, true)));
    // The one hairline the palette draws: between a list and its preview.
    let rule = col().h_full().pt(size::HEADER_BAND).pb(size::BOTTOM_BAR_H).child(el().w(size::HAIRLINE).grow(1.0).fill(p.separator));
    let right = match screen.selected().and_then(|i| i.detail.as_ref()) {
        Some(detail) => detail_pane(s, p, scale, detail),
        None => col().grow(1.0).h_full(),
    };
    row().pin(FULL, FULL, FULL, FULL).child(left).child(rule).child(right)
}

fn grid_body(s: &App, p: &Palette, scale: f32, screen: &Screen, columns: usize) -> El {
    if screen.items.is_empty() {
        return el().pin(FULL, FULL, FULL, FULL).child(empty_state(p, screen));
    }
    let gap = space::MD;
    let cell = ((size::PANEL_W - size::EMOJI_GRID_INSET * 2.0 - gap * (columns as f32 - 1.0)) / columns as f32).floor();
    let mut content = col().w_full().px(size::EMOJI_GRID_INSET).pt(size::LIST_TOP_PAD).pb(size::BOTTOM_BAR_H + space::SM);
    let mut line: Vec<El> = Vec::new();
    let flush = |content: El, line: &mut Vec<El>| {
        if line.is_empty() { content } else { content.child(row().w_full().gap(gap).pb(gap).children(line.drain(..))) }
    };
    for r in &screen.rows {
        match r {
            Row::Header { title, count, first } => {
                content = flush(content, &mut line);
                content = content.child(el().w_full().pt(if *first { 0.0 } else { space::XS })
                    .child(section_header(p, title, *count, *first).px(0.0).pb(space::MD)));
            }
            Row::Item(i) => {
                let (item, index) = (&screen.items[*i], *i);
                let selected = index == screen.sel;
                // An emoji fills its tile; anything else is a picture over its title.
                let face = match &item.icon {
                    IconSrc::Emoji(e) => text(e.clone()).font(TextStyle::new((cell * 0.42).min(34.0), 400.0).lh(cell * 0.56)),
                    other => col().center().gap(space::XS)
                        .child(icon_slot(s, p, scale, other, (cell * 0.5).round()))
                        .child(text(item.title.clone()).font(type_::KEY_CAP).color(p.text_secondary).truncate().max_w(cell - space::MD)),
                };
                let mut tile = el().key(format!("row-{}", item.id)).size(cell, cell).radius(radius::EMOJI_CELL).center()
                    .fill(if selected { p.selection } else { p.emoji_cell })
                    .border(if selected { 1.5 } else { 1.0 }, if selected { p.emoji_selection_border } else { Color::TRANSPARENT })
                    .on_press_tx(move |s: &mut App, tx| s.select(index, tx))
                    .on_double_press_tx(|s: &mut App, tx| s.run_primary(tx))
                    .child(face);
                if !selected {
                    tile = tile.hover(p.row_hover);
                }
                line.push(tile);
                if line.len() == columns {
                    content = flush(content, &mut line);
                }
            }
        }
    }
    content = flush(content, &mut line);
    el().pin(FULL, FULL, FULL, FULL).child(list_viewport(content))
}

fn form_field(p: &Palette, form: &Form, index: usize) -> El {
    let field = &form.fields[index];
    let focused = form.focus == index;
    let chrome = |e: El| {
        e.radius(radius::FORM_FIELD).fill(p.card_fill)
            .border(if focused { 2.0 } else { 1.0 }, if !field.error.is_empty() { p.destructive } else if focused { p.border.alpha(0.45) } else { p.card_stroke })
    };
    let control = match &field.kind {
        FieldKind::Separator => return el().w_full().py(space::MD).child(el().w_full().h(size::HAIRLINE).fill(p.separator)),
        FieldKind::Description => {
            return row().w_full().gap(space::XL).child(el().w(140.0))
                .child(text(field.edit.text.clone()).font(type_::ROW_TRAILING).color(p.text_secondary).wrap().grow(1.0));
        }
        FieldKind::Text | FieldKind::Password => {
            let mut input = text_input(&field.edit, type_::ROW_TITLE).single_line().grow(1.0).color(p.text)
                .placeholder(field.placeholder.clone(), p.text_tertiary).no_focus_ring()
                .on_edit(move |s: &mut App| &mut s.form.as_mut().expect("a form is up").fields[index].edit)
                .on_input_key(|s: &mut App, ev, tx| forms::key(s, ev, tx));
            if field.kind == FieldKind::Password {
                input = input.secure();
            }
            chrome(row().key(format!("field-{}", field.id)).grow(1.0).h(34.0).px(space::XL).center_y()).child(input)
                .on_press_tx(move |s: &mut App, tx| forms::press_field(s, index, tx))
        }
        FieldKind::Area => {
            let input = text_input(&field.edit, type_::ROW_TITLE).grow(1.0).w_full().color(p.text)
                .placeholder(field.placeholder.clone(), p.text_tertiary).no_focus_ring()
                .on_edit(move |s: &mut App| &mut s.form.as_mut().expect("a form is up").fields[index].edit)
                .on_input_key(|s: &mut App, ev, tx| forms::key(s, ev, tx));
            chrome(col().key(format!("field-{}", field.id)).grow(1.0).min_h(96.0).px(space::XL).py(space::MD)).child(input)
                .on_press_tx(move |s: &mut App, tx| forms::press_field(s, index, tx))
        }
        FieldKind::Dropdown(options) => {
            let title = options.get(field.choice).map(|(_, t)| t.clone()).unwrap_or_default();
            chrome(row().key(format!("field-{}", field.id)).grow(1.0).h(34.0).px(space::XL).gap(space::MD).center_y())
                .hover(p.row_hover)
                .child(text(title).font(type_::ROW_TITLE).color(p.text).truncate()).child(spacer())
                .child(icon(&icons::CHEVRON_DOWN, 12.0).color(p.text_secondary))
                .on_press_tx(move |s: &mut App, tx| forms::press_field(s, index, tx))
        }
        FieldKind::Checkbox(label) => {
            let boxed = el().size(16.0, 16.0).radius(5.0).center()
                .fill(if field.checked { p.primary_action } else { p.card_fill })
                .border(if focused { 2.0 } else { 1.0 }, if field.checked { p.primary_action } else if focused { p.border.alpha(0.45) } else { p.border });
            let boxed = if field.checked { boxed.child(icon(&icons::CHECK, 11.0).color(Color::WHITE)) } else { boxed };
            row().key(format!("field-{}", field.id)).grow(1.0).h(34.0).gap(space::MD).center_y()
                .child(boxed).child(text(label.clone()).font(type_::ROW_TITLE).color(p.text))
                .on_press_tx(move |s: &mut App, tx| forms::press_field(s, index, tx))
        }
    };
    let label = row().w(140.0).h(34.0).center_y().justify_end()
        .child(text(field.label.clone()).font(type_::ROW_TRAILING).color(p.text_secondary).truncate());
    let mut block = col().w_full().gap(space::XS).child(row().w_full().gap(space::XL).items_start().child(label).child(control));
    let note = if !field.error.is_empty() { Some((field.error.clone(), p.destructive)) } else if !field.info.is_empty() { Some((field.info.clone(), p.text_tertiary)) } else { None };
    if let Some((note, color)) = note {
        block = block.child(row().w_full().gap(space::XL).child(el().w(140.0))
            .child(text(note).font(type_::SECTION).color(color).wrap().grow(1.0)));
    }
    block
}

fn form_body(p: &Palette, form: &Form) -> El {
    let content = col().w_full().px(space::XXL + space::XL).pr(72.0).pt(size::LIST_TOP_PAD + space::MD).pb(size::BOTTOM_BAR_H + space::XL).gap(space::XL)
        .children((0..form.fields.len()).map(|i| form_field(p, form, i)));
    el().pin(FULL, FULL, FULL, FULL).child(list_viewport(content))
}

fn bar_button(p: &Palette, key: &str) -> El {
    row().key(key.to_string()).h(size::BAR_BUTTON_H).px(space::MD).gap(space::SM).center_y().radius(size::BAR_BUTTON_H / 2.0)
        .hover(p.row_hover).press(p.control_surface).motion(motion::HOVER)
}

fn header(s: &App, p: &Palette, screen: &Screen) -> El {
    let root = s.screens.len() == 1;
    let lead = if root {
        el().size(size::HEADER_ICON_SLOT, size::HEADER_ICON_SLOT).center().child(icon(&icons::SEARCH, 19.0).color(p.text_secondary))
    } else {
        el().key("back").size(size::HEADER_ICON_SLOT + 4.0, size::HEADER_ICON_SLOT + 4.0).ml(-2.0).mr(-2.0).center().radius(radius::BAR_CONTROL)
            .hover(p.row_hover).on_press_tx(|s: &mut App, tx| s.pop(tx))
            .child(icon(&icons::CHEVRON_LEFT, 19.0).color(p.text_secondary))
    };
    let mut bar = row().key("header").pin(FULL, FULL, FULL, None).h(size::HEADER_BAND).pl(space::MD * 2.0).pr(space::MD * 2.0)
        .gap(space::XL + 2.0).center_y().child(lead);
    if let Some(form) = &s.form {
        bar = bar.child(text(form.title.clone()).font(type_::SEARCH).color(p.text).grow(1.0));
    } else {
        bar = bar.child(text_input(&screen.query, type_::SEARCH).key("search").single_line().grow(1.0).color(p.text)
            .placeholder(screen.placeholder.clone(), p.text_tertiary).no_focus_ring()
            .on_edit(|s: &mut App| &mut s.top_mut().query)
            .on_input_key(|s: &mut App, ev, tx| s.key(ev, tx))
            .on_input_change(|s: &mut App, tx| s.query_changed(tx)));
    }
    if root && s.form.is_none() {
        // Tab hands whatever is typed to the model.
        bar = bar.child(row().key("quick-ai").h(size::BAR_BUTTON_H).px(space::MD).gap(space::SM).center_y().radius(radius::BAR_CONTROL)
            .hover(p.row_hover).press(p.control_surface)
            .on_press_tx(|s: &mut App, tx| {
                let prompt = s.top().query.text.clone();
                s.perform(Act::Ai(AiOp::Open(prompt)), tx);
            })
            .child(text("Quick AI").font(type_::BAR).color(p.text_secondary))
            .child(keycap(p, "⇥", true)));
    }
    if screen.loading {
        bar = bar.child(icon(&icons::REFRESH, 14.0).color(p.text_tertiary).looping(rune_loop()));
    }
    if !screen.filters.is_empty() && s.form.is_none() {
        let name = screen.filters.get(screen.filter).cloned().unwrap_or_default();
        bar = bar.child(row().key("filter").h(size::BAR_BUTTON_H).px(space::MD).gap(space::SM).center_y().radius(radius::BAR_CONTROL)
            .hover(p.row_hover).press(p.control_surface)
            .on_press(|s: &mut App| s.open_filter_menu())
            .child(icon(if screen.route == Route::Ai { &icons::AI } else if matches!(screen.layout, Layout::Grid { .. }) { &icons::GRID } else { &icons::LIST }, 14.0)
                .color(if screen.route == Route::Ai { theme::Tint::Violet.stops().0 } else { p.text_secondary }))
            .child(text(name).font(type_::BAR).color(p.text.alpha(0.9)))
            .child(icon(&icons::CHEVRON_DOWN, 11.0).color(p.text_secondary)));
    }
    bar
}

fn rune_loop() -> rune::LoopSpec {
    rune::LoopSpec::rotation(0.9)
}

fn toast_pill(p: &Palette, toast: &crate::app::Toast) -> El {
    glass(row().key("toast").h(size::MENU_BUTTON).px(space::XL).gap(space::MD).center_y(), p, size::MENU_BUTTON / 2.0)
        .transition(Transition::opacity().combined(Transition::offset(0.0, 6.0)))
        .child(crate::hud::tone_glyph(toast.tone, p))
        .child(text(toast.text.clone()).font(type_::BAR).color(p.text).truncate())
}

fn bottom_bar(s: &App, p: &Palette, screen: &Screen) -> El {
    let menu_circle = glass(el().key("menu-button").size(size::MENU_BUTTON, size::MENU_BUTTON).center(), p, size::MENU_BUTTON / 2.0)
        .hover(p.row_hover).press(p.control_surface)
        .on_press(|s: &mut App| if s.menu.is_some() { s.close_menu() } else { s.open_main_menu() })
        .child(icon(&icons::MENU, 18.0).color(p.text.alpha(0.82)));
    let mut bar = row().key("bottom").pin(FULL, None, FULL, FULL).h(size::BOTTOM_BAR_H).px(space::MD).gap(space::MD).center_y().child(menu_circle);
    if let Some(toast) = &s.toast {
        bar = bar.child(toast_pill(p, toast));
    }
    bar = bar.child(spacer());

    let (primary, has_actions) = match &s.form {
        Some(form) => (Some((form.submit.clone(), "ctrl+enter".to_string())), false),
        None => {
            let item = screen.selected();
            (item.and_then(|i| i.actions.first()).map(|a| (a.title.clone(), "enter".to_string())),
             item.is_some_and(|i| i.actions.len() > 1))
        }
    };
    let Some((title, keys)) = primary else { return bar };
    let mut group = row().key("actions").h(size::MENU_BUTTON).px(space::XS).center_y();
    let primary_button = bar_button(p, "primary")
        .child(text(title).font(TextStyle::new(12.0, 600.0).lh(16.0)).color(p.text).truncate())
        .child(keycaps_row(p, &keys, false));
    group = group.child(if s.form.is_some() {
        primary_button.on_press_tx(|s: &mut App, tx| forms::submit(s, tx))
    } else {
        primary_button.on_press_tx(|s: &mut App, tx| s.run_primary(tx))
    });
    if has_actions {
        group = group.child(bar_button(p, "actions-toggle")
            .on_press(|s: &mut App| if s.menu.is_some() { s.close_menu() } else { s.open_actions() })
            .child(text("Actions").font(type_::BAR).color(p.text.alpha(0.86)))
            .child(keycaps_row(p, "ctrl+k", false)));
    }
    bar.child(glass(group, p, size::MENU_BUTTON / 2.0))
}

fn menu_row(p: &Palette, action: &Action, index: usize, selected: bool, checked: Option<bool>) -> El {
    let ink = if action.destructive { p.destructive } else { p.text };
    let glyph = match checked {
        Some(on) => el().size(size::MENU_ICON, size::MENU_ICON).center()
            .child(if on { icon(&icons::CHECK, 14.0).color(p.menu_symbol) } else { el() }),
        None => el().size(size::MENU_ICON, size::MENU_ICON).center()
            .child(icon(action.icon, 15.0).color(if action.destructive { p.destructive } else { p.menu_symbol })),
    };
    let mut r = row().key(format!("menu-row-{index}")).w_full().h(size::MENU_ROW_H).px(space::MD).gap(space::MD).center_y().radius(radius::MENU_ROW);
    r = if selected { r.fill(p.menu_hover) } else { r.fill(Color::TRANSPARENT).hover(p.menu_hover.alpha(0.06)) }
        .motion(motion::HOVER)
        .on_press_tx(move |s: &mut App, tx| s.run_menu(index, tx))
        .child(glyph)
        .child(text(action.title.clone()).font(type_::MENU_ROW).color(ink).truncate())
        .child(spacer());
    if !action.keys.is_empty() {
        r = r.child(keycaps_row(p, &action.keys, true));
    }
    r
}

fn menu_view(s: &App, p: &Palette, menu: &Menu) -> El {
    let visible = menu.visible();
    let mut rows = col().w_full().padding(space::SM).gap(size::MENU_ROW_GAP);
    if visible.is_empty() {
        rows = rows.child(row().w_full().h(size::MENU_ROW_H).center()
            .child(text("No Results").font(type_::MENU_ROW).color(p.text_secondary)));
    }
    let filter = s.top().filter;
    for (n, (index, action)) in visible.iter().enumerate() {
        if n > 0 && action.section.is_some() {
            rows = rows.child(el().w_full().px(space::MD).py(space::XS).child(el().w_full().h(size::HAIRLINE).fill(p.separator)));
        }
        let checked = (menu.kind == MenuKind::Filter).then_some(*index == filter);
        rows = rows.child(menu_row(p, action, *index, *index == menu.sel, checked));
    }
    let max_rows = if menu.kind == MenuKind::Filter { 8.5 } else { size::MENU_VISIBLE_ROWS + 1.0 };
    let max_h = (max_rows * (size::MENU_ROW_H + size::MENU_ROW_GAP)).round() + space::SM;
    let list = col().key("menu-list").w_full().max_h(max_h).scroll(ScrollSpec { stick_bottom: false }).scroll_edge_fade(18.0).child(rows);
    let field = row().w_full().h(size::MENU_ROW_H + 2.0).px(space::DIALOG_INSET).center_y()
        .child(text_input(&menu.query, type_::MENU_ROW).key("menu-search").single_line().grow(1.0).color(p.text)
            .placeholder(match menu.kind {
                MenuKind::Actions => "Search for actions...",
                MenuKind::Main => "Search...",
                MenuKind::Filter => "Filter...",
            }, p.text_tertiary).no_focus_ring()
            .on_edit(|s: &mut App| &mut s.menu.as_mut().expect("a menu is open").query)
            .on_input_key(|s: &mut App, ev, tx| s.key(ev, tx))
            .on_input_change(|s: &mut App, _| {
                if let Some(menu) = &mut s.menu {
                    menu.sel = menu.visible().first().map(|(i, _)| *i).unwrap_or(0);
                }
            }));
    let rule = el().w_full().h(size::HAIRLINE).fill(p.separator);
    // Header menus hang under their control with the field on top; footer menus rise above
    // theirs with the field at the bottom, next to the bar that opened them.
    let (width, panel) = match menu.kind {
        MenuKind::Filter => (size::EMOJI_CATEGORY_MENU_W, col().child(field).child(rule).child(list)
            .pin(None, Some(size::HEADER_BAND - space::XS), Some(space::MD * 2.0), None)),
        MenuKind::Actions => (size::ACTION_MENU_W, col().child(list).child(rule).child(field)
            .pin(None, None, Some(space::MD), Some(size::BOTTOM_BAR_H))),
        MenuKind::Main => (size::MENU_W, col().child(list).child(rule).child(field)
            .pin(Some(space::MD), None, None, Some(size::BOTTOM_BAR_H))),
    };
    let panel = glass(panel.key("menu").w(width), p, radius::MENU_PANEL).clip_rounded(radius::MENU_PANEL)
        .fill(if p.dark { rune::color::hex("#2a2a2c").unwrap_or(Color::BLACK).alpha(0.72) } else { Color::WHITE.alpha(0.78) })
        .shadow(10.0, 34.0, Color::BLACK.alpha(if p.dark { 0.42 } else { 0.18 }))
        .transition(Transition::opacity().combined(Transition::scale(0.97)));
    // A press outside the menu closes it and nothing else.
    let catcher = el().key("menu-catcher").pin(FULL, FULL, FULL, FULL).hit().on_press(|s: &mut App| s.close_menu());
    el().pin(FULL, FULL, FULL, FULL).child(catcher).child(panel)
}

fn dialog_view(s: &App, p: &Palette, d: &Confirm) -> El {
    let button = |label: &str, index: usize, color: Color| {
        let lit = s.dialog_sel == index;
        row().key(format!("dialog-{index}")).grow(1.0).h(size::DIALOG_BUTTON_H).center().radius(radius::ROW)
            .fill(if lit { p.selection.alpha(0.16) } else { p.control_surface }).hover(p.row_hover).press(p.control_pressed)
            .border(1.0, if lit { p.border } else { Color::TRANSPARENT })
            .on_press_tx(move |s: &mut App, tx| s.answer_dialog(index == 1, tx))
            .child(text(label.to_string()).font(type_::ROW_TRAILING).color(color))
    };
    let tint = if d.destructive { p.destructive } else { p.text_secondary };
    let card = col().key("dialog").w(size::DIALOG_COMPACT_W).radius(radius::PANEL).clip_rounded(radius::PANEL)
        .backdrop_blur(22.0).fill(if p.dark { rune::color::hex("#1e1e20").unwrap_or(Color::BLACK).alpha(0.78) } else { Color::WHITE.alpha(0.82) })
        .border_gradient(1.0, p.glass_rim_top, p.glass_rim_bottom).shadow(14.0, 44.0, Color::BLACK.alpha(0.45))
        .transition(Transition::opacity().combined(Transition::scale(0.95)))
        .child(col().w_full().padding(22.0).gap(space::XL).items_center()
            .child(el().size(size::DIALOG_SYMBOL_BOX, size::DIALOG_SYMBOL_BOX).radius(radius::DIALOG_SYMBOL).fill(p.control_surface).center()
                .child(icon(d.icon, size::DIALOG_SYMBOL - 4.0).color(tint)))
            .child(col().w_full().gap(space::XS).items_center()
                .child(text(d.title.clone()).font(type_::PANEL_TITLE).color(p.text).align(rune::Align::Center).wrap())
                .child(text(d.message.clone()).font(type_::ROW_TRAILING).color(p.text_secondary).align(rune::Align::Center).wrap())))
        .child(row().w_full().px(space::DIALOG_INSET).pb(space::DIALOG_INSET).gap(space::MD)
            .child(button("Cancel", 0, p.text_secondary))
            .child(button(&d.confirm, 1, if d.destructive { p.destructive } else { p.primary_action })));
    let dim = el().key("dialog-dim").pin(FULL, FULL, FULL, FULL).fill(p.dialog_dimming).hit()
        .transition(Transition::opacity()).on_press_tx(|s: &mut App, tx| s.answer_dialog(false, tx));
    el().pin(FULL, FULL, FULL, FULL).child(dim).child(row().pin(FULL, FULL, FULL, FULL).center().child(card))
}

/// The surface every EchoSpot window stands on: a rounded panel inset in its window by
/// `WINDOW_MARGIN` (room for the shadow), filled with the screen behind it, blurred, under the
/// scrim. `backdrop` is that capture when there is one; `mode` is the Surface setting.
pub fn panel_surface(key: &str, p: &Palette, (w, h): (f32, f32), backdrop: Option<&rune::Pixels>, mode: &str) -> El {
    let panel = el().key(key.to_string()).absolute(size::WINDOW_MARGIN, size::WINDOW_MARGIN).size(w, h)
        .radius(radius::PANEL).clip_rounded(radius::PANEL)
        .shadow(16.0, 48.0, Color::BLACK.alpha(if p.dark { 0.42 } else { 0.24 }))
        .box_shadow(rune::Shadow::core_animation(0.0, 0.0, 0.5, Color::BLACK.alpha(0.55)));
    match (backdrop, mode) {
        (Some(pixels), _) => {
            let bleed = crate::backdrop::BLEED;
            panel.child(el().absolute(-bleed, -bleed).size(w + bleed * 2.0, h + bleed * 2.0).pixels(pixels).blur(26.0).inert())
                .child(el().pin(FULL, FULL, FULL, FULL).fill(p.panel_scrim).inert())
        }
        (None, "compositor") => panel.fill(p.panel_scrim),
        _ => panel.fill(if p.dark { rune::color::hex("#1b1b1d").unwrap_or(Color::BLACK).alpha(0.94) } else { Color::WHITE.alpha(0.94) }),
    }
}

/// The rim Liquid Glass lights along a panel's top edge, fading around its corners. Drawn last.
pub fn panel_rim(p: &Palette) -> El {
    el().pin(FULL, FULL, FULL, FULL).radius(radius::PANEL).border_gradient(1.0, p.panel_rim_top, p.panel_rim_bottom).inert()
}

pub fn view(s: &App, cx: &mut Cx<App>) -> El {
    let env = cx.env();
    let p = theme::palette(s.settings.dark(env.system_dark));
    let scale = env.scale.max(1.0);
    let screen = s.top();
    let body = match (&s.form, screen.layout) {
        (Some(form), _) => form_body(&p, form),
        (None, Layout::List) => match &screen.route {
            Route::Ext(id) => crate::ext::view::body(s, &p, scale, *id).unwrap_or_else(|| list_body(s, &p, scale, screen)),
            Route::Ai if !s.chat.messages.is_empty() => crate::features::ai::body(s, &p),
            _ => list_body(s, &p, scale, screen),
        },
        (None, Layout::Split) => split_body(s, &p, scale, screen),
        (None, Layout::Grid { columns }) => grid_body(s, &p, scale, screen, columns),
    };
    let mut panel = panel_surface("panel", &p, (size::PANEL_W, size::PANEL_H), s.has_backdrop.then_some(&s.backdrop), &s.settings.backdrop);
    panel = panel.child(body).child(header(s, &p, screen)).child(bottom_bar(s, &p, screen));
    if let Some(menu) = &s.menu {
        panel = panel.child(menu_view(s, &p, menu));
    }
    if let Some(dialog) = &s.dialog {
        panel = panel.child(dialog_view(s, &p, dialog));
    }
    panel = panel.child(panel_rim(&p));
    // The window is larger than the panel so the shadow has room; a press out there dismisses.
    el().w_full().h_full()
        .child(el().key("outside").pin(FULL, FULL, FULL, FULL).hit().on_press_tx(|s: &mut App, tx| s.hide(tx)))
        .child(panel)
}
