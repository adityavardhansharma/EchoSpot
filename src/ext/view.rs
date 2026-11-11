//! A running command's tree as the palette's screens: List and Grid become rows, Detail a
//! Markdown page with its metadata, Form the palette's own form.

use std::path::Path;

use rune::{Color, El, ScrollSpec, Tx, col, el, row, spacer, text};
use rune_kit::markdown::{Markdown, MarkdownStyle};
use serde_json::Value;

use crate::app::{App, Built};
use crate::ext::{actions_from, handler, icon_from, raycast_color, text_of};
use crate::features::forms::{self, Field, FieldKind, Form, Target};
use crate::features::fuzzy;
use crate::model::*;
use crate::theme::{Palette, radius, size, space, type_};

fn props(node: &Value) -> &Value {
    node.get("props").unwrap_or(&Value::Null)
}
fn kind(node: &Value) -> &str {
    node.get("type").and_then(Value::as_str).unwrap_or("")
}
fn children(node: &Value) -> &[Value] {
    node.get("children").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}
fn flag(node: &Value, key: &str) -> bool {
    props(node).get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// `(value, title)` for each item of a dropdown, sections flattened.
pub fn dropdown_options(dropdown: &Value) -> Vec<(String, String)> {
    let mut out = Vec::new();
    fn walk(node: &Value, out: &mut Vec<(String, String)>) {
        for child in children(node) {
            match kind(child) {
                "Dropdown.Item" => out.push((text_of(props(child).get("value")), text_of(props(child).get("title")))),
                _ => walk(child, out),
            }
        }
    }
    walk(dropdown, &mut out);
    out
}

fn metadata_fields(node: Option<&Value>) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for child in node.map(children).unwrap_or(&[]) {
        let p = props(child);
        match kind(child) {
            "Metadata.Label" => out.push((text_of(p.get("title")), text_of(p.get("text")))),
            "Metadata.Link" => out.push((text_of(p.get("title")), text_of(p.get("text")))),
            "Metadata.TagList" => {
                let tags: Vec<String> = children(child).iter().map(|t| text_of(props(t).get("text"))).collect();
                out.push((text_of(p.get("title")), tags.join(", ")));
            }
            _ => {}
        }
    }
    out
}

fn accessories(node: &Value) -> Vec<Accessory> {
    props(node).get("accessories").and_then(Value::as_array).into_iter().flatten().filter_map(|a| {
        if let Some(tag) = a.get("tag") {
            let value = text_of(tag.get("value").or(Some(tag)));
            let color = tag.get("color").and_then(|c| raycast_color(c, true)).unwrap_or(Color::WHITE.alpha(0.7));
            return (!value.is_empty()).then_some(Accessory { text: value, tag: Some(color) });
        }
        let value = match a.get("text") {
            Some(Value::Object(o)) => text_of(o.get("value")),
            other => text_of(other),
        };
        let value = if value.is_empty() { a.get("date").map(|d| text_of(d.get("$date")).chars().take(10).collect()).unwrap_or_default() } else { value };
        (!value.is_empty()).then_some(Accessory { text: value, tag: None })
    }).collect()
}

fn list_item(app: &App, session: u64, node: &Value, dir: &Path, grid: bool, root_actions: Option<&Value>) -> (Item, String) {
    let p = props(node);
    let id = p.get("id").and_then(Value::as_str).map(String::from).unwrap_or_else(|| format!("n{}", node.get("id").and_then(Value::as_u64).unwrap_or(0)));
    let icon_value = if grid { p.get("content").or(p.get("icon")) } else { p.get("icon") };
    let icon = icon_value.map(|v| icon_from(v, dir)).unwrap_or(IconSrc::None);
    let mut item = Item::new(format!("x{session}:{id}"), text_of(p.get("title")), icon);
    let subtitle = text_of(p.get("subtitle"));
    if !subtitle.is_empty() {
        item.subtitle = Some(subtitle.clone());
    }
    item.accessories = accessories(node);
    item.actions = actions_from(app, session, p.get("actions").or(root_actions), dir);
    if let Some(detail) = p.get("detail") {
        let dp = props(detail);
        item.detail = Some(Detail {
            markdown: dp.get("markdown").and_then(Value::as_str).map(String::from),
            fields: metadata_fields(dp.get("metadata")),
            ..Default::default()
        });
    }
    let keywords = p.get("keywords").and_then(Value::as_array).map(|k| k.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" ")).unwrap_or_default();
    (item, format!("{subtitle} {keywords}"))
}

pub fn build(app: &App, session: u64, query: &str) -> Built {
    let Some(s) = app.ext.session(session) else { return Built::list(Sections::default(), "") };
    let dir = app.ext.get(&s.extension).map(|e| e.dir.clone()).unwrap_or_default();
    let Some(root) = s.root() else {
        return Built::list(Sections::default(), "").loading(true).empty(if s.ready { "Nothing to Show" } else { "Loading…" }, "");
    };
    let rp = props(root);
    let placeholder = text_of(rp.get("searchBarPlaceholder"));
    let loading = flag(root, "isLoading");
    match kind(root) {
        "List" | "Grid" => {
            let grid = kind(root) == "Grid";
            // A command that listens for the search text filters for itself, unless it says otherwise.
            let own_search = handler(rp.get("onSearchTextChange")).is_some();
            let filtering = match rp.get("filtering") {
                Some(Value::Bool(b)) => *b,
                Some(Value::Object(_)) => true,
                _ => !own_search,
            };
            let q = query.trim();
            let mut sections = Sections::default();
            let mut loose: Vec<Item> = Vec::new();
            let mut empty = ("No Results".to_string(), String::new());
            let keep = |item: &Item, keywords: &str| !filtering || q.is_empty() || fuzzy::score_item(q, &item.title, keywords).is_some();
            for child in children(root) {
                match kind(child) {
                    "List.Item" | "Grid.Item" => {
                        let (item, keywords) = list_item(app, session, child, &dir, grid, rp.get("actions"));
                        if keep(&item, &keywords) {
                            loose.push(item);
                        }
                    }
                    "List.Section" | "Grid.Section" => {
                        if !loose.is_empty() {
                            sections.0.push((String::new(), std::mem::take(&mut loose)));
                        }
                        let items: Vec<Item> = children(child).iter().filter(|c| matches!(kind(c), "List.Item" | "Grid.Item"))
                            .map(|c| list_item(app, session, c, &dir, grid, rp.get("actions")))
                            .filter(|(item, keywords)| keep(item, keywords)).map(|(item, _)| item).collect();
                        sections.push(&text_of(props(child).get("title")), items);
                    }
                    "EmptyView" => empty = (text_of(props(child).get("title")), text_of(props(child).get("description"))),
                    _ => {}
                }
            }
            if !loose.is_empty() {
                sections.0.push((String::new(), loose));
            }
            let columns = rp.get("columns").and_then(Value::as_u64).unwrap_or(5).clamp(2, 8) as usize;
            let layout = if grid { Layout::Grid { columns } } else if flag(root, "isShowingDetail") { Layout::Split } else { Layout::List };
            let filters: Vec<String> = rp.get("searchBarAccessory").map(|d| dropdown_options(d).into_iter().map(|(_, title)| title).collect()).unwrap_or_default();
            let mut built = Built::list(sections, if placeholder.is_empty() { "Search..." } else { &placeholder }).layout(layout).loading(loading).keep();
            built.filters = filters;
            built.empty = if loading && built.items.is_empty() { ("Loading…".into(), String::new()) } else { empty };
            built
        }
        "Detail" => {
            // One unseen row carries the view's actions to the bottom bar.
            let mut item = Item::new(format!("x{session}:detail"), s.title.clone(), IconSrc::None);
            item.actions = actions_from(app, session, rp.get("actions"), &dir);
            let mut built = Built::list(Sections::default(), "").loading(loading).keep();
            built.items = vec![item];
            built.rows = vec![Row::Item(0)];
            built
        }
        _ => Built::list(Sections::default(), "").loading(loading),
    }
}

pub fn markdown(p: &Palette, source: &str) -> El {
    Markdown::new(source.to_string()).selectable(false).render(&MarkdownStyle::neutral(p.dark))
}

/// A Detail view's page: Markdown, with its metadata in a column beside it.
pub fn body(app: &App, p: &Palette, _scale: f32, session: u64) -> Option<El> {
    let s = app.ext.session(session)?;
    let root = s.root()?;
    if kind(root) != "Detail" {
        return None;
    }
    let rp = props(root);
    let source = rp.get("markdown").and_then(Value::as_str).unwrap_or("");
    let page = col().grow(1.0).h_full().min_h(0.0).pt(size::HEADER_BAND).pb(size::BOTTOM_BAR_H)
        .child(col().key("list").w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).scroll_edge_fade(28.0)
            .child(col().w_full().px(space::XXL + space::XS).pb(space::XL).child(markdown(p, source))));
    let mut layout = row().pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0)).child(page);
    if let Some(meta) = rp.get("metadata") {
        let mut side = col().w(240.0).px(space::XL + 2.0).gap(space::LG);
        for child in children(meta) {
            let cp = props(child);
            let label = |title: String| text(title).font(type_::SECTION).color(p.text_secondary);
            side = side.child(match kind(child) {
                "Metadata.Separator" => el().w_full().h(size::HAIRLINE).fill(p.separator),
                "Metadata.TagList" => {
                    let mut tags = row().w_full().flex_wrap().gaps(space::XS, space::XS);
                    for tag in children(child) {
                        let color = props(tag).get("color").and_then(|c| raycast_color(c, p.dark)).unwrap_or(p.text_secondary);
                        tags = tags.child(row().px(space::SM).py(space::XXS).radius(radius::MENU).fill(color.alpha(0.18))
                            .child(text(text_of(props(tag).get("text"))).font(type_::ROW_TRAILING).color(color)));
                    }
                    col().w_full().gap(space::XS).child(label(text_of(cp.get("title")))).child(tags)
                }
                "Metadata.Link" => {
                    let target = text_of(cp.get("target"));
                    col().w_full().gap(space::XXS).child(label(text_of(cp.get("title"))))
                        .child(text(text_of(cp.get("text"))).font(type_::ROW_TITLE).color(p.primary_action).truncate()
                            .on_press_tx(move |s: &mut App, tx| s.perform(Act::OpenUrl(target.clone()), tx)))
                }
                _ => col().w_full().gap(space::XXS).child(label(text_of(cp.get("title"))))
                    .child(text(text_of(cp.get("text"))).font(type_::ROW_TITLE).color(p.text).wrap()),
            });
        }
        layout = layout
            .child(col().h_full().pt(size::HEADER_BAND).pb(size::BOTTOM_BAR_H).child(el().w(size::HAIRLINE).grow(1.0).fill(p.separator)))
            .child(col().h_full().pt(size::HEADER_BAND + space::XS).pb(size::BOTTOM_BAR_H).child(side).child(spacer()));
    }
    Some(layout)
}

/// The actions of the Form on screen; the first submits.
pub fn form_actions(app: &App, session: u64) -> Vec<Action> {
    let Some(s) = app.ext.session(session) else { return Vec::new() };
    let dir = app.ext.get(&s.extension).map(|e| e.dir.clone()).unwrap_or_default();
    let Some(root) = s.root().filter(|r| kind(r) == "Form") else { return Vec::new() };
    actions_from(app, session, props(root).get("actions"), &dir)
}

fn form_fields(root: &Value) -> Vec<Field> {
    let mut fields = Vec::new();
    for (n, child) in children(root).iter().enumerate() {
        let p = props(child);
        let id = p.get("id").and_then(Value::as_str).map(String::from).unwrap_or_else(|| format!("field-{n}"));
        let title = text_of(p.get("title"));
        let initial = text_of(p.get("value").or(p.get("defaultValue")));
        let mut field = match kind(child) {
            "Form.TextField" | "Form.DatePicker" | "Form.FilePicker" | "Form.TagPicker" => Field::new(&id, &title, FieldKind::Text).value(&initial),
            "Form.PasswordField" => Field::new(&id, &title, FieldKind::Password).value(&initial),
            "Form.TextArea" => Field::new(&id, &title, FieldKind::Area).value(&initial),
            "Form.Dropdown" => Field::new(&id, &title, FieldKind::Dropdown(dropdown_options(child))).value(&initial),
            "Form.Checkbox" => {
                let mut f = Field::new(&id, &title, FieldKind::Checkbox(text_of(p.get("label"))));
                f.checked = p.get("value").or(p.get("defaultValue")).and_then(Value::as_bool).unwrap_or(false);
                f
            }
            "Form.Description" => {
                let mut f = Field::new(&id, &title, FieldKind::Description);
                f.edit = rune::TextEdit::new(text_of(p.get("text")));
                f
            }
            "Form.Separator" => Field::new(&id, "", FieldKind::Separator),
            _ => continue,
        };
        field.placeholder = text_of(p.get("placeholder"));
        field.info = text_of(p.get("info"));
        field.error = text_of(p.get("error"));
        fields.push(field);
    }
    fields
}

/// Keeps the palette's form in step with a command whose view is a Form: built once, then only
/// its errors follow each render, so what was typed is never replaced.
pub fn sync_form(app: &mut App, session: u64, tx: &mut Tx) {
    let Some(s) = app.ext.session(session) else { return };
    let root = s.root().filter(|r| kind(r) == "Form").cloned();
    let ours = matches!(app.form.as_ref().map(|f| &f.target), Some(Target::ExtForm { session: id }) if *id == session);
    match (root, ours) {
        (Some(root), false) if app.form.is_none() => {
            let submit = form_actions(app, session).first().map(|a| a.title.clone()).unwrap_or_else(|| "Submit".into());
            let title = text_of(props(&root).get("navigationTitle"));
            let title = if title.is_empty() { s.title.clone() } else { title };
            let form = Form::new(&title, &submit, form_fields(&root), Target::ExtForm { session });
            forms::open(app, form, tx);
        }
        (Some(root), true) => {
            let fresh = form_fields(&root);
            if let Some(form) = &mut app.form {
                for field in &mut form.fields {
                    if let Some(new) = fresh.iter().find(|f| f.id == field.id) {
                        field.error = new.error.clone();
                    }
                }
            }
        }
        (None, true) => {
            // The command moved on from its form.
            if app.top().route == Route::Form {
                app.screens.pop();
            }
            app.form = None;
            app.top_mut().query.focused = true;
            app.refresh();
        }
        _ => {}
    }
}
